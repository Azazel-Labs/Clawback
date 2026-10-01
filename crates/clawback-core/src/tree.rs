//! Arena-backed size tree.
//!
//! Every file and directory is a [`Node`] in a paged arena, addressed by a
//! `u32` [`NodeId`]. Snapshots share unchanged pages. Directory sizes are the sum of everything below them and
//! are kept up to date as the scanner adds entries, so the tree can be drawn
//! while a scan is still running.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PAGE_SIZE: usize = 256;

/// Snapshots share unchanged pages; live edits copy only touched pages.
#[derive(Clone, Debug, Default)]
struct Nodes {
    pages: Vec<Arc<Vec<Node>>>,
    len: usize,
}

impl Nodes {
    fn len(&self) -> usize {
        self.len
    }
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn reserve(&mut self, additional: usize) {
        self.pages.reserve(additional.div_ceil(PAGE_SIZE));
    }
    fn push(&mut self, node: Node) {
        if self.len.is_multiple_of(PAGE_SIZE) {
            self.pages.push(Arc::new(Vec::with_capacity(PAGE_SIZE)));
        }
        Arc::make_mut(self.pages.last_mut().expect("node page")).push(node);
        self.len += 1;
    }
    fn get(&self, index: usize) -> Option<&Node> {
        self.pages.get(index / PAGE_SIZE)?.get(index % PAGE_SIZE)
    }
    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &Node> {
        self.pages.iter().flat_map(|page| page.iter())
    }
    fn into_nodes(self) -> impl Iterator<Item = Node> {
        self.pages.into_iter().flat_map(|page| Arc::unwrap_or_clone(page).into_iter())
    }
}

impl From<Vec<Node>> for Nodes {
    fn from(nodes: Vec<Node>) -> Self {
        let mut result = Self::default();
        for node in nodes {
            result.push(node);
        }
        result
    }
}

impl std::ops::Index<usize> for Nodes {
    type Output = Node;
    fn index(&self, index: usize) -> &Node {
        &self.pages[index / PAGE_SIZE][index % PAGE_SIZE]
    }
}

impl std::ops::IndexMut<usize> for Nodes {
    fn index_mut(&mut self, index: usize) -> &mut Node {
        &mut Arc::make_mut(&mut self.pages[index / PAGE_SIZE])[index % PAGE_SIZE]
    }
}

pub type NodeId = u32;
pub const NO_NODE: NodeId = u32::MAX;
pub const ROOT: NodeId = 0;
/// Filesystem volume/device and file identity; shared by hard links to a file.
pub type FileId = (u64, u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Dir,
    File,
    Symlink,
    Other,
}

/// Bit flags stored on each node.
pub mod flags {
    /// The directory could not be listed (usually permission denied).
    pub const DENIED: u8 = 1 << 0;
    /// Some entries inside the directory could not be read.
    pub const PARTIAL: u8 = 1 << 1;
    /// A directory on another filesystem / volume that was not descended into.
    pub const OTHER_FS: u8 = 1 << 2;
    /// A virtual/pseudo filesystem (e.g. `/proc`) that is never scanned.
    pub const VIRTUAL: u8 = 1 << 3;
    /// A hard link whose data was already counted elsewhere.
    pub const HARDLINK_DUP: u8 = 1 << 4;
    /// The node was deleted by the user and is no longer part of the tree.
    pub const REMOVED: u8 = 1 << 5;
}

#[derive(Clone, Debug)]
pub struct Node {
    /// File name. For the root node this is the full root path.
    pub name: Box<OsStr>,
    /// Size used for layout: bytes allocated on disk (or the length when
    /// scanning apparent sizes). For directories, everything below it.
    pub size: u64,
    /// Actual file length in bytes (for directories this equals `size`).
    pub len: u64,
    /// Last modification time, seconds since the Unix epoch (`i64::MIN` if unknown).
    pub mtime: i64,
    /// Number of non-directory entries at or below this node.
    pub files: u64,
    pub parent: NodeId,
    pub children: Vec<NodeId>,
    pub kind: Kind,
    pub flags: u8,
    pub file_id: Option<FileId>,
}

impl Node {
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
    #[inline]
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
    pub fn name_lossy(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }
    /// The byte count SpaceMonger shows on labels: file length, or folder total.
    pub fn display_len(&self) -> u64 {
        if self.is_dir() { self.size } else { self.len }
    }
}

/// A new entry handed to [`Tree::add_children`].
#[derive(Debug)]
pub struct NewEntry {
    pub name: OsString,
    pub kind: Kind,
    pub size: u64,
    pub len: u64,
    pub mtime: i64,
    pub flags: u8,
    pub file_id: Option<FileId>,
}

#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Nodes,
}

impl Tree {
    /// A tree containing only a root directory for `root`.
    pub fn new(root: &Path) -> Self {
        Tree {
            nodes: vec![Node {
                name: root.as_os_str().into(),
                size: 0,
                len: 0,
                mtime: i64::MIN,
                files: 0,
                parent: NO_NODE,
                children: Vec::new(),
                kind: Kind::Dir,
                flags: 0,
                file_id: None,
            }]
            .into(),
        }
    }

    pub(crate) fn placeholder() -> Self {
        Tree::new(Path::new(""))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    #[inline]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    pub fn root(&self) -> &Node {
        &self.nodes[ROOT as usize]
    }

    /// A bounded, breadth-first view for live rendering. Omitted siblings are
    /// represented by one aggregate so their space remains visible. Preview
    /// node ids are independent of the source tree and must not be used for edits.
    #[must_use]
    pub fn preview(&self, limit: usize) -> Self {
        fn copy_node(node: &Node, parent: NodeId) -> Node {
            Node {
                name: node.name.clone(),
                size: node.size,
                len: node.len,
                mtime: node.mtime,
                files: node.files,
                parent,
                children: Vec::new(),
                kind: node.kind,
                flags: node.flags,
                file_id: node.file_id,
            }
        }
        let mut out = Self { nodes: vec![copy_node(self.root(), NO_NODE)].into() };
        let mut pending = std::collections::VecDeque::from([(ROOT, ROOT)]);
        while let Some((source, dest)) = pending.pop_front() {
            let room = limit.saturating_sub(out.len());
            if room < 2 {
                break;
            }
            let children = &self.node(source).children;
            if children.is_empty() {
                continue;
            }
            let count = children.len().min(room - 1).min(64);
            let mut largest = BinaryHeap::new();
            for &id in children {
                largest.push(Reverse((self.node(id).size, id)));
                if largest.len() > count {
                    largest.pop();
                }
            }
            let mut ids = Vec::new();
            let mut remaining_size = self.node(source).size;
            let mut remaining_files = self.node(source).files;
            for Reverse((_, id)) in largest.into_sorted_vec() {
                let node = self.node(id);
                let new_id = out.len() as NodeId;
                out.nodes.push(copy_node(node, dest));
                ids.push(new_id);
                remaining_size = remaining_size.saturating_sub(node.size);
                remaining_files = remaining_files.saturating_sub(node.files);
                if node.is_dir() {
                    pending.push_back((id, new_id));
                }
            }
            if children.len() > count {
                ids.push(out.len() as NodeId);
                out.nodes.push(Node {
                    name: OsString::from("Other entries (scanning)").into_boxed_os_str(),
                    size: remaining_size,
                    len: remaining_size,
                    files: remaining_files,
                    mtime: i64::MIN,
                    parent: dest,
                    children: Vec::new(),
                    kind: Kind::Other,
                    flags: 0,
                    file_id: None,
                });
            }
            out.nodes[dest as usize].children = ids;
        }
        out
    }

    pub fn root_path(&self) -> &Path {
        Path::new(&*self.nodes[ROOT as usize].name)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id as usize].parent;
        (p != NO_NODE).then_some(p)
    }

    /// Full path of a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let chain = self.chain(id);
        let mut p = PathBuf::from(&*self.nodes[ROOT as usize].name);
        for &c in &chain[1..] {
            p.push(&*self.nodes[c as usize].name);
        }
        p
    }

    /// Path of `id` relative to `base` (which must be an ancestor), for display.
    pub fn relative_path(&self, base: NodeId, id: NodeId) -> PathBuf {
        let chain = self.chain(id);
        let mut p = PathBuf::new();
        let mut on = false;
        for &c in &chain {
            if on {
                p.push(&*self.nodes[c as usize].name);
            }
            if c == base {
                on = true;
            }
        }
        p
    }

    /// Node ids from the root down to (and including) `id`.
    pub fn chain(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        let mut cur = id;
        while cur != NO_NODE {
            v.push(cur);
            cur = self.nodes[cur as usize].parent;
        }
        v.reverse();
        v
    }

    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.nodes[id as usize].parent;
        while cur != NO_NODE {
            d += 1;
            cur = self.nodes[cur as usize].parent;
        }
        d
    }

    /// True if `ancestor` is `id` or one of its ancestors.
    pub fn is_ancestor_or_self(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut cur = id;
        while cur != NO_NODE {
            if cur == ancestor {
                return true;
            }
            cur = self.nodes[cur as usize].parent;
        }
        false
    }

    /// True if the node is still attached to the tree (not deleted or replaced).
    pub fn is_live(&self, id: NodeId) -> bool {
        let Some(mut n) = self.get(id) else { return false };
        let mut cur = id;
        loop {
            if n.has(flags::REMOVED) {
                return false;
            }
            if cur == ROOT {
                return true;
            }
            let p = n.parent;
            if p == NO_NODE {
                return false;
            }
            cur = p;
            n = &self.nodes[p as usize];
        }
    }

    /// Append `entries` as the children of `parent` and add their sizes to
    /// `parent` and all of its ancestors. Returns the id range of the new nodes.
    ///
    /// Multiple batches may be appended while a directory is being scanned.
    pub fn add_children(&mut self, parent: NodeId, entries: Vec<NewEntry>) -> std::ops::Range<NodeId> {
        let start = self.nodes.len() as NodeId;
        let mut size = 0u64;
        let mut files = 0u64;
        self.nodes.reserve(entries.len());
        for e in entries {
            let is_dir = e.kind == Kind::Dir;
            size += e.size;
            files += u64::from(!is_dir);
            self.nodes.push(Node {
                name: e.name.into_boxed_os_str(),
                size: e.size,
                len: e.len,
                mtime: e.mtime,
                files: u64::from(!is_dir),
                parent,
                children: Vec::new(),
                kind: e.kind,
                flags: e.flags,
                file_id: e.file_id,
            });
        }
        let end = self.nodes.len() as NodeId;
        let children = &mut self.nodes[parent as usize].children;
        children.extend(start..end);
        self.add_up(parent, size, files);
        start..end
    }

    fn add_up(&mut self, mut id: NodeId, size: u64, files: u64) {
        while id != NO_NODE {
            let n = &mut self.nodes[id as usize];
            n.size += size;
            n.files += files;
            id = n.parent;
        }
    }

    fn sub_up(&mut self, mut id: NodeId, size: u64, files: u64) {
        while id != NO_NODE {
            let n = &mut self.nodes[id as usize];
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(files);
            id = n.parent;
        }
    }

    fn sort_children_of(&mut self, id: NodeId) {
        let mut ch = std::mem::take(&mut self.nodes[id as usize].children);
        let nodes = &self.nodes;
        ch.sort_unstable_by(|&a, &b| {
            let (na, nb) = (&nodes[a as usize], &nodes[b as usize]);
            nb.size.cmp(&na.size).then_with(|| na.name.cmp(&nb.name))
        });
        self.nodes[id as usize].children = ch;
    }

    /// Sort every directory's children by size, largest first.
    pub fn sort_all(&mut self) {
        for i in 0..self.nodes.len() {
            if self.nodes[i].children.len() > 1 {
                self.sort_children_of(i as NodeId);
            }
        }
    }

    /// After `id`'s size changed, restore size ordering in every list that
    /// contains `id` or one of its ancestors.
    fn resort_upwards(&mut self, mut id: NodeId) {
        while let Some(p) = self.parent(id) {
            self.sort_children_of(p);
            id = p;
        }
    }

    pub(crate) fn resort_from(&mut self, id: NodeId) {
        self.resort_upwards(id);
    }

    /// Remove a node (after it was deleted on disk). Returns false if it was
    /// the root or already removed.
    pub fn remove(&mut self, id: NodeId) -> bool {
        if id == ROOT || id as usize >= self.nodes.len() || self.nodes[id as usize].has(flags::REMOVED) {
            return false;
        }
        let (size, files, parent) = {
            let n = &mut self.nodes[id as usize];
            n.flags |= flags::REMOVED;
            (n.size, n.files, n.parent)
        };
        let kept: Vec<NodeId> = self.nodes[parent as usize].children.iter().copied().filter(|&c| c != id).collect();
        self.nodes[parent as usize].children = kept;
        self.sub_up(parent, size, files);
        self.resort_upwards(parent);
        true
    }

    /// Replace the contents of directory `at` with a freshly scanned tree of
    /// the same directory. `at` keeps its id; its old descendants are detached.
    pub fn graft(&mut self, at: NodeId, sub: Tree) {
        // Detached roots make liveness checks proportional to depth, not to
        // the number of siblings in a potentially enormous directory.
        let old_children = self.node(at).children.clone();
        for child in old_children {
            self.node_mut(child).flags |= flags::REMOVED;
        }
        let offset = self.nodes.len() as NodeId - 1;
        let remap = |i: NodeId| if i == ROOT { at } else { offset + i };
        self.nodes.reserve(sub.len().saturating_sub(1));
        let mut it = sub.nodes.into_nodes();
        let sroot = it.next().expect("scanned tree has a root");
        for mut n in it {
            n.parent = remap(n.parent);
            n.children = n.children.iter().map(|&c| remap(c)).collect();
            self.nodes.push(n);
        }
        let (old_size, old_files, parent) = {
            let n = &mut self.nodes[at as usize];
            let old = (n.size, n.files, n.parent);
            n.children = sroot.children.iter().map(|&c| remap(c)).collect();
            n.size = sroot.size;
            n.files = sroot.files;
            n.mtime = sroot.mtime;
            n.flags = sroot.flags;
            old
        };
        if parent != NO_NODE {
            self.sub_up(parent, old_size, old_files);
            self.add_up(parent, sroot.size, sroot.files);
            self.resort_upwards(at);
        }
    }

    /// The `n` largest non-directory entries at or below `under`, largest first.
    pub fn largest_files(&self, under: NodeId, n: usize) -> Vec<NodeId> {
        if n == 0 {
            return Vec::new();
        }
        let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(n + 1);
        let mut stack = vec![under];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id as usize];
            let floor = if heap.len() == n { heap.peek().map_or(0, |r| r.0.0) } else { 0 };
            if node.size <= floor && heap.len() == n {
                continue; // nothing in here can beat the current top-n
            }
            if node.is_dir() {
                stack.extend(node.children.iter().copied());
            } else if node.size > 0 {
                heap.push(Reverse((node.size, id)));
                if heap.len() > n {
                    heap.pop();
                }
            }
        }
        let mut v: Vec<(u64, NodeId)> = heap.into_iter().map(|Reverse(x)| x).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Number of directories at or below `under`.
    pub fn dir_count(&self, under: NodeId) -> u64 {
        let mut count = 0;
        let mut stack = vec![under];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id as usize];
            if node.is_dir() {
                count += 1;
                stack.extend(node.children.iter().copied());
            }
        }
        count
    }

    /// Find the direct child of `parent` with the given name.
    pub fn child_named(&self, parent: NodeId, name: &OsStr) -> Option<NodeId> {
        self.nodes[parent as usize].children.iter().copied().find(|&c| &*self.nodes[c as usize].name == name)
    }

    /// Resolve an absolute path inside this tree to a node.
    pub fn find_path(&self, path: &Path) -> Option<NodeId> {
        let rel = path.strip_prefix(self.root_path()).ok()?;
        let mut cur = ROOT;
        for comp in rel.components() {
            cur = self.child_named(cur, comp.as_os_str())?;
        }
        Some(cur)
    }

    /// Replace file metadata without changing its identity or copying the tree.
    pub fn update_file(&mut self, id: NodeId, entry: NewEntry) {
        let (parent, old_size) = (self.node(id).parent, self.node(id).size);
        let node = self.node_mut(id);
        node.name = entry.name.into_boxed_os_str();
        node.size = entry.size;
        node.len = entry.len;
        node.mtime = entry.mtime;
        node.flags = entry.flags;
        node.file_id = entry.file_id;
        node.kind = entry.kind;
        self.sub_up(parent, old_size, 0);
        self.add_up(parent, entry.size, 0);
        self.resort_upwards(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: Kind, size: u64) -> NewEntry {
        NewEntry { name: name.into(), kind, size, len: size, mtime: 0, flags: 0, file_id: None }
    }

    /// root/{a/{x:10, y:30}, b:5, c/{z:100}}
    fn sample() -> Tree {
        let mut t = Tree::new(Path::new("/r"));
        let r =
            t.add_children(ROOT, vec![entry("a", Kind::Dir, 0), entry("b", Kind::File, 5), entry("c", Kind::Dir, 0)]);
        let a = r.start;
        let c = r.start + 2;
        t.add_children(a, vec![entry("x", Kind::File, 10), entry("y", Kind::File, 30)]);
        t.add_children(c, vec![entry("z", Kind::File, 100)]);
        t.sort_all();
        t
    }

    #[test]
    fn snapshots_only_copy_pages_touched_by_an_edit() {
        let mut tree = Tree::new(Path::new("/r"));
        tree.add_children(ROOT, (0..10_000).map(|i| entry(&format!("file-{i}"), Kind::File, 1)).collect());
        let snapshot = tree.clone();
        assert!(tree.nodes.pages.iter().zip(&snapshot.nodes.pages).all(|(a, b)| Arc::ptr_eq(a, b)));
        tree.update_file(5000, entry("file-4999", Kind::File, 100));
        let copied = tree.nodes.pages.iter().zip(&snapshot.nodes.pages).filter(|(a, b)| !Arc::ptr_eq(a, b)).count();
        assert_eq!(copied, 2, "only the root and edited-file pages are copied");
        assert_eq!(snapshot.root().size, 10_000);
        assert_eq!(snapshot.node(5000).size, 1);
        assert_eq!(tree.root().size, 10_099);
    }

    #[test]
    fn batches_preserve_children_and_ancestor_totals() {
        let mut tree = Tree::new(Path::new("/r"));
        let dir = tree.add_children(ROOT, vec![entry("dir", Kind::Dir, 0)]).start;
        let first = tree.add_children(dir, vec![entry("a", Kind::File, 10)]).start;
        let second = tree.add_children(dir, vec![entry("b", Kind::File, 20)]).start;
        tree.add_children(dir, Vec::new());
        assert_eq!(tree.node(dir).children, vec![first, second]);
        assert_eq!(tree.root().size, 30);
        assert_eq!(tree.root().files, 2);
        assert_eq!(tree.path(second), PathBuf::from("/r/dir/b"));
    }

    #[test]
    fn preview_bounds_work_and_preserves_space() {
        let mut tree = sample();
        tree.add_children(ROOT, (0..1000).map(|i| entry(&format!("f{i}"), Kind::File, i)).collect());
        for budget in [1, 2, 8, 32, 4096] {
            let preview = tree.preview(budget);
            assert!(preview.len() <= budget);
            assert_eq!(preview.root().size, tree.root().size);
            assert_eq!(preview.root().files, tree.root().files);
            for (id, node) in preview.nodes.iter().enumerate() {
                if !node.children.is_empty() {
                    assert_eq!(node.children.iter().map(|&c| preview.node(c).size).sum::<u64>(), node.size);
                    assert_eq!(node.children.iter().map(|&c| preview.node(c).files).sum::<u64>(), node.files);
                }
                for &child in &node.children {
                    assert_eq!(preview.node(child).parent as usize, id);
                }
            }
        }
        let preview = tree.preview(8);
        assert!(preview.root().children.iter().any(|&id| preview.node(id).name_lossy() == "f999"));
    }

    #[test]
    fn sizes_propagate_and_sort() {
        let t = sample();
        assert_eq!(t.root().size, 145);
        assert_eq!(t.root().files, 4);
        let names: Vec<_> = t.root().children.iter().map(|&c| t.node(c).name_lossy().into_owned()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn paths_and_lookup() {
        let t = sample();
        let y = t.find_path(Path::new("/r/a/y")).unwrap();
        assert_eq!(t.node(y).size, 30);
        assert_eq!(t.path(y), PathBuf::from("/r/a/y"));
        let a = t.find_path(Path::new("/r/a")).unwrap();
        assert_eq!(t.relative_path(a, y), PathBuf::from("y"));
        assert_eq!(t.depth(y), 2);
        assert!(t.is_ancestor_or_self(a, y));
        assert!(!t.is_ancestor_or_self(y, a));
    }

    #[test]
    fn remove_updates_ancestors_and_order() {
        let mut t = sample();
        let z = t.find_path(Path::new("/r/c/z")).unwrap();
        assert!(t.remove(z));
        assert!(!t.remove(z));
        assert!(!t.is_live(z));
        assert_eq!(t.root().size, 45);
        assert_eq!(t.root().files, 3);
        let first = t.root().children[0];
        assert_eq!(t.node(first).name_lossy(), "a");
    }

    #[test]
    fn graft_replaces_subtree() {
        let mut t = sample();
        let a = t.find_path(Path::new("/r/a")).unwrap();
        let old_x = t.find_path(Path::new("/r/a/x")).unwrap();
        let mut sub = Tree::new(Path::new("/r/a"));
        sub.add_children(ROOT, vec![entry("new", Kind::File, 1000)]);
        t.graft(a, sub);
        assert_eq!(t.node(a).size, 1000);
        assert_eq!(t.root().size, 1105);
        assert_eq!(t.root().files, 3);
        assert_eq!(t.root().children[0], a, "a is now the largest child");
        assert!(!t.is_live(old_x));
        let n = t.find_path(Path::new("/r/a/new")).unwrap();
        assert_eq!(t.path(n), PathBuf::from("/r/a/new"));
        assert!(t.is_live(n));
    }

    #[test]
    fn largest_files_top_n() {
        let t = sample();
        let top: Vec<u64> = t.largest_files(ROOT, 2).iter().map(|&i| t.node(i).size).collect();
        assert_eq!(top, [100, 30]);
        assert_eq!(t.largest_files(ROOT, 10).len(), 4);
        assert_eq!(t.dir_count(ROOT), 3);
    }
}
