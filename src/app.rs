//! The application window: SpaceMonger's toolbar, commands and dialogs
//! around the folder map.
use crate::i18n::tr;

use crate::directoryview::DirectoryView;
use crate::mapview::{Command, MapInput, MapView};
use crate::platform::{self, DiskInfo};
use crate::scanning::{Running, Update};
use crate::theme;
use crate::{background::retire, icon};
use clawback_core::{NodeId, ROOT, Settings, SkipReason, Skipped, Tree, format};
use eframe::egui::{self, Align, Align2, Id, Key, Layout, Modifiers, RichText, Ui, vec2};
use std::collections::VecDeque;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// A completed scan being viewed (SpaceMonger's `CFolderTree` document).
struct Doc {
    id: u64,
    tree: Arc<Tree>,
    root: PathBuf,
    view: NodeId,
    generation: u64,
    skipped: Arc<Vec<Skipped>>,
    disk: Option<DiskInfo>,
    is_mount: bool,
    files: u64,
    folders: u64,
}

impl Doc {
    /// SpaceMonger's `totalspace`: the drive size when viewing a whole drive,
    /// otherwise the scanned folder's size.
    fn total_space(&self) -> u64 {
        match &self.disk {
            Some(d) if self.is_mount => d.total,
            _ => self.tree.root().size,
        }
    }
    fn free_space(&self) -> u64 {
        self.disk.as_ref().map_or(0, |d| d.free)
    }
    fn recycle_bin(&self) -> Option<NodeId> {
        #[cfg(windows)]
        return crate::recycle_bin::find(&self.tree, self.is_mount);
        #[cfg(not(windows))]
        None
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
    /// Confirmed emptying of the user's Recycle Bin folder on the scanned drive.
    EmptyBin,
}

enum DeleteDone {
    /// Everything went; free space is re-read.
    Removed(Option<DiskInfo>),
    /// The user answered no to a Windows prompt; nothing changed.
    Declined,
    /// Too large for the Recycle Bin. Nothing was deleted; ask before purging.
    TooLarge,
    /// Cancelled or partly failed; the message explains failures.
    Partial(Option<String>, Option<DiskInfo>),
}
type DeleteResult = Result<DeleteDone, String>;

struct Deleting {
    doc: u64,
    node: Option<NodeId>,
    path: PathBuf,
    kind: DeleteKind,
    /// Paths that leave the map when the job succeeds.
    targets: Vec<PathBuf>,
    rx: mpsc::Receiver<DeleteResult>,
    progress: Arc<crate::deletion::Progress>,
    started: Instant,
    size: u64,
    files: u64,
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

/// Kept when emptying a Recycle Bin folder: Explorer's view settings for it.
const BIN_KEEP: &str = "desktop.ini";

/// How long a recycled path is hidden from live snapshots that predate its removal.
const RECYCLED_GRACE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Drive(usize),
    Recent(usize),
}

struct OpenDialog {
    drives: Vec<PickerDrive>,
    choice: Option<Choice>,
    loading: Option<mpsc::Receiver<Vec<PickerDrive>>>,
}

struct PickerDrive {
    disk: DiskInfo,
    #[cfg(windows)]
    turbo: bool,
}

struct Properties {
    title: String,
    rows: Vec<(String, String)>,
    path: PathBuf,
}

#[derive(Clone, Copy)]
enum Tool {
    Open,
    Rescan,
    ZoomFull,
    ZoomIn,
    ZoomOut,
    Back,
    Free,
    Run,
    Delete,
    Setup,
    About,
    Unreadable,
}

pub struct ClawbackApp {
    #[cfg(feature = "perf-probe")]
    probe: Option<crate::perf_probe::Probe>,
    settings: Settings,
    doc: Option<Doc>,
    next_doc_id: u64,
    history: Vec<PathBuf>,
    scan: Option<Running>,
    live: Option<crate::watching::Live>,
    live_status: String,
    map: MapView,
    directories: DirectoryView,
    file_types: crate::filetypes::FileTypes,
    narrow_types: bool,
    open: Option<OpenDialog>,
    setup: Option<Settings>,
    about: bool,
    props: Option<Properties>,
    props_rx: Option<mpsc::Receiver<Properties>>,
    show_unreadable: bool,
    deleting: Option<Deleting>,
    delete_queue: VecDeque<QueuedDelete>,
    delete_errors: Vec<String>,
    /// Recycled paths the live watcher may not have caught up with yet.
    recycled: Vec<(PathBuf, Instant)>,
    /// Permanent deletes waiting for the user's confirmation, asked one at a time.
    confirm: VecDeque<QueuedDelete>,
    error: Option<String>,
    title: String,
    /// The directory panel splitter is being dragged; its new share is saved on release.
    split_dragging: bool,
    /// Native picker icons: 0 fixed drive, 1 removable drive, [`FOLDER_ICON`] folder.
    picker_icons: std::collections::HashMap<u8, egui::TextureHandle>,
}

impl ClawbackApp {
    #[cfg(feature = "perf-probe")]
    fn probe_step(&mut self, ctx: &egui::Context) {
        use crate::perf_probe::Action;
        let action = self.probe.as_mut().and_then(|probe| probe.next(ctx));
        match action {
            Some(Action::Load(tree)) => {
                self.set_doc(Doc {
                    id: self.next_doc_id,
                    root: tree.root_path().to_path_buf(),
                    view: ROOT,
                    generation: 0,
                    files: tree.root().files,
                    folders: tree.dir_count(ROOT),
                    tree: Arc::new(tree),
                    skipped: Arc::new(Vec::new()),
                    disk: None,
                    is_mount: false,
                });
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
        ClawbackApp {
            #[cfg(feature = "perf-probe")]
            probe: crate::perf_probe::Probe::new(),
            settings,
            doc: None,
            next_doc_id: 1,
            history: Vec::new(),
            scan: None,
            live: None,
            live_status: String::new(),
            map: MapView::default(),
            directories: DirectoryView::default(),
            file_types: crate::filetypes::FileTypes::default(),
            narrow_types: false,
            open: None,
            setup: None,
            about: false,
            props: None,
            props_rx: None,
            show_unreadable: false,
            deleting: None,
            delete_queue: VecDeque::new(),
            delete_errors: Vec::new(),
            recycled: Vec::new(),
            confirm: VecDeque::new(),
            error: None,
            title: String::new(),
            split_dragging: false,
            picker_icons: std::collections::HashMap::new(),
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, path: Option<PathBuf>) -> Self {
        let language = crate::i18n::set_language(&settings.language);
        crate::startup::mark("translations_ready");
        theme::apply(&cc.egui_ctx, language);
        crate::startup::mark("theme_ready");
        #[allow(unused_mut)] // Only the screenshot build edits it further.
        let mut app = Self::with_settings(settings);
        #[cfg(feature = "screenshots")]
        if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() {
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
            app.live_status = "Demo data - fictional files and sizes".into();
            if view != ROOT {
                app.history.push(tree.root_path().to_path_buf());
            }
            app.doc = Some(Doc {
                id: 1,
                root: tree.root_path().to_path_buf(),
                view,
                generation: 0,
                files: tree.root().files,
                folders: tree.dir_count(ROOT),
                tree: Arc::new(tree),
                skipped: Arc::new(Vec::new()),
                disk: Some(crate::demo::disk()),
                is_mount: true,
            });
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

    fn save_settings(&self) {
        let _span = crate::perf::span("settings.save");
        #[cfg(feature = "perf-probe")]
        if self.probe.is_some() {
            return;
        }
        #[cfg(feature = "screenshots")]
        if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() {
            return;
        }
        let _ = self.settings.save();
    }

    fn start_scan(&mut self, root: PathBuf, ctx: &egui::Context) {
        if let Some(live) = self.live.take() {
            retire(live);
        }
        self.live_status.clear();
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
                self.props_rx = None;
                self.show_unreadable = false;
                self.map.reset();
                self.history.clear();
                self.scan = Some(scan);
                self.settings.push_recent(&root);
                let persist = true;
                #[cfg(feature = "perf-probe")]
                let persist = persist && self.probe.is_none();
                if persist {
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
        }
        if let Some(old) = self.doc.replace(doc) {
            retire(old);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let _span = crate::perf::span("ui.poll");
        if let Some(rx) = &self.props_rx {
            match rx.try_recv() {
                Ok(props) => {
                    self.props = Some(props);
                    self.props_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.props_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let update = self.scan.as_ref().map(|run| run.rx.try_recv());
        match update {
            Some(Ok(Update::Preview(preview))) => {
                let run = self.scan.as_mut().expect("active scan");
                run.progress = preview.progress;
                run.current = preview.current;
                run.disk.clone_from(&preview.disk);
                run.is_mount = preview.is_mount;
                #[cfg(windows)]
                {
                    run.turbo_available = preview.turbo_available;
                }
                let root = run.root.clone();
                self.set_doc(Doc {
                    id: self.next_doc_id,
                    tree: Arc::new(preview.tree),
                    root,
                    view: ROOT,
                    generation: self.doc.as_ref().map_or(0, |d| d.generation + 1),
                    skipped: Arc::new(Vec::new()),
                    disk: preview.disk,
                    is_mount: preview.is_mount,
                    files: preview.progress.files,
                    folders: preview.progress.dirs,
                });
            }
            Some(Ok(Update::Finished(r, disk, is_mount, started))) => {
                self.scan = None;
                self.live = started.live;
                self.live_status = started.status;
                self.set_doc(Doc {
                    id: self.next_doc_id,
                    tree: Arc::new(r.tree),
                    root: r.root,
                    view: ROOT,
                    generation: self.doc.as_ref().map_or(0, |d| d.generation + 1),
                    skipped: Arc::new(r.skipped),
                    disk,
                    is_mount,
                    files: r.files,
                    folders: r.dirs,
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

        let finished = self.deleting.as_ref().and_then(|d| match d.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(tr!("delete-worker-stopped"))),
        });
        if let Some(result) = finished
            && let Some(del) = self.deleting.take()
        {
            match result {
                Ok(DeleteDone::Removed(disk)) => {
                    self.forget_removed(&del.targets, disk);
                    if self.delete_queue.is_empty()
                        && self.live.is_none()
                        && self.settings.auto_rescan
                        && let Some(root) = self.doc.as_ref().map(|d| d.root.clone())
                    {
                        self.start_scan(root, ctx);
                    }
                }
                Ok(DeleteDone::Declined) => {
                    // Keep it on the map, but let the watcher catch any partial change.
                    if let Some(live) = &self.live {
                        live.invalidate(del.path.clone());
                    }
                }
                Ok(DeleteDone::TooLarge) => {
                    if let Some(node) = del.node {
                        self.confirm.push_back(QueuedDelete {
                            doc: del.doc,
                            node,
                            path: del.path.clone(),
                            size: del.size,
                            files: del.files,
                            kind: DeleteKind::Permanent,
                        });
                    }
                }
                Ok(DeleteDone::Partial(error, disk)) => {
                    // Some of it is gone: the watcher (or a rescan) reconciles what remains.
                    if let Some(live) = &self.live {
                        for target in &del.targets {
                            live.invalidate(target.clone());
                        }
                    }
                    if let (Some(doc), Some(disk)) = (&mut self.doc, disk) {
                        doc.disk = Some(disk);
                        doc.generation += 1;
                    }
                    if let Some(error) = error {
                        self.delete_errors.push(tr!(
                            "delete-error",
                            path = del.path.display().to_string(),
                            error = error.as_str()
                        ));
                    }
                }
                Err(e) => {
                    if let Some(live) = &self.live
                        && self.doc.as_ref().is_some_and(|d| d.id == del.doc)
                    {
                        live.invalidate(del.path.clone());
                    }
                    self.delete_errors.push(tr!(
                        "delete-error",
                        path = del.path.display().to_string(),
                        error = e.as_str()
                    ));
                }
            }
            self.start_next_delete(ctx);
        }
        if self.deleting.is_none() {
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
                    if status.starts_with("Live stopped:")
                        && let Some(live) = self.live.take()
                    {
                        retire(live);
                    }
                    self.live_status = status;
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.live_status.starts_with("Live ·") {
                        self.live_status = "Live stopped · Rescan to reconnect".into();
                    }
                    if let Some(live) = self.live.take() {
                        retire(live);
                    }
                }
                _ => {}
            }
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
                    && d.recycle_bin() != Some(n)
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
            Command::RunOpen(n) => {
                if let Some(d) = &self.doc
                    && let Err(e) = platform::open(&d.tree.path(n))
                {
                    self.error = Some(e);
                }
            }
            Command::Delete(n) => self.delete(n, ctx),
            Command::EmptyRecycleBin => {
                #[cfg(windows)]
                self.request_empty_recycle_bin();
            }
            Command::OpenDrive => {
                let (tx, rx) = mpsc::channel();
                let repaint = ctx.clone();
                std::thread::spawn(move || {
                    let drives = platform::drive_list()
                        .into_iter()
                        .map(|disk| PickerDrive {
                            #[cfg(windows)]
                            turbo: crate::turbo::eligible(Some(&disk), true),
                            disk,
                        })
                        .collect();
                    let _ = tx.send(drives);
                    repaint.request_repaint();
                });
                self.open = Some(OpenDialog { drives: Vec::new(), choice: None, loading: Some(rx) });
            }
            Command::Rescan => {
                if let Some(root) = self.doc.as_ref().map(|d| d.root.clone()) {
                    self.start_scan(root, ctx);
                }
            }
            Command::ToggleFree => {
                self.settings.show_free = !self.settings.show_free;
                self.save_settings();
            }
            Command::Properties(n) => {
                if let Some(d) = &self.doc {
                    let tree = d.tree.clone();
                    let repaint = ctx.clone();
                    let (tx, rx) = mpsc::channel();
                    self.props = None;
                    self.props_rx = Some(rx);
                    std::thread::spawn(move || {
                        let _ = tx.send(properties(&tree, n));
                        repaint.request_repaint();
                    });
                }
            }
        }
    }

    /// Move to the trash with no confirmation, as SpaceMonger did (it is undoable).
    /// Deletes queue behind the one in progress so the map stays usable.
    fn delete(&mut self, node: NodeId, ctx: &egui::Context) {
        if self.settings.disable_delete || node == ROOT {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let path = doc.tree.path(node);
        // Already covered by a pending delete of itself or an ancestor.
        if self.pending_paths().any(|pending| path.starts_with(pending)) {
            return;
        }
        // A queued descendant would fail once this folder is gone.
        self.delete_queue.retain(|q| !q.path.starts_with(&path));
        let n = doc.tree.node(node);
        let (size, files) = (n.size, n.files);
        self.delete_queue.push_back(QueuedDelete { doc: doc.id, node, path, size, files, kind: DeleteKind::Recycle });
        self.start_next_delete(ctx);
    }

    /// Everything being deleted, queued, or awaiting confirmation.
    fn pending_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.deleting
            .iter()
            .map(|d| &d.path)
            .chain(self.delete_queue.iter().map(|q| &q.path))
            .chain(self.confirm.iter().map(|q| &q.path))
    }

    /// Ask to empty the user's Recycle Bin folder on the scanned drive.
    #[cfg(windows)]
    fn request_empty_recycle_bin(&mut self) {
        if self.settings.disable_delete {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let Some(folder) = doc.recycle_bin().and_then(|bin| crate::recycle_bin::user_folder(&doc.tree, bin)) else {
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
            kind: DeleteKind::EmptyBin,
        });
    }

    /// Drop deleted paths from the map now. The live watcher can take a while to reconcile a
    /// large delete, and its snapshots until then must not bring them back.
    fn forget_removed(&mut self, targets: &[PathBuf], disk: Option<DiskInfo>) {
        let Some(doc) = &mut self.doc else { return };
        let tree = Arc::make_mut(&mut doc.tree);
        for target in targets {
            if let Some(node) = tree.find_path(target).filter(|&n| n != ROOT) {
                tree.remove(node);
            }
            if let Some(live) = &self.live {
                live.invalidate(target.clone());
                self.recycled.push((target.clone(), Instant::now()));
            }
        }
        doc.files = doc.tree.root().files;
        if disk.is_some() {
            doc.disk = disk;
        }
        doc.generation += 1;
    }

    fn start_next_delete(&mut self, ctx: &egui::Context) {
        if self.deleting.is_some() {
            return;
        }
        let Some(QueuedDelete { path, size, files, kind, .. }) = self.delete_queue.pop_front() else { return };
        let Some(doc) = &self.doc else {
            self.delete_queue.clear();
            return;
        };
        // Earlier deletes or a rescan may have replaced the tree since this was queued.
        let node = doc.tree.find_path(&path).filter(|&n| n != ROOT);
        // Emptying the bin keeps the user's folder and its desktop.ini; only the contents go.
        let targets = match (kind, node) {
            (DeleteKind::EmptyBin, Some(folder)) => doc
                .tree
                .node(folder)
                .children
                .iter()
                .filter(|&&c| !doc.tree.node(c).name_lossy().eq_ignore_ascii_case(BIN_KEEP))
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
            progress.set_total(files);
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
                DeleteKind::Permanent | DeleteKind::EmptyBin => {
                    let target = crate::deletion::Target {
                        root: &p,
                        contents_only: kind == DeleteKind::EmptyBin,
                        keep: &[BIN_KEEP],
                    };
                    let report = crate::deletion::purge(&target, threads, &worker_progress);
                    #[cfg(windows)]
                    if kind == DeleteKind::EmptyBin {
                        crate::recycle_bin::notify_changed(&p);
                    }
                    if report.failed == 0 && !report.cancelled {
                        Ok(DeleteDone::Removed(refresh()))
                    } else {
                        let error =
                            report.first_error.map(|first| tr!("purge-failed", count = report.failed, error = first));
                        Ok(DeleteDone::Partial(error, refresh()))
                    }
                }
            };
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.deleting = Some(Deleting { doc: doc.id, node, path, kind, targets, rx, progress, started, size, files });
    }

    fn busy(&self) -> bool {
        self.scan.is_some() || self.open.is_some() || self.setup.is_some() || self.error.is_some()
    }

    /// Keyboard shortcuts for the toolbar commands (SpaceMonger had none; the
    /// mouse behaviour is unchanged).
    fn keys(&mut self, ctx: &egui::Context) {
        if self.open.is_some()
            || self.setup.is_some()
            || self.error.is_some()
            || !self.confirm.is_empty()
            || self.about
            || self.props.is_some()
            || self.show_unreadable
            || ctx.egui_wants_keyboard_input()
            || ctx.any_popup_open()
        {
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
        if self.busy() {
            return;
        }
        let tool = if pressed(Modifiers::COMMAND, Key::O) {
            Some(Tool::Open)
        } else if pressed(Modifiers::NONE, Key::F5) {
            Some(Tool::Rescan)
        } else if pressed(Modifiers::NONE, Key::Home) {
            Some(Tool::ZoomFull)
        } else if pressed(Modifiers::NONE, Key::Backspace) {
            Some(Tool::ZoomOut)
        } else if pressed(Modifiers::NONE, Key::Enter) {
            Some(if self.map.selected_is_folder() { Tool::ZoomIn } else { Tool::Run })
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
            Tool::Run => {
                if let Some(n) = sel {
                    self.apply(Command::RunOpen(n), ctx);
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
                if menu_item(ui, tr!("open-selected-item"), ready && selected, "") {
                    action = Some(Tool::Run);
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
                    for depth in 0..8 {
                        let [r, g, b] =
                            clawback_core::palette::display_color(scheme, depth, self.settings.mute_palette);
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(
                                preview_x + depth as f32 * (swatch_width + swatch_gap),
                                response.rect.center().y - 8.0,
                            ),
                            vec2(swatch_width, 16.0),
                        );
                        ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                    }
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
                    let stats = tr!("scan-summary", size = format::size(bytes), files = files, folders = folders);
                    ui.add(egui::Label::new(RichText::new(&stats).size(12.0)).truncate()).on_hover_text(stats);
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
                            if self.live_status.is_empty() { text } else { format!("{text}\n{}", self.live_status) },
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
        let _span = crate::perf::span("ui.drive_picker");
        if self.open.is_none() {
            return;
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Shell::{SIID_DRIVEFIXED, SIID_DRIVEREMOVE, SIID_FOLDER};
            for (key, id) in [(0, SIID_DRIVEFIXED), (1, SIID_DRIVEREMOVE), (FOLDER_ICON, SIID_FOLDER)] {
                if !self.picker_icons.contains_key(&key)
                    && let Some(image) = crate::filetype_icons::windows::stock(id, 64)
                {
                    self.picker_icons.insert(key, ctx.load_texture("drive-icon", image, egui::TextureOptions::LINEAR));
                }
            }
        }
        let Some(dlg) = &mut self.open else { return };
        if let Some(rx) = &dlg.loading {
            match rx.try_recv() {
                Ok(drives) => {
                    dlg.drives = drives;
                    dlg.loading = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => dlg.loading = None,
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        let recent = &self.settings.recent;
        let mut chosen: Option<PathBuf> = None;
        #[cfg(windows)]
        let mut start_turbo = false;
        let (mut cancel, mut browse) = (false, false);
        let path_of = |c: Choice, dlg: &OpenDialog| match c {
            Choice::Drive(i) => dlg.drives[i].disk.mount.clone(),
            Choice::Recent(i) => recent[i].clone(),
        };
        let icons = &self.picker_icons;
        let modal = egui::Modal::new(Id::new("clawback-open-drive"))
            .backdrop_color(egui::Color32::from_black_alpha(170))
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::PANEL_EDGE))
                    .corner_radius(14)
                    .inner_margin(22)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 40,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(120),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 560.0));
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(RichText::new(tr!("select-drive-to-view")).size(24.0).strong().color(theme::TEXT));
                ui.add_space(4.0);
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 240.0).clamp(160.0, 420.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if dlg.loading.is_some() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.weak(tr!("finding-drives"));
                            });
                        } else if dlg.drives.is_empty() {
                            ui.weak(tr!("no-drives-found-use-other-folder-to-pick"));
                        }
                        for (i, drive) in dlg.drives.iter().enumerate() {
                            #[cfg(windows)]
                            let turbo = drive.turbo;
                            #[cfg(not(windows))]
                            let turbo = false;
                            let icon = icons.get(&u8::from(drive.disk.removable));
                            let r = drive_card(ui, &drive.disk, icon, turbo, dlg.choice == Some(Choice::Drive(i)));
                            if r.clicked() {
                                dlg.choice = Some(Choice::Drive(i));
                            }
                            if r.double_clicked() {
                                chosen = Some(drive.disk.mount.clone());
                            }
                        }
                        if !recent.is_empty() {
                            ui.add_space(8.0);
                            ui.label(RichText::new(tr!("recent")).size(11.0).strong().color(theme::MUTED));
                            for (i, p) in recent.iter().enumerate() {
                                let r =
                                    folder_card(ui, p, icons.get(&FOLDER_ICON), dlg.choice == Some(Choice::Recent(i)));
                                if r.clicked() {
                                    dlg.choice = Some(Choice::Recent(i));
                                }
                                if r.double_clicked() {
                                    chosen = Some(p.clone());
                                }
                            }
                        }
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(tr!("other-folder")).min_size(vec2(0.0, 34.0))).clicked() {
                        browse = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let open = egui::Button::new(RichText::new(tr!("ok").trim()).strong().color(theme::BG))
                            .fill(theme::ACCENT)
                            .min_size(vec2(96.0, 34.0));
                        if ui.add_enabled(dlg.choice.is_some(), open).clicked()
                            && let Some(c) = dlg.choice
                        {
                            chosen = Some(path_of(c, dlg));
                        }
                        #[cfg(windows)]
                        if matches!(dlg.choice, Some(Choice::Drive(i)) if dlg.drives[i].turbo)
                            && ui
                                .add(turbo_button())
                                .on_hover_text(tr!("try-a-faster-ntfs-scan-with-administrator-permission"))
                                .clicked()
                            && let Some(c) = dlg.choice
                        {
                            chosen = Some(path_of(c, dlg));
                            start_turbo = true;
                        }
                        if ui.add(egui::Button::new(tr!("cancel")).min_size(vec2(88.0, 34.0))).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if chosen.is_none()
            && ctx.input(|i| i.key_pressed(Key::Enter))
            && let Some(c) = dlg.choice
        {
            chosen = Some(path_of(c, dlg));
        }
        if modal.should_close() {
            cancel = true;
        }
        if browse {
            let start = self.doc.as_ref().map(|d| d.root.clone());
            chosen = platform::pick_folder(start.as_deref()).or(chosen);
        }
        if let Some(p) = chosen {
            self.open = None;
            self.start_scan(p, ctx);
            #[cfg(windows)]
            if start_turbo && let Some(run) = &self.scan {
                run.turbo.request();
            }
        } else if cancel || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = None;
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
                        ui.horizontal_top(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                if ui.small_button("×").on_hover_text(tr!("ok")).clicked() {
                                    dismissed = Some(i);
                                }
                                ui.add(egui::Label::new(RichText::new(error).color(theme::TEXT)).wrap());
                            });
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
        let empty_bin = job.kind == DeleteKind::EmptyBin;
        let name =
            job.path.file_name().map_or_else(|| job.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let drive = self.doc.as_ref().map(|d| d.root.display().to_string()).unwrap_or_default();
        let details = tr!("delete-size-files", size = format::size(job.size), files = job.files);
        let mut answer = None;
        let modal = egui::Modal::new(Id::new("clawback-confirm-purge"))
            .backdrop_color(egui::Color32::from_black_alpha(170))
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::PANEL_EDGE))
                    .corner_radius(14)
                    .inner_margin(22),
            )
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 440.0));
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(egui_phosphor::regular::WARNING).size(26.0).color(theme::DANGER));
                    ui.label(
                        RichText::new(if empty_bin {
                            tr!("empty-recycle-bin-title")
                        } else {
                            tr!("delete-permanently-title")
                        })
                        .size(20.0)
                        .strong()
                        .color(theme::TEXT),
                    );
                });
                ui.add(
                    egui::Label::new(if empty_bin {
                        tr!("empty-recycle-bin-body", drive = drive)
                    } else {
                        tr!("delete-too-big-for-recycle-bin", name = name)
                    })
                    .wrap(),
                )
                .on_hover_text(job.path.display().to_string());
                ui.label(RichText::new(details).color(theme::MUTED));
                ui.label(RichText::new(tr!("delete-cannot-be-undone")).strong().color(theme::DANGER));
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label =
                        if empty_bin { tr!("empty-recycle-bin-button") } else { tr!("delete-permanently-button") };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).strong().color(theme::BG))
                                .fill(theme::DANGER)
                                .min_size(vec2(0.0, 34.0)),
                        )
                        .clicked()
                    {
                        answer = Some(true);
                    }
                    if ui.add(egui::Button::new(tr!("cancel")).min_size(vec2(88.0, 34.0))).clicked() {
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
        let purging = d.kind != DeleteKind::Recycle;
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            ui.strong(match progress.phase {
                crate::deletion::Phase::Preparing => tr!("delete-preparing"),
                crate::deletion::Phase::Recycling => tr!("delete-recycling"),
                crate::deletion::Phase::Deleting if d.kind == DeleteKind::EmptyBin => tr!("emptying-recycle-bin"),
                crate::deletion::Phase::Deleting => tr!("delete-permanently"),
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
        let path = d.path.display().to_string();
        ui.add(egui::Label::new(RichText::new(&path).size(12.0)).truncate()).on_hover_text(&path);
        ui.label(
            RichText::new(tr!(
                "delete-details",
                size = format::size(d.size),
                seconds = d.started.elapsed().as_secs().to_string()
            ))
            .size(11.0)
            .color(theme::MUTED),
        );
        if purging && progress.total > 0 && progress.phase == crate::deletion::Phase::Deleting {
            ui.add(
                egui::ProgressBar::new((progress.done as f64 / progress.total as f64).min(1.0) as f32)
                    .fill(theme::DANGER)
                    .desired_height(4.0),
            );
            ui.label(
                RichText::new(tr!(
                    "delete-progress-files",
                    done = format::count(progress.done.min(progress.total)),
                    total = format::count(progress.total)
                ))
                .size(11.0)
                .color(theme::MUTED),
            );
        }
        if progress.total > 0 && progress.phase == crate::deletion::Phase::Recycling {
            ui.add(
                egui::ProgressBar::new(progress.done as f32 / progress.total as f32)
                    .fill(theme::ACCENT)
                    .desired_height(4.0),
            );
        }
        if !progress.current.is_empty() && progress.current != path {
            ui.add(egui::Label::new(RichText::new(&progress.current).size(11.0).color(theme::MUTED)).truncate())
                .on_hover_text(&progress.current);
        }
        if !self.delete_queue.is_empty() {
            let queued = tr!("delete-queued", count = self.delete_queue.len());
            let paths = self.delete_queue.iter().map(|q| q.path.display().to_string()).collect::<Vec<_>>().join(
                "
",
            );
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
        let summary =
            tr!("scan-summary", size = format::size(progress.bytes), files = progress.files, folders = progress.dirs);
        ui.add(egui::Label::new(RichText::new(&summary).size(12.0)).truncate()).on_hover_text(summary);
        let path = if turbo_leads { tr!("turbo-main-progress") } else { run.current.display().to_string() };
        ui.add(egui::Label::new(RichText::new(&path).size(11.0).color(theme::MUTED)).truncate()).on_hover_text(path);
        let fraction = run
            .disk
            .as_ref()
            .filter(|_| run.is_mount)
            .map(|disk| disk.total.saturating_sub(disk.free))
            .filter(|&used| used > 0)
            .map(|used| (progress.bytes as f64 / used as f64).min(1.0) as f32);
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
            let status = run.turbo.status();
            let busy = matches!(status, Status::Requested | Status::AwaitingConsent | Status::Reading);
            egui::Frame::new().fill(theme::SURFACE).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, turbo_button()).clicked() {
                        run.turbo.request();
                    }
                    let (message, detail) = match status {
                        Status::Requested | Status::AwaitingConsent => (tr!("waiting-for-windows-permission"), None),
                        Status::Reading => {
                            let p = run.turbo.progress().unwrap_or_default();
                            let message = match p.phase {
                                0 => tr!(
                                    "turbo-read-progress",
                                    percent =
                                        if p.total == 0 { 0 } else { (100.0 * p.read as f64 / p.total as f64) as u64 },
                                    records = format::count(p.records)
                                ),
                                1 => tr!("turbo-resolving"),
                                2 => tr!("turbo-assembling", files = format::count(p.files)),
                                3 => tr!("turbo-sorting"),
                                _ => tr!("turbo-transferring"),
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
        let Some(d) = &mut self.setup else { return };
        let done = crate::settings_ui::show(ctx, d);
        match done {
            Some(true) => {
                if let Some(mut s) = self.setup.take() {
                    s.recent = std::mem::take(&mut self.settings.recent);
                    s.sanitize();
                    let language_changed = self.settings.language != s.language;
                    self.settings = s;
                    let language = crate::i18n::set_language(&self.settings.language);
                    theme::set_fonts(ctx, language);
                    if language_changed {
                        // Cached map galleys reference the previous font atlas.
                        self.map = MapView::default();
                    }
                    self.file_types = crate::filetypes::FileTypes::default();
                    self.props = None;
                    self.props_rx = None;
                    self.save_settings();
                }
            }
            Some(false) => self.setup = None,
            None => {}
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
                ui.hyperlink_to("github.com/Azazel-Labs/Clawback", env!("CARGO_PKG_REPOSITORY"));
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

    fn properties_dialog(&mut self, ctx: &egui::Context) {
        if self.props_rx.is_some() {
            let mut open = true;
            egui::Window::new(tr!("properties")).id(Id::new("clawback-properties")).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr!("reading-file-details"));
                });
            });
            if !open {
                self.props_rx = None;
            }
        }
        let Some(p) = &self.props else { return };
        let mut open = true;
        let mut close = false;
        egui::Window::new(p.title.as_str())
            .id(Id::new("clawback-properties"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("props").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    for (k, v) in &p.rows {
                        ui.strong(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(tr!("show-in-file-manager")).clicked() {
                        let _ = platform::reveal(&p.path);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(tr!("ok")).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if !open || close {
            self.props = None;
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
            let rows: Vec<&Skipped> = doc.skipped.iter().collect();
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show_rows(
                ui,
                row_h,
                rows.len(),
                |ui, range| {
                    for s in &rows[range] {
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

    fn modal_dialogs(&mut self, ctx: &egui::Context) {
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
            let directory_panel = "directory-tree-v2";
            #[cfg(feature = "screenshots")]
            let directory_panel = if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() {
                "demo-directories-v2"
            } else {
                directory_panel
            };
            // The split is a share of the window, so it survives restarts and window resizes.
            // egui's own saved panel size is clamped to whatever height the first frames had.
            let panel_id = Id::new(directory_panel);
            let below_toolbar = ui.available_height();
            let (min_split, max_split) = clawback_core::settings::DIRECTORY_SPLIT;
            let max_height = (below_toolbar * max_split as f32 / 1000.0).max(100.0);
            let dragging = ctx.read_response(panel_id.with("__resize")).is_some_and(|r| r.dragged());
            if !dragging {
                let height = (below_toolbar * self.settings.directory_split as f32 / 1000.0).clamp(100.0, max_height);
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
                    {
                        let selected = self
                            .map
                            .selected_node()
                            .filter(|&id| {
                                (id as usize) < doc.tree.len()
                                    && !doc.tree.node(id).has(clawback_core::tree::flags::REMOVED)
                            })
                            .unwrap_or(doc.view);
                        let scope =
                            if doc.tree.node(selected).is_dir() { selected } else { doc.tree.node(selected).parent };
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
                        self.directories.ui(ui, &doc.tree, doc.id, doc.generation, doc.view, self.scan.is_some())
                    }
                });
            if dragging && below_toolbar > 0.0 {
                let split = (folder.response.rect.height() / below_toolbar * 1000.0).round() as u32;
                self.settings.directory_split = split.clamp(min_split, max_split);
                self.split_dragging = true;
            } else if std::mem::take(&mut self.split_dragging) {
                self.save_settings();
            }
            if let Some(node) = folder.inner {
                self.apply(Command::ZoomTo(node), &ctx);
            }
        }

        let commands = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(8))
            .show(ui, |ui| {
                let settings = &self.settings;
                let pending_delete: Vec<(NodeId, bool)> = self
                    .doc
                    .as_ref()
                    .map(|d| {
                        let active = self.deleting.iter().filter(|x| x.doc == d.id).filter_map(|x| x.node);
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
                    recycle_bin: d.recycle_bin(),
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
        self.properties_dialog(&ctx);
        self.unreadable_window(&ctx);
        self.modal_dialogs(&ctx);

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
        #[cfg(feature = "screenshots")]
        if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() {
            return false;
        }
        #[cfg(feature = "perf-probe")]
        if self.probe.is_some() {
            return false;
        }
        true
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

fn properties(t: &Tree, n: NodeId) -> Properties {
    let node = t.node(n);
    let path = t.path(n);
    let name = node.name_lossy().into_owned();
    let kind = match node.kind {
        clawback_core::Kind::Dir => tr!("folder-2"),
        clawback_core::Kind::File => tr!("file"),
        clawback_core::Kind::Symlink => tr!("symbolic-link"),
        clawback_core::Kind::Other => tr!("special-file"),
    };
    let mut rows = vec![
        (tr!("name"), name.clone()),
        (tr!("type-2"), kind),
        (tr!("location"), path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
        (tr!("size-2"), format::size(node.display_len())),
        (tr!("size-on-disk"), format::size(node.size)),
    ];
    if node.is_dir() {
        let folders = t.dir_count(n).saturating_sub(1);
        rows.push((tr!("contains"), tr!("contents-count", files = node.files, folders = folders)));
    }
    rows.push((tr!("modified"), format::date(node.mtime)));
    let attrs = platform::attributes(&path);
    if !attrs.is_empty() {
        rows.push((tr!("attributes-2"), attrs.join(" ")));
    }
    Properties { title: tr!("properties-title", name = name), rows, path }
}

/// Prominent, consistent action shared by the picker and scan status.
#[cfg(windows)]
fn turbo_button() -> egui::Button<'static> {
    egui::Button::new(RichText::new(format!("⚡  {}", tr!("turbo"))).strong().color(theme::BG))
        .fill(theme::ACCENT)
        .min_size(vec2(112.0, 34.0))
}

/// One drive in the picker: a large native icon, its name, a usage bar and free space.
fn drive_card(
    ui: &mut Ui,
    disk: &DiskInfo,
    icon: Option<&egui::TextureHandle>,
    turbo: bool,
    selected: bool,
) -> egui::Response {
    let (rect, response) = picker_card(ui, 78.0, selected);
    let p = ui.painter();
    let icon_rect = egui::Rect::from_center_size(rect.left_center() + vec2(42.0, 0.0), vec2(56.0, 56.0));
    if let Some(icon) = icon {
        p.image(
            icon.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        // Portable stand-in: a drive body with an activity light.
        let body = egui::Rect::from_center_size(icon_rect.center(), vec2(48.0, 30.0));
        p.rect_filled(body, 6, egui::Color32::from_rgb(70, 78, 88));
        p.circle_filled(body.right_center() - vec2(9.0, 0.0), 3.0, theme::ACCENT);
    }
    let left = icon_rect.right() + 16.0;
    let right = rect.right() - 16.0;
    let used = disk.total.saturating_sub(disk.free);
    let fraction = if disk.total == 0 { 0.0 } else { used as f32 / disk.total as f32 };
    let name = p.layout_no_wrap(disk.label(), egui::FontId::proportional(16.0), theme::TEXT);
    let mut name_right = right;
    if turbo {
        let badge = p.layout_no_wrap(format!("⚡ {}", tr!("turbo")), egui::FontId::proportional(11.0), theme::ACCENT);
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(right - badge.size().x - 14.0, rect.top() + 12.0),
            badge.size() + vec2(14.0, 6.0),
        );
        p.rect_filled(badge_rect, 9, theme::ACCENT.gamma_multiply(0.14));
        p.galley(badge_rect.min + vec2(7.0, 3.0), badge, theme::ACCENT);
        name_right = badge_rect.left() - 8.0;
    }
    let name_pos = egui::pos2(left, rect.top() + 12.0);
    p.with_clip_rect(egui::Rect::from_min_max(name_pos, egui::pos2(name_right, rect.bottom()))).galley(
        name_pos,
        name,
        theme::TEXT,
    );
    let bar = egui::Rect::from_min_max(egui::pos2(left, rect.top() + 40.0), egui::pos2(right, rect.top() + 46.0));
    p.rect_filled(bar, 3, theme::BG);
    let mut filled = bar;
    filled.max.x = bar.left() + bar.width() * fraction.clamp(0.0, 1.0);
    p.rect_filled(filled, 3, if fraction >= 0.9 { theme::DANGER } else { theme::ACCENT });
    p.text(
        egui::pos2(left, rect.bottom() - 14.0),
        Align2::LEFT_CENTER,
        tr!("drive-free-of-total", free = format::size(disk.free), total = format::size(disk.total)),
        egui::FontId::proportional(12.0),
        theme::MUTED,
    );
    p.text(
        egui::pos2(right, rect.bottom() - 14.0),
        Align2::RIGHT_CENTER,
        format!("{}  ·  {}", format::percent(used, disk.total), disk.fs),
        egui::FontId::proportional(12.0),
        theme::MUTED,
    );
    response.on_hover_text(disk.mount.display().to_string())
}

const FOLDER_ICON: u8 = 2;

/// A clickable picker card with hover and selection chrome; contents are painted by the caller.
fn picker_card(ui: &mut Ui, height: f32, selected: bool) -> (egui::Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::click());
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    let p = ui.painter();
    let fill = if selected { egui::Color32::from_rgb(35, 47, 58) } else { theme::NAVIGATOR };
    p.rect_filled(rect, 10, fill.lerp_to_gamma(theme::ROW_ALT, if selected { 0.0 } else { hover }));
    p.rect_stroke(
        rect,
        10,
        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { theme::ACCENT } else { theme::BORDER }),
        egui::StrokeKind::Inside,
    );
    (rect, response.on_hover_cursor(egui::CursorIcon::PointingHand))
}

/// A recently opened folder: its icon and name, with the containing folder beneath.
fn folder_card(ui: &mut Ui, path: &Path, icon: Option<&egui::TextureHandle>, selected: bool) -> egui::Response {
    let (rect, response) = picker_card(ui, 52.0, selected);
    let p = ui.painter();
    let icon_rect = egui::Rect::from_center_size(rect.left_center() + vec2(30.0, 0.0), vec2(32.0, 32.0));
    if let Some(icon) = icon {
        p.image(
            icon.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        // Portable stand-in: a folder tab and body.
        let body = egui::Rect::from_center_size(icon_rect.center() + vec2(0.0, 2.0), vec2(28.0, 20.0));
        p.rect_filled(egui::Rect::from_min_size(body.min - vec2(0.0, 4.0), vec2(12.0, 6.0)), 2, theme::FOLDER);
        p.rect_filled(body, 3, theme::FOLDER);
    }
    let left = icon_rect.right() + 14.0;
    let text = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(rect.right() - 14.0, rect.bottom()));
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let parent = path.parent().map(dir_display).unwrap_or_default();
    let clipped = p.with_clip_rect(text);
    clipped.text(
        egui::pos2(left, rect.top() + 17.0),
        Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(14.0),
        theme::TEXT,
    );
    clipped.text(
        egui::pos2(left, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        parent,
        egui::FontId::proportional(11.0),
        theme::MUTED,
    );
    response.on_hover_text(path.display().to_string())
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

/// A folder path with a trailing separator, as SpaceMonger titled folders.
fn dir_display(p: &Path) -> String {
    let mut s = p.display().to_string();
    if !s.ends_with(MAIN_SEPARATOR) {
        s.push(MAIN_SEPARATOR);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
            use clawback_core::tree::{Kind, NewEntry};
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
                    vec![NewEntry {
                        name: path.file_name().unwrap().into(),
                        kind: if *dir { Kind::Dir } else { Kind::File },
                        size: if *dir { 0 } else { 8 },
                        len: if *dir { 0 } else { 8 },
                        mtime: 0,
                        flags: 0,
                        file_id: None,
                    }],
                );
            }
            tree.sort_all();
            let mut app = ClawbackApp::with_settings(Settings { auto_rescan: false, ..Settings::default() });
            app.doc = Some(Doc {
                id: 1,
                root: root.clone(),
                view: ROOT,
                generation: 0,
                files: tree.root().files,
                folders: tree.dir_count(ROOT),
                tree: Arc::new(tree),
                skipped: Arc::new(Vec::new()),
                disk: None,
                is_mount,
            });
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
            self.app.deleting = Some(Deleting {
                doc: 1,
                node: Some(node),
                path: self.root.join(relative),
                kind: DeleteKind::Recycle,
                targets: vec![self.root.join(relative)],
                rx,
                progress: Arc::new(crate::deletion::Progress::default()),
                started: Instant::now(),
                size: 8,
                files: 2,
            });
            self.app.poll(&self.ctx);
        }

        fn finish(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            while self.app.deleting.is_some() || !self.app.delete_queue.is_empty() {
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
        assert!(f.app.doc.as_ref().unwrap().generation > generation);
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
        assert!(f.app.delete_errors[0].contains("a.bin"));
        assert!(f.root.join("big/a.bin").exists());
        assert!(f.in_tree("big"), "a partly deleted folder stays until it is reconciled");
    }

    #[cfg(windows)]
    #[test]
    fn emptying_the_recycle_bin_asks_then_clears_only_the_users_folder() {
        let sid = crate::recycle_bin::user_sid().expect("current user's SID");
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
        f.app.apply(Command::EmptyRecycleBin, &ctx);
        assert_eq!(f.app.confirm.len(), 1, "emptying always asks first");
        assert!(f.app.confirm[0].kind == DeleteKind::EmptyBin);
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
    fn picker_rows_keep_their_geometry_on_hover_and_selection() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, "en");
        let mut row = egui::Rect::NOTHING;
        let mut footer = egui::Rect::NOTHING;
        let mut draw = |pointer, selected| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 600.0))),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(440.0);
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for i in 0..20 {
                                let disk = DiskInfo {
                                    name: format!("Drive {i}: {}", "long drive name ".repeat(12)),
                                    mount: PathBuf::from(format!("/mnt/{i}")),
                                    fs: "NTFS".into(),
                                    total: 1 << 40,
                                    free: 100 << 30,
                                    removable: false,
                                    kind: clawback_core::adaptive::StorageKind::Unknown,
                                };
                                let response = drive_card(ui, &disk, None, true, selected);
                                if i == 0 {
                                    row = response.rect;
                                }
                            }
                        });
                    footer = ui.button("Other folder").rect;
                },
            );
            output.textures_delta.clear();
            (row, footer)
        };
        let outside = egui::pos2(620.0, 580.0);
        for _ in 0..4 {
            draw(outside, false);
        }
        let expected = draw(outside, false);
        for selected in [false, true, false] {
            for pointer in [expected.0.center(), egui::pos2(expected.0.right() + 6.0, expected.0.center().y), outside] {
                for _ in 0..4 {
                    assert_eq!(draw(pointer, selected), expected);
                }
            }
        }
    }

    #[test]
    fn back_follows_visited_views_and_skips_deleted_folders() {
        use clawback_core::tree::{Kind, NewEntry};
        let mut tree = Tree::new(Path::new("/scan"));
        let mut add = |parent, name: &str| {
            tree.add_children(
                parent,
                vec![NewEntry {
                    name: name.into(),
                    kind: Kind::Dir,
                    size: 0,
                    len: 0,
                    mtime: 0,
                    flags: 0,
                    file_id: None,
                }],
            )
            .start
        };
        let a = add(ROOT, "a");
        let deep = add(a, "deep");
        let b = add(ROOT, "b");
        let mut history = vec![tree.path(ROOT), tree.path(deep), tree.path(b)];
        tree.remove(b);
        assert_eq!(previous_view(&mut history, &tree, a), Some(deep));
        assert_eq!(previous_view(&mut history, &tree, deep), Some(ROOT));
        assert_eq!(previous_view(&mut history, &tree, ROOT), None);
    }

    #[test]
    fn directory_display_has_trailing_separator() {
        assert!(dir_display(Path::new("/a/b")).ends_with(MAIN_SEPARATOR));
    }
}
