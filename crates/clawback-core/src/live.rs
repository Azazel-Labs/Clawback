//! Incremental metadata reconciliation. All calls run on a background worker.
use crate::hardlinks::LinkIndex;
use crate::scan::{FileAccounting, mtime_secs, virtual_paths};
use crate::tree::{NewEntry, flags};
use crate::{Kind, NodeId, ROOT, Scan, ScanOptions, Tree};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Refresh {
    options: ScanOptions,
    accounting: FileAccounting,
    links: Option<LinkIndex>,
    /// Devices new subdirectories may live on under `one_filesystem`.
    #[cfg(unix)]
    allowed_devs: Vec<u64>,
    excludes: Vec<PathBuf>,
    pub dirs: u64,
}

/// Non-directory nodes at or below `id`, collected before the tree is edited.
fn files_below(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    tree.descendants(id).filter(|&n| !tree.node(n).is_dir()).collect()
}

impl Refresh {
    pub fn new(tree: &Tree, options: ScanOptions) -> Self {
        let root = tree.root_path();
        Self {
            // Only a tree built from the MFT carries a root identity.
            accounting: FileAccounting::new(root, tree.root().file_id.is_some()),
            links: options.dedupe_hardlinks.then(|| LinkIndex::new(tree)),
            #[cfg(unix)]
            allowed_devs: fs::metadata(root).map(|md| crate::scan::allowed_devices(root, &md)).unwrap_or_default(),
            excludes: if options.skip_virtual { virtual_paths(root) } else { Vec::new() },
            dirs: tree.dir_count(ROOT),
            options,
        }
    }

    pub fn path(&mut self, tree: &mut Tree, path: &Path, stop: &AtomicBool) -> io::Result<bool> {
        if stop.load(Ordering::Relaxed) || !path.starts_with(tree.root_path()) {
            return Ok(false);
        }
        // Reconcile the nearest known parent if a whole new hierarchy appeared.
        let Some(target) =
            path.ancestors().find(|t| *t == tree.root_path() || t.parent().and_then(|p| tree.find_path(p)).is_some())
        else {
            return Ok(false);
        };
        if let Some(parent) = target.parent().and_then(|p| tree.find_path(p))
            && tree.ancestors(parent).any(|id| {
                let node = tree.node(id);
                !node.is_dir() || node.has(flags::OTHER_FS | flags::VIRTUAL)
            })
        {
            return Ok(false);
        }
        self.entry(tree, target, true, stop)
    }

    fn remove(&mut self, tree: &mut Tree, id: NodeId) -> bool {
        self.dirs = self.dirs.saturating_sub(tree.dir_count(id));
        if let Some(links) = &mut self.links {
            for node in files_below(tree, id) {
                links.detach(tree, node);
            }
        }
        tree.remove(id)
    }

    fn entry(&mut self, tree: &mut Tree, path: &Path, list_existing: bool, stop: &AtomicBool) -> io::Result<bool> {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let mut old = tree.find_path(path);
        let md = match fs::symlink_metadata(path) {
            Ok(md) => md,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if old == Some(ROOT) {
                    return Err(e);
                }
                return Ok(old.is_some_and(|id| self.remove(tree, id)));
            }
            Err(e) => return Err(e),
        };
        // Windows may deliver a late event using the old spelling after a
        // case-only rename. Both spellings resolve, but only one entry exists.
        #[cfg(windows)]
        if old.is_none()
            && !md.is_symlink()
            && let Ok(actual) = fs::canonicalize(path)
            && actual.file_name() != path.file_name()
            && let (Some(parent), Some(name)) = (path.parent(), actual.file_name())
        {
            return self.entry(tree, &parent.join(name), list_existing, stop);
        }
        let kind = Kind::from(md.file_type());
        if let Some(id) = old
            && tree.node(id).kind != kind
        {
            if id == ROOT {
                return Err(io::Error::other("Watched root is no longer a directory"));
            }
            self.remove(tree, id);
            old = None;
        }
        if kind == Kind::Dir {
            if let Some(id) = old {
                if !list_existing || tree.node(id).has(flags::OTHER_FS | flags::VIRTUAL) {
                    return Ok(false);
                }
                if tree.node(id).has(flags::DENIED | flags::PARTIAL) {
                    return Err(io::Error::other("Reconcile previously unreadable directory"));
                }
                // Directory notifications need only enumerate direct children.
                // Existing subdirectories keep their IDs and are not rescanned.
                let mut names = HashSet::new();
                let mut changed = false;
                for entry in fs::read_dir(path)? {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(changed);
                    }
                    let entry = entry?;
                    names.insert(entry.file_name());
                    changed |= self.entry(tree, &entry.path(), false, stop)?;
                }
                let removed: Vec<_> = tree
                    .node(id)
                    .children
                    .iter()
                    .copied()
                    .filter(|&child| !names.contains(tree.node(child).name.as_ref()))
                    .collect();
                for child in removed {
                    changed |= self.remove(tree, child);
                }
                return Ok(changed);
            }
            let Some(parent) = path.parent().and_then(|p| tree.find_path(p)) else { return Ok(false) };
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if self.options.one_filesystem && !self.allowed_devs.contains(&md.dev()) {
                    return Ok(false);
                }
            }
            if self.excludes.iter().any(|p| path.starts_with(p)) {
                return Ok(false);
            }
            let mut options = self.options.clone();
            options.threads = 1;
            // Keep raw allocated sizes while scanning the new subtree; the
            // document-wide inode index then deduplicates against existing files.
            options.dedupe_hardlinks = false;
            let result = Scan::start_with_accounting(path, options, None, self.accounting.exact())?.wait_or_stop(stop);
            if result.cancelled {
                return Ok(false);
            }
            // Let global reconciliation report unreadable newly added trees.
            if !result.skipped.is_empty() {
                return Err(io::Error::other("New directory contains unreadable entries"));
            }
            let id = tree
                .add_children(
                    parent,
                    vec![NewEntry {
                        name: path.file_name().unwrap_or_default().into(),
                        kind,
                        size: 0,
                        len: 0,
                        mtime: mtime_secs(&md),
                        flags: 0,
                        file_id: None,
                    }],
                )
                .start;
            self.dirs += result.dirs;
            tree.graft(id, result.tree);
            if let Some(links) = &mut self.links {
                for node_id in files_below(tree, id) {
                    let node = tree.node(node_id);
                    let entry = NewEntry {
                        name: node.name.clone().into(),
                        kind: node.kind,
                        size: node.size,
                        len: node.len,
                        mtime: node.mtime,
                        flags: node.flags,
                        file_id: node.file_id,
                    };
                    links.update(tree, node_id, &entry);
                }
            }
            return Ok(true);
        }
        let (size, len, file_id) = self.accounting.measure(path, &md, self.options.apparent_size)?;
        let entry = NewEntry {
            name: path.file_name().unwrap_or_default().into(),
            kind,
            size,
            len,
            mtime: mtime_secs(&md),
            flags: 0,
            file_id,
        };
        if let Some(links) = &mut self.links {
            if let Some(id) = old {
                return Ok(links.update(tree, id, &entry));
            }
            if let Some(parent) = path.parent().and_then(|p| tree.find_path(p)) {
                let id = tree.add_children(parent, vec![entry.clone()]).start;
                links.update(tree, id, &entry);
                tree.resort_upwards(id);
                return Ok(true);
            }
            return Ok(false);
        }
        if let Some(id) = old {
            let node = tree.node(id);
            if (node.size, node.len, node.mtime) == (size, entry.len, entry.mtime) {
                return Ok(false);
            }
            tree.update_file(id, entry);
        } else if let Some(parent) = path.parent().and_then(|p| tree.find_path(p)) {
            let id = tree.add_children(parent, vec![entry]).start;
            // Also restore ancestor ordering after an insertion.
            tree.resort_upwards(id);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    fn temp() -> TempDir {
        TempDir::new("refresh")
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hardlinks_remain_incremental_across_edits_moves_and_subtree_deletes() {
        let dir = temp();
        let root = &dir.0;
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("a"), [1; 100]).unwrap();
        fs::hard_link(root.join("a"), root.join("sub/b")).unwrap();
        let options = ScanOptions { apparent_size: true, threads: 1, ..ScanOptions::default() };
        let mut tree = Scan::start_with_accounting(root, options.clone(), None, true).unwrap().wait().tree;
        // Model an MFT document: its root has a volume/file identity. Ordinary
        // Windows directory documents intentionally do not deduplicate links.
        #[cfg(windows)]
        {
            tree.node_mut(ROOT).file_id = Some((0, 5));
        }
        let mut refresh = Refresh::new(&tree, options.clone());
        assert!(refresh.links.is_some());
        let stop = AtomicBool::new(false);
        let a = tree.find_path(&root.join("a")).unwrap();
        let b = tree.find_path(&root.join("sub/b")).unwrap();
        let (owner, survivor) = if tree.node(a).size > 0 { (a, b) } else { (b, a) };
        let removed = tree.path(owner);
        let kept = tree.path(survivor);
        fs::remove_file(&removed).unwrap();
        refresh.path(&mut tree, &removed, &stop).unwrap();
        assert_eq!(tree.root().size, 100);
        assert_eq!(tree.node(survivor).size, 100);
        fs::write(&kept, [2; 222]).unwrap();
        refresh.path(&mut tree, &kept, &stop).unwrap();
        assert_eq!(tree.root().size, 222);
        fs::hard_link(&kept, root.join("c")).unwrap();
        refresh.path(&mut tree, &root.join("c"), &stop).unwrap();
        assert_eq!(tree.root().size, 222);
        let c = tree.find_path(&root.join("c")).unwrap();
        fs::write(root.join("c"), [3; 300]).unwrap();
        refresh.path(&mut tree, &root.join("c"), &stop).unwrap();
        assert_eq!(tree.node(survivor).len, 300);
        assert_eq!(tree.root().size, 300);
        // Replacing one name creates an independent inode; other links retain data.
        fs::write(root.join("temporary"), [4; 40]).unwrap();
        fs::rename(root.join("temporary"), &kept).unwrap();
        refresh.path(&mut tree, &kept, &stop).unwrap();
        assert_eq!(tree.root().size, 340);
        assert_eq!(tree.node(c).size, 300);
        // A newly moved-in subtree can contain aliases of files already indexed.
        fs::create_dir_all(root.join("new/nested")).unwrap();
        fs::hard_link(root.join("c"), root.join("new/nested/copy")).unwrap();
        fs::hard_link(&kept, root.join("new/other-copy")).unwrap();
        refresh.path(&mut tree, &root.join("new"), &stop).unwrap();
        assert_eq!(tree.root().size, 340);
        fs::remove_file(root.join("c")).unwrap();
        refresh.path(&mut tree, &root.join("c"), &stop).unwrap();
        assert_eq!(tree.root().size, 340);
        fs::remove_dir_all(root.join("new")).unwrap();
        refresh.path(&mut tree, &root.join("new"), &stop).unwrap();
        let fresh = crate::scan::scan(root, options).unwrap();
        assert_eq!((tree.root().size, tree.root().files, refresh.dirs), (fresh.bytes, fresh.files, fresh.dirs));
        assert_eq!(tree.root().size, 40);
    }

    #[cfg(unix)]
    #[test]
    fn unix_sparse_hardlinks_use_allocated_blocks_and_ignore_outside_aliases() {
        let dir = temp();
        let outside = temp();
        let file = fs::File::create(dir.0.join("sparse")).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        fs::hard_link(dir.0.join("sparse"), outside.0.join("outside")).unwrap();
        fs::hard_link(dir.0.join("sparse"), dir.0.join("alias")).unwrap();
        let options = ScanOptions { threads: 1, ..ScanOptions::default() };
        let mut tree = crate::scan::scan(&dir.0, options.clone()).unwrap().tree;
        let mut refresh = Refresh::new(&tree, options.clone());
        fs::write(dir.0.join("alias"), [7; 8192]).unwrap();
        refresh.path(&mut tree, &dir.0.join("alias"), &AtomicBool::new(false)).unwrap();
        let fresh = crate::scan::scan(&dir.0, options).unwrap();
        assert_eq!(tree.root().size, fresh.bytes);
        assert_eq!(tree.root().files, 2);
        assert_eq!(tree.node(tree.find_path(&dir.0.join("sparse")).unwrap()).len, 8192);
    }

    #[test]
    fn incremental_edits_match_fresh_scan_and_preserve_snapshots() {
        let dir = temp();
        fs::create_dir(dir.0.join("keep")).unwrap();
        fs::write(dir.0.join("keep/a"), [1; 10]).unwrap();
        fs::write(dir.0.join("b"), [2; 20]).unwrap();
        let options =
            ScanOptions { apparent_size: true, dedupe_hardlinks: false, threads: 1, ..ScanOptions::default() };
        let mut tree = crate::scan::scan(&dir.0, options.clone()).unwrap().tree;
        let before = tree.clone();
        let kept = tree.find_path(&dir.0.join("keep")).unwrap();
        let file = tree.find_path(&dir.0.join("keep/a")).unwrap();
        let mut refresh = Refresh::new(&tree, options.clone());
        let stop = AtomicBool::new(false);
        fs::write(dir.0.join("keep/a"), [3; 70]).unwrap();
        assert!(refresh.path(&mut tree, &dir.0.join("keep/a"), &stop).unwrap());
        assert_eq!(tree.find_path(&dir.0.join("keep/a")), Some(file));
        assert_eq!(tree.root().size, 90);
        assert_eq!(before.root().size, 30);
        assert!(!refresh.path(&mut tree, &dir.0.join("keep/a"), &stop).unwrap());
        fs::rename(dir.0.join("b"), dir.0.join("renamed")).unwrap();
        refresh.path(&mut tree, &dir.0.join("b"), &stop).unwrap();
        refresh.path(&mut tree, &dir.0.join("renamed"), &stop).unwrap();
        fs::create_dir_all(dir.0.join("new/nested")).unwrap();
        fs::write(dir.0.join("new/nested/data"), [4; 123]).unwrap();
        refresh.path(&mut tree, &dir.0.join("new/nested/data"), &stop).unwrap();
        let fresh = crate::scan::scan(&dir.0, options.clone()).unwrap();
        assert_eq!((tree.root().size, tree.root().files, refresh.dirs), (fresh.bytes, fresh.files, fresh.dirs));
        assert_eq!(tree.find_path(&dir.0.join("keep")), Some(kept));
        let removed = tree.find_path(&dir.0.join("new/nested/data")).unwrap();
        fs::remove_dir_all(dir.0.join("new")).unwrap();
        refresh.path(&mut tree, &dir.0.join("new"), &stop).unwrap();
        assert!(!tree.is_live(removed));
        let fresh = crate::scan::scan(&dir.0, options).unwrap();
        assert_eq!((tree.root().size, tree.root().files, refresh.dirs), (fresh.bytes, fresh.files, fresh.dirs));
        // A cancelled refresh leaves the last published state untouched.
        stop.store(true, Ordering::Relaxed);
        fs::write(dir.0.join("keep/a"), [0; 999]).unwrap();
        assert!(!refresh.path(&mut tree, &dir.0, &stop).unwrap());
        assert_eq!(tree.root().size, fresh.bytes);
    }
}
