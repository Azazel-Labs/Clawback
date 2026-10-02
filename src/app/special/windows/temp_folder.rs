//! The current user's `AppData` Temp directory, represented by a cleanup cell.
use clawback_core::{NodeId, ROOT, Tree};
use std::path::{Path, PathBuf};

pub fn path() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Temp"))
}

pub fn find(tree: &Tree) -> Option<NodeId> {
    find_path(tree, &path()?)
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").trim_start_matches("\\\\?\\").trim_end_matches('\\').to_lowercase()
}

/// Match Windows paths case-insensitively, including scans rooted inside `AppData`.
fn find_path(tree: &Tree, path: &Path) -> Option<NodeId> {
    let wanted = normalized(path);
    let root = normalized(tree.root_path());
    let mut node = ROOT;
    if wanted != root {
        let prefix = format!("{root}\\");
        for component in wanted.strip_prefix(&prefix)?.split('\\') {
            node = tree
                .node(node)
                .children
                .iter()
                .copied()
                .find(|&child| tree.node(child).name_lossy().to_lowercase() == component)?;
        }
    }
    tree.node(node).is_dir().then_some(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::tree::NewEntry;

    #[test]
    fn temp_folder_matches_only_the_current_users_exact_directory() {
        let mut tree = Tree::new(Path::new("C:/Users/Nick/AppData/Local"));
        let ids = tree.add_children(ROOT, vec![NewEntry::dir("TEMP"), NewEntry::dir("Temp-other")]);
        assert_eq!(find_path(&tree, Path::new("c:/users/nick/appdata/local/Temp")), Some(ids.start));
        assert_eq!(find_path(&tree, Path::new("C:/Users/Other/AppData/Local/Temp")), None);
        assert_eq!(find_path(&tree, Path::new("C:/Users/Nick/AppData/Local/Temp/nested")), None);
        assert_eq!(find_path(&tree, Path::new("C:/Users/Nick/AppData/Locality/Temp")), None);
        let tree = Tree::new(Path::new("C:/Users/Nick/AppData/Local/Temp"));
        assert_eq!(find_path(&tree, Path::new("C:/Users/Nick/AppData/Local/Temp")), Some(ROOT));
    }
}
