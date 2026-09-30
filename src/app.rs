//! The application window: SpaceMonger's toolbar, commands and dialogs
//! around the folder map.

use crate::directoryview::DirectoryView;
use crate::mapview::{Command, MapInput, MapView};
use crate::platform::{self, DiskInfo};
use crate::scanning::{Running, Update};
use crate::theme;
use crate::{background::retire, icon};
use clawback_core::layout::DENSITY_NAMES;
use clawback_core::palette::SCHEME_NAMES;
use clawback_core::{NodeId, ROOT, Settings, SkipReason, Skipped, Tree, format};
use eframe::egui::{self, Align, Align2, Id, Key, Layout, Modifiers, RichText, Ui, vec2};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Drive(usize),
    Recent(usize),
}

struct OpenDialog {
    drives: Vec<DiskInfo>,
    choice: Option<Choice>,
    loading: Option<mpsc::Receiver<Vec<DiskInfo>>>,
}

struct Properties {
    title: String,
    rows: Vec<(&'static str, String)>,
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
    settings: Settings,
    doc: Option<Doc>,
    next_doc_id: u64,
    history: Vec<PathBuf>,
    scan: Option<Running>,
    live: Option<crate::watching::Live>,
    live_status: String,
    map: MapView,
    directories: DirectoryView,
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
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = ClawbackApp {
            settings,
            doc: None,
            next_doc_id: 1,
            history: Vec::new(),
            scan: None,
            live: None,
            live_status: String::new(),
            map: MapView::default(),
            directories: DirectoryView::default(),
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
            return app;
        }
        if let Some(p) = path {
            app.start_scan(p, &cc.egui_ctx);
        }
        app
    }

    fn save_settings(&self) {
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
                let settings = self.settings.clone();
                std::thread::spawn(move || {
                    let _ = settings.save();
                });
            }
            Err(e) => self.error = Some(format!("Clawback could not scan {}.\n\n{e}", root.display())),
        }
    }

    fn set_doc(&mut self, mut doc: Doc) {
        if let Some(previous) = &self.doc {
            let path = previous.tree.path(previous.view);
            doc.view = doc.tree.find_path(&path).unwrap_or(ROOT);
        }
        if let Some(old) = self.doc.replace(doc) {
            retire(old);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
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
                self.error = Some(format!("Clawback could not scan the folder.\n\n{error}"));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.scan = None;
                self.error = Some("The scan worker stopped unexpectedly.".into());
            }
            _ => {}
        }
        if self.scan.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let finished = self.deleting.as_ref().and_then(|d| d.rx.try_recv().ok());
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
                Err(e) => self.error = Some(format!("Failed to delete {}.\n\n{e}", del.path.display())),
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
                    let _ = tx.send(platform::drive_list());
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
        let mount = doc.disk.as_ref().map(|d| d.mount.clone());
        std::thread::spawn(move || {
            let result = platform::trash(&p).map(|()| {
                Arc::make_mut(&mut tree).remove(node);
                let disk = mount.and_then(|m| platform::all_disks().into_iter().find(|d| d.mount == m));
                (tree, disk)
            });
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.deleting = Some(Deleting { doc: doc.id, path, rx });
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
        let ready = self.doc.is_some() && self.scan.is_none();
        let zoomed = self.doc.as_ref().is_some_and(|d| d.view != ROOT);
        let selected = self.map.selected_node().is_some();
        let mut action = None;
        ui.spacing_mut().button_padding = vec2(8.0, 3.0);
        ui.spacing_mut().interact_size.y = 24.0;
        ui.horizontal_centered(|ui| {
            if ui.add(egui::Button::new("Open folder")).clicked() {
                action = Some(Tool::Open);
            }
            if tb(ui, "Back", !self.history.is_empty(), "Previous view / Escape / Backspace") {
                action = Some(Tool::Back);
            }
            ui.menu_button("Palette", |ui| {
                for (scheme, name) in clawback_core::palette::MAP_PRESETS {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                self.settings.file_color == scheme && self.settings.folder_color == scheme,
                                name,
                            )
                            .clicked()
                        {
                            self.settings.file_color = scheme;
                            self.settings.folder_color = scheme;
                            self.save_settings();
                            ui.close();
                        }
                        for depth in 0..8 {
                            let [r, g, b] = clawback_core::palette::map_color(scheme, depth);
                            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 16.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                        }
                    });
                }
            });
            ui.menu_button("More", |ui| {
                for (label, enabled, tool) in [
                    ("Up a level", ready && zoomed, Tool::ZoomOut),
                    ("Rescan / F5", ready, Tool::Rescan),
                    ("All files / Home", ready && zoomed, Tool::ZoomFull),
                    ("Zoom in / Enter", ready && selected && self.map.selected_is_folder(), Tool::ZoomIn),
                    ("Settings", true, Tool::Setup),
                    ("About Clawback", true, Tool::About),
                    ("Open selected item", ready && selected, Tool::Run),
                    ("Move selected item to trash", ready && selected && !self.settings.disable_delete, Tool::Delete),
                ] {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        action = Some(tool);
                        ui.close();
                    }
                }
                let can_show_free = ready && self.doc.as_ref().is_some_and(|d| d.is_mount);
                if ui
                    .add_enabled(
                        can_show_free,
                        egui::Button::new("Show free space").selected(self.settings.show_free && can_show_free),
                    )
                    .clicked()
                {
                    action = Some(Tool::Free);
                    ui.close();
                }
                if self.doc.as_ref().is_some_and(|d| d.unreadable() > 0) && ui.button("Unreadable folders").clicked() {
                    action = Some(Tool::Unreadable);
                    ui.close();
                }
            });
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let counts = self
                    .scan
                    .as_ref()
                    .map(|r| (r.progress.bytes, r.progress.files, r.progress.dirs))
                    .or_else(|| self.doc.as_ref().map(|d| (d.tree.root().size, d.files, d.folders)));
                if let Some((bytes, files, folders)) = counts {
                    let stats = format!(
                        "{} | {} files | {} folders",
                        format::size(bytes),
                        format::count(files),
                        format::count(folders)
                    );
                    ui.add(egui::Label::new(RichText::new(&stats).size(12.0)).truncate()).on_hover_text(stats);
                    ui.separator();
                }
                let path = self
                    .doc
                    .as_ref()
                    .map(|d| d.tree.path(d.view))
                    .or_else(|| self.scan.as_ref().map(|r| r.root.clone()));
                let text = path.map_or_else(|| "Clawback".into(), |p| p.display().to_string());
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&text).color(theme::MUTED)).truncate()).on_hover_text(text);
                });
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
            format!(
                "{}  -  {} Total  -  {} Free  -  Clawback",
                dir_display(&doc.tree.path(doc.view)),
                format::size(size),
                format::size(doc.free_space())
            )
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
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
        let (mut cancel, mut browse) = (false, false);
        let path_of = |c: Choice, dlg: &OpenDialog| match c {
            Choice::Drive(i) => dlg.drives[i].mount.clone(),
            Choice::Recent(i) => recent[i].clone(),
        };
        egui::Window::new("Select Drive to View")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(440.0);
                egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
                    if dlg.loading.is_some() {
                        ui.spinner();
                        ui.weak("Finding drives...");
                    } else if dlg.drives.is_empty() {
                        ui.weak("No drives found. Use \"Other Folder...\" to pick a folder.");
                    }
                    for (i, d) in dlg.drives.iter().enumerate() {
                        let used = d.total.saturating_sub(d.free);
                        let text = format!(
                            "{}  {}\n        {} free of {}  ·  {} used  ·  {}",
                            if d.removable { "💾" } else { "💽" },
                            d.label(),
                            format::size(d.free),
                            format::size(d.total),
                            format::percent(used, d.total),
                            d.fs
                        );
                        let r = ui.selectable_label(dlg.choice == Some(Choice::Drive(i)), text);
                        if r.clicked() {
                            dlg.choice = Some(Choice::Drive(i));
                        }
                        if r.double_clicked() {
                            chosen = Some(d.mount.clone());
                        }
                    }
                    if !recent.is_empty() {
                        ui.separator();
                        ui.weak("Recent");
                        for (i, p) in recent.iter().enumerate() {
                            let r = ui.selectable_label(
                                dlg.choice == Some(Choice::Recent(i)),
                                format!("📁  {}", p.display()),
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
                ui.horizontal(|ui| {
                    if ui.button("Other Folder...").clicked() {
                        browse = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(dlg.choice.is_some(), egui::Button::new("   OK   ")).clicked()
                            && let Some(c) = dlg.choice
                        {
                            chosen = Some(path_of(c, dlg));
                        }
                        if ui.button("Cancel").clicked() {
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
        } else if cancel || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = None;
        }
    }

    /// Scan activity lives below the map, without covering its contents.
    fn status_bar(&self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().button_padding = vec2(8.0, 2.0);
        ui.spacing_mut().interact_size.y = 22.0;
        if let Some(run) = &self.scan {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0));
                ui.label(RichText::new("Scanning").color(theme::TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Stop scan").clicked() {
                        run.cancel();
                    }
                    if run.progress.workers > 0 {
                        ui.weak(format!("{} workers", run.progress.workers)).on_hover_text(
                            "Current concurrency limit; automatically adapts to measured throughput and latency.",
                        );
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let path = run.current.display().to_string();
                        ui.add(egui::Label::new(RichText::new(&path).size(11.0).color(theme::MUTED)).truncate())
                            .on_hover_text(path);
                    });
                });
            });
            let fraction = run
                .disk
                .as_ref()
                .filter(|_| run.is_mount)
                .map(|d| d.total.saturating_sub(d.free))
                .filter(|&used| used > 0)
                .map(|used| (run.progress.bytes as f64 / used as f64).min(1.0) as f32);
            ui.add(
                egui::ProgressBar::new(fraction.unwrap_or(0.0))
                    .animate(fraction.is_none())
                    .fill(theme::ACCENT)
                    .desired_height(3.0),
            )
            .on_hover_text("Mapped bytes relative to used drive space; an estimate, not a file-count percentage.");
        } else {
            ui.horizontal_centered(|ui| {
                let status = if !self.live_status.is_empty() {
                    &self.live_status
                } else if self.doc.is_some() {
                    "Ready"
                } else {
                    "Open a folder to begin"
                };
                ui.add(egui::Label::new(RichText::new(status).color(theme::MUTED).size(11.0)).truncate());
                if ui.available_width() > 340.0 {
                    ui.label(
                        RichText::new("Double-click to explore / Right-click for actions")
                            .color(theme::MUTED)
                            .size(11.0),
                    );
                }
            });
        }
    }

    /// SpaceMonger's Setup dialog, plus Clawback's scanning options.
    fn setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.setup else { return };
        let mut done: Option<bool> = None;
        egui::Window::new("Clawback Settings")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.strong("File Layout");
                            ui.horizontal(|ui| {
                                ui.label("Density:");
                                let mut idx = (d.density + 3).clamp(0, 5) as usize;
                                if egui::ComboBox::from_id_salt("density")
                                    .width(140.0)
                                    .show_index(ui, &mut idx, DENSITY_NAMES.len(), |i| DENSITY_NAMES[i])
                                    .changed()
                                {
                                    d.density = idx as i32 - 3;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Bias:");
                                ui.vertical(|ui| {
                                    ui.add(egui::Slider::new(&mut d.bias, -20..=20).show_value(false));
                                    ui.horizontal(|ui| {
                                        ui.small("Horz");
                                        ui.add_space(36.0);
                                        ui.small("Equal");
                                        ui.add_space(36.0);
                                        ui.small("Vert");
                                    });
                                });
                            });
                        });
                    });
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.strong("Display Colors");
                            egui::Grid::new("colors").num_columns(2).show(ui, |ui| {
                                for (label, value) in [("Files:", &mut d.file_color), ("Folders:", &mut d.folder_color)]
                                {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(label).width(120.0).show_index(
                                        ui,
                                        value,
                                        SCHEME_NAMES.len(),
                                        |i| SCHEME_NAMES[i],
                                    );
                                    ui.end_row();
                                }
                            });
                        });
                    });
                });
                ui.group(|ui| {
                    ui.strong("ToolTips");
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.checkbox(&mut d.show_name_tips, "Show file-name-tips");
                            delay(ui, &mut d.nametip_delay_ms);
                        });
                        ui.add_space(24.0);
                        ui.vertical(|ui| {
                            ui.checkbox(&mut d.show_info_tips, "Show file-info-tips");
                            ui.add_enabled_ui(d.show_info_tips, |ui| {
                                egui::Grid::new("tipflags").num_columns(2).show(ui, |ui| {
                                    ui.checkbox(&mut d.tip_path, "Full Path");
                                    ui.checkbox(&mut d.tip_date, "Date / Time");
                                    ui.end_row();
                                    ui.checkbox(&mut d.tip_name, "Filename");
                                    ui.checkbox(&mut d.tip_size, "File Size");
                                    ui.end_row();
                                    ui.checkbox(&mut d.tip_icon, "Icon");
                                    ui.checkbox(&mut d.tip_attrib, "Attributes");
                                    ui.end_row();
                                });
                                delay(ui, &mut d.infotip_delay_ms);
                            });
                        });
                    });
                });
                ui.group(|ui| {
                    ui.strong("Miscellaneous Options");
                    egui::Grid::new("misc").num_columns(2).show(ui, |ui| {
                        ui.checkbox(&mut d.auto_rescan, "Auto Rescan on Delete");
                        ui.end_row();
                        ui.checkbox(&mut d.disable_delete, "Disable \"Delete\" Command");
                        ui.checkbox(&mut d.save_pos, "Remember Window Position")
                            .on_hover_text("Takes effect the next time Clawback starts");
                        ui.end_row();
                        ui.checkbox(&mut d.rollover_box, "Show Rollover Boxes");
                        ui.end_row();
                    });
                });
                ui.group(|ui| {
                    ui.strong("Scanning");
                    egui::Grid::new("scanning").num_columns(2).show(ui, |ui| {
                        ui.checkbox(&mut d.one_filesystem, "Stay on one filesystem").on_hover_text(
                            "Don't descend into other drives or network mounts inside the scanned folder",
                        );
                        ui.checkbox(&mut d.apparent_size, "Use file lengths, not size on disk");
                        ui.end_row();
                        if cfg!(unix) {
                            ui.checkbox(&mut d.dedupe_hardlinks, "Count hard-linked files once");
                            ui.end_row();
                        }
                    });
                    ui.weak("Scanning options apply to the next scan.");
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("   OK   ").clicked() {
                            done = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            done = Some(false);
                        }
                    });
                });
            });
        match done {
            Some(true) => {
                if let Some(mut s) = self.setup.take() {
                    s.recent = std::mem::take(&mut self.settings.recent);
                    s.sanitize();
                    self.settings = s;
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
        egui::Window::new("About Clawback")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
                    icon::paint(ui.painter(), rect);
                    ui.vertical(|ui| {
                        ui.heading(format!("Clawback {}", env!("CARGO_PKG_VERSION")));
                        ui.label("A fast, cross-platform disk space map.");
                        ui.add_space(6.0);
                        ui.label(
                            "Clawback is a Rust homage to SpaceMonger 1.4 by Sean Werkema (1997–2000). Its layout, \
                             colours and mouse behaviour follow the original source code.",
                        );
                        ui.add_space(6.0);
                        ui.label("Claw back your disk space.");
                        ui.add_space(6.0);
                        ui.weak("Copyright © 2026 Azazel Labs.");
                        ui.weak("Licensed under MIT-0 (MIT No Attribution). No warranty of any kind.");
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("   OK   ").clicked() {
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
            egui::Window::new("Properties").id(Id::new("clawback-properties")).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading file details…");
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
                        ui.strong(*k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Show in File Manager").clicked() {
                        let _ = platform::reveal(&p.path);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("   OK   ").clicked() {
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
        egui::Window::new("Folders Not Scanned").open(&mut open).default_width(560.0).show(ctx, |ui| {
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
            egui::Modal::new(Id::new("clawback-deleting")).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Deleting...\n{}", elide(&d.path.display().to_string(), 60)));
                });
            });
        }
        if let Some(msg) = &self.error {
            let mut close = false;
            let r = egui::Modal::new(Id::new("clawback-error")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(msg);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("   OK   ").clicked() {
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

impl eframe::App for ClawbackApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(feature = "screenshots")]
        crate::demo::capture(&ctx);
        self.poll(&ctx);
        self.keys(&ctx);

        // Two compact bands leave roughly 90% of the window for the data.
        let band_height = (ui.available_height() * 0.05).clamp(32.0, 44.0);
        egui::Panel::top("scan-progress")
            .exact_size(band_height)
            .frame(egui::Frame::new().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.status_bar(ui));
        let tool = egui::Panel::top("compact-toolbar")
            .exact_size(band_height)
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.toolbar(ui))
            .inner;
        if let Some(t) = tool {
            self.tool(t, &ctx);
        }

        let directory_panel = "directory-tree";
        #[cfg(feature = "screenshots")]
        let directory_panel =
            if std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some() { "demo-directories" } else { directory_panel };
        let folder = egui::Panel::top(directory_panel)
            .resizable(true)
            .default_size(190.0)
            .size_range(90.0..=400.0)
            .frame(egui::Frame::new().fill(theme::SURFACE).inner_margin(8))
            .show(ui, |ui| {
                if let Some(doc) = &self.doc {
                    self.directories.ui(ui, &doc.tree, doc.id, doc.generation, doc.view, self.scan.is_some())
                } else {
                    ui.strong("Directories");
                    ui.weak(if self.scan.is_some() {
                        "Discovering folders…"
                    } else {
                        "Open a folder or drive to browse its directory tree."
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
                        "Make room for what matters.",
                        egui::FontId::proportional(24.0),
                        theme::TEXT,
                    );
                    p.text(
                        center + vec2(0.0, 34.0),
                        Align2::CENTER_CENTER,
                        "Open a drive or folder to see where your space goes.",
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
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_settings();
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
        clawback_core::Kind::Dir => "Folder",
        clawback_core::Kind::File => "File",
        clawback_core::Kind::Symlink => "Symbolic link",
        clawback_core::Kind::Other => "Special file",
    };
    let mut rows = vec![
        ("Name:", name.clone()),
        ("Type:", kind.to_owned()),
        ("Location:", path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
        ("Size:", format!("{} ({})", format::size(node.display_len()), format::bytes_exact(node.display_len()))),
        ("Size on disk:", format!("{} ({})", format::size(node.size), format::bytes_exact(node.size))),
    ];
    if node.is_dir() {
        let folders = t.dir_count(n).saturating_sub(1);
        rows.push(("Contains:", format!("{} Files, {} Folders", format::count(node.files), format::count(folders))));
    }
    rows.push(("Modified:", format::date(node.mtime)));
    let attrs = platform::attributes(&path);
    if !attrs.is_empty() {
        rows.push(("Attributes:", attrs.join(" ")));
    }
    Properties { title: format!("{name} Properties"), rows, path }
}

/// A toolbar button.
fn tb(ui: &mut Ui, label: &str, enabled: bool, hint: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(hint).clicked()
}

fn delay(ui: &mut Ui, ms: &mut u32) {
    ui.horizontal(|ui| {
        ui.label("Delay:");
        ui.add(egui::DragValue::new(ms).range(0..=99_999).speed(5));
        ui.label("msec");
    });
}

/// A folder path with a trailing separator, as SpaceMonger titled folders.
fn dir_display(p: &Path) -> String {
    let mut s = p.display().to_string();
    if !s.ends_with(MAIN_SEPARATOR) {
        s.push(MAIN_SEPARATOR);
    }
    s
}

/// Shorten long text from the front: "…/some/deep/path".
fn elide(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn eliding() {
        assert_eq!(elide("abc", 5), "abc");
        assert_eq!(elide("abcdefgh", 5), "…efgh");
        assert!(dir_display(Path::new("/a/b")).ends_with(MAIN_SEPARATOR));
    }
}
