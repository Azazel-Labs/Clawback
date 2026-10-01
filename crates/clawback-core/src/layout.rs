//! SpaceMonger 1.4's nested box layout, reproduced exactly.
//!
//! The algorithm (from `CFolderView::SizeFolders` in the original source):
//!
//! 1. Take the folder's entries, largest first. Deal them greedily into two
//!    lists, always adding to whichever list currently has the smaller total.
//! 2. Split the rectangle in proportion to the two totals — across its width
//!    if it is wider than it is tall (adjusted by the *bias* setting),
//!    otherwise across its height.
//! 3. For each half: if it holds several entries and is larger than the
//!    minimum box size (set by *density*), repeat from step 1. If it holds one
//!    entry and is big enough, that entry becomes a box; folders then lay out
//!    their own contents inside a 3px frame below a 12px title band. Anything
//!    too small becomes an unnamed "filler" box.
//!
//! Boxes are emitted in pre-order (a folder before its contents), which is
//! what the hit-testing rules rely on. Coordinates are integers; like the
//! original, a box `(x, y, w, h)` is drawn covering `w + 1` by `h + 1` pixels
//! so neighbours share their 1px black outlines.

use crate::tree::{NodeId, Tree, flags};

/// Minimum box sizes (width, height) for density -3 ..= 3.
pub const MIN_SIZES: [(i32, i32); 7] = [(96, 64), (64, 48), (48, 32), (32, 24), (24, 16), (16, 12), (8, 6)];

/// Folder frame: left/right/bottom border width and top title band height.
pub const FRAME: i32 = 3;
pub const TITLE: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LayoutSettings {
    /// -3 (big boxes, few files) ..= 3 (tiny boxes). SpaceMonger's default is 0.
    pub density: i32,
    /// -20 (prefer horizontal splits) ..= 20 (prefer vertical splits).
    pub bias: i32,
}

impl LayoutSettings {
    pub fn min_size(&self) -> (i32, i32) {
        MIN_SIZES[(self.density + 3).clamp(0, 6) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A real file or folder.
    Node(NodeId),
    /// The free-space pseudo entry shown beside the drive's root contents.
    Free,
    /// Space for entries too small to draw individually.
    Filler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub item: Item,
    /// Nesting depth used for colouring (the zoomed folder's depth for its
    /// direct contents). -1 for free space.
    pub depth: i32,
    pub folder: bool,
}

impl DisplayBox {
    /// Strict containment, as in the original (`x < px < x + w`).
    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px > self.x && py > self.y && px < self.x + self.w && py < self.y + self.h
    }

    /// True if the point is on a folder's frame or title band rather than
    /// over its contents.
    #[inline]
    pub fn on_frame(&self, px: i32, py: i32) -> bool {
        px < self.x + FRAME || py < self.y + TITLE || px > self.x + self.w - FRAME || py > self.y + self.h - FRAME
    }

    pub fn node(&self) -> Option<NodeId> {
        match self.item {
            Item::Node(n) => Some(n),
            _ => None,
        }
    }
}

type Entry = (u64, Item);

enum Task {
    /// Lay out a folder's contents in a rectangle.
    Folder { x: i32, y: i32, w: i32, h: i32, node: NodeId, depth: i32 },
    /// Split a list of entries across a rectangle.
    Size { x: i32, y: i32, w: i32, h: i32, list: Vec<Entry>, depth: i32 },
    /// Place one half of a split.
    Place { x: i32, y: i32, w: i32, h: i32, list: Vec<Entry>, depth: i32 },
}

/// Lay out `view` into a `width` x `height` area.
///
/// `depth` is the view's nesting depth (SpaceMonger's zoom level), so colours
/// stay the same as you zoom. `free` adds the free-space pseudo entry to the
/// view's contents.
pub fn build(
    tree: &Tree,
    view: NodeId,
    width: i32,
    height: i32,
    settings: &LayoutSettings,
    free: Option<u64>,
) -> Vec<DisplayBox> {
    let mut out = Vec::new();
    if width < 2 || height < 2 {
        return out;
    }
    let depth = i32::try_from(tree.depth(view)).unwrap_or(i32::MAX);
    let (hmin, vmin) = settings.min_size();
    let (wbias, hbias) = match settings.bias {
        b if b > 0 => (b + 8, 8),
        b if b < 0 => (8, -b + 8),
        _ => (8, 8),
    };

    let mut stack = vec![Task::Folder { x: 0, y: 0, w: width - 1, h: height - 1, node: view, depth }];
    let mut first = true;
    while let Some(task) = stack.pop() {
        match task {
            Task::Folder { x, y, w, h, node, depth } => {
                let mut list: Vec<Entry> = tree
                    .node(node)
                    .children
                    .iter()
                    .filter_map(|&c| {
                        let n = tree.node(c);
                        (!n.has(flags::REMOVED)).then_some((n.size, Item::Node(c)))
                    })
                    .collect();
                if first {
                    if let Some(f) = free {
                        list.push((f, Item::Free));
                    }
                    first = false;
                }
                // Largest first; stable so equal sizes keep directory order.
                list.sort_by_key(|e| std::cmp::Reverse(e.0));
                stack.push(Task::Size { x, y, w, h, list, depth });
            }
            Task::Size { x, y, w, h, list, depth } => {
                let mut l1 = Vec::new();
                let mut l2 = Vec::new();
                let (mut s1, mut s2) = (0u128, 0u128);
                for e in list {
                    if e.0 == 0 {
                        continue;
                    }
                    if s1 <= s2 {
                        s1 += u128::from(e.0);
                        l1.push(e);
                    } else {
                        s2 += u128::from(e.0);
                        l2.push(e);
                    }
                }
                let total = s1 + s2;
                if total == 0 {
                    continue;
                }
                let (r1, r2) = if (w * wbias) / 8 > (h * hbias) / 8 {
                    let split = split_at(w, s1, total);
                    ((x, y, split, h), (x + split, y, w - split, h))
                } else {
                    let split = split_at(h, s1, total);
                    ((x, y, w, split), (x, y + split, w, h - split))
                };
                // Stack is LIFO: push the second half first.
                stack.push(Task::Place { x: r2.0, y: r2.1, w: r2.2, h: r2.3, list: l2, depth });
                stack.push(Task::Place { x: r1.0, y: r1.1, w: r1.2, h: r1.3, list: l1, depth });
            }
            Task::Place { x, y, w, h, list, depth } => {
                let big = w > hmin && h > vmin;
                if list.len() > 1 && big {
                    stack.push(Task::Size { x, y, w, h, list, depth });
                } else if let Some(&(_, item)) = list.first() {
                    if big {
                        let folder_node = match item {
                            Item::Node(n) if tree.node(n).is_dir() => Some(n),
                            _ => None,
                        };
                        let d = if item == Item::Free { -1 } else { depth };
                        out.push(DisplayBox { x, y, w, h, item, depth: d, folder: folder_node.is_some() });
                        if let Some(n) = folder_node {
                            stack.push(Task::Folder {
                                x: x + FRAME,
                                y: y + TITLE,
                                w: w - 2 * FRAME,
                                h: h - TITLE - FRAME,
                                node: n,
                                depth: depth + 1,
                            });
                        }
                    } else if w >= 0 && h >= 0 {
                        out.push(DisplayBox { x, y, w, h, item: Item::Filler, depth, folder: false });
                    }
                }
            }
        }
    }
    out
}

fn split_at(extent: i32, part: u128, total: u128) -> i32 {
    let e = u128::try_from(extent.max(0)).unwrap_or(0);
    i32::try_from(e * part / total).unwrap_or(extent)
}

/// The selectable box under a point (SpaceMonger's `GetDisplayFolderFromPoint`).
///
/// A folder is only hit on its frame and title band; a point over its contents
/// falls through to whatever is drawn there. Filler and free-space boxes are
/// never selectable.
pub fn hit_test(boxes: &[DisplayBox], px: i32, py: i32) -> Option<usize> {
    let i = boxes.iter().position(|b| b.contains(px, py) && (!b.folder || b.on_frame(px, py)))?;
    matches!(boxes[i].item, Item::Node(_)).then_some(i)
}

/// Every named box containing the point: the folder path and the entry under
/// the cursor. Used for rollover highlighting.
pub fn rollover(boxes: &[DisplayBox], px: i32, py: i32) -> impl Iterator<Item = usize> + '_ {
    boxes.iter().enumerate().filter(move |(_, b)| matches!(b.item, Item::Node(_)) && b.contains(px, py)).map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Kind, NewEntry, ROOT};
    use std::path::Path;

    fn e(name: &str, kind: Kind, size: u64) -> NewEntry {
        NewEntry { name: name.into(), kind, size, len: size, mtime: 0, flags: 0, file_id: None }
    }

    fn tree() -> Tree {
        let mut t = Tree::new(Path::new("/r"));
        let r = t.add_children(ROOT, vec![e("big", Kind::Dir, 0), e("f1", Kind::File, 300), e("f2", Kind::File, 100)]);
        t.add_children(r.start, vec![e("a", Kind::File, 400), e("b", Kind::File, 200)]);
        t.sort_all();
        t
    }

    fn overlap(a: &DisplayBox, b: &DisplayBox) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    #[test]
    fn greedy_split_matches_spacemonger() {
        // Root holds big(600), f1(300), f2(100) in 800x600 (w,h = 799,599).
        // Deal: big -> L1 (600); f1 -> L2 (300); f2 -> L2 (400).
        // 799 > 599 so split width: 799*600/1000 = 479.
        let t = tree();
        let b = build(&t, ROOT, 800, 600, &LayoutSettings::default(), None);
        let big = t.find_path(Path::new("/r/big")).unwrap();
        let f1 = t.find_path(Path::new("/r/f1")).unwrap();
        let f2 = t.find_path(Path::new("/r/f2")).unwrap();
        let first = b[0];
        assert_eq!((first.x, first.y, first.w, first.h), (0, 0, 479, 599));
        assert_eq!(first.item, Item::Node(big));
        assert!(first.folder);
        // big's contents in its frame: (3, 12, 473, 584); a(400) | b(200) split by height? 473 > 584? no -> height.
        let a = b.iter().find(|x| x.depth == 1 && x.y == 12).unwrap();
        assert_eq!((a.x, a.w, a.h), (3, 473, 584 * 400 / 600));
        // Right half (320 x 599) has f1 and f2: split by height, 599*300/400 = 449.
        let bf1 = b.iter().find(|x| x.item == Item::Node(f1)).unwrap();
        let bf2 = b.iter().find(|x| x.item == Item::Node(f2)).unwrap();
        assert_eq!((bf1.x, bf1.y, bf1.w, bf1.h), (479, 0, 320, 449));
        assert_eq!((bf2.x, bf2.y, bf2.w, bf2.h), (479, 449, 320, 150));
    }

    #[test]
    fn preorder_and_no_overlap_between_siblings() {
        let t = tree();
        let b = build(&t, ROOT, 1024, 768, &LayoutSettings::default(), Some(500));
        // Parents precede their contents.
        let big = b.iter().position(|x| x.folder).unwrap();
        assert!(b.iter().skip(big + 1).any(|x| x.depth == 1));
        let top: Vec<_> = b.iter().filter(|x| x.depth <= 0).collect();
        for (i, p) in top.iter().enumerate() {
            for q in &top[i + 1..] {
                assert!(!overlap(p, q), "{p:?} overlaps {q:?}");
            }
        }
        let free = b.iter().find(|x| x.item == Item::Free).unwrap();
        assert_eq!(free.depth, -1);
    }

    #[test]
    fn hit_testing_rules() {
        let t = tree();
        let b = build(&t, ROOT, 800, 600, &LayoutSettings::default(), None);
        let big = t.find_path(Path::new("/r/big")).unwrap();
        let a = t.find_path(Path::new("/r/big/a")).unwrap();
        // Title band selects the folder.
        assert_eq!(b[hit_test(&b, 100, 5).unwrap()].item, Item::Node(big));
        // Left frame too.
        assert_eq!(b[hit_test(&b, 1, 100).unwrap()].item, Item::Node(big));
        // Interior hits the child.
        assert_eq!(b[hit_test(&b, 100, 100).unwrap()].item, Item::Node(a));
        // Exact edges are outside (strict comparison).
        assert!(hit_test(&b, 0, 0).is_none());
        // Rollover over a child lights up the folder and the child.
        assert_eq!(rollover(&b, 100, 100).count(), 2);
    }

    #[test]
    fn small_areas_become_fillers() {
        let mut t = Tree::new(Path::new("/r"));
        let mut v: Vec<NewEntry> = (0..500).map(|i| e(&format!("f{i}"), Kind::File, 10)).collect();
        v.push(e("huge", Kind::File, 1_000_000));
        t.add_children(ROOT, v);
        t.sort_all();
        let b = build(&t, ROOT, 640, 480, &LayoutSettings::default(), None);
        assert!(b.iter().any(|x| x.item == Item::Filler));
        assert!(b.len() < 50);
        // Fillers never hit.
        let fill = b.iter().find(|x| x.item == Item::Filler).unwrap();
        if fill.w > 2 && fill.h > 2 {
            let (px, py) = (fill.x + fill.w / 2, fill.y + fill.h / 2);
            assert!(hit_test(&b, px, py).is_none());
        }
    }

    #[test]
    fn density_and_bias_change_layout() {
        let mut t = Tree::new(Path::new("/r"));
        t.add_children(ROOT, (0..40).map(|i| e(&format!("f{i}"), Kind::File, 1000 + i)).collect());
        t.sort_all();
        let dense = build(&t, ROOT, 400, 300, &LayoutSettings { density: 3, bias: 0 }, None);
        let sparse = build(&t, ROOT, 400, 300, &LayoutSettings { density: -3, bias: 0 }, None);
        let named = |v: &[DisplayBox]| v.iter().filter(|b| b.node().is_some()).count();
        assert!(named(&dense) > named(&sparse));
        let vert = build(&t, ROOT, 400, 400, &LayoutSettings { density: 0, bias: 20 }, None);
        let horz = build(&t, ROOT, 400, 400, &LayoutSettings { density: 0, bias: -20 }, None);
        assert_ne!(vert, horz);
    }

    #[test]
    fn huge_flat_directory_is_fast_and_shallow() {
        let mut t = Tree::new(Path::new("/r"));
        t.add_children(ROOT, (0..200_000u64).map(|i| e(&format!("f{i}"), Kind::File, 1 + i % 977)).collect());
        t.sort_all();
        let start = std::time::Instant::now();
        let b = build(&t, ROOT, 1920, 1080, &LayoutSettings { density: 3, bias: 0 }, None);
        assert!(!b.is_empty());
        assert!(start.elapsed().as_secs_f32() < 2.0);
    }
}
