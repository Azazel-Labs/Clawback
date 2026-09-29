//! Plain-text scan report for `clawback --report`.

use crate::format;
use crate::scan::{ScanResult, SkipReason};
use crate::tree::{NodeId, ROOT};
use std::fmt::Write as _;

pub fn render(r: &ScanResult, top: usize) -> String {
    let t = &r.tree;
    let root = t.root();
    let mut s = String::new();
    let _ = writeln!(s, "Clawback report for {}", r.root.display());
    let _ = writeln!(
        s,
        "{} in {} files and {} folders, scanned in {}{}",
        format::size(root.size),
        format::count(r.files),
        format::count(r.dirs),
        format::duration(r.elapsed),
        if r.cancelled { " (cancelled)" } else { "" }
    );
    let denied = r.skipped.iter().filter(|x| x.reason != SkipReason::OtherFilesystem).count();
    let other = r.skipped.len() - denied;
    if denied > 0 {
        let _ = writeln!(s, "{denied} folders could not be read (permissions)");
    }
    if other > 0 {
        let _ = writeln!(s, "{other} folders on other filesystems were not scanned");
    }

    let _ = writeln!(s, "\nLargest entries:");
    for &c in root.children.iter().take(top) {
        let n = t.node(c);
        if n.size == 0 {
            break;
        }
        let slash = if n.is_dir() { std::path::MAIN_SEPARATOR_STR } else { "" };
        let _ = writeln!(
            s,
            "  {:>6}  {:>10}  {}{}",
            format::percent(n.size, root.size),
            format::size(n.size),
            n.name_lossy(),
            slash
        );
    }

    let files: Vec<NodeId> = t.largest_files(ROOT, top);
    if !files.is_empty() {
        let _ = writeln!(s, "\nLargest files:");
        for id in files {
            let _ = writeln!(s, "  {:>10}  {}", format::size(t.node(id).size), t.relative_path(ROOT, id).display());
        }
    }
    s
}
