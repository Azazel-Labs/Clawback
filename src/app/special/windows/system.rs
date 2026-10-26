//! The running Windows installation: its folder and what it reserves at drive roots.
//! They are drawn as one cell, and nothing in them is ever offered for deletion.
use clawback_core::{NodeId, ROOT, Tree};
use std::path::{Path, PathBuf};

/// Paging, swap and hibernation files, and the restore points and shadow copies in System
/// Volume Information. Windows sizes and removes these itself. Each is (name, directory).
pub const RESERVED: [(&str, bool); 4] = [
    ("pagefile.sys", false),
    ("swapfile.sys", false),
    ("hiberfil.sys", false),
    ("System Volume Information", true),
];

/// Settings page where Windows offers its own cleanup of system files.
pub const STORAGE_SETTINGS: &str = "ms-settings:storagesense";

pub fn folder() -> Option<PathBuf> {
    std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")).map(PathBuf::from)
}

/// The Windows folder as one cell, unless the scan is rooted at or inside it.
pub fn find(tree: &Tree) -> Option<NodeId> {
    super::temp_folder::find_path(tree, &folder()?).filter(|&node| node != ROOT)
}

/// Reserved entries directly under a scanned drive root.
pub fn reserved(tree: &Tree) -> [Option<NodeId>; RESERVED.len()] {
    let mut found = [None; RESERVED.len()];
    if tree.root_path().parent().is_some() {
        return found;
    }
    for &child in &tree.node(ROOT).children {
        let node = tree.node(child);
        if let Some(index) = RESERVED
            .iter()
            .position(|&(name, directory)| node.is_dir() == directory && node.name_lossy().eq_ignore_ascii_case(name))
        {
            found[index] = Some(child);
        }
    }
    found
}

/// Anything inside the Windows folder or a reserved entry at a drive root.
pub fn protected(path: &Path) -> bool {
    folder().is_some_and(|folder| super::programs::within(path, &folder)) || reserved_path(path)
}

fn reserved_path(path: &Path) -> bool {
    // The ancestor directly below the drive root.
    let top = path.ancestors().find(|ancestor| ancestor.parent().is_some_and(|parent| parent.parent().is_none()));
    top.and_then(Path::file_name).is_some_and(|name| {
        RESERVED.iter().any(|(reserved, _)| name.to_string_lossy().eq_ignore_ascii_case(reserved))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::tree::NewEntry;

    #[test]
    fn windows_folder_is_one_cell_only_below_the_scan_root() {
        let windows = folder().expect("SystemRoot");
        let drive = windows.parent().expect("Windows folder's drive");
        let mut tree = Tree::new(drive);
        let name = windows.file_name().expect("Windows folder name").to_string_lossy().to_uppercase();
        let ids = tree.add_children(ROOT, vec![NewEntry::dir(&name), NewEntry::dir("Windows.old")]);
        assert_eq!(find(&tree), Some(ids.start));
        assert_eq!(find(&Tree::new(&windows)), None);
    }

    #[test]
    fn reserved_files_are_found_only_at_a_drive_root() {
        let mut tree = Tree::new(Path::new("D:\\"));
        let ids = tree.add_children(
            ROOT,
            vec![
                NewEntry::file("PageFile.sys", 8),
                NewEntry::file("hiberfil.sys", 4),
                NewEntry::file("notes.sys", 2),
                NewEntry::dir("System Volume Information"),
                NewEntry::dir("swapfile.sys"),
            ],
        );
        assert_eq!(reserved(&tree), [Some(ids.start), None, Some(ids.start + 1), Some(ids.start + 3)]);
        let mut nested = Tree::new(Path::new("D:\\Backup"));
        nested.add_children(ROOT, vec![NewEntry::file("pagefile.sys", 8)]);
        assert_eq!(reserved(&nested), [None; RESERVED.len()]);
    }

    #[test]
    fn system_paths_are_protected() {
        let windows = folder().expect("SystemRoot");
        assert!(protected(&windows));
        assert!(protected(&windows.join("System32").join("drivers")));
        assert!(protected(Path::new("C:\\pagefile.sys")));
        assert!(protected(Path::new("E:\\SWAPFILE.SYS")));
        assert!(protected(Path::new("D:\\System Volume Information\\{3808876b-c176-4e48-b7ae-04046e6cc752}")));
        assert!(!protected(Path::new("C:\\Backup\\pagefile.sys")));
        assert!(!protected(&windows.with_file_name("Windows.old")));
    }
}
