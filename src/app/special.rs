//! Platform-selected special locations: discovery, actions, dialogs and map presentation.
//!
//! Add a provider to the target platform's registry, then keep its native operations
//! and presentation here. The main app and treemap consume Inventory/Kind rather than
//! keeping one field and one platform branch for every location.
mod cleanup;
#[cfg(windows)]
mod dialogs;
#[cfg(windows)]
mod state;
mod visuals;
#[cfg(windows)]
pub(in crate::app) mod windows;

use clawback_core::{NodeId, Tree};
pub use cleanup::{Cleanup, Confirmation};
#[cfg(windows)]
pub(in crate::app) use state::Compacted;
#[cfg(windows)]
pub use state::State;
use std::path::{Path, PathBuf};
pub use visuals::Visuals;
#[cfg(windows)]
pub use windows::wsl_disks::worker_entry;

#[cfg(not(windows))]
#[derive(Default)]
pub struct State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))] // Native providers are selected by the target registry.
pub enum Kind {
    RecycleBin,
    TempFolder,
    WslDisk,
    InstalledSoftware,
}

/// Fast recognizers are used during interaction; registry/manifest enumeration is
/// kept in the separate worker-only removal hook.
type Activate = fn(&mut super::ClawbackApp, NodeId, &eframe::egui::Context);

struct Provider {
    kind: Kind,
    cell: Option<fn(&Tree, bool) -> Option<NodeId>>,
    recognizes: Option<fn(&Path) -> bool>,
    activate: Option<Activate>,
    #[cfg(windows)]
    removal: Option<fn(&Path) -> Option<windows::programs::Removal>>,
}

#[cfg(windows)]
const PROVIDERS: &[Provider] = windows::PROVIDERS;
#[cfg(not(windows))]
const PROVIDERS: &[Provider] = &[];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    node: NodeId,
    kind: Kind,
}

/// A small, immutable discovery result tied to a tree snapshot. It is part of the
/// layout cache key, so changed special cells invalidate the map automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inventory {
    cells: [Option<Cell>; PROVIDERS.len()],
}

impl Default for Inventory {
    fn default() -> Self {
        Self { cells: [None; PROVIDERS.len()] }
    }
}

impl Inventory {
    pub fn allows_zoom(self, node: NodeId) -> bool {
        self.cells.iter().flatten().find(|cell| cell.node == node).is_none_or(|cell| cell.kind != Kind::RecycleBin)
    }
    pub fn discover(tree: &Tree, is_mount: bool) -> Self {
        let mut inventory = Self::default();
        for (slot, provider) in inventory.cells.iter_mut().zip(PROVIDERS) {
            *slot = provider.cell.and_then(|find| find(tree, is_mount)).map(|node| Cell { node, kind: provider.kind });
        }
        inventory
    }

    pub fn collapsed(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.cells.iter().flatten().map(|cell| cell.node)
    }

    #[cfg(windows)]
    pub fn node(&self, kind: Kind) -> Option<NodeId> {
        self.cells.iter().flatten().find(|cell| cell.kind == kind).map(|cell| cell.node)
    }

    pub fn classify(self, tree: &Tree, node: NodeId) -> Option<Kind> {
        self.cells
            .iter()
            .flatten()
            .find(|cell| cell.node == node)
            .map(|cell| cell.kind)
            .or_else(|| recognize(&tree.path(node)))
    }

    #[cfg(all(test, windows))]
    pub fn fixture(kind: Kind, node: NodeId) -> Self {
        let mut inventory = Self::default();
        let slot = PROVIDERS.iter().position(|p| p.kind == kind).expect("registered test provider");
        inventory.cells[slot] = Some(Cell { node, kind });
        inventory
    }
}

pub fn recognize(path: &Path) -> Option<Kind> {
    PROVIDERS.iter().find(|provider| provider.recognizes.is_some_and(|recognize| recognize(path))).map(|p| p.kind)
}

pub fn protects_from_raw_delete(path: &Path) -> bool {
    #[cfg(windows)]
    return windows::programs::protected(path);
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[cfg(windows)]
pub(in crate::app) fn inspect_removal(path: &Path) -> Option<windows::programs::Removal> {
    PROVIDERS.iter().find_map(|provider| provider.removal.and_then(|inspect| inspect(path)))
}

#[cfg_attr(not(windows), allow(clippy::unused_self, clippy::unnecessary_wraps))]
impl State {
    pub(in crate::app) fn check_removal(
        &mut self,
        request: super::QueuedDelete,
        ctx: &eframe::egui::Context,
    ) -> Option<super::QueuedDelete> {
        #[cfg(windows)]
        {
            let path = request.path.clone();
            let job = crate::background::Job::spawn((), ctx, move || inspect_removal(&path));
            self.removal_checks.push((request, job));
            None
        }
        #[cfg(not(windows))]
        {
            let _ = ctx;
            Some(request)
        }
    }
    pub fn is_open(&self) -> bool {
        #[cfg(windows)]
        return self.wsl_disk.is_some() || !self.removal_checks.is_empty() || !self.removals.is_empty();
        #[cfg(not(windows))]
        false
    }

    pub fn pending_paths(&self) -> impl Iterator<Item = &PathBuf> {
        #[cfg(windows)]
        return self
            .removal_checks
            .iter()
            .map(|(request, _)| &request.path)
            .chain(self.removals.iter().map(|r| &r.path));
        #[cfg(not(windows))]
        std::iter::empty()
    }
}

#[cfg_attr(not(windows), allow(clippy::unused_self))]
impl super::ClawbackApp {
    pub(super) fn poll_special(&mut self, ctx: &eframe::egui::Context) {
        #[cfg(windows)]
        self.poll_removals(ctx);
        #[cfg(not(windows))]
        let _ = ctx;
    }

    pub(super) fn special_dialogs(&mut self, ctx: &eframe::egui::Context) {
        #[cfg(not(windows))]
        let _ = ctx;
        #[cfg(windows)]
        {
            self.wsl_disk_dialog(ctx);
            self.removal_dialog(ctx);
        }
    }

    pub(super) fn request_special(&mut self, kind: Kind, node: NodeId, ctx: &eframe::egui::Context) {
        let Some(doc) = &self.doc else { return };
        if doc.tree.get(node).is_none()
            || Inventory::discover(&doc.tree, doc.is_mount).classify(&doc.tree, node) != Some(kind)
        {
            return;
        }
        if kind.action_enabled(self.settings.disable_delete)
            && let Some(activate) =
                PROVIDERS.iter().find(|provider| provider.kind == kind).and_then(|provider| provider.activate)
        {
            activate(self, node, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{ROOT, tree::NewEntry};

    #[test]
    fn registry_entries_are_unique_and_interactive_cells_have_actions() {
        for (index, provider) in PROVIDERS.iter().enumerate() {
            assert!(PROVIDERS[..index].iter().all(|other| other.kind != provider.kind));
            assert!(provider.cell.is_none() || provider.activate.is_some());
        }
    }

    #[test]
    fn discovery_only_enables_the_compiled_platforms_handlers() {
        let mut tree = Tree::new(Path::new("C:/"));
        let bin = tree.add_children(ROOT, vec![NewEntry::dir("$Recycle.Bin")]).start;
        let ordinary = tree.add_children(ROOT, vec![NewEntry::file("ordinary.txt", 8)]).start;
        let inventory = Inventory::discover(&tree, true);
        assert_eq!(inventory.classify(&tree, ordinary), None);
        #[cfg(windows)]
        {
            assert_eq!(inventory.classify(&tree, bin), Some(Kind::RecycleBin));
            assert_eq!(inventory.collapsed().collect::<Vec<_>>(), [bin]);
            assert!(!inventory.allows_zoom(bin));
            assert!(Inventory::discover(&tree, false).collapsed().next().is_none());
            assert_eq!(
                recognize(Path::new("C:/Users/Test/AppData/Local/Docker/wsl/disk/docker_data.vhdx")),
                Some(Kind::WslDisk)
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(inventory.classify(&tree, bin), None);
            assert!(inventory.collapsed().next().is_none());
            assert_eq!(recognize(Path::new("C:/Users/Test/AppData/Local/Docker/wsl/disk/docker_data.vhdx")), None);
        }
    }
}
