//! The application window: SpaceMonger's toolbar, commands and dialogs
//! around the folder map.
pub(crate) mod special;

use crate::i18n::tr;

use crate::directoryview::DirectoryView;
use crate::mapview::{Command, MapInput, MapView};
use crate::picker::{OpenDialog, Picked, PickerIcons};
use crate::platform::{self, DiskInfo};
use crate::properties::Props;
use crate::scanning::{Running, Update};
use crate::theme;
use crate::watching::LiveStatus;
use crate::{background::retire, icon};
use clawback_core::format::{self, dir_display, display_name, fraction};
use clawback_core::{NodeId, ROOT, Settings, SkipReason, Skipped, Tree};
use eframe::egui::{self, Align, Align2, Id, Key, Layout, Modifiers, RichText, Ui, vec2};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct DeleteError {
    path: PathBuf,
    message: String,
}

/// A completed scan being viewed (SpaceMonger's `CFolderTree` document).
struct Doc {
    id: u64,
    tree: Arc<Tree>,
    view: NodeId,
    generation: u64,
    skipped: Arc<Vec<Skipped>>,
    disk: Option<DiskInfo>,
    is_mount: bool,
    files: u64,
    folders: u64,
}

impl Doc {
    /// `files` and `folders` are the scan's own counts: a preview tree holds only part of it.
    fn new(id: u64, tree: Tree, files: u64, folders: u64) -> Self {
        Doc {
            id,
            tree: Arc::new(tree),
            view: ROOT,
            generation: 0,
            skipped: Arc::new(Vec::new()),
            disk: None,
            is_mount: false,
            files,
            folders,
        }
    }
    /// A complete tree, counted by walking it.
    #[cfg(any(test, feature = "perf-probe", feature = "screenshots"))]
    fn counted(id: u64, tree: Tree) -> Self {
        let (files, folders) = (tree.root().files, tree.dir_count(ROOT));
        Self::new(id, tree, files, folders)
    }
    fn root(&self) -> PathBuf {
        self.tree.root_path().to_path_buf()
    }
    /// SpaceMonger's `totalspace`: the drive size when viewing a whole drive,
    /// otherwise the scanned folder's size.
    fn total_space(&self) -> u64 {
        match &self.disk {
            Some(d) if self.is_mount => d.total,
            _ => self.tree.root().size,
        }
    }
    /// Folder represented in the navigator for a map selection; files select their parent.
    fn selected_folder(&self, selection: Option<NodeId>) -> NodeId {
        let selected = selection
            .filter(|&id| {
                self.tree.get(id).is_some()
                    && self
                        .tree
                        .chain(id)
                        .into_iter()
                        .all(|ancestor| !self.tree.node(ancestor).has(clawback_core::tree::flags::REMOVED))
            })
            .unwrap_or(self.view);
        if self.tree.node(selected).is_dir() { selected } else { self.tree.node(selected).parent }
    }
    fn free_space(&self) -> u64 {
        self.disk.as_ref().map_or(0, |d| d.free)
    }
    fn special(&self) -> special::Inventory {
        special::Inventory::discover(&self.tree, self.is_mount)
    }
    fn unreadable(&self) -> usize {
        self.skipped.iter().filter(|s| s.reason != SkipReason::OtherFilesystem).count()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeleteKind {
    /// Move to the Recycle Bin; anything too large for it asks before a permanent delete.
    Recycle,
    /// Confirmed permanent delete, done by Clawback's fast purge.
    Permanent,
    /// Confirmed contents-only cleanup, with policy supplied by the special-location handler.
    #[cfg_attr(not(windows), allow(dead_code))] // Current cleanup providers are Windows-only.
    Cleanup(special::Cleanup),
}

impl DeleteKind {
    fn cleanup(self) -> Option<special::Cleanup> {
        match self {
            Self::Cleanup(cleanup) => Some(cleanup),
            _ => None,
        }
    }
}

enum DeleteDone {
    /// Everything went; free space is re-read.
    Removed(Option<DiskInfo>),
    /// The user answered no to a Windows prompt; nothing changed.
    Declined,
    /// Too large for the Recycle Bin. Nothing was deleted; ask before purging.
    TooLarge,
    /// Cancelled or partly failed; `error` explains failures.
    Partial { error: Option<String>, disk: Option<DiskInfo> },
}
type DeleteResult = Result<DeleteDone, String>;

struct Deleting {
    job: QueuedDelete,
    /// The job's node in the current tree; earlier deletes or a rescan may have replaced it.
    node: Option<NodeId>,
    /// Paths that leave the map when the job succeeds.
    targets: Vec<PathBuf>,
    rx: mpsc::Receiver<DeleteResult>,
    progress: Arc<crate::deletion::Progress>,
    started: Instant,
}

/// A delete waiting for the one in progress (or for confirmation); resolved by path when it starts.
#[derive(Clone)]
struct QueuedDelete {
    doc: u64,
    node: NodeId,
    path: PathBuf,
    size: u64,
    files: u64,
    kind: DeleteKind,
}

/// How long a recycled path is hidden from live snapshots that predate its removal.
const RECYCLED_GRACE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
enum Tool {
    Open,
    Rescan,
    ZoomFull,
    ZoomIn,
    ZoomOut,
    Back,
    Free,
    Reveal,
    Delete,
    Setup,
    About,
    Unreadable,
}

#[derive(Default)]
pub struct ClawbackApp {
    #[cfg(feature = "perf-probe")]
    probe: Option<crate::perf_probe::Probe>,
    settings: Settings,
    doc: Option<Doc>,
    next_doc_id: u64,
    history: Vec<PathBuf>,
    scan: Option<Running>,
    live: Option<crate::watching::Live>,
    live_status: Option<LiveStatus>,
    map: MapView,
    directories: DirectoryView,
    file_types: crate::filetypes::FileTypes,
    narrow_types: bool,
    open: Option<OpenDialog>,
    setup: Option<Settings>,
    about: bool,
    props: Option<Props>,
    show_unreadable: bool,
    deleting: Option<Deleting>,
    delete_queue: VecDeque<QueuedDelete>,
    delete_errors: Vec<DeleteError>,
    /// Recycled paths the live watcher may not have caught up with yet.
    recycled: Vec<(PathBuf, Instant)>,
    /// Permanent deletes waiting for the user's confirmation, asked one at a time.
    confirm: VecDeque<QueuedDelete>,
    error: Option<String>,
    special: special::State,
    title: String,
    /// The directory panel splitter is being dragged; its new share is saved on release.
    split_dragging: bool,
    /// Loaded the first time the picker opens.
    picker_icons: Option<PickerIcons>,
}

impl ClawbackApp {
    #[cfg(feature = "perf-probe")]
    fn probe_step(&mut self, ctx: &egui::Context) {
        use crate::perf_probe::Action;
        let action = self.probe.as_mut().and_then(|probe| probe.next(ctx));
        match action {
            Some(Action::Load(tree)) => {
                self.set_doc(Doc::counted(self.next_doc_id, tree));
                self.next_doc_id += 1;
            }
            Some(Action::ZoomIn) => {
                let node = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.tree.root().children.iter().copied().find(|&n| d.tree.node(n).is_dir()));
                if let Some(node) = node {
                    self.apply(Command::ZoomTo(node), ctx);
                }
            }
            Some(Action::ZoomOut) => self.apply(Command::ZoomFull, ctx),
            Some(Action::Resize(large)) => ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(if large {
                vec2(1120.0, 760.0)
            } else {
                vec2(840.0, 620.0)
            })),
            Some(Action::Close) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            None => {}
        }
    }
    /// The initial state, before any document or window exists.
    fn with_settings(settings: Settings) -> Self {
        // Field by field: the Drop impl rules out `..Default::default()`.
        let mut app = Self::default();
        app.settings = settings;
        app.next_doc_id = 1;
        #[cfg(feature = "perf-probe")]
        {
            app.probe = crate::perf_probe::Probe::new();
        }
        app
    }

    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, path: Option<PathBuf>) -> Self {
        let language = crate::i18n::set_language(&settings.language);
        crate::startup::mark("translations_ready");
        theme::apply(&cc.egui_ctx, language);
        crate::startup::mark("theme_ready");
        #[allow(unused_mut)] // Only the screenshot build edits it further.
        let mut app = Self::with_settings(settings);
        #[cfg(feature = "screenshots")]
        if crate::demo::capturing() {
            let tree = crate::demo::tree();
            let view = crate::demo::view(&tree);
            app.settings = Settings {
                show_free: true,
                file_color: crate::demo::PALETTE,
                folder_color: crate::demo::PALETTE,
                ..Settings::default()
            };
            if let Ok(language) = std::env::var("CLAWBACK_DEMO_LANGUAGE") {
                app.settings.language = language;
                let language = crate::i18n::set_language(&app.settings.language);
                theme::set_fonts(&cc.egui_ctx, language);
            }
            app.live_status = Some(LiveStatus::Demo);
            if view != ROOT {
                app.history.push(tree.root_path().to_path_buf());
            }
            app.doc = Some(Doc { view, disk: Some(crate::demo::disk()), is_mount: true, ..Doc::counted(1, tree) });
            if let Ok(mode) = std::env::var("CLAWBACK_DEMO_SCAN") {
                app.scan = Some(Running::demo(mode == "paused"));
            }
            if let Ok(page) = std::env::var("CLAWBACK_DEMO_SETTINGS") {
                app.setup = Some(app.settings.clone());
                cc.egui_ctx
                    .data_mut(|data| data.insert_temp(Id::new("settings-page"), page.parse::<usize>().unwrap_or(0)));
            }
            return app;
        }
        if let Some(p) = path {
            app.start_scan(p, &cc.egui_ctx);
        }
        app
    }

    /// Probe and capture runs leave the user's saved settings and window state alone.
    #[cfg_attr(not(any(feature = "perf-probe", feature = "screenshots")), allow(clippy::unused_self))]
    fn persistent(&self) -> bool {
        #[cfg(feature = "perf-probe")]
        if self.probe.is_some() {
            return false;
        }
        #[cfg(feature = "screenshots")]
        if crate::demo::capturing() {
            return false;
        }
        true
    }

    fn save_settings(&self) {
        let _span = crate::perf::span("settings.save");
        if self.persistent() {
            let _ = self.settings.save();
        }
    }

    fn start_scan(&mut self, root: PathBuf, ctx: &egui::Context) {
        if let Some(live) = self.live.take() {
            retire(live);
        }
        self.live_status = None;
        if let Some(run) = self.scan.take() {
            run.cancel();
            retire(run);
        }
        let root = std::path::absolute(&root).unwrap_or(root);
        match Running::start(root.clone(), self.settings.scan_options(), ctx.clone()) {
            Ok(scan) => {
                // An abandoned scan may never reach Finished. Give its replacement
                // a fresh identity so cached panels cannot reuse the old root's data.
                self.next_doc_id += 1;
                // Releasing a previous large tree can itself take time.
                if let Some(old) = self.doc.take() {
                    retire(old);
                }
                self.props = None;
                self.show_unreadable = false;
                self.map.reset();
                self.history.clear();
                self.scan = Some(scan);
                self.settings.push_recent(&root);
                if self.persistent() {
                    let settings = self.settings.clone();
                    std::thread::spawn(move || {
                        let _span = crate::perf::span("worker.settings_save");
                        let _ = settings.save();
                    });
                }
            }
            Err(e) => {
                self.error = Some(tr!("scan-path-error", path = root.display().to_string(), error = e.to_string()));
            }
        }
    }

    fn set_doc(&mut self, mut doc: Doc) {
        crate::perf::counter("document.nodes", doc.tree.len() as f64);
        if let Some(previous) = &self.doc {
            let path = previous.tree.path(previous.view);
            doc.view = doc.tree.find_path(&path).unwrap_or(ROOT);
            doc.generation = previous.generation + 1;
        }
        if let Some(old) = self.doc.replace(doc) {
            retire(old);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let _span = crate::perf::span("ui.poll");
        crate::properties::poll(&mut self.props);
        self.poll_scan(ctx);
        self.poll_delete(ctx);
        self.poll_special(ctx);
        if self.deleting.is_none() {
            self.poll_live();
        }
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let update = self.scan.as_ref().map(|run| run.rx.try_recv());
        match update {
            Some(Ok(Update::Volume {
                disk,
                is_mount,
                #[cfg(windows)]
                turbo_available,
            })) => {
                let run = self.scan.as_mut().expect("active scan");
                run.disk = disk;
                run.is_mount = is_mount;
                #[cfg(windows)]
                {
                    run.turbo_available = turbo_available;
                }
            }
            Some(Ok(Update::Preview { tree, progress, current })) => {
                let run = self.scan.as_mut().expect("active scan");
                run.progress = progress;
                run.current = current;
                let (disk, is_mount) = (run.disk.clone(), run.is_mount);
                self.set_doc(Doc { disk, is_mount, ..Doc::new(self.next_doc_id, tree, progress.files, progress.dirs) });
            }
            Some(Ok(Update::Finished { result: r, started })) => {
                let mut run = self.scan.take().expect("active scan");
                self.live = started.live;
                self.live_status = Some(started.status);
                self.set_doc(Doc {
                    skipped: Arc::new(r.skipped),
                    disk: run.disk.take(),
                    is_mount: run.is_mount,
                    ..Doc::new(self.next_doc_id, r.tree, r.files, r.dirs)
                });
                self.next_doc_id += 1;
            }
            Some(Ok(Update::Cancelled)) => self.scan = None,
            Some(Ok(Update::Failed(error))) => {
                self.scan = None;
                self.error = Some(tr!("scan-folder-error", error = error));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.scan = None;
                self.error = Some(tr!("the-scan-worker-stopped-unexpectedly"));
            }
            _ => {}
        }
        if self.scan.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn poll_delete(&mut self, ctx: &egui::Context) {
        let finished = self.deleting.as_ref().and_then(|d| match d.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(tr!("delete-worker-stopped"))),
        });
        if let Some(result) = finished
            && let Some(del) = self.deleting.take()
        {
            self.finish_delete(del, result, ctx);
            self.start_next_delete(ctx);
        }
    }

    fn finish_delete(&mut self, del: Deleting, result: DeleteResult, ctx: &egui::Context) {
        let path = &del.job.path;
        match result {
            Ok(DeleteDone::Removed(disk)) => {
                self.forget_removed(&del.targets, disk);
                if del.job.kind.cleanup().is_some()
                    && let Some(live) = &self.live
                {
                    live.invalidate(path.clone());
                }
                if self.delete_queue.is_empty()
                    && self.live.is_none()
                    && (self.settings.auto_rescan
                        || del.job.kind.cleanup().is_some_and(special::Cleanup::rescan_after_partial))
                    && let Some(root) = self.doc.as_ref().map(Doc::root)
                {
                    self.start_scan(root, ctx);
                }
            }
            Ok(DeleteDone::Declined) => {
                // Keep it on the map, but let the watcher catch any partial change.
                if let Some(live) = &self.live {
                    live.invalidate(path.clone());
                }
            }
            Ok(DeleteDone::TooLarge) => {
                if let Some(node) = del.node {
                    self.confirm.push_back(QueuedDelete { node, kind: DeleteKind::Permanent, ..del.job });
                }
            }
            Ok(DeleteDone::Partial { error, disk }) => {
                // Some of it is gone: the watcher (or a rescan) reconciles what remains.
                if let Some(live) = &self.live {
                    if del.job.kind.cleanup().is_some() {
                        live.invalidate(path.clone());
                    } else {
                        for target in &del.targets {
                            live.invalidate(target.clone());
                        }
                    }
                }
                if let Some(doc) = &mut self.doc {
                    doc.disk = disk.or(doc.disk.take());
                    doc.generation += 1;
                }
                if let Some(error) = error {
                    self.delete_errors.push(DeleteError { path: path.clone(), message: error });
                }
                if del.job.kind.cleanup().is_some_and(special::Cleanup::rescan_after_partial)
                    && self.live.is_none()
                    && let Some(root) = self.doc.as_ref().map(Doc::root)
                {
                    self.start_scan(root, ctx);
                }
            }
            Err(e) => {
                if let Some(live) = &self.live
                    && self.doc.as_ref().is_some_and(|d| d.id == del.job.doc)
                {
                    live.invalidate(path.clone());
                }
                self.delete_errors.push(DeleteError { path: path.clone(), message: e });
            }
        }
    }

    fn poll_live(&mut self) {
        let update = self.live.as_ref().map(|live| live.rx.try_recv());
        match update {
            Some(Ok(crate::watching::Update::Snapshot(mut snapshot))) => {
                self.recycled.retain(|(path, at)| {
                    let Some(node) = snapshot.tree.find_path(path).filter(|&n| n != ROOT) else {
                        return false; // The watcher has caught up.
                    };
                    Arc::make_mut(&mut snapshot.tree).remove(node);
                    at.elapsed() < RECYCLED_GRACE
                });
                if let Some(doc) = &mut self.doc {
                    if snapshot.reset {
                        doc.id = self.next_doc_id;
                        self.next_doc_id += 1;
                        doc.view = ROOT;
                    } else {
                        while !snapshot.tree.is_live(doc.view) {
                            doc.view = doc.tree.parent(doc.view).unwrap_or(ROOT);
                        }
                    }
                    self.map.clear_selection();
                    retire(std::mem::replace(&mut doc.tree, snapshot.tree));
                    retire(std::mem::replace(&mut doc.skipped, snapshot.skipped));
                    doc.files = doc.tree.root().files;
                    doc.folders = snapshot.dirs;
                    doc.disk = snapshot.disk;
                    doc.generation += 1;
                } else {
                    retire(snapshot);
                }
            }
            Some(Ok(crate::watching::Update::Status(status))) => {
                if status.is_stopped()
                    && let Some(live) = self.live.take()
                {
                    retire(live);
                }
                self.live_status = Some(status);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                if self.live_status.as_ref().is_some_and(LiveStatus::is_live) {
                    self.live_status = Some(LiveStatus::Disconnected);
                }
                if let Some(live) = self.live.take() {
                    retire(live);
                }
            }
            _ => {}
        }
    }

    fn apply(&mut self, cmd: Command, ctx: &egui::Context) {
        let _span = crate::perf::span("ui.command");
        match cmd {
            Command::ZoomPath(path) => {
                if let Some(n) = self.doc.as_ref().and_then(|d| d.tree.find_path(&path)) {
                    self.apply(Command::ZoomTo(n), ctx);
                }
            }
            Command::ZoomTo(n) => {
                if let Some(d) = &mut self.doc
                    && d.special().allows_zoom(n)
                    && d.tree.is_live(n)
                    && d.tree.node(n).is_dir()
                    && d.view != n
                {
                    self.history.push(d.tree.path(d.view));
                    d.view = n;
                    self.map.clear_selection();
                    ctx.request_repaint();
                }
            }
            Command::Back => {
                if let Some(d) = &mut self.doc
                    && let Some(n) = previous_view(&mut self.history, &d.tree, d.view)
                {
                    d.view = n;
                    self.map.clear_selection();
                    ctx.request_repaint();
                }
            }
            Command::ZoomOut => {
                if let Some(n) = self.doc.as_ref().and_then(|d| d.tree.parent(d.view)) {
                    self.apply(Command::ZoomTo(n), ctx);
                }
            }
            Command::ZoomFull => self.apply(Command::ZoomTo(ROOT), ctx),
            Command::Reveal(n) => {
                if let Some(d) = &self.doc
                    && let Err(e) = platform::reveal(&d.tree.path(n))
                {
                    self.error = Some(e);
                }
            }
            Command::Delete(n) => self.delete(n, ctx),
            Command::Special(kind, node) => self.request_special(kind, node, ctx),
            Command::OpenDrive => self.open = Some(OpenDialog::start(ctx)),
            Command::Rescan => {
                if let Some(root) = self.doc.as_ref().map(Doc::root) {
                    self.start_scan(root, ctx);
                }
            }
            Command::ToggleFree => {
                self.settings.show_free = !self.settings.show_free;
                self.save_settings();
            }
            Command::Properties(n) => {
                if let Some(d) = &self.doc {
                    self.props = Some(Props::request(d.tree.clone(), n, ctx));
                }
            }
        }
    }

    /// Move to the trash with no confirmation, as SpaceMonger did (it is undoable).
    /// Deletes queue behind the one in progress so the map stays usable.
    fn delete(&mut self, node: NodeId, ctx: &egui::Context) {
        if self.settings.disable_delete {
            return;
        }
        let Some(doc) = &self.doc else { return };
        if node == ROOT && doc.special().classify(&doc.tree, node) != Some(special::Kind::InstalledSoftware) {
            return;
        }
        if let Some(kind) = doc.special().classify(&doc.tree, node)
            && kind != special::Kind::InstalledSoftware
        {
            self.request_special(kind, node, ctx);
            return;
        }
        let path = doc.tree.path(node);
        // Already covered by a pending delete of itself or an ancestor.
        if self.pending_paths().any(|pending| path.starts_with(pending)) {
            return;
        }
        let n = doc.tree.node(node);
        let (size, files) = (n.size, n.files);
        let request = QueuedDelete { doc: doc.id, node, path, size, files, kind: DeleteKind::Recycle };
        if let Some(request) = self.special.check_removal(request, ctx) {
            self.queue_delete(request, ctx);
        }
    }

    fn queue_delete(&mut self, request: QueuedDelete, ctx: &egui::Context) {
        if self.settings.disable_delete {
            return;
        }
        // A queued descendant would fail once this folder is gone.
        self.delete_queue.retain(|q| !q.path.starts_with(&request.path));
        self.delete_queue.push_back(request);
        self.start_next_delete(ctx);
    }

    /// Everything being deleted, queued, or awaiting confirmation.
    fn pending_paths(&self) -> impl Iterator<Item = &PathBuf> {
        let paths = self
            .deleting
            .iter()
            .map(|d| &d.job.path)
            .chain(self.delete_queue.iter().map(|q| &q.path))
            .chain(self.confirm.iter().map(|q| &q.path));
        paths.chain(self.special.pending_paths())
    }

    /// Drop deleted paths from the map now. The live watcher can take a while to reconcile a
    /// large delete, and its snapshots until then must not bring them back.
    fn forget_removed(&mut self, targets: &[PathBuf], disk: Option<DiskInfo>) {
        let Some(doc) = &mut self.doc else { return };
        let tree = Arc::make_mut(&mut doc.tree);
        for target in targets {
            if let Some(node) = tree.find_path(target).filter(|&n| n != ROOT) {
                doc.folders = doc.folders.saturating_sub(tree.dir_count(node));
                tree.remove(node);
            }
            if let Some(live) = &self.live {
                live.invalidate(target.clone());
                self.recycled.push((target.clone(), Instant::now()));
            }
        }
        doc.files = doc.tree.root().files;
        doc.disk = disk.or(doc.disk.take());
        doc.generation += 1;
    }

    fn start_next_delete(&mut self, ctx: &egui::Context) {
        if self.deleting.is_some() {
            return;
        }
        let Some(job) = self.delete_queue.pop_front() else { return };
        let Some(doc) = &self.doc else {
            self.delete_queue.clear();
            return;
        };
        let (path, kind) = (&job.path, job.kind);
        // Earlier deletes or a rescan may have replaced the tree since this was queued.
        let node = doc.tree.find_path(path).filter(|&n| n != ROOT || kind.cleanup().is_some());
        // Contents-only cleanup preserves its root and any provider-defined exclusions.
        let targets = match (kind, node) {
            (DeleteKind::Cleanup(cleanup), Some(folder)) => doc
                .tree
                .node(folder)
                .children
                .iter()
                .filter(|&&c| {
                    !cleanup.keep().iter().any(|name| doc.tree.node(c).name_lossy().eq_ignore_ascii_case(name))
                })
                .map(|&c| doc.tree.path(c))
                .collect(),
            _ => vec![path.clone()],
        };
        let threads = match doc.disk.as_ref().map(|d| d.kind) {
            Some(clawback_core::adaptive::StorageKind::Rotational) => 2,
            _ => 8,
        };
        let (tx, rx) = mpsc::channel();
        let (p, repaint) = (path.clone(), ctx.clone());
        let mut disk = doc.disk.clone();
        let progress = Arc::new(crate::deletion::Progress::default());
        if kind != DeleteKind::Recycle {
            progress.set_total(job.files);
        }
        let worker_progress = progress.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            use crate::deletion::Recycled;
            let mut refresh = || {
                worker_progress.phase(crate::deletion::Phase::Updating);
                if let Some(disk) = &mut disk {
                    platform::refresh_disk_space(disk);
                }
                disk.take()
            };
            let result = match kind {
                DeleteKind::Recycle => crate::deletion::trash(&p, &worker_progress).map(|outcome| match outcome {
                    Recycled::Done => DeleteDone::Removed(refresh()),
                    Recycled::Declined => DeleteDone::Declined,
                    Recycled::TooLarge => DeleteDone::TooLarge,
                }),
                DeleteKind::Permanent | DeleteKind::Cleanup(_) => {
                    let target = crate::deletion::Target {
                        root: &p,
                        contents_only: kind.cleanup().is_some(),
                        keep: kind.cleanup().map_or(&[][..], special::Cleanup::keep),
                    };
                    let report = if let Some(cleanup) = kind.cleanup() {
                        cleanup.run(&target, threads, &worker_progress)
                    } else {
                        crate::deletion::purge(&target, threads, &worker_progress)
                    };
                    if report.failed == 0 && !report.cancelled {
                        Ok(DeleteDone::Removed(refresh()))
                    } else {
                        let error = report.first_error.and_then(|first| {
                            if let Some(cleanup) = kind.cleanup() {
                                cleanup.failure_message(report.failed, first)
                            } else {
                                Some(tr!("purge-failed", count = report.failed, error = first))
                            }
                        });
                        Ok(DeleteDone::Partial { error, disk: refresh() })
                    }
                }
            };
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.deleting = Some(Deleting { job, node, targets, rx, progress, started });
    }

    /// A dialog or window that the keyboard belongs to is showing.
    fn dialog_open(&self) -> bool {
        self.open.is_some()
            || self.setup.is_some()
            || self.error.is_some()
            || self.special.is_open()
            || !self.confirm.is_empty()
            || self.about
            || self.props.is_some()
            || self.show_unreadable
    }

    /// Keyboard shortcuts for the toolbar commands (SpaceMonger had none; the
    /// mouse behaviour is unchanged).
    fn keys(&mut self, ctx: &egui::Context) {
        if self.dialog_open() || ctx.egui_wants_keyboard_input() || ctx.any_popup_open() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::NONE, Key::Escape)
            || pressed(Modifiers::NONE, Key::Backspace)
            || pressed(Modifiers::ALT, Key::ArrowLeft)
            || ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1))
        {
            self.apply(Command::Back, ctx);
            return;
        }
        if self.scan.is_some() {
            return;
        }
        let tool = if pressed(Modifiers::COMMAND, Key::O) {
            Some(Tool::Open)
        } else if pressed(Modifiers::NONE, Key::F5) {
            Some(Tool::Rescan)
        } else if pressed(Modifiers::NONE, Key::Home) {
            Some(Tool::ZoomFull)
        } else if pressed(Modifiers::NONE, Key::Enter) {
            Some(if self.map.selected_is_folder() { Tool::ZoomIn } else { Tool::Reveal })
        } else if pressed(Modifiers::NONE, Key::Delete) {
            Some(Tool::Delete)
        } else {
            None
        };
        if let Some(t) = tool {
            self.tool(t, ctx);
        }
    }

    fn tool(&mut self, t: Tool, ctx: &egui::Context) {
        let zoomed = self.doc.as_ref().is_some_and(|d| d.view != ROOT);
        let sel = self.map.selected_node();
        match t {
            Tool::Back => self.apply(Command::Back, ctx),
            Tool::Open => self.apply(Command::OpenDrive, ctx),
            Tool::Rescan => self.apply(Command::Rescan, ctx),
            Tool::ZoomFull if zoomed => self.map.zoom_full(),
            Tool::ZoomOut if zoomed => self.map.zoom_out(),
            Tool::ZoomIn => self.map.zoom_in(),
            Tool::Free => self.apply(Command::ToggleFree, ctx),
            Tool::Reveal => {
                if let Some(n) = sel {
                    self.apply(Command::Reveal(n), ctx);
                }
            }
            Tool::Delete => {
                if let Some(n) = sel {
                    self.apply(Command::Delete(n), ctx);
                }
            }
            Tool::Setup => self.setup = Some(self.settings.clone()),
            Tool::About => self.about = true,
            Tool::Unreadable => self.show_unreadable = true,
            Tool::ZoomFull | Tool::ZoomOut => {}
        }
    }

    /// Primary actions, navigation, and an at-a-glance disk summary.
    fn toolbar(&mut self, ui: &mut Ui) -> Option<Tool> {
        let _span = crate::perf::span("ui.toolbar");
        let ready = self.doc.is_some() && self.scan.is_none();
        let zoomed = self.doc.as_ref().is_some_and(|d| d.view != ROOT);
        let selected = self.map.selected_node().is_some();
        let mut action = None;
        ui.spacing_mut().button_padding = vec2(8.0, 3.0);
        ui.spacing_mut().interact_size.y = 24.0;
        let bar_key = Id::new("menu_bar_open");
        let was_open = ui.data(|d| d.get_temp::<bool>(bar_key).unwrap_or(false));
        let mut open = false;
        egui::MenuBar::new().ui(ui, |ui| {
            bar_menu(ui, was_open, &mut open, tr!("file"), |ui| {
                let open_shortcut = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::O));
                if menu_item(ui, tr!("open-folder"), true, &open_shortcut) {
                    action = Some(Tool::Open);
                }
                if menu_item(ui, tr!("rescan"), ready, "F5") {
                    action = Some(Tool::Rescan);
                }
                ui.separator();
                if menu_item(ui, platform::file_manager_label(), ready && selected, "") {
                    action = Some(Tool::Reveal);
                }
                if menu_item(
                    ui,
                    tr!("move-selected-item-to-trash"),
                    ready && selected && !self.settings.disable_delete,
                    "Delete",
                ) {
                    action = Some(Tool::Delete);
                }
                ui.separator();
                if menu_item(ui, tr!("settings"), true, "") {
                    action = Some(Tool::Setup);
                }
                ui.separator();
                if menu_item(ui, tr!("exit"), true, "") {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            bar_menu(ui, was_open, &mut open, tr!("view"), |ui| {
                for (label, enabled, shortcut, tool) in [
                    (tr!("back"), !self.history.is_empty(), "Backspace", Tool::Back),
                    (tr!("up-a-level"), ready && zoomed, "", Tool::ZoomOut),
                    (tr!("all-files"), ready && zoomed, "Home", Tool::ZoomFull),
                    (tr!("zoom-in-2"), ready && selected && self.map.selected_is_folder(), "Enter", Tool::ZoomIn),
                ] {
                    if menu_item(ui, label, enabled, shortcut) {
                        action = Some(tool);
                    }
                }
                ui.separator();
                let can_show_free = ready && self.doc.as_ref().is_some_and(|d| d.is_mount);
                let mut show_free = self.settings.show_free && can_show_free;
                if ui
                    .add_enabled(can_show_free, egui::Checkbox::new(&mut show_free, tr!("show-free-space-2")))
                    .changed()
                {
                    action = Some(Tool::Free);
                    ui.close();
                }
                if menu_item(ui, tr!("unreadable-folders"), self.doc.as_ref().is_some_and(|d| d.unreadable() > 0), "") {
                    action = Some(Tool::Unreadable);
                }
            });
            bar_menu(ui, was_open, &mut open, tr!("palette"), |ui| {
                if ui.checkbox(&mut self.settings.mute_palette, tr!("mute-colors")).changed() {
                    self.save_settings();
                }
                ui.separator();
                let font = egui::TextStyle::Button.resolve(ui.style());
                let label_width = clawback_core::palette::MAP_PRESETS
                    .iter()
                    .map(|(_, name)| ui.painter().layout_no_wrap((*name).into(), font.clone(), theme::TEXT).size().x)
                    .fold(0.0_f32, f32::max);
                let swatch_width = 14.0;
                let swatch_gap = 4.0;
                let preview_width = 8.0 * (swatch_width + swatch_gap) - swatch_gap;
                let row_width = label_width + preview_width + 40.0;
                ui.set_min_width(row_width);
                ui.spacing_mut().item_spacing.y = 3.0;
                for (scheme, name) in clawback_core::palette::MAP_PRESETS {
                    let selected = self.settings.file_color == scheme && self.settings.folder_color == scheme;
                    // The button owns the entire row, including the painted preview.
                    let response =
                        ui.add_sized(vec2(row_width, 28.0), egui::Button::selectable(selected, name).right_text(()));
                    let preview_x = response.rect.right() - preview_width - 8.0;
                    let preview = egui::Rect::from_min_size(
                        egui::pos2(preview_x, response.rect.center().y - 8.0),
                        vec2(preview_width, 16.0),
                    );
                    let muted = self.settings.mute_palette;
                    theme::paint_swatches(ui.painter(), preview, scheme, muted, swatch_gap, 2.0);
                    if response.clicked() {
                        self.settings.file_color = scheme;
                        self.settings.folder_color = scheme;
                        self.save_settings();
                        ui.close();
                    }
                }
            });
            bar_menu(ui, was_open, &mut open, tr!("help"), |ui| {
                if menu_item(ui, tr!("about-clawback"), true, "") {
                    action = Some(Tool::About);
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let counts = self
                    .scan
                    .as_ref()
                    .map(|r| {
                        let p = r.display_progress();
                        (p.bytes, p.files, p.dirs)
                    })
                    .or_else(|| self.doc.as_ref().map(|d| (d.tree.root().size, d.files, d.folders)));
                if let Some((bytes, files, folders)) = counts {
                    scan_summary(ui, bytes, files, folders);
                    ui.separator();
                }
                let path = self
                    .doc
                    .as_ref()
                    .map(|d| d.tree.path(d.view))
                    .or_else(|| self.scan.as_ref().map(|r| r.root.clone()));
                if let Some(path) = path {
                    let text = path.display().to_string();
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&text).color(theme::MUTED)).truncate()).on_hover_text(
                            match &self.live_status {
                                Some(status) => format!("{text}\n{}", status.text()),
                                None => text,
                            },
                        );
                    });
                }
            });
        });
        ui.data_mut(|d| d.insert_temp(bar_key, open));
        action
    }

    /// SpaceMonger's window title: the selection, or the current folder.
    fn compute_title(&self) -> String {
        let Some(doc) = &self.doc else { return "Clawback".to_owned() };
        if let Some(n) = self.map.selected_node() {
            let size = doc.tree.node(n).size;
            format!(
                "{}  -  {}  -  {}  -  Clawback",
                doc.tree.path(n).display(),
                format::percent(size, doc.total_space()),
                format::size(size)
            )
        } else {
            let size = if doc.view == ROOT { doc.total_space() } else { doc.tree.node(doc.view).size };
            tr!(
                "window-title",
                path = dir_display(&doc.tree.path(doc.view)),
                total = format::size(size),
                free = format::size(doc.free_space())
            )
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.open else { return };
        let icons = self.picker_icons.get_or_insert_with(|| PickerIcons::load(ctx));
        let browse_from = self.doc.as_ref().map(|d| d.tree.root_path());
        match crate::picker::show(ctx, dlg, icons, &self.settings.recent, browse_from) {
            Some(Picked::Scan { path, turbo }) => {
                self.open = None;
                self.start_scan(path, ctx);
                #[cfg(windows)]
                if turbo && let Some(run) = &self.scan {
                    run.turbo.request();
                }
                #[cfg(not(windows))]
                let _ = turbo;
            }
            Some(Picked::Cancelled) => self.open = None,
            None => {}
        }
    }

    /// Non-modal scan and delete activity, stacked in the lower right over the map.
    fn toasts(&mut self, ctx: &egui::Context) {
        if self.scan.is_none() && self.deleting.is_none() && self.delete_errors.is_empty() {
            return;
        }
        let card = |ui: &mut Ui, add_contents: &mut dyn FnMut(&mut Ui)| {
            egui::Frame::new()
                .fill(theme::NAVIGATOR)
                .stroke(egui::Stroke::new(1.0, theme::PANEL_EDGE))
                .corner_radius(10)
                .inner_margin(12)
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 20,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(140),
                })
                .show(ui, |ui| {
                    ui.set_width((ctx.content_rect().width() - 56.0).min(380.0));
                    add_contents(ui);
                });
        };
        egui::Area::new(Id::new("scan-toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .movable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let mut dismissed = None;
                for (i, error) in self.delete_errors.iter().enumerate() {
                    card(ui, &mut |ui| {
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(egui_phosphor::regular::WARNING_CIRCLE).color(theme::DANGER));
                                ui.strong(tr!("delete-failed-title"));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("×").on_hover_text(tr!("close-dialog")).clicked() {
                                        dismissed = Some(i);
                                    }
                                });
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(error.path.display().to_string()).small().color(theme::MUTED),
                                )
                                .wrap()
                                .selectable(true),
                            );
                            ui.separator();
                            ui.add(egui::Label::new(&error.message).wrap().selectable(true));
                            if ui.small_button(tr!("copy-error-details")).clicked() {
                                ui.ctx().copy_text(format!("{}\n\n{}", error.path.display(), error.message));
                            }
                        });
                    });
                }
                if let Some(i) = dismissed {
                    self.delete_errors.remove(i);
                }
                if self.deleting.is_some() {
                    card(ui, &mut |ui| self.delete_activity(ui));
                }
                if self.scan.is_some() {
                    card(ui, &mut |ui| self.scan_activity(ui));
                }
            });
    }

    /// Clawback's own confirmation before anything is deleted permanently.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(job) = self.confirm.front() else { return };
        let drive = self.doc.as_ref().map(|d| d.tree.root_path().display().to_string()).unwrap_or_default();
        let special::Confirmation { title, body, button } = job.kind.cleanup().map_or_else(
            || special::Confirmation {
                title: tr!("delete-permanently-title"),
                body: tr!("delete-too-big-for-recycle-bin", name = display_name(&job.path)),
                button: tr!("delete-permanently-button"),
            },
            |cleanup| cleanup.confirmation(&job.path, &drive),
        );
        let details = tr!("delete-size-files", size = format::size(job.size), files = job.files);
        let mut answer = None;
        let flat = theme::modal_frame().shadow(egui::Shadow::NONE);
        let modal = theme::modal("clawback-confirm-purge").frame(flat).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 440.0));
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(egui_phosphor::regular::WARNING).size(26.0).color(theme::DANGER));
                ui.label(RichText::new(title).size(20.0).strong().color(theme::TEXT));
            });
            ui.add(egui::Label::new(body).wrap()).on_hover_text(job.path.display().to_string());
            ui.label(RichText::new(details).color(theme::MUTED));
            ui.label(RichText::new(tr!("delete-cannot-be-undone")).strong().color(theme::DANGER));
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(theme::primary_button(button, theme::DANGER)).clicked() {
                    answer = Some(true);
                }
                if ui.add(theme::secondary_button(tr!("cancel"))).clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(confirmed) = answer {
            self.answer_confirm(confirmed, ctx);
        }
    }

    fn answer_confirm(&mut self, confirmed: bool, ctx: &egui::Context) {
        let Some(job) = self.confirm.pop_front() else { return };
        if confirmed && !self.settings.disable_delete {
            // Confirmed work goes ahead of plain recycling still in the queue.
            self.delete_queue.push_front(job);
            self.start_next_delete(ctx);
        }
    }

    fn delete_activity(&self, ui: &mut Ui) {
        let Some(d) = &self.deleting else { return };
        let progress = d.progress.snapshot();
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        ui.spacing_mut().item_spacing = vec2(8.0, 7.0);
        let purging = d.job.kind != DeleteKind::Recycle;
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            ui.strong(match progress.phase {
                crate::deletion::Phase::Preparing => tr!("delete-preparing"),
                crate::deletion::Phase::Recycling => tr!("delete-recycling"),
                crate::deletion::Phase::Deleting => {
                    d.job.kind.cleanup().map_or_else(|| tr!("delete-permanently"), special::Cleanup::progress_label)
                }
                crate::deletion::Phase::Updating => tr!("delete-updating"),
            });
            if purging && progress.phase == crate::deletion::Phase::Deleting {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(tr!("cancel")).clicked() {
                        d.progress.cancel();
                    }
                });
            }
        });
        let path = d.job.path.display().to_string();
        ui.add(egui::Label::new(RichText::new(&path).size(12.0)).truncate()).on_hover_text(&path);
        ui.label(
            RichText::new(tr!(
                "delete-details",
                size = format::size(d.job.size),
                seconds = d.started.elapsed().as_secs().to_string()
            ))
            .size(11.0)
            .color(theme::MUTED),
        );
        if purging && progress.total > 0 && progress.phase == crate::deletion::Phase::Deleting {
            ui.add(
                egui::ProgressBar::new(fraction(progress.done, progress.total)).fill(theme::DANGER).desired_height(4.0),
            );
            ui.label(
                RichText::new(tr!(
                    "delete-progress-files",
                    done = crate::i18n::count(progress.done.min(progress.total)),
                    total = crate::i18n::count(progress.total)
                ))
                .size(11.0)
                .color(theme::MUTED),
            );
        }
        if progress.total > 0 && progress.phase == crate::deletion::Phase::Recycling {
            ui.add(
                egui::ProgressBar::new(fraction(progress.done, progress.total)).fill(theme::ACCENT).desired_height(4.0),
            );
        }
        if !progress.current.is_empty() && progress.current != path {
            ui.add(egui::Label::new(RichText::new(&progress.current).size(11.0).color(theme::MUTED)).truncate())
                .on_hover_text(&progress.current);
        }
        if !self.delete_queue.is_empty() {
            let queued = tr!("delete-queued", count = self.delete_queue.len());
            let paths = self.delete_queue.iter().map(|q| q.path.display().to_string()).collect::<Vec<_>>().join("\n");
            ui.label(RichText::new(queued).size(11.0).color(theme::MUTED)).on_hover_text(paths);
        }
    }

    fn scan_activity(&self, ui: &mut Ui) {
        let _span = crate::perf::span("ui.status");
        let Some(run) = &self.scan else { return };
        ui.spacing_mut().item_spacing = vec2(8.0, 7.0);
        ui.spacing_mut().button_padding = vec2(8.0, 3.0);
        ui.spacing_mut().interact_size.y = 24.0;
        let paused = run.is_paused();
        let progress = run.display_progress();
        let turbo_leads = progress != run.progress;
        ui.horizontal(|ui| {
            if !paused {
                ui.add(egui::Spinner::new().size(14.0));
            }
            ui.strong(if paused { tr!("scan-paused") } else { tr!("scanning") });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(if paused { tr!("resume-scan") } else { tr!("pause-scan") }).clicked() {
                    run.toggle_pause();
                }
            });
        });
        scan_summary(ui, progress.bytes, progress.files, progress.dirs);
        let path = if turbo_leads { tr!("turbo-main-progress") } else { run.current.display().to_string() };
        ui.add(egui::Label::new(RichText::new(&path).size(11.0).color(theme::MUTED)).truncate()).on_hover_text(path);
        let fraction = run
            .disk
            .as_ref()
            .filter(|_| run.is_mount)
            .map(|disk| disk.total.saturating_sub(disk.free))
            .filter(|&used| used > 0)
            .map(|used| fraction(progress.bytes, used));
        ui.add(
            egui::ProgressBar::new(fraction.unwrap_or(0.0))
                .animate(!paused && fraction.is_none())
                .fill(theme::ACCENT)
                .desired_height(4.0),
        )
        .on_hover_text(tr!("mapped-bytes-relative-to-used-drive-space-an"));
        if progress.workers > 0 {
            ui.small(tr!("workers", count = progress.workers)).on_hover_text(tr!("workers-help"));
        }
        #[cfg(windows)]
        if run.turbo_available {
            use crate::turbo::Status;
            use clawback_core::scan::MftPhase;
            let status = run.turbo.status();
            let busy = matches!(status, Status::Requested | Status::AwaitingConsent | Status::Reading);
            egui::Frame::new().fill(theme::SURFACE).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, crate::picker::turbo_button()).clicked() {
                        run.turbo.request();
                    }
                    let (message, detail) = match status {
                        Status::Requested | Status::AwaitingConsent => (tr!("waiting-for-windows-permission"), None),
                        Status::Reading => {
                            let p = run.turbo.progress().unwrap_or_default();
                            let message = match p.phase {
                                MftPhase::Reading => tr!(
                                    "turbo-read-progress",
                                    percent = p.read.saturating_mul(100).checked_div(p.total).unwrap_or(0),
                                    records = crate::i18n::count(p.records)
                                ),
                                MftPhase::Resolving => tr!("turbo-resolving"),
                                MftPhase::Assembling => tr!("turbo-assembling", files = crate::i18n::count(p.files)),
                                MftPhase::Sorting => tr!("turbo-sorting"),
                                MftPhase::Transferring => tr!("turbo-transferring"),
                            };
                            (message, Some(tr!("turbo-progress-help")))
                        }
                        Status::Declined => (tr!("turbo-cancelled-normal-scan-continues"), None),
                        Status::Failed(error) => (tr!("turbo-unavailable-normal-scan-continues"), Some(error)),
                        Status::Idle => (tr!("try-a-faster-ntfs-scan-with-administrator-permission"), None),
                    };
                    let response = ui.add(egui::Label::new(RichText::new(&message).color(theme::TEXT)).truncate());
                    response.on_hover_text(detail.unwrap_or(message));
                });
            });
        }
    }

    /// Edit a draft; only Save applies changes.
    fn setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.setup.take() else { return };
        match crate::settings_ui::show(ctx, &mut s) {
            Some(true) => {
                s.recent = std::mem::take(&mut self.settings.recent);
                s.sanitize();
                if self.settings.language != s.language {
                    theme::set_fonts(ctx, crate::i18n::set_language(&s.language));
                    // Cached map galleys reference the previous font atlas.
                    self.map = MapView::default();
                }
                self.settings = s;
                self.file_types = crate::filetypes::FileTypes::default();
                self.props = None;
                self.save_settings();
            }
            Some(false) => {}
            None => self.setup = Some(s),
        }
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(Id::new("clawback-about")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(vec2(72.0, 72.0), egui::Sense::hover());
                icon::paint(ui.painter(), rect);
                ui.add_space(10.0);
                ui.label(RichText::new("Clawback").size(24.0).strong().color(theme::TEXT));
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).color(theme::MUTED));
                ui.add_space(12.0);
                ui.label(tr!("a-fast-cross-platform-disk-space-map"));
                ui.label(RichText::new(tr!("claw-back-your-disk-space")).color(theme::ACCENT));
                ui.add_space(8.0);
                let repository = env!("CARGO_PKG_REPOSITORY");
                ui.hyperlink_to(repository.trim_start_matches("https://"), repository);
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(RichText::new(tr!("copyright-2026-azazel-labs")).size(11.0).color(theme::MUTED));
                ui.label(
                    RichText::new(tr!("licensed-under-mit-0-mit-no-attribution-no")).size(11.0).color(theme::MUTED),
                );
                ui.add_space(10.0);
                if ui.button(tr!("ok")).clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.about = false;
        }
    }

    fn unreadable_window(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else { return };
        if !self.show_unreadable {
            return;
        }
        let mut open = true;
        egui::Window::new(tr!("folders-not-scanned")).open(&mut open).default_width(560.0).show(ctx, |ui| {
            ui.label(platform::permission_hint());
            ui.separator();
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show_rows(
                ui,
                row_h,
                doc.skipped.len(),
                |ui, range| {
                    for s in &doc.skipped[range] {
                        ui.horizontal(|ui| {
                            ui.weak(s.reason.label());
                            ui.label(s.path.display().to_string()).on_hover_text(&s.detail);
                        });
                    }
                },
            );
        });
        if !open {
            self.show_unreadable = false;
        }
    }

    /// The directory tree and file types above the map.
    fn directory_panel(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let directory_panel = "directory-tree-v2";
        #[cfg(feature = "screenshots")]
        let directory_panel = if crate::demo::capturing() { "demo-directories-v2" } else { directory_panel };
        // The split is a share of the window, so it survives restarts and window resizes.
        // egui's own saved panel size is clamped to whatever height the first frames had.
        let panel_id = Id::new(directory_panel);
        let below_toolbar = ui.available_height();
        let (min_split, max_split) = clawback_core::settings::DIRECTORY_SPLIT;
        let max_height = from_permille(max_split, below_toolbar).max(100.0);
        let dragging = ctx.read_response(panel_id.with("__resize")).is_some_and(|r| r.dragged());
        if !dragging {
            let height = from_permille(self.settings.directory_split, below_toolbar).clamp(100.0, max_height);
            let outer_rect =
                egui::Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(ui.available_width(), height));
            ctx.data_mut(|d| d.insert_persisted(panel_id, egui::containers::panel::PanelState { outer_rect }));
        }
        let folder = egui::Panel::top(panel_id)
            .resizable(true)
            .size_range(100.0..=max_height)
            .frame(
                egui::Frame::new()
                    .fill(theme::NAVIGATOR)
                    .stroke(egui::Stroke::new(1.0, theme::PANEL_EDGE))
                    .inner_margin(6),
            )
            .show(ui, |ui| {
                let doc = self.doc.as_ref()?;
                let scope = doc.selected_folder(self.map.selected_node());
                if ui.available_width() >= 760.0 {
                    egui::Panel::right("file-types-panel-v2")
                        .resizable(true)
                        .default_size(ui.available_width() * 0.32)
                        .size_range(300.0..=ui.available_width() * 0.6)
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)))
                        .show(ui, |ui| self.file_types.ui(ui, &doc.tree, doc.id, doc.generation, scope));
                } else {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.narrow_types, false, tr!("directories"));
                        ui.selectable_value(&mut self.narrow_types, true, tr!("file-types"));
                    });
                    if self.narrow_types {
                        self.file_types.ui(ui, &doc.tree, doc.id, doc.generation, scope);
                        return None;
                    }
                }
                self.directories.ui(ui, &doc.tree, doc.id, doc.generation, scope, self.scan.is_some())
            });
        if dragging && below_toolbar > 0.0 {
            let split = to_permille(folder.response.rect.height(), below_toolbar);
            self.settings.directory_split = split.clamp(min_split, max_split);
            self.split_dragging = true;
        } else if std::mem::take(&mut self.split_dragging) {
            self.save_settings();
        }
        if let Some(node) = folder.inner {
            self.apply(Command::ZoomTo(node), ctx);
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        if let Some(msg) = &self.error {
            let mut close = false;
            let r = egui::Modal::new(Id::new("clawback-error")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(msg);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(tr!("ok")).clicked() {
                        close = true;
                    }
                });
            });
            if close || r.should_close() {
                self.error = None;
            }
        }
    }
}

impl Drop for ClawbackApp {
    fn drop(&mut self) {
        let _span = crate::perf::span("shutdown.app_drop");
        {
            let _span = crate::perf::span("shutdown.document_drop");
            drop(self.doc.take());
        }
        {
            let _span = crate::perf::span("shutdown.scan_and_watch_drop");
            drop(self.scan.take());
            drop(self.live.take());
        }
        {
            let _span = crate::perf::span("shutdown.map_drop");
            drop(std::mem::take(&mut self.map));
        }
        {
            let _span = crate::perf::span("shutdown.panels_drop");
            drop(std::mem::take(&mut self.directories));
            drop(std::mem::take(&mut self.file_types));
        }
    }
}

impl eframe::App for ClawbackApp {
    #[cfg(any(feature = "perf-probe", feature = "screenshots"))]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        #[cfg(feature = "perf-probe")]
        if let Some(probe) = &self.probe {
            probe.pointer(input);
        }
        #[cfg(feature = "screenshots")]
        crate::demo::isolate_input(input);
    }
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _span = crate::perf::frame(&ctx, frame);
        crate::startup::frame_started(&ctx);
        #[cfg(feature = "perf-probe")]
        self.probe_step(&ctx);
        #[cfg(feature = "screenshots")]
        crate::demo::capture(&ctx);
        self.poll(&ctx);
        self.keys(&ctx);

        // Scan progress floats over the map instead of reserving a permanent band,
        // so the menu bar only needs its 24 px items plus the frame margins.
        let tool = egui::Panel::top("compact-toolbar")
            .exact_size(30.0)
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.toolbar(ui))
            .inner;
        if let Some(t) = tool {
            self.tool(t, &ctx);
        }

        // Nothing to browse until a scan produces a tree; the map gets the whole window.
        if self.doc.is_some() {
            self.directory_panel(ui, &ctx);
        }

        let commands = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(8))
            .show(ui, |ui| {
                let settings = &self.settings;
                let pending_delete: Vec<(NodeId, bool)> = self
                    .doc
                    .as_ref()
                    .map(|d| {
                        let active = self.deleting.iter().filter(|x| x.job.doc == d.id).filter_map(|x| x.node);
                        let queued = self.delete_queue.iter().filter(|q| q.doc == d.id).map(|q| q.node);
                        active.map(|n| (n, true)).chain(queued.map(|n| (n, false))).collect()
                    })
                    .unwrap_or_default();
                let input = self.doc.as_ref().map(|d| MapInput {
                    scanning: self.scan.is_some(),
                    tree: &d.tree,
                    view: d.view,
                    generation: d.generation,
                    settings,
                    free: (settings.show_free && d.view == ROOT && d.is_mount).then(|| d.free_space()),
                    disk_free: d.free_space(),
                    disk_total: d.total_space(),
                    deleting: &pending_delete,
                    special: d.special(),
                });
                let mut out = self.map.ui(ui, input.as_ref());
                if self.doc.is_none() && self.scan.is_none() {
                    // The icon is the way in: click it to choose a drive.
                    let center = ui.max_rect().center();
                    let icon_rect = egui::Rect::from_center_size(center - vec2(0.0, 92.0), vec2(112.0, 112.0));
                    let response = ui
                        .interact(icon_rect, Id::new("empty-open-drive"), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tr!("select-drive-to-view"));
                    let hover = ui.ctx().animate_bool(response.id, response.hovered());
                    let p = ui.painter();
                    p.circle_filled(icon_rect.center(), 74.0, theme::ACCENT.gamma_multiply(0.04 + 0.08 * hover));
                    icon::paint(p, icon_rect.expand(6.0 * hover));
                    p.text(
                        center + vec2(0.0, 6.0),
                        Align2::CENTER_CENTER,
                        tr!("make-room-for-what-matters"),
                        egui::FontId::proportional(24.0),
                        theme::TEXT,
                    );
                    p.text(
                        center + vec2(0.0, 40.0),
                        Align2::CENTER_CENTER,
                        tr!("open-a-drive-or-folder-to-see-where"),
                        egui::FontId::proportional(13.0),
                        theme::MUTED,
                    );
                    if response.clicked() {
                        out.push(Command::OpenDrive);
                    }
                }
                out
            })
            .inner;
        for c in commands {
            self.apply(c, &ctx);
        }

        self.open_dialog(&ctx);
        self.toasts(&ctx);
        self.confirm_dialog(&ctx);
        self.setup_dialog(&ctx);
        self.about_dialog(&ctx);
        crate::properties::show(&ctx, &mut self.props);
        self.unreadable_window(&ctx);
        self.error_dialog(&ctx);
        self.special_dialogs(&ctx);

        let title = self.compute_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        crate::startup::frame_finished();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_settings();
    }

    fn on_exit(&mut self) {
        crate::perf::instant("shutdown.on_exit");
    }

    fn persist_egui_memory(&self) -> bool {
        self.persistent()
    }
}

fn previous_view(history: &mut Vec<PathBuf>, tree: &Tree, current: NodeId) -> Option<NodeId> {
    while let Some(path) = history.pop() {
        if let Some(n) = tree.find_path(&path)
            && tree.is_live(n)
            && tree.node(n).is_dir()
            && n != current
        {
            return Some(n);
        }
    }
    None
}

/// Settings store the directory split in thousandths of the height below the toolbar.
fn from_permille(permille: u32, whole: f32) -> f32 {
    whole * permille as f32 / 1000.0
}

fn to_permille(part: f32, whole: f32) -> u32 {
    (part / whole * 1000.0).round() as u32
}

/// Mapped size and counts, truncated to fit, in full on hover.
fn scan_summary(ui: &mut Ui, bytes: u64, files: u64, folders: u64) {
    let summary = tr!("scan-summary", size = format::size(bytes), files = files, folders = folders);
    ui.add(egui::Label::new(RichText::new(&summary).size(12.0)).truncate()).on_hover_text(summary);
}

/// A menu-bar menu that, as in native menu bars, opens on hover while another menu in the bar is open.
/// `was_open` is whether any bar menu was open last frame; `open` collects that for this frame.
fn bar_menu<'a>(
    ui: &mut Ui,
    was_open: bool,
    open: &mut bool,
    label: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Response {
    let response = ui.menu_button(label, add_contents).response;
    let popup = egui::Popup::default_response_id(&response);
    // Skip the frame its title was clicked, so clicking an open menu's title still closes it.
    if was_open && response.hovered() && !response.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup) {
        egui::Popup::open_id(ui.ctx(), popup);
        ui.ctx().request_repaint();
    }
    *open |= egui::Popup::is_id_open(ui.ctx(), popup);
    response
}

/// A menu command with a separate, right-aligned shortcut column.
fn menu_item(ui: &mut Ui, label: impl Into<egui::WidgetText>, enabled: bool, shortcut: &str) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn menu_bar_switches_menus_on_hover_once_one_is_open() {
        let ctx = egui::Context::default();
        let mut buttons = Vec::new();
        let mut frame = |events: Vec<egui::Event>| {
            buttons.clear();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let key = Id::new("menu_bar_open");
                let was_open = ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false));
                let mut open = false;
                egui::MenuBar::new().ui(ui, |ui| {
                    for name in ["File", "View"] {
                        let response = bar_menu(ui, was_open, &mut open, name, |ui| {
                            ui.label(name);
                        });
                        buttons.push((response.rect.center(), egui::Popup::default_response_id(&response)));
                    }
                });
                ui.data_mut(|d| d.insert_temp(key, open));
            });
            output.textures_delta.clear();
            buttons.clone()
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let [(file, file_popup), (view, view_popup)] = frame(vec![])[..] else { panic!() };

        // Hovering alone opens nothing.
        frame(vec![egui::Event::PointerMoved(view)]);
        frame(vec![]);
        assert!(!egui::Popup::is_any_open(&ctx));

        frame(vec![egui::Event::PointerMoved(file), button(file, true)]);
        frame(vec![button(file, false)]);
        assert!(egui::Popup::is_id_open(&ctx, file_popup));

        frame(vec![egui::Event::PointerMoved(view)]);
        frame(vec![]);
        assert!(egui::Popup::is_id_open(&ctx, view_popup));
        assert!(!egui::Popup::is_id_open(&ctx, file_popup));

        // Clicking the open menu's title still closes it.
        frame(vec![button(view, true)]);
        frame(vec![button(view, false)]);
        frame(vec![]);
        assert!(!egui::Popup::is_any_open(&ctx));
    }

    /// A real folder on disk with a matching in-memory tree, as a scan would produce.
    struct DeleteFixture {
        app: ClawbackApp,
        ctx: egui::Context,
        root: PathBuf,
    }

    impl DeleteFixture {
        fn new(name: &str, layout: &[(&str, bool)], is_mount: bool) -> Self {
            use clawback_core::tree::NewEntry;
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("delete-flow-tests")
                .join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let mut tree = Tree::new(&root);
            for (relative, dir) in layout {
                let path = root.join(relative);
                if *dir {
                    std::fs::create_dir_all(&path).unwrap();
                } else {
                    std::fs::write(&path, b"clawback").unwrap();
                }
                let parent = path.parent().and_then(|p| tree.find_path(p)).expect("parent listed first");
                tree.add_children(
                    parent,
                    vec![{
                        let name = path.file_name().unwrap();
                        if *dir { NewEntry::dir(name) } else { NewEntry::file(name, 8) }
                    }],
                );
            }
            tree.sort_all();
            let mut app = ClawbackApp::with_settings(Settings { auto_rescan: false, ..Settings::default() });
            app.doc = Some(Doc { is_mount, ..Doc::counted(1, tree) });
            Self { app, ctx: egui::Context::default(), root }
        }

        fn node(&self, relative: &str) -> NodeId {
            self.app.doc.as_ref().unwrap().tree.find_path(&self.root.join(relative)).expect("in the tree")
        }

        fn in_tree(&self, relative: &str) -> bool {
            self.app.doc.as_ref().unwrap().tree.find_path(&self.root.join(relative)).is_some()
        }

        /// Pretend the Recycle Bin refused `relative`, as the shell does for oversized items.
        fn too_large(&mut self, relative: &str) {
            let node = self.node(relative);
            let (tx, rx) = mpsc::channel();
            tx.send(Ok(DeleteDone::TooLarge)).unwrap();
            let path = self.root.join(relative);
            self.app.deleting = Some(Deleting {
                job: QueuedDelete { doc: 1, node, path: path.clone(), size: 8, files: 2, kind: DeleteKind::Recycle },
                node: Some(node),
                targets: vec![path],
                rx,
                progress: Arc::new(crate::deletion::Progress::default()),
                started: Instant::now(),
            });
            self.app.poll(&self.ctx);
        }

        fn finish(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            while self.app.deleting.is_some() || !self.app.delete_queue.is_empty() || self.app.scan.is_some() {
                assert!(Instant::now() < deadline, "delete did not finish");
                self.app.poll(&self.ctx);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for DeleteFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const BIG: &[(&str, bool)] = &[("big", true), ("big/a.bin", false), ("big/b.bin", false), ("keep.txt", false)];

    #[test]
    fn map_files_focus_their_parent_and_folders_focus_themselves() {
        use clawback_core::tree::NewEntry;
        let mut tree = Tree::new(Path::new("/selection"));
        let folder = tree.add_children(ROOT, vec![NewEntry::dir("folder")]).start;
        let file = tree.add_children(folder, vec![NewEntry::file("file.txt", 8)]).start;
        let mut doc = Doc::counted(1, tree);
        assert_eq!(doc.selected_folder(Some(folder)), folder);
        assert_eq!(doc.selected_folder(Some(file)), folder);
        assert_eq!(doc.view, ROOT, "selection must not zoom the map");
        assert_eq!(doc.selected_folder(None), ROOT);
        Arc::make_mut(&mut doc.tree).remove(folder);
        assert_eq!(doc.selected_folder(Some(file)), ROOT);
    }

    #[cfg(windows)]
    #[test]
    fn cleaning_temp_asks_first_and_respects_disabled_deletion() {
        use clawback_core::tree::NewEntry;
        let path = special::windows::temp_folder::path().expect("current user's AppData");
        // In-memory tree only: this test never deletes the real user's Temp data.
        let mut tree = Tree::new(&path);
        tree.add_children(ROOT, vec![NewEntry::file("fixture.txt", 8)]);
        let mut app = ClawbackApp::with_settings(Settings::default());
        app.doc = Some(Doc::counted(1, tree));
        let ctx = egui::Context::default();
        app.apply(Command::Special(special::Kind::TempFolder, ROOT), &ctx);
        assert_eq!(app.confirm.len(), 1);
        assert!(app.confirm[0].kind == DeleteKind::Cleanup(special::Cleanup::TempFolder));
        assert_eq!(app.confirm[0].path, path);
        assert!(app.deleting.is_none() && app.delete_queue.is_empty());
        app.answer_confirm(false, &ctx);
        assert!(app.confirm.is_empty() && app.deleting.is_none());
        app.settings.disable_delete = true;
        app.apply(Command::Special(special::Kind::TempFolder, ROOT), &ctx);
        assert!(app.confirm.is_empty());
    }

    #[test]
    fn confirmed_temp_cleanup_preserves_the_folder_and_updates_its_contents() {
        let mut f = DeleteFixture::new(
            "temp-cleanup",
            &[
                ("Temp", true),
                ("Temp/a.bin", false),
                ("Temp/nested", true),
                ("Temp/nested/b.bin", false),
                ("keep.txt", false),
            ],
            false,
        );
        let node = f.node("Temp");
        let entry = f.app.doc.as_ref().unwrap().tree.node(node);
        f.app.confirm.push_back(QueuedDelete {
            doc: 1,
            node,
            path: f.root.join("Temp"),
            size: entry.size,
            files: entry.files,
            kind: DeleteKind::Cleanup(special::Cleanup::TempFolder),
        });
        f.app.answer_confirm(true, &f.ctx.clone());
        f.finish();
        assert!(f.app.delete_errors.is_empty(), "{:?}", f.app.delete_errors);
        assert!(f.root.join("Temp").is_dir() && f.in_tree("Temp"));
        assert!(!f.root.join("Temp/a.bin").exists() && !f.in_tree("Temp/a.bin"));
        assert!(!f.root.join("Temp/nested").exists());
        assert!(f.root.join("keep.txt").exists() && f.in_tree("keep.txt"));
    }

    #[cfg(windows)]
    #[test]
    fn steam_game_menu_routes_to_manifest_uninstall_without_deleting() {
        use special::windows::programs::Action;
        let mut f = DeleteFixture::new(
            "steam-menu",
            &[
                ("steamapps", true),
                ("steamapps/common", true),
                ("steamapps/common/Voyage", true),
                ("steamapps/common/Voyage/game.exe", false),
                ("steamapps/common/Voyage/assets", true),
                ("steamapps/common/Voyage/assets/large.pak", false),
                ("steamapps/appmanifest_123.acf", false),
            ],
            false,
        );
        std::fs::write(
            f.root.join("steamapps/appmanifest_123.acf"),
            r#""AppState" { "appid" "123" "name" "Voyage" "installdir" "Voyage" }"#,
        )
        .unwrap();
        for selected in
            ["steamapps/common/Voyage", "steamapps/common/Voyage/assets", "steamapps/common/Voyage/assets/large.pak"]
        {
            let node = f.node(selected);
            let doc = f.app.doc.as_ref().unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while doc.special().classify(&doc.tree, node) != Some(special::Kind::InstalledSoftware) {
                assert!(Instant::now() < deadline, "verified ownership lookup did not finish");
                std::thread::sleep(Duration::from_millis(5));
            }
            f.app.apply(Command::Special(special::Kind::InstalledSoftware, node), &f.ctx.clone());
            let deadline = Instant::now() + Duration::from_secs(20);
            while !f.app.special.removal_checks.is_empty() {
                assert!(Instant::now() < deadline);
                f.app.poll(&f.ctx);
                std::thread::sleep(Duration::from_millis(5));
            }
            let removal = f.app.special.removals.front().expect("uninstall chooser");
            assert!(removal.steam);
            assert_eq!(removal.applications.len(), 1);
            assert_eq!(removal.applications[0].action, Some(Action::Steam(123)));
            assert!(f.app.deleting.is_none() && f.app.delete_queue.is_empty());
            assert!(f.root.join("steamapps/common/Voyage/game.exe").exists());
            f.app.special.removals.clear();
        }
    }

    #[cfg(windows)]
    #[test]
    fn temp_cleanup_silently_skips_locked_files_and_rescans_without_auto_rescan() {
        use std::os::windows::fs::OpenOptionsExt;
        let mut f = DeleteFixture::new("temp-locked", BIG, false);
        let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(f.root.join("big/a.bin")).unwrap();
        let node = f.node("big");
        f.app.queue_delete(
            QueuedDelete {
                doc: 1,
                node,
                path: f.root.join("big"),
                size: 16,
                files: 2,
                kind: DeleteKind::Cleanup(special::Cleanup::TempFolder),
            },
            &f.ctx.clone(),
        );
        f.finish();
        assert!(f.app.delete_errors.is_empty(), "{:?}", f.app.delete_errors);
        assert!(f.root.join("big/a.bin").exists() && f.in_tree("big/a.bin"));
        assert!(!f.root.join("big/b.bin").exists() && !f.in_tree("big/b.bin"));
        assert!(f.in_tree("big") && f.in_tree("keep.txt"));
    }

    #[cfg(windows)]
    #[test]
    fn compaction_updates_only_its_file_without_restarting_the_scan() {
        use special::{
            Compacted,
            windows::wsl_disks::{DiskKind, Info},
        };
        let mut f = DeleteFixture::new("compact-refresh", BIG, false);
        let view = f.node("big");
        f.app.doc.as_mut().unwrap().view = view;
        let update = || Compacted {
            path: f.root.join("big/a.bin"),
            info: Info {
                kind: DiskKind::Linux,
                distribution: None,
                package: None,
                file_size: Some(4),
                size_on_disk: Some(2),
                modified: Some(123),
            },
            disk: None,
        };
        f.app.finish_compaction(999, update());
        assert_eq!(f.app.doc.as_ref().unwrap().tree.root().size, 24);
        f.app.finish_compaction(1, update());
        assert!(f.app.scan.is_none());
        let doc = f.app.doc.as_ref().unwrap();
        assert_eq!((doc.id, doc.view), (1, view));
        assert_eq!(doc.tree.root().size, 18);
        assert_eq!(doc.tree.node(view).size, 10);
        assert_eq!(doc.tree.node(doc.tree.find_path(&f.root.join("big/a.bin")).unwrap()).len, 4);
        assert!(f.in_tree("big/b.bin") && f.in_tree("keep.txt"));
    }

    #[cfg(windows)]
    #[test]
    fn wsl_disk_delete_opens_reclaim_dialog_without_queuing_deletion() {
        let mut f = DeleteFixture::new(
            "wsl-disk",
            &[
                ("Docker", true),
                ("Docker/wsl", true),
                ("Docker/wsl/disk", true),
                ("Docker/wsl/disk/docker_data.vhdx", false),
            ],
            false,
        );
        let relative = "Docker/wsl/disk/docker_data.vhdx";
        let node = f.node(relative);
        f.app.apply(Command::Delete(node), &f.ctx);
        assert_eq!(f.app.special.wsl_disk.as_deref(), Some(f.root.join(relative).as_path()));
        assert!(f.app.deleting.is_none() && f.app.delete_queue.is_empty() && f.app.confirm.is_empty());
        assert!(f.root.join(relative).exists());
        f.app.special.wsl_disk = None;
        f.app.apply(Command::Special(special::Kind::WslDisk, node), &f.ctx);
        assert!(f.app.special.wsl_disk.is_some());
        // Ordinary files must not acquire the WSL action.
        f.app.special.wsl_disk = None;
        f.app.apply(Command::Special(special::Kind::WslDisk, ROOT), &f.ctx);
        assert!(f.app.special.wsl_disk.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn unidentified_applications_do_not_offer_an_empty_chooser_or_delete_files() {
        let mut f = DeleteFixture::new("managed-program", BIG, false);
        let node = f.node("big");
        let path = f.root.join("big");
        let (sender, job) = crate::background::Job::manual(());
        sender
            .send(Some(special::windows::programs::Removal {
                path: path.clone(),
                steam: false,
                applications: Vec::new(),
            }))
            .unwrap();
        f.app
            .special
            .removal_checks
            .push((QueuedDelete { doc: 1, node, path, size: 16, files: 2, kind: DeleteKind::Recycle }, job));
        f.app.poll(&f.ctx);
        assert!(f.app.special.removals.is_empty());
        assert!(f.app.delete_queue.is_empty() && f.app.deleting.is_none());
        assert!(f.root.join("big/a.bin").exists());
        // Cancelling this chooser does not schedule a fallback raw deletion.
        f.app.special.removals.clear();
        f.app.poll(&f.ctx);
        assert!(f.app.delete_queue.is_empty() && f.app.deleting.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn delayed_removal_checks_cannot_delete_from_a_replaced_document() {
        let mut f = DeleteFixture::new("stale-program-check", BIG, false);
        let node = f.node("big");
        let (sender, job) = crate::background::Job::manual(());
        f.app.special.removal_checks.push((
            QueuedDelete { doc: 1, node, path: f.root.join("big"), size: 16, files: 2, kind: DeleteKind::Recycle },
            job,
        ));
        f.app.doc.as_mut().unwrap().id = 2;
        sender.send(None).unwrap();
        f.app.poll(&f.ctx);
        assert!(f.app.delete_queue.is_empty() && f.app.deleting.is_none() && f.app.special.removals.is_empty());
        assert!(f.root.join("big/a.bin").exists());
    }

    #[test]
    fn oversized_items_wait_for_confirmation_and_nothing_is_deleted() {
        let mut f = DeleteFixture::new("ask", BIG, false);
        f.too_large("big");
        assert_eq!(f.app.confirm.len(), 1);
        assert!(f.app.confirm[0].kind == DeleteKind::Permanent);
        assert!(f.app.deleting.is_none() && f.app.delete_errors.is_empty());
        assert!(f.root.join("big/a.bin").exists());
        assert!(f.in_tree("big"));
    }

    #[test]
    fn declining_the_confirmation_keeps_everything() {
        let mut f = DeleteFixture::new("decline", BIG, false);
        f.too_large("big");
        let ctx = f.ctx.clone();
        f.app.answer_confirm(false, &ctx);
        f.finish();
        assert!(f.app.confirm.is_empty() && f.app.delete_errors.is_empty());
        assert!(f.root.join("big/a.bin").exists());
        assert!(f.in_tree("big"));
    }

    #[test]
    fn confirming_purges_the_item_and_updates_the_map() {
        let mut f = DeleteFixture::new("confirm", BIG, false);
        f.too_large("big");
        let generation = f.app.doc.as_ref().unwrap().generation;
        let ctx = f.ctx.clone();
        f.app.answer_confirm(true, &ctx);
        f.finish();
        assert!(f.app.delete_errors.is_empty(), "{:?}", f.app.delete_errors);
        assert!(!f.root.join("big").exists());
        assert!(!f.in_tree("big"));
        assert!(f.root.join("keep.txt").exists() && f.in_tree("keep.txt"));
        let doc = f.app.doc.as_ref().unwrap();
        assert!(doc.generation > generation);
        // The root and keep.txt are all that is left.
        assert_eq!((doc.files, doc.folders), (1, 1));
    }

    #[test]
    fn disabled_delete_ignores_a_pending_confirmation() {
        let mut f = DeleteFixture::new("disabled", BIG, false);
        f.too_large("big");
        f.app.settings.disable_delete = true;
        let ctx = f.ctx.clone();
        f.app.answer_confirm(true, &ctx);
        f.finish();
        assert!(f.root.join("big/a.bin").exists());
    }

    #[test]
    fn a_cancelled_purge_reports_no_error_and_keeps_the_rest_on_the_map() {
        let mut f = DeleteFixture::new("cancel", BIG, false);
        f.too_large("big");
        let ctx = f.ctx.clone();
        f.app.answer_confirm(true, &ctx);
        if let Some(d) = &f.app.deleting {
            d.progress.cancel();
        }
        f.finish();
        assert!(f.app.delete_errors.is_empty(), "{:?}", f.app.delete_errors);
        // Whatever survived stays listed until a rescan or the watcher reconciles it.
        assert!(f.in_tree("big") || !f.root.join("big").exists());
    }

    #[test]
    fn a_failed_purge_is_reported_once_with_the_count() {
        let mut f = DeleteFixture::new("fail", BIG, false);
        f.too_large("big");
        // Hold one file open without delete sharing, so it cannot go.
        #[cfg(windows)]
        let _held = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).share_mode(0).open(f.root.join("big/a.bin")).unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.root.join("big"), std::fs::Permissions::from_mode(0o500)).unwrap();
        }
        let ctx = f.ctx.clone();
        f.app.answer_confirm(true, &ctx);
        f.finish();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.root.join("big"), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(f.app.delete_errors.len(), 1, "{:?}", f.app.delete_errors);
        let error = &f.app.delete_errors[0].message;
        // Windows blocks only the held file. A read-only Unix folder blocks both, and
        // which one is reported first follows the filesystem's (hashed) listing order.
        #[cfg(windows)]
        assert!(error.contains("1 item") && error.contains("a.bin"), "{error}");
        #[cfg(unix)]
        assert!(error.contains("2 items") && (error.contains("a.bin") || error.contains("b.bin")), "{error}");
        assert!(f.root.join("big/a.bin").exists());
        assert!(f.in_tree("big"), "a partly deleted folder stays until it is reconciled");
    }

    #[cfg(windows)]
    #[test]
    fn emptying_the_recycle_bin_asks_then_clears_only_the_users_folder() {
        let sid = special::windows::recycle_bin::user_sid().expect("current user's SID");
        let user = format!("$Recycle.Bin/{sid}");
        let layout = [
            ("$Recycle.Bin", true),
            (user.as_str(), true),
            (&format!("{user}/desktop.ini"), false),
            (&format!("{user}/$RABC123.txt"), false),
            (&format!("{user}/$IABC123.txt"), false),
            (&format!("{user}/$RDEF456"), true),
            (&format!("{user}/$RDEF456/inner.bin"), false),
            ("$Recycle.Bin/S-1-5-21-other-user", true),
            ("$Recycle.Bin/S-1-5-21-other-user/theirs.bin", false),
        ];
        let mut f = DeleteFixture::new("bin", &layout, true);
        let ctx = f.ctx.clone();
        f.app.apply(
            Command::Special(
                special::Kind::RecycleBin,
                f.app.doc.as_ref().unwrap().special().node(special::Kind::RecycleBin).unwrap(),
            ),
            &ctx,
        );
        assert_eq!(f.app.confirm.len(), 1, "emptying always asks first");
        assert!(f.app.confirm[0].kind == DeleteKind::Cleanup(special::Cleanup::RecycleBin));
        assert!(f.root.join(&user).join("$RABC123.txt").exists());

        f.app.answer_confirm(true, &ctx);
        f.finish();
        assert!(f.app.delete_errors.is_empty(), "{:?}", f.app.delete_errors);
        let left: Vec<_> = std::fs::read_dir(f.root.join(&user)).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["desktop.ini"]);
        assert!(f.root.join("$Recycle.Bin/S-1-5-21-other-user/theirs.bin").exists());
        assert!(!f.in_tree(&format!("{user}/$RDEF456")) && f.in_tree(&format!("{user}/desktop.ini")));
        assert!(f.in_tree("$Recycle.Bin/S-1-5-21-other-user/theirs.bin"));
    }

    #[test]
    fn back_follows_visited_views_and_skips_deleted_folders() {
        use clawback_core::tree::NewEntry;
        let mut tree = Tree::new(Path::new("/scan"));
        let mut add = |parent, name: &str| tree.add_children(parent, vec![NewEntry::dir(name)]).start;
        let a = add(ROOT, "a");
        let deep = add(a, "deep");
        let b = add(ROOT, "b");
        let mut history = vec![tree.path(ROOT), tree.path(deep), tree.path(b)];
        tree.remove(b);
        assert_eq!(previous_view(&mut history, &tree, a), Some(deep));
        assert_eq!(previous_view(&mut history, &tree, deep), Some(ROOT));
        assert_eq!(previous_view(&mut history, &tree, ROOT), None);
    }
}
