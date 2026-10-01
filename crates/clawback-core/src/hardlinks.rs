//! File identity accounting, independent of paths and notification ordering.
use crate::tree::{FileId, NewEntry, NodeId, ROOT, Tree, flags};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct LinkIndex {
    // Ordinary files need just one node ID. Allocate alias vectors only for
    // actual hard-link groups, not for every file on the drive.
    owners: HashMap<FileId, NodeId>,
    aliases: HashMap<FileId, Vec<NodeId>>,
}

impl LinkIndex {
    pub fn new(tree: &Tree) -> Self {
        let mut index = Self::default();
        for id in tree.descendants(ROOT) {
            if !tree.node(id).is_dir() {
                index.insert(tree, id);
            }
        }
        index
    }

    fn insert(&mut self, tree: &Tree, id: NodeId) {
        let node = tree.node(id);
        let Some(key) = node.file_id else { return };
        if let Some(owner) = self.owners.get_mut(&key) {
            if *owner == id {
                return;
            }
            let aliases = self.aliases.entry(key).or_default();
            if aliases.contains(&id) {
                return;
            }
            // Initial parallel scans may enumerate an uncounted alias first.
            if tree.node(*owner).has(flags::HARDLINK_DUP) && !node.has(flags::HARDLINK_DUP) {
                aliases.push(*owner);
                *owner = id;
            } else {
                aliases.push(id);
            }
        } else {
            self.owners.insert(key, id);
        }
    }

    /// Detach before deleting/replacing the node. Move its counted bytes to a
    /// surviving alias first; subtracting the old node then keeps totals exact.
    pub fn detach(&mut self, tree: &mut Tree, id: NodeId) {
        let Some(key) = tree.node(id).file_id else { return };
        let Some(&owner) = self.owners.get(&key) else { return };
        if owner == id {
            if let Some(next) = self.aliases.get_mut(&key).and_then(Vec::pop) {
                self.owners.insert(key, next);
                let old = tree.node(id);
                let entry = NewEntry {
                    name: tree.node(next).name.clone().into(),
                    kind: old.kind,
                    size: old.size,
                    len: old.len,
                    mtime: old.mtime,
                    flags: tree.node(next).flags & !flags::HARDLINK_DUP,
                    file_id: Some(key),
                };
                tree.update_file(next, entry);
            } else {
                self.owners.remove(&key);
            }
        } else if let Some(aliases) = self.aliases.get_mut(&key) {
            aliases.retain(|&alias| alias != id);
        }
        if self.aliases.get(&key).is_some_and(Vec::is_empty) {
            self.aliases.remove(&key);
        }
    }

    /// Apply real metadata (including allocated size) to every known alias.
    /// Only one alias contributes bytes; all retain the real length and date.
    pub fn update(&mut self, tree: &mut Tree, id: NodeId, entry: &NewEntry) -> bool {
        let changed_identity = tree.node(id).file_id != entry.file_id;
        if changed_identity {
            self.detach(tree, id);
            tree.node_mut(id).file_id = entry.file_id;
        }
        self.insert(tree, id);
        let owner = entry.file_id.and_then(|key| self.owners.get(&key).copied()).unwrap_or(id);
        let mut changed = changed_identity;
        changed |= apply(tree, owner, entry, false);
        if let Some(aliases) = entry.file_id.and_then(|key| self.aliases.get(&key)) {
            for &alias in aliases {
                changed |= apply(tree, alias, entry, true);
            }
        }
        changed
    }
}

fn apply(tree: &mut Tree, id: NodeId, metadata: &NewEntry, duplicate: bool) -> bool {
    let node = tree.node(id);
    let size = if duplicate { 0 } else { metadata.size };
    let flags = (node.flags & !flags::HARDLINK_DUP) | if duplicate { flags::HARDLINK_DUP } else { 0 };
    if (node.size, node.len, node.mtime, node.flags, node.file_id)
        == (size, metadata.len, metadata.mtime, flags, metadata.file_id)
    {
        return false;
    }
    let entry = NewEntry {
        name: node.name.clone().into(),
        kind: metadata.kind,
        size,
        len: metadata.len,
        mtime: metadata.mtime,
        flags,
        file_id: metadata.file_id,
    };
    tree.update_file(id, entry);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;
    use std::path::Path;

    fn entry(name: &str, key: FileId, size: u64) -> NewEntry {
        NewEntry { name: name.into(), kind: Kind::File, size, len: size, mtime: 1, flags: 0, file_id: Some(key) }
    }

    #[test]
    fn edits_and_owner_deletion_touch_only_the_link_group() {
        let mut tree = Tree::new(Path::new("/root"));
        let a = tree.add_children(ROOT, vec![entry("a", (1, 10), 100)]).start;
        let mut duplicate = entry("alias", (1, 10), 0);
        duplicate.len = 100;
        duplicate.flags = flags::HARDLINK_DUP;
        let b = tree.add_children(ROOT, vec![duplicate]).start;
        tree.add_children(ROOT, vec![entry("other-device", (2, 10), 999)]);
        let original = tree.clone();
        let mut index = LinkIndex::new(&tree);
        assert!(index.update(&mut tree, b, &entry("alias", (1, 10), 200)));
        assert_eq!(tree.root().size, 1199);
        assert_eq!((tree.node(a).size, tree.node(b).size, tree.node(b).len), (200, 0, 200));
        index.detach(&mut tree, a);
        tree.remove(a);
        assert_eq!(tree.node(b).size, 200);
        assert!(!tree.node(b).has(flags::HARDLINK_DUP));
        assert_eq!(tree.root().size, 1199);
        assert_eq!(original.root().size, 1099);
        index.detach(&mut tree, b);
        tree.remove(b);
        assert_eq!(tree.root().size, 999);
        assert!(!index.owners.contains_key(&(1, 10)));
    }

    #[test]
    fn atomic_replacement_splits_links_and_new_aliases_deduplicate() {
        let mut tree = Tree::new(Path::new("/root"));
        let a = tree.add_children(ROOT, vec![entry("a", (1, 10), 100)]).start;
        let mut index = LinkIndex::new(&tree);
        let b = tree.add_children(ROOT, vec![entry("b", (1, 10), 100)]).start;
        index.update(&mut tree, b, &entry("b", (1, 10), 100));
        assert_eq!(tree.root().size, 100);
        index.update(&mut tree, a, &entry("a", (1, 20), 40));
        assert_eq!(tree.root().size, 140);
        assert_eq!(tree.node(b).size, 100);
        index.update(&mut tree, b, &entry("b", (1, 10), 60));
        assert_eq!(tree.root().size, 100);
        assert_eq!(tree.node(a).size, 40);
    }
}
