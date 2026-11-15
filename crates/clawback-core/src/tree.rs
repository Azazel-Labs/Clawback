//! Arena-backed size tree.
//!
//! Every file and directory is a [`Node`] in a paged arena, addressed by a
//! `u32` [`NodeId`]. Snapshots share unchanged pages. Directory sizes are the sum of everything below them and
//! are kept up to date as the scanner adds entries, so the tree can be drawn
//! while a scan is still running.
//!
//! A node owns no heap memory: its name lives in its page's name buffer and a directory's
//! children in its page's child lists, so a page is a handful of allocations however many
//! entries it holds. Read them with [`Tree::name`] and [`Tree::children`].

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PAGE_SIZE: usize = 256;
/// [`Node::list`] of a node without a child list.
const NO_LIST: u16 = u16::MAX;
/// [`Node::identity`] of a node without a [`FileId`].
const NO_IDENTITY: [u64; 2] = [u64::MAX; 2];

/// Up to [`PAGE_SIZE`] nodes with their names and child lists.
#[derive(Clone, Debug, Default)]
struct Page {
    nodes: Vec<Node>,
    /// The nodes' names back to back. A rename appends; compaction drops what it replaced.
    names: Vec<u8>,
    /// Children of this page's directories, in display order.
    lists: Vec<Vec<NodeId>>,
}

/// Snapshots share unchanged pages; live edits copy only touched pages.
#[derive(Clone, Debug, Default)]
struct Nodes {
    pages: Vec<Arc<Page>>,
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
    fn push(&mut self, mut node: Node, name: &OsStr) {
        if self.len.is_multiple_of(PAGE_SIZE) {
            self.pages.push(Arc::new(Page { nodes: Vec::with_capacity(PAGE_SIZE), ..Page::default() }));
        }
        let page = Arc::make_mut(self.pages.last_mut().expect("node page"));
        node.list = NO_LIST;
        Self::store_name(page, &mut node, name);
        page.nodes.push(node);
        if page.nodes.len() == PAGE_SIZE {
            page.names.shrink_to_fit();
        }
        self.len += 1;
    }
    fn store_name(page: &mut Page, node: &mut Node, name: &OsStr) {
        let bytes = name.as_encoded_bytes();
        node.name_at = u32::try_from(page.names.len()).expect("page names fit in u32");
        node.name_len = u32::try_from(bytes.len()).expect("name fits in u32");
        page.names.extend_from_slice(bytes);
    }
    fn get(&self, index: usize) -> Option<&Node> {
        self.pages.get(index / PAGE_SIZE)?.nodes.get(index % PAGE_SIZE)
    }
    fn name(&self, index: usize) -> &OsStr {
        let page = &self.pages[index / PAGE_SIZE];
        let node = &page.nodes[index % PAGE_SIZE];
        let bytes = &page.names[node.name_at as usize..][..node.name_len as usize];
        // SAFETY: `store_name` copied exactly these bytes from `OsStr::as_encoded_bytes` of a
        // whole `OsStr` in this process.
        unsafe { OsStr::from_encoded_bytes_unchecked(bytes) }
    }
    fn rename(&mut self, index: usize, name: &OsStr) {
        if self.name(index) != name {
            let page = Arc::make_mut(&mut self.pages[index / PAGE_SIZE]);
            let mut node = page.nodes[index % PAGE_SIZE];
            Self::store_name(page, &mut node, name);
            page.nodes[index % PAGE_SIZE] = node;
        }
    }
    fn children(&self, index: usize) -> &[NodeId] {
        let page = &self.pages[index / PAGE_SIZE];
        match page.nodes[index % PAGE_SIZE].list {
            NO_LIST => &[],
            list => &page.lists[list as usize],
        }
    }
    /// The child list of `index`, created if it has none.
    fn children_mut(&mut self, index: usize) -> &mut Vec<NodeId> {
        let page = Arc::make_mut(&mut self.pages[index / PAGE_SIZE]);
        let node = &mut page.nodes[index % PAGE_SIZE];
        if node.list == NO_LIST {
            node.list = u16::try_from(page.lists.len()).expect("at most one list per node");
            page.lists.push(Vec::new());
        }
        &mut page.lists[node.list as usize]
    }
    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &Node> {
        self.pages.iter().flat_map(|page| page.nodes.iter())
    }
}

impl std::ops::Index<usize> for Nodes {
    type Output = Node;
    fn index(&self, index: usize) -> &Node {
        &self.pages[index / PAGE_SIZE].nodes[index % PAGE_SIZE]
    }
}

impl std::ops::IndexMut<usize> for Nodes {
    fn index_mut(&mut self, index: usize) -> &mut Node {
        &mut Arc::make_mut(&mut self.pages[index / PAGE_SIZE]).nodes[index % PAGE_SIZE]
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

impl From<std::fs::FileType> for Kind {
    /// Symlinks first: they are never followed.
    fn from(ft: std::fs::FileType) -> Self {
        if ft.is_symlink() {
            Kind::Symlink
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File
        } else {
            Kind::Other
        }
    }
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
    /// A file with several hard links, whose other names may share its data. Only these
    /// are indexed for live link accounting.
    pub const LINKED: u8 = 1 << 6;
}

/// One file or directory. The [`Tree`] stores its name and children: see [`Tree::name`]
/// and [`Tree::children`].
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Size used for layout: bytes allocated on disk (or the length when
    /// scanning apparent sizes). For directories, everything below it.
    pub size: u64,
    /// Actual file length in bytes (for directories this equals `size`).
    pub len: u64,
    /// Last modification time, seconds since the Unix epoch (`i64::MIN` if unknown).
    pub mtime: i64,
    /// Number of non-directory entries at or below this node.
    pub files: u64,
    /// [`FileId`] as an array, which packs without an `Option` tag; [`NO_IDENTITY`] if none.
    identity: [u64; 2],
    pub parent: NodeId,
    /// Where the name's bytes are in the page's name buffer.
    name_at: u32,
    name_len: u32,
    /// Index of the child list in the page, or [`NO_LIST`].
    list: u16,
    pub kind: Kind,
    pub flags: u8,
}

const _: () = assert!(size_of::<Node>() <= 64, "nodes are the bulk of a scan's memory");

impl Node {
    fn new(kind: Kind, parent: NodeId) -> Self {
        Node {
            size: 0,
            len: 0,
            mtime: i64::MIN,
            files: 0,
            identity: NO_IDENTITY,
            parent,
            name_at: 0,
            name_len: 0,
            list: NO_LIST,
            kind,
            flags: 0,
        }
    }
    fn from_entry(e: &NewEntry, parent: NodeId) -> Self {
        let mut node = Node::new(e.kind, parent);
        node.size = e.size;
        node.len = e.len;
        node.mtime = e.mtime;
        node.files = u64::from(e.kind != Kind::Dir);
        node.flags = e.flags;
        node.set_file_id(e.file_id);
        node
    }
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
    #[inline]
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
    /// The byte count SpaceMonger shows on labels: file length, or folder total.
    pub fn display_len(&self) -> u64 {
        if self.is_dir() { self.size } else { self.len }
    }
    /// Volume and file identity, shared by hard links to the same file.
    #[inline]
    pub fn file_id(&self) -> Option<FileId> {
        (self.identity != NO_IDENTITY).then_some((self.identity[0], self.identity[1]))
    }
    pub fn set_file_id(&mut self, id: Option<FileId>) {
        self.identity = id.map_or(NO_IDENTITY, |(volume, file)| [volume, file]);
    }
}

/// A new entry handed to [`Tree::add_children`].
#[derive(Clone, Debug)]
pub struct NewEntry {
    pub name: OsString,
    pub kind: Kind,
    pub size: u64,
    pub len: u64,
    pub mtime: i64,
    pub flags: u8,
    pub file_id: Option<FileId>,
}

impl NewEntry {
    /// A plain file whose allocated and logical sizes are both `size`.
    pub fn file(name: impl Into<OsString>, size: u64) -> Self {
        NewEntry { name: name.into(), kind: Kind::File, size, len: size, mtime: 0, flags: 0, file_id: None }
    }

    /// A plain directory; its size is the total of what is later added below it.
    pub fn dir(name: impl Into<OsString>) -> Self {
        NewEntry { name: name.into(), kind: Kind::Dir, size: 0, len: 0, mtime: 0, flags: 0, file_id: None }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Nodes,
}

impl Tree {
    /// A tree containing only a root directory for `root`.
    pub fn new(root: &Path) -> Self {
        let mut nodes = Nodes::default();
        nodes.push(Node::new(Kind::Dir, NO_NODE), root.as_os_str());
        Tree { nodes }
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

    /// File name of `id`. For the root this is the full root path.
    #[inline]
    pub fn name(&self, id: NodeId) -> &OsStr {
        self.nodes.name(id as usize)
    }

    pub fn name_lossy(&self, id: NodeId) -> Cow<'_, str> {
        self.name(id).to_string_lossy()
    }

    /// Children of `id`, largest first once sorted; empty for files.
    #[inline]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes.children(id as usize)
    }

    /// A bounded, breadth-first view for live rendering. Omitted siblings are
    /// represented by one aggregate so their space remains visible. Preview
    /// node ids are independent of the source tree and must not be used for edits.
    #[must_use]
    pub fn preview(&self, limit: usize) -> Self {
        let mut out = Self { nodes: Nodes::default() };
        out.push_copy(self, ROOT, NO_NODE);
        let mut pending = std::collections::VecDeque::from([(ROOT, ROOT)]);
        while let Some((source, dest)) = pending.pop_front() {
            let room = limit.saturating_sub(out.len());
            if room < 2 {
                break;
            }
            let children = self.children(source);
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
                out.push_copy(self, id, dest);
                ids.push(new_id);
                remaining_size = remaining_size.saturating_sub(node.size);
                remaining_files = remaining_files.saturating_sub(node.files);
                if node.is_dir() {
                    pending.push_back((id, new_id));
                }
            }
            if children.len() > count {
                ids.push(out.len() as NodeId);
                let mut other = Node::new(Kind::Other, dest);
                other.size = remaining_size;
                other.len = remaining_size;
                other.files = remaining_files;
                out.nodes.push(other, OsStr::new("Other entries (scanning)"));
            }
            *out.nodes.children_mut(dest as usize) = ids;
        }
        out
    }

    pub fn root_path(&self) -> &Path {
        Path::new(self.name(ROOT))
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id as usize].parent;
        (p != NO_NODE).then_some(p)
    }

    /// Full path of a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let chain = self.chain(id);
        let mut p = PathBuf::from(self.name(ROOT));
        for &c in &chain[1..] {
            p.push(self.name(c));
        }
        p
    }

    /// Path of `id` relative to `base` (which must be an ancestor), for display.
    pub fn relative_path(&self, base: NodeId, id: NodeId) -> PathBuf {
        self.chain(id).into_iter().skip_while(|&c| c != base).skip(1).map(|c| self.name(c)).collect()
    }

    /// `id`, then its parent, and so on up to the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), |&c| self.parent(c))
    }

    /// `id` and every node below it, depth first.
    pub fn descendants(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack = vec![id];
        std::iter::from_fn(move || {
            let id = stack.pop()?;
            stack.extend_from_slice(self.children(id));
            Some(id)
        })
    }

    /// Node ids from the root down to (and including) `id`.
    pub fn chain(&self, id: NodeId) -> Vec<NodeId> {
        let mut v: Vec<_> = self.ancestors(id).collect();
        v.reverse();
        v
    }

    pub fn depth(&self, id: NodeId) -> usize {
        self.ancestors(id).count() - 1
    }

    /// True if the node is still attached to the tree (not deleted or replaced).
    pub fn is_live(&self, id: NodeId) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        for cur in self.ancestors(id) {
            if self.nodes[cur as usize].has(flags::REMOVED) {
                return false;
            }
            if cur == ROOT {
                return true;
            }
        }
        false
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
            let node = Node::from_entry(&e, parent);
            size += node.size;
            files += node.files;
            self.nodes.push(node, &e.name);
        }
        let end = self.nodes.len() as NodeId;
        if end > start {
            self.nodes.children_mut(parent as usize).extend(start..end);
        }
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

    /// Largest first, then by name.
    fn sort_ids(&self, ids: &mut [NodeId]) {
        ids.sort_unstable_by(|&a, &b| {
            self.node(b).size.cmp(&self.node(a).size).then_with(|| self.name(a).cmp(self.name(b)))
        });
    }

    fn sort_children_of(&mut self, id: NodeId) {
        if self.children(id).len() < 2 {
            return;
        }
        let mut ch = std::mem::take(self.nodes.children_mut(id as usize));
        self.sort_ids(&mut ch);
        *self.nodes.children_mut(id as usize) = ch;
    }

    /// Sort every directory's children by size, largest first, and release spare capacity.
    pub fn sort_all(&mut self) {
        for p in 0..self.nodes.pages.len() {
            for l in 0..self.nodes.pages[p].lists.len() {
                let list = &self.nodes.pages[p].lists[l];
                if list.len() < 2 && list.capacity() == list.len() {
                    continue;
                }
                let mut list = std::mem::take(&mut Arc::make_mut(&mut self.nodes.pages[p]).lists[l]);
                self.sort_ids(&mut list);
                list.shrink_to_fit();
                Arc::make_mut(&mut self.nodes.pages[p]).lists[l] = list;
            }
        }
    }

    /// After `id`'s size changed, restore size ordering in every list that
    /// contains `id` or one of its ancestors.
    pub(crate) fn resort_upwards(&mut self, mut id: NodeId) {
        while let Some(p) = self.parent(id) {
            self.sort_children_of(p);
            id = p;
        }
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
        self.nodes.children_mut(parent as usize).retain(|&c| c != id);
        self.sub_up(parent, size, files);
        self.resort_upwards(parent);
        true
    }

    /// Append `source`'s node `id` and its name under `parent`, without children.
    fn push_copy(&mut self, source: &Tree, id: NodeId, parent: NodeId) {
        self.nodes.push(Node { parent, ..*source.node(id) }, source.name(id));
    }

    /// The same tree without the detached nodes and replaced names that edits and grafts
    /// leave behind. Node ids change; children keep their order.
    #[must_use]
    pub fn compacted(self) -> Self {
        // New ids in breadth-first order, so each directory's children stay contiguous.
        let mut out = Tree { nodes: Nodes::default() };
        out.nodes.reserve(self.len());
        out.push_copy(&self, ROOT, NO_NODE);
        let mut order = vec![ROOT];
        let mut next = 0;
        while let Some(&id) = order.get(next) {
            let new_parent = next as NodeId;
            next += 1;
            let children = self.children(id);
            if children.is_empty() {
                continue;
            }
            let start = out.len() as NodeId;
            for &child in children {
                out.push_copy(&self, child, new_parent);
                order.push(child);
            }
            let end = out.len() as NodeId;
            let list = out.nodes.children_mut(new_parent as usize);
            list.reserve_exact(children.len());
            list.extend(start..end);
        }
        out
    }

    /// Replace the contents of directory `at` with a freshly scanned tree of
    /// the same directory. `at` keeps its id; its old descendants are detached.
    pub fn graft(&mut self, at: NodeId, sub: Tree) {
        // Detached roots make liveness checks proportional to depth, not to
        // the number of siblings in a potentially enormous directory.
        let old_children = self.children(at).to_vec();
        for child in old_children {
            self.node_mut(child).flags |= flags::REMOVED;
        }
        let offset = self.nodes.len() as NodeId - 1;
        let remap = |i: NodeId| if i == ROOT { at } else { offset + i };
        self.nodes.reserve(sub.len().saturating_sub(1));
        for id in 1..sub.len() as NodeId {
            self.push_copy(&sub, id, remap(sub.node(id).parent));
        }
        for id in 0..sub.len() as NodeId {
            let children = sub.children(id);
            if id == ROOT || !children.is_empty() {
                let list = self.nodes.children_mut(remap(id) as usize);
                list.clear();
                list.extend(children.iter().map(|&c| remap(c)));
            }
        }
        let sroot = *sub.root();
        drop(sub);
        let (old_size, old_files, parent) = {
            let n = &mut self.nodes[at as usize];
            let old = (n.size, n.files, n.parent);
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
            if heap.len() == n && heap.peek().is_some_and(|r| node.size <= r.0.0) {
                continue; // nothing in here can beat the current top-n
            }
            if node.is_dir() {
                stack.extend_from_slice(self.children(id));
            } else if node.size > 0 {
                heap.push(Reverse((node.size, id)));
                if heap.len() > n {
                    heap.pop();
                }
            }
        }
        heap.into_sorted_vec().into_iter().map(|Reverse((_, id))| id).collect()
    }

    /// Number of directories at or below `under`.
    pub fn dir_count(&self, under: NodeId) -> u64 {
        self.descendants(under).filter(|&id| self.nodes[id as usize].is_dir()).count() as u64
    }

    /// Find the direct child of `parent` with the given name.
    pub fn child_named(&self, parent: NodeId, name: &OsStr) -> Option<NodeId> {
        self.children(parent).iter().copied().find(|&c| self.name(c) == name)
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
        let NewEntry { name, kind, size, len, mtime, flags, file_id } = entry;
        self.nodes.rename(id as usize, &name);
        let node = self.node_mut(id);
        node.size = size;
        node.len = len;
        node.mtime = mtime;
        node.flags = flags;
        node.set_file_id(file_id);
        node.kind = kind;
        self.sub_up(parent, old_size, 0);
        self.add_up(parent, size, 0);
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
        assert_eq!(tree.children(dir), [first, second]);
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
                let children = preview.children(id as NodeId);
                if !children.is_empty() {
                    assert_eq!(children.iter().map(|&c| preview.node(c).size).sum::<u64>(), node.size);
                    assert_eq!(children.iter().map(|&c| preview.node(c).files).sum::<u64>(), node.files);
                }
                for &child in children {
                    assert_eq!(preview.node(child).parent as usize, id);
                }
            }
        }
        let preview = tree.preview(8);
        assert!(preview.children(ROOT).iter().any(|&id| preview.name_lossy(id) == "f999"));
    }

    #[test]
    fn sizes_propagate_and_sort() {
        let t = sample();
        assert_eq!(t.root().size, 145);
        assert_eq!(t.root().files, 4);
        let names: Vec<_> = t.children(ROOT).iter().map(|&c| t.name_lossy(c).into_owned()).collect();
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
        assert_eq!(t.depth(ROOT), 0);
        assert_eq!(t.chain(y), [ROOT, a, y]);
        assert_eq!(t.ancestors(y).collect::<Vec<_>>(), [y, a, ROOT]);
        assert_eq!(t.descendants(a).count(), 3);
        assert_eq!(t.relative_path(ROOT, y), PathBuf::from("a/y"));
        assert!(t.relative_path(y, a).as_os_str().is_empty());
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
        let first = t.children(ROOT)[0];
        assert_eq!(t.name_lossy(first), "a");
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
        assert_eq!(t.children(ROOT)[0], a, "a is now the largest child");
        assert!(!t.is_live(old_x));
        let n = t.find_path(Path::new("/r/a/new")).unwrap();
        assert_eq!(t.path(n), PathBuf::from("/r/a/new"));
        assert!(t.is_live(n));
    }

    #[test]
    fn compaction_drops_detached_nodes_and_keeps_paths() {
        let mut t = sample();
        let a = t.find_path(Path::new("/r/a")).unwrap();
        let mut sub = Tree::new(Path::new("/r/a"));
        sub.add_children(ROOT, vec![entry("new", Kind::File, 1000)]);
        t.graft(a, sub);
        let removed = t.children(ROOT)[1];
        t.remove(removed);
        let (size, files, before) = (t.root().size, t.root().files, t.len());
        let compact = t.compacted();
        assert!(compact.len() < before);
        assert_eq!(compact.descendants(ROOT).count(), compact.len());
        assert_eq!((compact.root().size, compact.root().files), (size, files));
        let n = compact.find_path(Path::new("/r/a/new")).unwrap();
        assert_eq!(compact.path(n), PathBuf::from("/r/a/new"));
        assert!(
            compact
                .descendants(ROOT)
                .skip(1)
                .all(|id| compact.parent(id).is_some_and(|p| compact.children(p).contains(&id)))
        );
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
