//! Saved scans and what changed between them.
//!
//! A snapshot keeps every folder's size and file count, and optionally the files at or
//! above a size. Entries are stored parent-first with each parent's index, so a snapshot
//! is a compact tree that reads back without rebuilding paths. Names are stored as UTF-8;
//! the rare name that is not valid Unicode is kept lossily.
use crate::tree::{NodeId, ROOT, Tree, flags};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"CLAWSNP1";
/// Bounds that keep a damaged or hostile file from allocating without limit.
const MAX_ENTRIES: u32 = 200_000_000;
const MAX_TEXT: usize = 64 * 1024;

/// What a snapshot records besides folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Folders only.
    Folders,
    /// Folders, and files of at least `min_size` bytes; 0 keeps every file.
    Files { min_size: u64 },
}

impl Detail {
    fn keeps_file(self, size: u64) -> bool {
        matches!(self, Self::Files { min_size } if size >= min_size)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// Index of the parent entry; the root's is `NO_PARENT`.
    parent: u32,
    name: String,
    dir: bool,
    size: u64,
    files: u64,
}

const NO_PARENT: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub root: PathBuf,
    /// Seconds since the Unix epoch.
    pub taken: i64,
    pub detail: Detail,
    /// A name the user gave it, possibly empty.
    pub label: String,
    entries: Vec<Entry>,
}

impl Snapshot {
    /// Record `tree` at `detail`.
    pub fn capture(tree: &Tree, detail: Detail, taken: i64, label: impl Into<String>) -> Self {
        let mut entries = Vec::new();
        let mut stack = vec![(ROOT, NO_PARENT)];
        while let Some((id, parent)) = stack.pop() {
            let node = tree.node(id);
            if node.has(flags::REMOVED) || (!node.is_dir() && !detail.keeps_file(node.size)) {
                continue;
            }
            let index = entries.len() as u32;
            entries.push(Entry {
                parent,
                name: if id == ROOT { String::new() } else { node.name_lossy().into_owned() },
                dir: node.is_dir(),
                size: node.size,
                files: node.files,
            });
            if node.is_dir() {
                stack.extend(node.children.iter().rev().map(|&child| (child, index)));
            }
        }
        Self { root: tree.root_path().to_owned(), taken, detail, label: label.into(), entries }
    }

    /// Total bytes below the root.
    pub fn size(&self) -> u64 {
        self.entries.first().map_or(0, |root| root.size)
    }

    pub fn files(&self) -> u64 {
        self.entries.first().map_or(0, |root| root.files)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(MAGIC)?;
        out.write_all(&self.taken.to_le_bytes())?;
        match self.detail {
            Detail::Folders => out.write_all(&[0])?,
            Detail::Files { min_size } => {
                out.write_all(&[1])?;
                out.write_all(&min_size.to_le_bytes())?;
            }
        }
        write_text(out, &self.root.to_string_lossy())?;
        write_text(out, &self.label)?;
        out.write_all(&u32::try_from(self.entries.len()).map_err(|_| invalid())?.to_le_bytes())?;
        for entry in &self.entries {
            out.write_all(&entry.parent.to_le_bytes())?;
            out.write_all(&[u8::from(entry.dir)])?;
            out.write_all(&entry.size.to_le_bytes())?;
            out.write_all(&entry.files.to_le_bytes())?;
            write_text(out, &entry.name)?;
        }
        Ok(())
    }

    /// Read a snapshot written by [`Self::write`], rejecting anything malformed.
    pub fn read(input: &mut impl Read) -> io::Result<Self> {
        let (Summary { root, taken, detail, label, .. }, count) = Summary::read_header(input)?;
        let mut entries = Vec::with_capacity(count.min(1 << 20) as usize);
        for index in 0..count {
            let parent = u32::from_le_bytes(array(input)?);
            let dir = match array::<1>(input)? {
                [0] => false,
                [1] => true,
                _ => return Err(invalid()),
            };
            let size = u64::from_le_bytes(array(input)?);
            let files = u64::from_le_bytes(array(input)?);
            let name = read_text(input)?;
            // Parents come first and are folders; only the first entry is the root.
            let valid = if index == 0 {
                parent == NO_PARENT && dir
            } else {
                parent < index && entries.get(parent as usize).is_some_and(|p: &Entry| p.dir) && !name.is_empty()
            };
            if !valid {
                return Err(invalid());
            }
            entries.push(Entry { parent, name, dir, size, files });
        }
        Ok(Self { root, taken, detail, label, entries })
    }

    pub fn summary(&self) -> Summary {
        Summary {
            root: self.root.clone(),
            taken: self.taken,
            detail: self.detail,
            label: self.label.clone(),
            size: self.size(),
            files: self.files(),
        }
    }

    /// Each entry's children, as index ranges into one list.
    fn children(&self) -> (Vec<u32>, Vec<u32>) {
        let mut offsets = vec![0u32; self.entries.len() + 1];
        for entry in &self.entries[1..] {
            offsets[entry.parent as usize + 1] += 1;
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let mut next = offsets.clone();
        let mut list = vec![0u32; self.entries.len().saturating_sub(1)];
        for (index, entry) in self.entries.iter().enumerate().skip(1) {
            let slot = &mut next[entry.parent as usize];
            list[*slot as usize] = index as u32;
            *slot += 1;
        }
        (offsets, list)
    }
}

/// A saved snapshot's description, read without its entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub root: PathBuf,
    pub taken: i64,
    pub detail: Detail,
    pub label: String,
    pub size: u64,
    pub files: u64,
}

impl Summary {
    /// Read only the header and the root entry.
    pub fn peek(input: &mut impl Read) -> io::Result<Self> {
        let (mut summary, _) = Self::read_header(input)?;
        let _parent: [u8; 4] = array(input)?;
        let _dir: [u8; 1] = array(input)?;
        summary.size = u64::from_le_bytes(array(input)?);
        summary.files = u64::from_le_bytes(array(input)?);
        Ok(summary)
    }

    /// The header, and the number of entries that follow it.
    fn read_header(input: &mut impl Read) -> io::Result<(Self, u32)> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid());
        }
        let taken = i64::from_le_bytes(array(input)?);
        let detail = match array::<1>(input)? {
            [0] => Detail::Folders,
            [1] => Detail::Files { min_size: u64::from_le_bytes(array(input)?) },
            _ => return Err(invalid()),
        };
        let root = PathBuf::from(read_text(input)?);
        let label = read_text(input)?;
        let count = u32::from_le_bytes(array(input)?);
        if count == 0 || count > MAX_ENTRIES {
            return Err(invalid());
        }
        Ok((Self { root, taken, detail, label, size: 0, files: 0 }, count))
    }
}

/// How an entry differs between two snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Grew,
    Shrank,
    /// Not in the older snapshot (for files: or below the size it recorded).
    Added,
    /// Not in the newer snapshot (for files: or below the size it recorded).
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Relative to the snapshots' root.
    pub path: PathBuf,
    pub dir: bool,
    pub before: u64,
    pub after: u64,
    pub kind: ChangeKind,
}

impl Change {
    /// Bytes gained (positive) or freed (negative).
    pub fn delta(&self) -> i128 {
        i128::from(self.after) - i128::from(self.before)
    }
}

/// What changed from `old` to `new`, largest change first. Files are compared only when
/// both snapshots recorded files, and only at the larger of their size thresholds.
pub fn compare(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let threshold = match (old.detail, new.detail) {
        (Detail::Files { min_size: a }, Detail::Files { min_size: b }) => Some(a.max(b)),
        _ => None,
    };
    let counted = |entry: &Entry| entry.dir || threshold.is_some_and(|min| entry.size >= min);
    let (old_offsets, old_children) = old.children();
    let (new_offsets, new_children) = new.children();
    let kids = |offsets: &[u32], list: &[u32], index: u32| {
        list[offsets[index as usize] as usize..offsets[index as usize + 1] as usize].to_vec()
    };
    let mut changes = Vec::new();
    if old.is_empty() || new.is_empty() {
        return changes;
    }
    // (old index, new index, relative path) of matched folders still to compare.
    let mut pending = vec![(0u32, 0u32, PathBuf::new())];
    while let Some((before, after, path)) = pending.pop() {
        let (a, b) = (&old.entries[before as usize], &new.entries[after as usize]);
        if a.size != b.size && !path.as_os_str().is_empty() {
            changes.push(change(&path, true, a.size, b.size));
        }
        let mut older: HashMap<(&str, bool), u32> = HashMap::new();
        for child in kids(&old_offsets, &old_children, before) {
            older.insert((old.entries[child as usize].name.as_str(), old.entries[child as usize].dir), child);
        }
        for child in kids(&new_offsets, &new_children, after) {
            let entry = &new.entries[child as usize];
            let child_path = path.join(&entry.name);
            match older.remove(&(entry.name.as_str(), entry.dir)) {
                Some(previous) if entry.dir => pending.push((previous, child, child_path)),
                Some(previous) => {
                    let previous = &old.entries[previous as usize];
                    if (counted(previous) || counted(entry)) && previous.size != entry.size {
                        changes.push(change(&child_path, false, previous.size, entry.size));
                    }
                }
                None if counted(entry) => changes.push(change(&child_path, entry.dir, 0, entry.size)),
                None => {}
            }
        }
        for ((name, dir), previous) in older {
            let entry = &old.entries[previous as usize];
            if counted(entry) {
                changes.push(change(&path.join(name), dir, entry.size, 0));
            }
        }
    }
    changes.sort_by(|a, b| b.delta().unsigned_abs().cmp(&a.delta().unsigned_abs()).then_with(|| a.path.cmp(&b.path)));
    changes
}

fn change(path: &Path, dir: bool, before: u64, after: u64) -> Change {
    let kind = match (before, after) {
        (0, _) => ChangeKind::Added,
        (_, 0) => ChangeKind::Removed,
        (before, after) if after > before => ChangeKind::Grew,
        _ => ChangeKind::Shrank,
    };
    Change { path: path.to_owned(), dir, before, after, kind }
}

/// The node for a change's path in `tree`, if it is still there.
pub fn locate(tree: &Tree, path: &Path) -> Option<NodeId> {
    let mut node = ROOT;
    for component in path {
        node = tree.child_named(node, component)?;
    }
    Some(node)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "not a valid Clawback snapshot")
}

fn array<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_text(out: &mut impl Write, text: &str) -> io::Result<()> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_TEXT {
        return Err(invalid());
    }
    out.write_all(&(bytes.len() as u16).to_le_bytes())?;
    out.write_all(bytes)
}

fn read_text(input: &mut impl Read) -> io::Result<String> {
    let len = usize::from(u16::from_le_bytes(array(input)?));
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::tree::NewEntry;

    fn tree(files: &[(&str, &str, u64)]) -> Tree {
        let mut tree = Tree::new(Path::new("/r"));
        let dirs = tree.add_children(ROOT, vec![NewEntry::dir("a"), NewEntry::dir("b")]);
        for &(dir, name, size) in files {
            let parent = if dir == "a" { dirs.start } else { dirs.start + 1 };
            tree.add_children(parent, vec![NewEntry::file(name, size)]);
        }
        tree.sort_all();
        tree
    }

    #[test]
    fn snapshots_round_trip_and_keep_only_requested_files() {
        let tree = tree(&[("a", "big", 100), ("a", "small", 5), ("b", "c", 50)]);
        let snapshot = Snapshot::capture(&tree, Detail::Files { min_size: 50 }, 7, "before");
        assert_eq!(snapshot.len(), 5); // root, a, b, big, c
        let mut bytes = Vec::new();
        snapshot.write(&mut bytes).unwrap();
        assert_eq!(Snapshot::read(&mut bytes.as_slice()).unwrap(), snapshot);
        assert_eq!(Summary::peek(&mut bytes.as_slice()).unwrap(), snapshot.summary());
        assert_eq!(Snapshot::capture(&tree, Detail::Folders, 7, "").len(), 3);
        assert!(Snapshot::read(&mut &bytes[..bytes.len() - 1]).is_err());
        bytes[8 + 8 + 1 + 8 + 2 + 2 + 2 + 6 + 4] ^= 0xff; // the root's parent index
        assert!(Snapshot::read(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn comparison_reports_growth_and_file_changes_at_the_shared_threshold() {
        let old = tree(&[("a", "big", 100), ("a", "small", 5), ("b", "gone", 60)]);
        let new = tree(&[("a", "big", 300), ("a", "small", 9), ("b", "fresh", 70)]);
        let before = Snapshot::capture(&old, Detail::Files { min_size: 50 }, 1, "");
        let after = Snapshot::capture(&new, Detail::Files { min_size: 0 }, 2, "");
        let changes = compare(&before, &after);
        let summary: Vec<_> = changes.iter().map(|c| (c.path.to_string_lossy().replace('\\', "/"), c.kind)).collect();
        assert_eq!(
            summary,
            [
                ("a".to_owned(), ChangeKind::Grew),
                ("a/big".to_owned(), ChangeKind::Grew),
                ("b/fresh".to_owned(), ChangeKind::Added),
                ("b/gone".to_owned(), ChangeKind::Removed),
                ("b".to_owned(), ChangeKind::Grew),
            ]
        );
        assert_eq!(changes[0].delta(), 204);
        let folders = compare(&Snapshot::capture(&old, Detail::Folders, 1, ""), &after);
        assert!(folders.iter().all(|c| c.dir));
        let found = locate(&new, Path::new("a/big")).unwrap();
        assert_eq!(new.node(found).size, 300);
        assert!(locate(&new, Path::new("a/missing")).is_none());
    }
}
