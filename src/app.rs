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
    fn unreadable(&self) -> usize {
        self.skipped.iter().filter(|s| s.reason != SkipReason::OtherFilesystem).count()
    }
}

type DeleteResult = Result<(Arc<Tree>, Option<DiskInfo>), String>;

struct Deleting {
    doc: u64,
    path: PathBuf,
    rx: mpsc::Receiver<DeleteResult>,
    progress: Arc<crate::deletion::Progress>,
    started: Instant,
    size: u64,
}

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
    error: Option<String>,
    title: String,
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
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, path: Option<PathBuf>) -> Self {
        let language = crate::i18n::set_language(&settings.language);
        crate::startup::mark("translations_ready");
        theme::apply(&cc.egui_ctx, language);
        crate::startup::mark("theme_ready");
        let mut app = ClawbackApp {
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
            error: None,
            title: String::new(),
        };
        #[cfg(feature = "screenshots")]
        if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() {
            let tree = crate::demo::tree();
            let view = crate::demo::view(&tree);
            app.settings = Settings { show_free: false, ..Settings::default() };
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
                disk: None,
                is_mount: false,
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
                Ok((tree, disk)) => {
                    if let Some(live) = &self.live
                        && self.doc.as_ref().is_some_and(|d| d.id == del.doc)
                    {
                        live.invalidate(del.path.clone());
                        retire(tree);
                    } else if let Some(doc) = self.doc.as_mut().filter(|d| d.id == del.doc) {
                        retire(std::mem::replace(&mut doc.tree, tree));
                        doc.generation += 1;
                        if disk.is_some() {
                            doc.disk = disk;
                        }
                    } else {
                        retire(tree);
                    }
                    if self.live.is_none()
                        && self.settings.auto_rescan
                        && let Some(root) = self.doc.as_ref().map(|d| d.root.clone())
                    {
                        self.start_scan(root, ctx);
                    }
                }
                Err(e) => {
                    if let Some(live) = &self.live
                        && self.doc.as_ref().is_some_and(|d| d.id == del.doc)
                    {
                        live.invalidate(del.path.clone());
                    }
                    self.error = Some(tr!("delete-error", path = del.path.display().to_string(), error = e.as_str()));
                }
            }
        }
        if self.deleting.is_none() {
            let update = self.live.as_ref().map(|live| live.rx.try_recv());
            match update {
                Some(Ok(crate::watching::Update::Snapshot(snapshot))) => {
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
    fn delete(&mut self, node: NodeId, ctx: &egui::Context) {
        if self.settings.disable_delete || self.deleting.is_some() {
            return;
        }
        let Some(doc) = &self.doc else { return };
        if node == ROOT {
            return;
        }
        let path = doc.tree.path(node);
        let (tx, rx) = mpsc::channel();
        let (p, repaint) = (path.clone(), ctx.clone());
        let mut tree = doc.tree.clone();
        let mut disk = doc.disk.clone();
        let live = self.live.is_some();
        let progress = Arc::new(crate::deletion::Progress::default());
        let worker_progress = progress.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            let result = crate::deletion::trash(&p, &worker_progress).map(|()| {
                worker_progress.phase(crate::deletion::Phase::Updating);
                let _span = crate::perf::span("delete.reconcile");
                if !live {
                    Arc::make_mut(&mut tree).remove(node);
                    if let Some(disk) = &mut disk {
                        platform::refresh_disk_space(disk);
                    }
                }
                (tree, disk)
            });
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.deleting = Some(Deleting { doc: doc.id, path, rx, progress, started, size: doc.tree.node(node).size });
    }

    fn busy(&self) -> bool {
        self.scan.is_some()
            || self.deleting.is_some()
            || self.open.is_some()
            || self.setup.is_some()
            || self.error.is_some()
    }

    /// Keyboard shortcuts for the toolbar commands (SpaceMonger had none; the
    /// mouse behaviour is unchanged).
    fn keys(&mut self, ctx: &egui::Context) {
        if self.open.is_some()
            || self.setup.is_some()
            || self.error.is_some()
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
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr!("file"), |ui| {
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
            ui.menu_button(tr!("view"), |ui| {
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
            ui.menu_button(tr!("palette"), |ui| {
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
            ui.menu_button(tr!("help"), |ui| {
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
        egui::Window::new(tr!("select-drive-to-view"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if dlg.loading.is_some() {
                            ui.spinner();
                            ui.weak(tr!("finding-drives"));
                        } else if dlg.drives.is_empty() {
                            ui.weak(tr!("no-drives-found-use-other-folder-to-pick"));
                        }
                        for (i, drive) in dlg.drives.iter().enumerate() {
                            let d = &drive.disk;
                            let used = d.total.saturating_sub(d.free);
                            let text = tr!(
                                "drive-summary",
                                icon = if d.removable { "💾" } else { "💽" },
                                drive = d.label(),
                                free = format::size(d.free),
                                total = format::size(d.total),
                                used = format::percent(used, d.total),
                                filesystem = d.fs.as_str()
                            );
                            let r = picker_row(ui, dlg.choice == Some(Choice::Drive(i)), &text);
                            if r.clicked() {
                                dlg.choice = Some(Choice::Drive(i));
                            }
                            if r.double_clicked() {
                                chosen = Some(d.mount.clone());
                            }
                        }
                        if !recent.is_empty() {
                            ui.separator();
                            ui.weak(tr!("recent"));
                            for (i, p) in recent.iter().enumerate() {
                                let r = picker_row(
                                    ui,
                                    dlg.choice == Some(Choice::Recent(i)),
                                    &format!("📁  {}", p.display()),
                                );
                                if r.clicked() {
                                    dlg.choice = Some(Choice::Recent(i));
                                }
                                if r.double_clicked() {
                                    chosen = Some(p.clone());
                                }
                            }
                        }
                    });
                ui.separator();
                #[cfg(windows)]
                ui.horizontal(|ui| {
                    let eligible = match dlg.choice {
                        Some(Choice::Drive(i)) => dlg.drives[i].turbo,
                        _ => false,
                    };
                    if ui.add_enabled(eligible, turbo_button()).clicked()
                        && let Some(c) = dlg.choice
                    {
                        chosen = Some(path_of(c, dlg));
                        start_turbo = true;
                    }
                    ui.add(egui::Label::new(tr!("try-a-faster-ntfs-scan-with-administrator-permission")).wrap());
                });
                #[cfg(windows)]
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr!("other-folder")).clicked() {
                        browse = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(dlg.choice.is_some(), egui::Button::new(tr!("ok"))).clicked()
                            && let Some(c) = dlg.choice
                        {
                            chosen = Some(path_of(c, dlg));
                        }
                        if ui.button(tr!("cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
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

    /// A scan-only overlay; completing a scan returns the space to the map.
    fn scan_toast(&self, ctx: &egui::Context) {
        if self.scan.is_none() {
            return;
        }
        egui::Area::new(Id::new("scan-toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .movable(false)
            .show(ctx, |ui| {
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
                        self.scan_activity(ui);
                    });
            });
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
        egui::Window::new(tr!("about-clawback"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
                    icon::paint(ui.painter(), rect);
                    ui.vertical(|ui| {
                        ui.heading(format!("Clawback {}", env!("CARGO_PKG_VERSION")));
                        ui.label(tr!("a-fast-cross-platform-disk-space-map"));
                        ui.add_space(6.0);
                        ui.label(tr!("about-history",));
                        ui.add_space(6.0);
                        ui.label(tr!("claw-back-your-disk-space"));
                        ui.add_space(6.0);
                        ui.weak(tr!("copyright-2026-azazel-labs"));
                        ui.weak(tr!("licensed-under-mit-0-mit-no-attribution-no"));
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(tr!("ok")).clicked() {
                        close = true;
                    }
                });
            });
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
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
        if let Some(d) = &self.deleting {
            let progress = d.progress.snapshot();
            ctx.request_repaint_after(Duration::from_millis(100));
            egui::Modal::new(Id::new("clawback-deleting")).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(240.0, 420.0));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.strong(match progress.phase {
                        crate::deletion::Phase::Preparing => tr!("delete-preparing"),
                        crate::deletion::Phase::Recycling => tr!("delete-recycling"),
                        crate::deletion::Phase::Updating => tr!("delete-updating"),
                    });
                });
                ui.add_space(8.0);
                ui.add(egui::Label::new(d.path.display().to_string()).truncate())
                    .on_hover_text(d.path.display().to_string());
                ui.label(
                    RichText::new(tr!(
                        "delete-details",
                        size = format::size(d.size),
                        seconds = d.started.elapsed().as_secs().to_string()
                    ))
                    .color(theme::MUTED),
                );
                if progress.total > 0 && progress.phase == crate::deletion::Phase::Recycling {
                    ui.add_space(8.0);
                    ui.add(egui::ProgressBar::new(progress.done as f32 / progress.total as f32).show_percentage());
                }
                if !progress.current.is_empty() && progress.current != d.path.display().to_string() {
                    ui.add(egui::Label::new(RichText::new(&progress.current).small().color(theme::MUTED)).truncate())
                        .on_hover_text(&progress.current);
                }
            });
        }
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
    #[cfg(feature = "perf-probe")]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Some(probe) = &self.probe {
            probe.pointer(input);
        }
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

        // Scan progress floats over the map instead of reserving a permanent band.
        let band_height = (ui.available_height() * 0.05).clamp(32.0, 44.0);
        let tool = egui::Panel::top("compact-toolbar")
            .exact_size(band_height)
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.toolbar(ui))
            .inner;
        if let Some(t) = tool {
            self.tool(t, &ctx);
        }

        let directory_panel = "directory-tree-v2";
        #[cfg(feature = "screenshots")]
        let directory_panel =
            if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() { "demo-directories-v2" } else { directory_panel };
        let folder = egui::Panel::top(directory_panel)
            .resizable(true)
            .default_size(290.0)
            .size_range(100.0..=(ui.available_height() * 0.65).max(100.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::NAVIGATOR)
                    .stroke(egui::Stroke::new(1.0, theme::PANEL_EDGE))
                    .inner_margin(6),
            )
            .show(ui, |ui| {
                if let Some(doc) = &self.doc {
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
                } else {
                    ui.strong(tr!("directories"));
                    ui.weak(if self.scan.is_some() {
                        tr!("discovering-folders")
                    } else {
                        tr!("open-a-folder-or-drive-to-browse-its")
                    });
                    None
                }
            })
            .inner;
        if let Some(node) = folder {
            self.apply(Command::ZoomTo(node), &ctx);
        }

        let commands = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(8))
            .show(ui, |ui| {
                let settings = &self.settings;
                let input = self.doc.as_ref().map(|d| MapInput {
                    scanning: self.scan.is_some(),
                    tree: &d.tree,
                    view: d.view,
                    generation: d.generation,
                    settings,
                    free: (settings.show_free && d.view == ROOT && d.is_mount).then(|| d.free_space()),
                    disk_free: d.free_space(),
                    disk_total: d.total_space(),
                });
                let out = self.map.ui(ui, input.as_ref());
                if self.doc.is_none() && self.scan.is_none() {
                    let center = ui.max_rect().center();
                    let p = ui.painter();
                    for (offset, size, color) in [
                        (vec2(-54.0, -100.0), vec2(64.0, 62.0), egui::Color32::from_rgb(49, 65, 71)),
                        (vec2(16.0, -100.0), vec2(38.0, 30.0), egui::Color32::from_rgb(54, 60, 77)),
                        (vec2(16.0, -64.0), vec2(38.0, 26.0), egui::Color32::from_rgb(68, 56, 74)),
                    ] {
                        p.rect_filled(egui::Rect::from_min_size(center + offset, size), 6.0, color);
                    }
                    p.text(
                        center,
                        Align2::CENTER_CENTER,
                        tr!("make-room-for-what-matters"),
                        egui::FontId::proportional(24.0),
                        theme::TEXT,
                    );
                    p.text(
                        center + vec2(0.0, 34.0),
                        Align2::CENTER_CENTER,
                        tr!("open-a-drive-or-folder-to-see-where"),
                        egui::FontId::proportional(13.0),
                        theme::MUTED,
                    );
                }
                out
            })
            .inner;
        for c in commands {
            self.apply(c, &ctx);
        }

        self.open_dialog(&ctx);
        self.scan_toast(&ctx);
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

fn picker_row(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    let height = if text.contains('\n') { 56.0 } else { 34.0 };
    ui.add_sized(vec2(ui.available_width(), height), egui::Button::selectable(selected, text).truncate())
        .on_hover_text(text)
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
                                let response = picker_row(
                                    ui,
                                    selected,
                                    &format!(
                                        "Drive {i}: {}\n100 GB free of 1 TB · 90% used · NTFS",
                                        "long drive name ".repeat(12)
                                    ),
                                );
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
