use crate::app::special::{
    Cleanup, Kind,
    windows::{recycle_bin, temp_folder},
};
use crate::app::{ClawbackApp, DeleteKind, QueuedDelete};

impl ClawbackApp {
    pub(in crate::app) fn request_empty_temp(&mut self) {
        if self.settings.disable_delete {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let Some(node) = temp_folder::find(&doc.tree) else { return };
        let path = doc.tree.path(node);
        let entry = doc.tree.node(node);
        if entry.children.is_empty() || self.pending_paths().any(|pending| path.starts_with(pending)) {
            return;
        }
        self.confirm.push_back(QueuedDelete {
            doc: doc.id,
            node,
            path,
            size: entry.size,
            files: entry.files,
            kind: DeleteKind::Cleanup(Cleanup::TempFolder),
        });
    }

    /// Ask to empty the user's Recycle Bin folder on the scanned drive.
    pub(in crate::app) fn request_empty_recycle_bin(&mut self) {
        if self.settings.disable_delete {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let Some(folder) =
            doc.special().node(Kind::RecycleBin).and_then(|bin| recycle_bin::user_folder(&doc.tree, bin))
        else {
            return;
        };
        let path = doc.tree.path(folder);
        let n = doc.tree.node(folder);
        if n.children.is_empty() || self.pending_paths().any(|pending| *pending == path) {
            return;
        }
        let (size, files) = (n.size, n.files);
        self.confirm.push_back(QueuedDelete {
            doc: doc.id,
            node: folder,
            path,
            size,
            files,
            kind: DeleteKind::Cleanup(Cleanup::RecycleBin),
        });
    }
}
