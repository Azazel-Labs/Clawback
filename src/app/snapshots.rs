//! Saved snapshots of a scan, and the open document compared with one of them.
//!
//! Snapshots live in the settings folder; the comparison captures the current tree at the
//! snapshot's level of detail on a worker, lists the changes and tints the map with them.
use crate::background::{Job, retire};
use crate::i18n::tr;
use crate::maprender::Tint;
use crate::mapview::Command;
use crate::theme;
use clawback_core::snapshot::{self, Change, ChangeKind, Detail, Snapshot, Summary};
use clawback_core::{NodeId, Tree, format};
use eframe::egui::{self, Align, Align2, FontId, Id, Layout, RichText, Sense, Ui, vec2};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const EXTENSION: &str = "clawsnap";
const ROW_HEIGHT: f32 = 22.0;

pub fn folder() -> Option<PathBuf> {
    clawback_core::settings::config_dir().map(|dir| dir.join("snapshots"))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// Whether two scan roots are the same folder, ignoring separators and, where the file
/// system does, letter case.
pub fn same_root(a: &Path, b: &Path) -> bool {
    let normal = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.trim_end_matches('/').to_owned();
        if cfg!(any(windows, target_os = "macos")) { text.to_lowercase() } else { text }
    };
    normal(a) == normal(b)
}

fn save(snapshot: &Snapshot) -> io::Result<PathBuf> {
    let dir = folder().ok_or_else(|| io::Error::other(tr!("snapshot-no-folder")))?;
    std::fs::create_dir_all(&dir)?;
    let path = (0..1000)
        .map(|n| dir.join(format!("{}-{n}.{EXTENSION}", snapshot.taken)))
        .find(|path| !path.exists())
        .ok_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists))?;
    // Write beside the final name, so a failed save never leaves a damaged snapshot.
    let partial = path.with_extension("partial");
    {
        let mut out = io::BufWriter::new(std::fs::File::create(&partial)?);
        snapshot.write(&mut out)?;
        out.flush()?;
    }
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

fn load(path: &Path) -> io::Result<Snapshot> {
    Snapshot::read(&mut io::BufReader::new(std::fs::File::open(path)?))
}

#[derive(Clone)]
struct Stored {
    path: PathBuf,
    summary: Summary,
}

/// Saved snapshots, newest first.
fn list() -> Vec<Stored> {
    let Some(dir) = folder() else { return Vec::new() };
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let mut found: Vec<_> = entries
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
        .filter_map(|path| {
            let file = std::fs::File::open(&path).ok()?;
            let summary = Summary::peek(&mut io::BufReader::new(file)).ok()?;
            Some(Stored { path, summary })
        })
        .collect();
    found.sort_by_key(|stored| std::cmp::Reverse(stored.summary.taken));
    found
}

fn detail_label(detail: Detail) -> String {
    match detail {
        Detail::Folders => tr!("snapshot-folders-only"),
        Detail::Files { min_size: 0 } => tr!("snapshot-every-file"),
        Detail::Files { min_size } => tr!("snapshot-files-over", size = format::size(min_size)),
    }
}

fn title(summary: &Summary) -> String {
    let date = format::date(summary.taken);
    if summary.label.is_empty() { date } else { format!("{} · {date}", summary.label) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Folders,
    LargeFiles,
    AllFiles,
}

struct SaveDialog {
    choice: Choice,
    min_mb: u32,
    label: String,
    job: Option<Job<(), io::Result<PathBuf>>>,
}

struct Picker {
    root: PathBuf,
    listing: Option<Job<(), Vec<Stored>>>,
    list: Vec<Stored>,
    opening: Option<Job<(), io::Result<Snapshot>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    document: u64,
    generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Grew,
    Shrank,
}

struct Row {
    change: Change,
    node: Option<NodeId>,
}

struct Compared {
    rows: Vec<Row>,
    tint: Arc<Tint>,
    id: u64,
    /// Bytes gained and freed across the root's direct entries.
    grew: u64,
    freed: u64,
}

struct Comparison {
    baseline: Arc<Snapshot>,
    result: Option<Compared>,
    shown: Option<Key>,
    pending: Option<Job<Key, Compared>>,
    last_started: Option<Instant>,
    filter: Filter,
}

/// What the dialogs report back to the app.
pub enum Event {
    /// A comparison began; show its changes.
    Comparing,
    Error(String),
}

#[derive(Default)]
pub struct Snapshots {
    save: Option<SaveDialog>,
    picker: Option<Picker>,
    comparison: Option<Comparison>,
}

impl Drop for Snapshots {
    fn drop(&mut self) {
        if let Some(comparison) = self.comparison.take() {
            retire(comparison.baseline);
            retire(comparison.result.map(|result| result.rows));
        }
    }
}

impl Snapshots {
    pub fn open_save(&mut self) {
        self.save = Some(SaveDialog { choice: Choice::LargeFiles, min_mb: 10, label: String::new(), job: None });
    }

    pub fn open_picker(&mut self, root: &Path, ctx: &egui::Context) {
        self.picker = Some(Picker {
            root: root.to_owned(),
            listing: Some(Job::spawn((), ctx, list)),
            list: Vec::new(),
            opening: None,
        });
    }

    /// Compare the open document with `baseline` from now on.
    pub fn compare_with(&mut self, baseline: Snapshot) {
        self.stop();
        self.comparison = Some(Comparison {
            baseline: Arc::new(baseline),
            result: None,
            shown: None,
            pending: None,
            last_started: None,
            filter: Filter::All,
        });
    }

    pub fn comparing(&self) -> bool {
        self.comparison.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(comparison) = self.comparison.take() {
            retire(comparison.baseline);
            retire(comparison.result.map(|result| result.rows));
        }
    }

    /// The map colors for the current comparison, once computed.
    pub fn tint(&self) -> Option<(u64, &Arc<Tint>)> {
        let result = self.comparison.as_ref()?.result.as_ref()?;
        Some((result.id, &result.tint))
    }

    /// Keep the comparison current with the open document; a different folder ends it.
    pub fn sync(&mut self, ctx: &egui::Context, tree: &Arc<Tree>, document: u64, generation: u64) {
        let Some(comparison) = &mut self.comparison else { return };
        if !same_root(&comparison.baseline.root, tree.root_path()) {
            self.stop();
            return;
        }
        let key = Key { document, generation };
        if let Some((done, compared)) = Job::poll(&mut comparison.pending) {
            if let Some(old) = comparison.result.replace(compared) {
                retire(old.rows);
            }
            comparison.shown = Some(done);
        }
        // Live edits arrive in bursts; recompare at most every couple of seconds.
        let ready = comparison.last_started.is_none_or(|at| at.elapsed() >= Duration::from_secs(2));
        if comparison.shown != Some(key) && comparison.pending.is_none() {
            if ready || comparison.shown.is_none_or(|shown| shown.document != document) {
                let (tree, baseline) = (tree.clone(), comparison.baseline.clone());
                comparison.pending = Some(Job::spawn(key, ctx, move || compare(&tree, &baseline)));
                comparison.last_started = Some(Instant::now());
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    /// The save and compare dialogs. `tree` is the open document, if any.
    pub fn dialogs(&mut self, ctx: &egui::Context, tree: Option<&Arc<Tree>>) -> Option<Event> {
        let mut event = self.save_dialog(ctx, tree);
        event = self.picker_dialog(ctx).or(event);
        event
    }

    fn save_dialog(&mut self, ctx: &egui::Context, tree: Option<&Arc<Tree>>) -> Option<Event> {
        let dialog = self.save.as_mut()?;
        if let Some(((), result)) = Job::poll(&mut dialog.job) {
            self.save = None;
            return result.err().map(|error| Event::Error(tr!("snapshot-save-error", error = error.to_string())));
        }
        let Some(tree) = tree else {
            self.save = None;
            return None;
        };
        let saving = dialog.job.is_some();
        let mut close = false;
        let mut start = false;
        let modal = egui::Modal::new(Id::new("snapshot-save")).show(ctx, |ui| {
            ui.set_width(440.0);
            close = super::widgets::dialog_header(ui, &tr!("snapshot-save-title"), egui_phosphor::regular::CAMERA);
            ui.add(egui::Label::new(RichText::new(tr!("snapshot-save-body")).color(theme::MUTED)).wrap());
            ui.add_space(8.0);
            ui.add_enabled_ui(!saving, |ui| {
                ui.radio_value(&mut dialog.choice, Choice::Folders, tr!("snapshot-detail-folders"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut dialog.choice, Choice::LargeFiles, tr!("snapshot-detail-large-files"));
                    ui.add_enabled(
                        dialog.choice == Choice::LargeFiles,
                        egui::DragValue::new(&mut dialog.min_mb).range(1..=100_000).suffix(" MB"),
                    );
                });
                ui.radio_value(&mut dialog.choice, Choice::AllFiles, tr!("snapshot-detail-all-files"));
                ui.add_space(6.0);
                ui.label(tr!("snapshot-label"));
                ui.add(egui::TextEdit::singleline(&mut dialog.label).desired_width(f32::INFINITY).char_limit(80));
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if saving {
                    ui.spinner();
                    ui.label(tr!("snapshot-saving"));
                } else {
                    start = ui.button(tr!("snapshot-save-button")).clicked();
                    close |= ui.button(tr!("cancel")).clicked();
                }
            });
        });
        if start {
            let detail = match dialog.choice {
                Choice::Folders => Detail::Folders,
                Choice::LargeFiles => Detail::Files { min_size: u64::from(dialog.min_mb) * 1024 * 1024 },
                Choice::AllFiles => Detail::Files { min_size: 0 },
            };
            let (tree, label) = (tree.clone(), dialog.label.trim().to_owned());
            dialog.job = Some(Job::spawn((), ctx, move || save(&Snapshot::capture(&tree, detail, now(), label))));
        } else if !saving && (close || modal.should_close()) {
            self.save = None;
        }
        None
    }

    fn picker_dialog(&mut self, ctx: &egui::Context) -> Option<Event> {
        let picker = self.picker.as_mut()?;
        if let Some(((), list)) = Job::poll(&mut picker.listing) {
            picker.list = list;
        }
        if let Some(((), opened)) = Job::poll(&mut picker.opening) {
            self.picker = None;
            return Some(match opened {
                Ok(baseline) => {
                    self.compare_with(baseline);
                    Event::Comparing
                }
                Err(error) => Event::Error(error.to_string()),
            });
        }
        let busy = picker.opening.is_some();
        let mut close = false;
        let mut open = None;
        let mut remove = None;
        let modal = egui::Modal::new(Id::new("snapshot-pick")).show(ctx, |ui| {
            ui.set_width(520.0);
            close = super::widgets::dialog_header(ui, &tr!("snapshot-pick-title"), egui_phosphor::regular::GIT_DIFF);
            let root = picker.root.clone();
            let matching: Vec<_> = picker.list.iter().filter(|stored| same_root(&stored.summary.root, &root)).collect();
            if picker.listing.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr!("snapshot-loading"));
                });
            } else if matching.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(tr!("snapshot-none", path = root.display().to_string())).color(theme::MUTED),
                    )
                    .wrap(),
                );
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for stored in matching {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.strong(title(&stored.summary));
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    format::size(stored.summary.size),
                                    detail_label(stored.summary.detail)
                                ))
                                .small()
                                .color(theme::MUTED),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(!busy, egui::Button::new(egui_phosphor::regular::TRASH).frame(false))
                                .on_hover_text(tr!("snapshot-delete"))
                                .clicked()
                            {
                                remove = Some(stored.path.clone());
                            }
                            if ui.add_enabled(!busy, egui::Button::new(tr!("snapshot-compare-button"))).clicked() {
                                open = Some(stored.path.clone());
                            }
                        });
                    });
                    ui.separator();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if super::widgets::external_link(ui, &tr!("snapshot-browse")).clicked()
                    && let Some(path) = crate::platform::pick_snapshot(folder().as_deref())
                {
                    open = Some(path);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                    } else {
                        close |= ui.button(tr!("cancel")).clicked();
                    }
                });
            });
        });
        if let Some(path) = remove {
            // A snapshot the user saved goes to the Recycle Bin, where it can be restored.
            if let Err(error) = trash::delete(&path) {
                return Some(Event::Error(error.to_string()));
            }
            picker.list.retain(|stored| stored.path != path);
        }
        if let Some(path) = open {
            let root = picker.root.clone();
            picker.opening = Some(Job::spawn((), ctx, move || {
                let snapshot = load(&path)?;
                if same_root(&snapshot.root, &root) {
                    Ok(snapshot)
                } else {
                    Err(io::Error::other(tr!("snapshot-other-root", path = snapshot.root.display().to_string())))
                }
            }));
        } else if !busy && (close || modal.should_close()) {
            self.picker = None;
        }
        None
    }

    /// The Changes tab: what differs from the snapshot, largest first.
    pub fn changes_ui(&mut self, ui: &mut Ui) -> Vec<Command> {
        let _span = crate::perf::span("ui.changes");
        let mut out = Vec::new();
        let Some(comparison) = &mut self.comparison else { return out };
        let mut stop = false;
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            let against = title(&comparison.baseline.summary());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                stop = ui.small_button(tr!("snapshot-stop")).clicked();
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(tr!("changes-against", snapshot = against)).strong()).truncate(),
                    );
                });
            });
        });
        ui.horizontal(|ui| {
            for (filter, label) in [
                (Filter::All, tr!("changes-all")),
                (Filter::Grew, tr!("changes-grew")),
                (Filter::Shrank, tr!("changes-shrank")),
            ] {
                ui.selectable_value(&mut comparison.filter, filter, label);
            }
            if comparison.pending.is_some() {
                ui.spinner();
            }
            // The totals double as the map's color key.
            if let Some(result) = &comparison.result {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (_, shrank) = marker(ChangeKind::Shrank);
                    let (_, grew) = marker(ChangeKind::Grew);
                    let freed = format!(
                        "{} {}",
                        egui_phosphor::regular::CARET_DOWN,
                        tr!("changes-freed-total", size = format::size(result.freed))
                    );
                    let more = format!(
                        "{} {}",
                        egui_phosphor::regular::CARET_UP,
                        tr!("changes-grew-total", size = format::size(result.grew))
                    );
                    ui.label(RichText::new(freed).small().color(shrank));
                    ui.label(RichText::new(more).small().color(grew));
                });
            }
        });
        if stop {
            self.stop();
            return out;
        }
        let Some(result) = &comparison.result else {
            ui.weak(tr!("changes-comparing"));
            return out;
        };
        let filter = comparison.filter;
        let rows: Vec<&Row> = result
            .rows
            .iter()
            .filter(|row| match filter {
                Filter::All => true,
                Filter::Grew => matches!(row.change.kind, ChangeKind::Grew | ChangeKind::Added),
                Filter::Shrank => matches!(row.change.kind, ChangeKind::Shrank | ChangeKind::Removed),
            })
            .collect();
        if rows.is_empty() {
            ui.weak(tr!("changes-none"));
            return out;
        }
        let font = FontId::proportional(12.0);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("change-rows").auto_shrink([false, false]).show_rows(
                ui,
                ROW_HEIGHT,
                rows.len(),
                |ui, range| {
                    for index in range {
                        let row = rows[index];
                        let sense = if row.node.is_some() { Sense::click() } else { Sense::hover() };
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), sense);
                        let painter = ui.painter_at(rect);
                        let fill = if response.hovered() && row.node.is_some() {
                            theme::SELECTED_FILL
                        } else if index.is_multiple_of(2) {
                            theme::NAVIGATOR
                        } else {
                            theme::ROW_ALT
                        };
                        painter.rect_filled(rect, 0.0, fill);
                        let (glyph, color) = marker(row.change.kind);
                        painter.text(
                            rect.left_center() + vec2(10.0, 0.0),
                            Align2::CENTER_CENTER,
                            glyph,
                            font.clone(),
                            color,
                        );
                        let delta = signed_size(row.change.delta());
                        let sizes = format!("{} → {}", format::size(row.change.before), format::size(row.change.after));
                        painter.text(
                            rect.right_center() - vec2(4.0, 0.0),
                            Align2::RIGHT_CENTER,
                            &delta,
                            font.clone(),
                            color,
                        );
                        let sizes_rect = painter.text(
                            egui::pos2(rect.right() - 90.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            &sizes,
                            FontId::proportional(11.0),
                            theme::MUTED,
                        );
                        let separator = std::path::MAIN_SEPARATOR;
                        let own = row.change.path.file_name().unwrap_or_default().to_string_lossy();
                        let own = if row.change.dir { format!("{own}{separator}") } else { own.into_owned() };
                        let parent = row
                            .change
                            .path
                            .parent()
                            .filter(|parent| !parent.as_os_str().is_empty())
                            .map(|parent| format!("{}{separator}", parent.display()))
                            .unwrap_or_default();
                        let name = format!("{parent}{own}");
                        // The entry's own name comes first; its folder gets the room that remains,
                        // shortened from the start.
                        let left = rect.left() + 22.0;
                        let width = (sizes_rect.left() - 12.0 - left).max(0.0);
                        let own_color = if row.node.is_some() { theme::TEXT } else { theme::MUTED };
                        let own = super::widgets::fit_text(&painter, &own, &font, width, false);
                        let own_width = painter.layout_no_wrap(own.clone(), font.clone(), own_color).size().x;
                        let parent = super::widgets::fit_text(&painter, &parent, &font, width - own_width, true);
                        let parent_rect = painter.text(
                            egui::pos2(left, rect.center().y),
                            Align2::LEFT_CENTER,
                            parent,
                            font.clone(),
                            theme::MUTED,
                        );
                        let own_left = if parent_rect.width() > 0.0 { parent_rect.right() } else { left };
                        painter.text(
                            egui::pos2(own_left, rect.center().y),
                            Align2::LEFT_CENTER,
                            own,
                            font.clone(),
                            own_color,
                        );
                        let response = response.on_hover_text(format!("{name}\n{sizes} ({delta})"));
                        if let Some(node) = row.node
                            && response.clicked()
                        {
                            out.push(Command::Show(node));
                        }
                    }
                },
            );
        });
        out
    }
}

fn marker(kind: ChangeKind) -> (&'static str, egui::Color32) {
    use egui_phosphor::regular as icon;
    let (grew, shrank) = (theme::DANGER, egui::Color32::from_rgb(96, 186, 128));
    match kind {
        ChangeKind::Grew => (icon::CARET_UP, grew),
        ChangeKind::Added => (icon::PLUS, grew),
        ChangeKind::Shrank => (icon::CARET_DOWN, shrank),
        ChangeKind::Removed => (icon::MINUS, shrank),
    }
}

fn signed_size(delta: i128) -> String {
    let size = format::size(u64::try_from(delta.unsigned_abs()).unwrap_or(u64::MAX));
    if delta < 0 { format!("−{size}") } else { format!("+{size}") }
}

/// The current tree at `baseline`'s detail, compared with it, with each change's node.
fn compare(tree: &Tree, baseline: &Snapshot) -> Compared {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let _span = crate::perf::span("worker.snapshot_compare");
    let current = Snapshot::capture(tree, baseline.detail, now(), "");
    let changes = snapshot::compare(baseline, &current);
    drop(current);
    let (mut grew, mut freed) = (0u64, 0u64);
    for change in changes.iter().filter(|change| change.path.components().count() == 1) {
        let bytes = u64::try_from(change.delta().unsigned_abs()).unwrap_or(u64::MAX);
        if change.delta() > 0 {
            grew = grew.saturating_add(bytes);
        } else {
            freed = freed.saturating_add(bytes);
        }
    }
    // A folder whose whole change happened inside one child says nothing more than that
    // child; list only the deepest entry of such a chain. The map still tints every level.
    let explained: HashSet<(&Path, i128)> =
        changes.iter().filter_map(|change| Some((change.path.parent()?, change.delta()))).collect();
    let redundant: Vec<bool> = changes
        .iter()
        .map(|change| change.dir && explained.contains(&(change.path.as_path(), change.delta())))
        .collect();
    let mut tint = HashMap::new();
    let rows = changes
        .iter()
        .zip(redundant)
        .filter_map(|(change, redundant)| {
            let node = snapshot::locate(tree, &change.path);
            if let Some(node) = node {
                tint.insert(node, Tint::level(change.delta()));
            }
            (!redundant).then(|| Row { change: change.clone(), node })
        })
        .collect();
    Compared { rows, tint: Arc::new(Tint(tint)), id: NEXT.fetch_add(1, Ordering::Relaxed), grew, freed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_match_regardless_of_trailing_separators() {
        assert!(same_root(Path::new("/data/photos/"), Path::new("/data/photos")));
        assert!(!same_root(Path::new("/data/photos"), Path::new("/data/photo")));
        #[cfg(windows)]
        assert!(same_root(Path::new(r"C:\Users\"), Path::new("c:/users")));
    }

    #[test]
    fn comparison_finds_nodes_and_signs_their_tint() {
        use clawback_core::{ROOT, tree::NewEntry};
        let mut tree = Tree::new(Path::new("/r"));
        let dir = tree.add_children(ROOT, vec![NewEntry::dir("a")]).start;
        tree.add_children(dir, vec![NewEntry::file("f", 5_000_000)]);
        tree.sort_all();
        let baseline = Snapshot::capture(&tree, Detail::Files { min_size: 0 }, 1, "");
        let file = snapshot::locate(&tree, Path::new("a/f")).expect("file");
        tree.update_file(file, NewEntry::file("f", 9_000_000_000));
        let compared = compare(&tree, &baseline);
        // The folder's change is all inside the file: one row, but both are tinted.
        assert_eq!(compared.rows.len(), 1);
        assert_eq!(compared.rows[0].node, Some(file));
        assert_eq!(compared.tint.0.len(), 2);
        assert!(compared.tint.0[&file] > 0.9);
        assert_eq!((compared.grew, compared.freed), (9_000_000_000 - 5_000_000, 0));
    }
}
