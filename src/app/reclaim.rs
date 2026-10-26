//! Space that is likely safe to reclaim: cleanup locations, rebuildable developer output,
//! download and browser caches, and installed games by when they were last played.
//!
//! Discovery runs on a worker over the whole document; every action reuses the app's own
//! commands, so special locations keep their confirmations and deletes their safeguards.
use super::special::{self, Kind};
use crate::background::{Job, retire};
use crate::i18n::tr;
use crate::mapview::Command;
use crate::theme;
use clawback_core::{NodeId, ROOT, Tree, format, tree::flags};
use eframe::egui::{self, Align2, FontId, Sense, Ui, vec2};
use egui_phosphor::regular as icon;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const ROW_HEIGHT: f32 = 36.0;
/// Caches smaller than this are not worth a row.
const MIN_CACHE: u64 = 10 * 1024 * 1024;
/// Rebuilding a project's packages or output costs time; below this it is not worth it.
const MIN_DEVELOPER: u64 = 500 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    /// The Recycle Bin, Temp and WSL disks.
    Cleanup,
    /// Installed games.
    Games,
    Developer,
    Caches,
}

impl Section {
    fn title(self) -> String {
        match self {
            Self::Cleanup => tr!("reclaim-cleanup"),
            Self::Games => tr!("reclaim-games"),
            Self::Developer => tr!("reclaim-developer"),
            Self::Caches => tr!("reclaim-caches"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// The special location's own action, such as emptying the Recycle Bin.
    Special(Kind),
    /// The ordinary delete, which offers an uninstaller when one owns the folder.
    Delete,
    /// A game: the ordinary delete, which resolves to Steam's uninstaller.
    #[cfg_attr(not(windows), allow(dead_code))] // Steam games are found on Windows.
    Uninstall,
}

#[derive(Clone, Debug)]
struct Item {
    node: NodeId,
    section: Section,
    icon: &'static str,
    title: String,
    detail: String,
    path: PathBuf,
    size: u64,
    action: Action,
    /// Steam app ID, for its library icon.
    #[cfg_attr(not(windows), allow(dead_code))] // Steam games are found on Windows.
    game: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    document: u64,
    generation: u64,
}

#[derive(Default)]
pub struct Reclaim {
    items: Option<Vec<Item>>,
    shown: Option<Key>,
    pending: Option<Job<Key, Vec<Item>>>,
    last_started: Option<Instant>,
    #[cfg(windows)]
    art: special::GameArt,
}

impl Drop for Reclaim {
    fn drop(&mut self) {
        retire(self.items.take());
    }
}

impl Reclaim {
    /// The panel; returns the commands its rows asked for.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        tree: &Arc<Tree>,
        document: u64,
        generation: u64,
        is_mount: bool,
        deletion_disabled: bool,
    ) -> Vec<Command> {
        let _span = crate::perf::span("ui.reclaim");
        let key = Key { document, generation };
        self.sync(ui.ctx(), tree, key, is_mount);
        let mut out = Vec::new();
        ui.spacing_mut().item_spacing.y = 4.0;
        let Some(items) = &self.items else {
            ui.weak(tr!("reclaim-finding"));
            return out;
        };
        let total: u64 = items.iter().map(|item| item.size).sum();
        if total > 0 {
            ui.strong(tr!("reclaim-found", size = format::size(total)));
        }
        if self.shown.is_some_and(|shown| shown.document != document) {
            ui.multiply_opacity(0.55);
        }
        if items.is_empty() {
            ui.weak(tr!("reclaim-nothing"));
            return out;
        }
        let items = items.clone();
        egui::ScrollArea::vertical().id_salt("reclaim-rows").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut section = None;
            for (index, item) in items.iter().enumerate() {
                if section != Some(item.section) {
                    section = Some(item.section);
                    let size: u64 = items.iter().filter(|other| other.section == item.section).map(|i| i.size).sum();
                    ui.add_space(if index == 0 { 0.0 } else { 6.0 });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(item.section.title()).small().strong().color(theme::MUTED));
                        ui.label(egui::RichText::new(format::size(size)).small().color(theme::MUTED));
                    });
                    ui.add_space(2.0);
                }
                self.row(ui, item, index, total, deletion_disabled, &mut out);
            }
        });
        out
    }

    fn sync(&mut self, ctx: &egui::Context, tree: &Arc<Tree>, key: Key, is_mount: bool) {
        if let Some((done, items)) = Job::poll(&mut self.pending) {
            if done.document == key.document {
                retire(self.items.replace(items));
                self.shown = Some(done);
            } else {
                retire(items);
            }
        }
        // Edits arrive in bursts; refresh at most every couple of seconds for one document.
        let same_document = self.shown.is_some_and(|shown| shown.document == key.document);
        let ready = !same_document || self.last_started.is_none_or(|at| at.elapsed() >= Duration::from_secs(2));
        if self.shown != Some(key) && self.pending.is_none() {
            if ready {
                let tree = tree.clone();
                self.pending = Some(Job::spawn_cancellable(key, ctx, move |cancel| find(&tree, is_mount, cancel)));
                self.last_started = Some(Instant::now());
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    fn row(
        &mut self,
        ui: &mut Ui,
        item: &Item,
        index: usize,
        total: u64,
        deletion_disabled: bool,
        out: &mut Vec<Command>,
    ) {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let painter = ui.painter_at(rect);
        let background = if response.hovered() {
            theme::SELECTED_FILL
        } else if index.is_multiple_of(2) {
            theme::NAVIGATOR
        } else {
            theme::ROW_ALT
        };
        painter.rect_filled(rect, 0.0, background);
        // Each row's share of everything found, as in the file types list.
        let mut bar = rect.shrink2(vec2(0.0, 4.0));
        bar.max.x = bar.min.x + bar.width() * (item.size as f32 / total.max(1) as f32);
        painter.rect_filled(bar, 2.0, theme::ACCENT.gamma_multiply(0.10));
        let icon_rect = egui::Rect::from_center_size(rect.left_center() + vec2(16.0, 0.0), vec2(24.0, 24.0));
        if !self.paint_game_icon(ui.ctx(), &painter, item, icon_rect) {
            painter.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                item.icon,
                FontId::proportional(18.0),
                theme::ACCENT,
            );
        }
        // A compact action at the right edge leaves the row's width to its name.
        let (glyph, label, enabled) = action_label(item.action, deletion_disabled);
        let button = egui::Rect::from_min_size(rect.right_center() - vec2(34.0, 13.0), vec2(28.0, 26.0));
        let size_right = button.left() - 10.0;
        let size = painter.text(
            egui::pos2(size_right, rect.center().y),
            Align2::RIGHT_CENTER,
            format::size(item.size),
            FontId::proportional(13.0),
            theme::TEXT,
        );
        let text_left = rect.left() + 34.0;
        let text_width = (size.left() - 12.0 - text_left).max(0.0);
        for (text, size, color, top) in
            [(&item.title, 13.0, theme::TEXT, true), (&item.detail, 11.0, theme::MUTED, false)]
        {
            let font = FontId::proportional(size);
            let (y, align) =
                if top { (rect.top() + 4.0, Align2::LEFT_TOP) } else { (rect.bottom() - 4.0, Align2::LEFT_BOTTOM) };
            let text = super::widgets::fit_text(&painter, text, &font, text_width, false);
            painter.text(egui::pos2(text_left, y), align, text, font, color);
        }
        // Painted by hand: placing a widget inside the row would move the layout cursor.
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let action = ui.interact(button, ui.id().with(("reclaim-action", item.node)), sense);
        let fill = if enabled && action.hovered() { theme::CURRENT_ROW } else { theme::SELECTED_FILL };
        painter.rect(button, 4.0, fill, egui::Stroke::new(1.0, theme::DIALOG_EDGE), egui::StrokeKind::Inside);
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(15.0),
            if enabled { theme::TEXT } else { theme::MUTED },
        );
        let clicked = action.clicked();
        action.on_hover_text(label);
        if clicked {
            out.push(match item.action {
                Action::Special(kind) => Command::Special(kind, item.node),
                Action::Delete | Action::Uninstall => Command::Delete(item.node),
            });
        } else if response.clicked() {
            out.push(Command::Show(item.node));
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!(
            "{}
{}",
            item.path.display(),
            tr!("reclaim-show")
        ));
    }

    /// Steam's icon for a game row; false when there is none to draw (yet).
    #[cfg(windows)]
    fn paint_game_icon(&mut self, ctx: &egui::Context, painter: &egui::Painter, item: &Item, rect: egui::Rect) -> bool {
        use super::special::windows::programs::steam::Art;
        let Some(id) = item.game else { return false };
        let Some((_, texture)) = self.art.first(ctx, id, &[Art::Icon, Art::Capsule, Art::Header]) else { return false };
        let size = texture.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        painter.image(
            texture.id(),
            egui::Rect::from_center_size(rect.center(), size * scale),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        true
    }

    #[cfg(not(windows))]
    #[allow(clippy::unused_self)]
    fn paint_game_icon(&mut self, _: &egui::Context, _: &egui::Painter, _: &Item, _: egui::Rect) -> bool {
        false
    }
}

/// The icon and name for `action`, and whether it may run.
fn action_label(action: Action, deletion_disabled: bool) -> (&'static str, String, bool) {
    match action {
        Action::Special(kind) => {
            let (glyph, label) = kind.menu_entry();
            (glyph, label, kind.action_enabled(deletion_disabled))
        }
        Action::Delete => (icon::TRASH, tr!("delete"), !deletion_disabled),
        Action::Uninstall => (icon::TRASH, tr!("uninstall-software"), !deletion_disabled),
    }
}

/// Every reclaimable item in `tree`, by section, largest first.
fn find(tree: &Tree, is_mount: bool, cancel: &AtomicBool) -> Option<Vec<Item>> {
    let _span = crate::perf::span("worker.reclaim");
    let mut items = Vec::new();
    cleanup(tree, is_mount, &mut items);
    caches(tree, &mut items);
    // Folders already offered as caches are not walked again for developer output.
    let mut claimed: HashSet<NodeId> = items.iter().map(|item| item.node).collect();
    let mut stack = vec![ROOT];
    while let Some(id) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let node = tree.node(id);
        if node.has(flags::REMOVED) || (id != ROOT && claimed.contains(&id)) {
            continue;
        }
        if !node.is_dir() {
            if let Some(item) = wsl_disk(tree, id) {
                items.push(item);
            }
            continue;
        }
        if let Some(item) = developer(tree, id).or_else(|| game(tree, id)).or_else(|| tagged_cache(tree, id)) {
            claimed.insert(id);
            items.push(item);
            continue;
        }
        stack.extend(node.children.iter().copied());
    }
    items.retain(|item| {
        item.size
            >= match item.section {
                Section::Cleanup | Section::Games => 1,
                Section::Developer => MIN_DEVELOPER,
                Section::Caches => MIN_CACHE,
            }
    });
    items.sort_by(|a, b| a.section.cmp(&b.section).then(b.size.cmp(&a.size)));
    Some(items)
}

fn item(tree: &Tree, node: NodeId, section: Section, icon: &'static str, title: String, detail: String) -> Item {
    Item {
        node,
        section,
        icon,
        title,
        detail,
        path: tree.path(node),
        size: tree.node(node).size,
        action: Action::Delete,
        game: None,
    }
}

/// The Recycle Bin and the user's Temp folder.
fn cleanup(tree: &Tree, is_mount: bool, items: &mut Vec<Item>) {
    let inventory = special::Inventory::discover(tree, is_mount);
    for node in inventory.collapsed() {
        let Some(kind) = inventory.classify(tree, node) else { continue };
        let (icon, title, detail) = match kind {
            Kind::RecycleBin => (icon::TRASH, tr!("recycle-bin"), tr!("reclaim-recycle-bin")),
            Kind::TempFolder => (icon::BROOM, "Temp".to_owned(), tr!("reclaim-temp")),
            _ => continue,
        };
        items.push(Item { action: Action::Special(kind), ..item(tree, node, Section::Cleanup, icon, title, detail) });
    }
}

fn wsl_disk(tree: &Tree, node: NodeId) -> Option<Item> {
    let name = tree.node(node).name_lossy();
    if !name.to_ascii_lowercase().ends_with(".vhdx") || special::recognize(&tree.path(node)) != Some(Kind::WslDisk) {
        return None;
    }
    Some(Item {
        action: Action::Special(Kind::WslDisk),
        ..item(tree, node, Section::Cleanup, icon::HARD_DRIVES, name.into_owned(), tr!("reclaim-wsl"))
    })
}

fn has_child(tree: &Tree, dir: NodeId, name: &str) -> bool {
    tree.node(dir).children.iter().any(|&child| tree.node(child).name_lossy().eq_ignore_ascii_case(name))
}

/// Build output and installed packages that their project's tools recreate, recognized by
/// the project file next to them rather than by name alone.
fn developer(tree: &Tree, id: NodeId) -> Option<Item> {
    let node = tree.node(id);
    let parent = node.parent;
    if id == ROOT || parent == clawback_core::NO_NODE {
        return None;
    }
    let name = node.name_lossy();
    let beside = |file: &str| has_child(tree, parent, file);
    // Names match in any letter case, like the files beside them.
    let detail = match name.to_ascii_lowercase().as_str() {
        "node_modules" if beside("package.json") => tr!("reclaim-node-modules"),
        "target"
            if beside("Cargo.toml")
                && (has_child(tree, id, "CACHEDIR.TAG")
                    || has_child(tree, id, "debug")
                    || has_child(tree, id, "release")) =>
        {
            tr!("reclaim-rust-target")
        }
        ".venv" | "venv" if has_child(tree, id, "pyvenv.cfg") => tr!("reclaim-python-venv"),
        ".gradle"
            if ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]
                .iter()
                .any(|f| beside(f)) =>
        {
            tr!("reclaim-gradle")
        }
        "pods" if beside("Podfile") => tr!("reclaim-pods"),
        _ => return None,
    };
    let project = tree.node(parent).name_lossy().into_owned();
    Some(item(tree, id, Section::Developer, icon::PACKAGE, format!("{project} › {name}"), detail))
}

/// A folder its program marked as a cache it can recreate, under the Cache Directory
/// Tagging Specification (<https://bford.info/cachedir/>). Only a tag with the
/// specification's signature counts, so a stray file of that name does not.
fn tagged_cache(tree: &Tree, id: NodeId) -> Option<Item> {
    const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
    let tag = tree.child_named(id, std::ffi::OsStr::new("CACHEDIR.TAG"))?;
    if tree.node(tag).is_dir() || id == ROOT {
        return None;
    }
    let mut start = [0; SIGNATURE.len()];
    std::io::Read::read_exact(&mut std::fs::File::open(tree.path(tag)).ok()?, &mut start).ok()?;
    if start != SIGNATURE {
        return None;
    }
    let node = tree.node(id);
    let title = format!("{} › {}", tree.node(node.parent).name_lossy(), node.name_lossy());
    Some(item(tree, id, Section::Caches, icon::DATABASE, title, tr!("reclaim-cache-tag")))
}

/// An installed Steam game, with when it was last played.
#[cfg(windows)]
fn game(tree: &Tree, id: NodeId) -> Option<Item> {
    let game = special::windows::programs::steam::game_at(tree, id).filter(|game| !game.is_tool())?;
    let detail = game.last_played.map_or_else(
        || tr!("reclaim-never-played"),
        |seconds| tr!("reclaim-last-played", date = format::date(seconds)),
    );
    Some(Item {
        action: Action::Uninstall,
        game: Some(game.id),
        ..item(tree, id, Section::Games, icon::GAME_CONTROLLER, game.name, detail)
    })
}

#[cfg(not(windows))]
fn game(_: &Tree, _: NodeId) -> Option<Item> {
    None
}

/// Package managers' download caches and browsers' caches that are present in `tree`.
fn caches(tree: &Tree, items: &mut Vec<Item>) {
    for (path, tool, part) in package_caches() {
        if let Some(node) = tree.find_path(&path).filter(|&node| node != ROOT && tree.node(node).is_dir()) {
            let detail = tr!("reclaim-package-cache", tool = tool);
            let title = part.map_or_else(|| tool.to_owned(), |part| format!("{tool} › {part}"));
            items.push(item(tree, node, Section::Caches, icon::DATABASE, title, detail));
        }
    }
    for (profiles, cache, browser) in browser_caches() {
        let Some(base) = tree.find_path(&profiles).filter(|&node| tree.node(node).is_dir()) else { continue };
        for &profile in &tree.node(base).children {
            let Some(node) = tree.child_named(profile, std::ffi::OsStr::new(cache)) else { continue };
            if !tree.node(node).is_dir() {
                continue;
            }
            let title = format!("{browser} › {}", tree.node(profile).name_lossy());
            let detail = tr!("reclaim-browser-cache", browser = browser);
            items.push(item(tree, node, Section::Caches, icon::GLOBE, title, detail));
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute())
}

/// (cache folder, tool, which part when a tool has several) for package managers whose
/// caches are rebuilt on demand.
fn package_caches() -> Vec<(PathBuf, &'static str, Option<&'static str>)> {
    let mut found = Vec::new();
    let home = home();
    let local = if cfg!(windows) {
        env_dir("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        home.as_ref().map(|home| home.join("Library").join("Caches"))
    } else {
        env_dir("XDG_CACHE_HOME").or_else(|| home.as_ref().map(|home| home.join(".cache")))
    };
    if let Some(local) = &local {
        if cfg!(windows) {
            found.push((local.join("npm-cache"), "npm", None));
            found.push((local.join("pip").join("Cache"), "pip", None));
            found.push((local.join("Yarn").join("Cache"), "Yarn", None));
        } else if cfg!(target_os = "macos") {
            found.push((local.join("pip"), "pip", None));
            found.push((local.join("Yarn"), "Yarn", None));
        } else {
            found.push((local.join("pip"), "pip", None));
            found.push((local.join("yarn"), "Yarn", None));
        }
    }
    if let Some(home) = &home {
        if !cfg!(windows) {
            found.push((home.join(".npm").join("_cacache"), "npm", None));
        }
        let cargo = env_dir("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
        // Downloaded and unpacked crates; the index stays, so Cargo can still resolve offline.
        found.push((cargo.join("registry").join("cache"), "Cargo", Some("registry/cache")));
        found.push((cargo.join("registry").join("src"), "Cargo", Some("registry/src")));
        found.push((home.join(".gradle").join("caches"), "Gradle", None));
        found.push((home.join(".nuget").join("packages"), "NuGet", None));
    }
    found
}

/// (profiles folder, cache folder inside each profile, browser) for common browsers.
fn browser_caches() -> Vec<(PathBuf, &'static str, &'static str)> {
    let mut found = Vec::new();
    if cfg!(windows) {
        if let Some(local) = env_dir("LOCALAPPDATA") {
            found.push((local.join(r"Google\Chrome\User Data"), "Cache", "Chrome"));
            found.push((local.join(r"Microsoft\Edge\User Data"), "Cache", "Edge"));
            found.push((local.join(r"Mozilla\Firefox\Profiles"), "cache2", "Firefox"));
        }
    } else if let Some(home) = home() {
        if cfg!(target_os = "macos") {
            let caches = home.join("Library").join("Caches");
            found.push((caches.join("Google").join("Chrome"), "Cache", "Chrome"));
            found.push((caches.join("Microsoft Edge"), "Cache", "Edge"));
            found.push((caches.join("Firefox").join("Profiles"), "cache2", "Firefox"));
        } else {
            let caches = env_dir("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
            found.push((caches.join("google-chrome"), "Cache", "Chrome"));
            found.push((caches.join("microsoft-edge"), "Cache", "Edge"));
            found.push((caches.join("mozilla").join("firefox"), "cache2", "Firefox"));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::tree::NewEntry;
    use std::path::Path;

    #[test]
    fn developer_output_needs_its_project_file() {
        let mut tree = Tree::new(Path::new("/work"));
        let ids = tree.add_children(ROOT, vec![NewEntry::dir("app"), NewEntry::dir("loose"), NewEntry::dir("crate")]);
        let (app, loose, krate) = (ids.start, ids.start + 1, ids.start + 2);
        let modules =
            tree.add_children(app, vec![NewEntry::dir("node_modules"), NewEntry::file("package.json", 1)]).start;
        tree.add_children(modules, vec![NewEntry::file("big.js", 600 * 1024 * 1024)]);
        let stray = tree.add_children(loose, vec![NewEntry::dir("node_modules")]).start;
        tree.add_children(stray, vec![NewEntry::file("big.js", 600 * 1024 * 1024)]);
        let target = tree.add_children(krate, vec![NewEntry::dir("target"), NewEntry::file("Cargo.toml", 1)]).start;
        let debug = tree.add_children(target, vec![NewEntry::dir("debug")]).start;
        tree.add_children(debug, vec![NewEntry::file("app.exe", 700 * 1024 * 1024)]);
        tree.sort_all();
        let items = find(&tree, false, &AtomicBool::new(false)).expect("not cancelled");
        let titles: Vec<_> =
            items.iter().filter(|i| i.section == Section::Developer).map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["crate › target", "app › node_modules"]);
        assert!(find(&tree, false, &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn tagged_caches_need_the_signature_and_names_ignore_case() {
        let root = std::env::temp_dir().join(format!("clawback-reclaim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, tag) in [
            (
                "real",
                "Signature: 8a477f597d28d172789f06886806bc55
",
            ),
            (
                "fake",
                "not a cache
",
            ),
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("CACHEDIR.TAG"), tag).unwrap();
        }
        let mut tree = Tree::new(&root);
        let ids = tree.add_children(ROOT, vec![NewEntry::dir("real"), NewEntry::dir("fake"), NewEntry::dir("App")]);
        for dir in [ids.start, ids.start + 1] {
            tree.add_children(dir, vec![NewEntry::file("CACHEDIR.TAG", 43), NewEntry::file("blob", 20 * 1024 * 1024)]);
        }
        let modules =
            tree.add_children(ids.start + 2, vec![NewEntry::dir("NODE_MODULES"), NewEntry::file("Package.json", 1)]);
        tree.add_children(modules.start, vec![NewEntry::file("big.js", 600 * 1024 * 1024)]);
        tree.sort_all();
        let items = find(&tree, false, &AtomicBool::new(false)).expect("not cancelled");
        std::fs::remove_dir_all(&root).unwrap();
        let mut titles: Vec<_> = items.iter().map(|item| item.title.rsplit(" › ").next().unwrap().to_owned()).collect();
        titles.sort();
        assert_eq!(titles, ["NODE_MODULES", "real"]);
    }

    #[test]
    fn small_rebuildable_folders_are_left_out() {
        let mut tree = Tree::new(Path::new("/work"));
        let app = tree.add_children(ROOT, vec![NewEntry::dir("app")]).start;
        let modules =
            tree.add_children(app, vec![NewEntry::dir("node_modules"), NewEntry::file("package.json", 1)]).start;
        tree.add_children(modules, vec![NewEntry::file("small.js", 400 * 1024 * 1024)]);
        tree.sort_all();
        assert!(find(&tree, false, &AtomicBool::new(false)).expect("not cancelled").is_empty());
    }
}
