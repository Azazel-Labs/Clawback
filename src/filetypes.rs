//! Selection-scoped extension totals, prepared on one worker and virtualized in the UI.
mod cache;
use crate::i18n::tr;
use crate::{background::retire, theme};
use clawback_core::{NodeId, Tree, format, tree::flags};
use eframe::egui::{self, Align2, FontId, Sense, Ui, vec2};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    document: u64,
    generation: u64,
    scope: NodeId,
}

struct Row {
    extension: String,
    kind: String,
    size: String,
    files: String,
    share: String,
    fraction: f32,
}
struct Summary {
    rows: Vec<Row>,
    name: String,
}
struct Pending {
    key: Key,
    rx: mpsc::Receiver<Summary>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct FileTypes {
    cache: cache::Cache,
    icons: crate::filetype_icons::Icons,
    displayed: Option<Key>,
    summary: Option<Summary>,
    pending: Option<Pending>,
    last_started: Option<Instant>,
    /// When the shown summary stopped matching the tree; brief refreshes stay silent.
    stale_since: Option<Instant>,
}

impl Drop for FileTypes {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
            retire(pending);
        }
        if let Some(summary) = self.summary.take() {
            retire(summary);
        }
    }
}

impl FileTypes {
    fn show_summary(&mut self, key: Key, summary: Summary) {
        if let Some(old) = self.summary.replace(summary) {
            if let Some(previous) = self.displayed {
                self.cache.insert(previous, old);
            } else {
                retire(old);
            }
        }
        self.displayed = Some(key);
    }

    pub fn ui(&mut self, ui: &mut Ui, tree: &Arc<Tree>, document: u64, generation: u64, scope: NodeId) {
        let _span = crate::perf::span("ui.file_types");
        self.icons.begin_frame(ui.ctx());
        let key = Key { document, generation, scope };
        self.cache.prepare(key);
        if let Some(pending) = &self.pending {
            if pending.key.document != document || pending.key.scope != scope {
                pending.cancel.store(true, Ordering::Relaxed);
            }
            match pending.rx.try_recv() {
                Ok(summary) => {
                    if pending.key.document == document && pending.key.scope == scope {
                        let completed = pending.key;
                        self.show_summary(completed, summary);
                    } else {
                        retire(summary);
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.displayed != Some(key)
            && let Some(summary) = self.cache.take(key)
        {
            if let Some(pending) = &self.pending {
                pending.cancel.store(true, Ordering::Relaxed);
            }
            self.show_summary(key, summary);
            crate::perf::instant("file_types.cache_hit");
        }
        let same_scope = self.displayed.is_some_and(|old| old.document == document && old.scope == scope);
        let ready = self.last_started.is_none_or(|at| at.elapsed() >= Duration::from_millis(750));
        if self.pending.is_none() && self.displayed != Some(key) && (!same_scope || ready) {
            let tree = tree.clone();
            let repaint = ui.ctx().clone();
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            std::thread::spawn(move || {
                if let Some(summary) = summarize(&tree, scope, &worker_cancel) {
                    let _ = tx.send(summary);
                }
                repaint.request_repaint();
            });
            self.pending = Some(Pending { key, rx, cancel });
            self.last_started = Some(Instant::now());
        }
        if self.displayed == Some(key) {
            self.stale_since = None;
        } else {
            self.stale_since.get_or_insert_with(Instant::now);
        }
        let updating = self.stale_since.is_some_and(|since| since.elapsed() >= Duration::from_millis(600));
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.strong(tr!("file-types"));
            if let Some(summary) = &self.summary {
                ui.add(egui::Label::new(&summary.name).truncate()).on_hover_text(&summary.name);
            }
            if updating {
                ui.weak(tr!("updating"));
            }
        });
        if self.displayed != Some(key) {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(header, 3.0, theme::BG);
        let width = header.width();
        let font = FontId::proportional(11.0);
        if width > 420.0 {
            ui.painter().text(
                header.left_center() + vec2(98.0, 0.0),
                Align2::LEFT_CENTER,
                tr!("file-type"),
                font.clone(),
                theme::MUTED,
            );
        }
        for (x, align, label) in [
            (6.0, Align2::LEFT_CENTER, tr!("type")),
            (width - 166.0, Align2::RIGHT_CENTER, "%".to_owned()),
            (width - 65.0, Align2::RIGHT_CENTER, tr!("size")),
            (width - 4.0, Align2::RIGHT_CENTER, tr!("files")),
        ] {
            ui.painter().text(header.left_center() + vec2(x, 0.0), align, label, font.clone(), theme::MUTED);
        }
        let Some(summary) = &self.summary else { return };
        // Keep the previous folder's or snapshot's rows, dimmed, until the new summary is ready.
        if !same_scope {
            ui.multiply_opacity(0.55);
        }
        if summary.rows.is_empty() {
            ui.weak(tr!("no-files-in-this-folder"));
            return;
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("file-type-rows").auto_shrink([false, false]).show_rows(
                ui,
                22.0,
                summary.rows.len(),
                |ui, range| {
                    for index in range {
                        let row = &summary.rows[index];
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            if index % 2 == 0 { theme::NAVIGATOR } else { theme::ROW_ALT },
                        );
                        let mut bar = rect.shrink2(vec2(0.0, 3.0));
                        bar.max.x = bar.min.x + bar.width() * row.fraction;
                        ui.painter().rect_filled(bar, 2.0, theme::ACCENT.gamma_multiply(0.10));
                        let icon_rect =
                            egui::Rect::from_center_size(rect.left_center() + vec2(12.0, 0.0), vec2(16.0, 16.0));
                        self.icons.paint(ui, icon_rect, &row.extension);
                        let mut name_rect = rect;
                        name_rect.max.x = (rect.right() - 210.0).max(rect.left());
                        if width > 420.0 {
                            let mut description_rect = name_rect;
                            description_rect.min.x += 98.0;
                            ui.painter().with_clip_rect(description_rect).text(
                                description_rect.left_center(),
                                Align2::LEFT_CENTER,
                                &row.kind,
                                FontId::proportional(12.0),
                                theme::MUTED,
                            );
                            name_rect.max.x = name_rect.max.x.min(rect.left() + 92.0);
                        }
                        ui.painter().with_clip_rect(name_rect).text(
                            rect.left_center() + vec2(26.0, 0.0),
                            Align2::LEFT_CENTER,
                            &row.extension,
                            FontId::proportional(12.0),
                            theme::TEXT,
                        );
                        for (offset, text) in [(166.0, &row.share), (65.0, &row.size), (4.0, &row.files)] {
                            ui.painter().text(
                                rect.right_center() - vec2(offset, 0.0),
                                Align2::RIGHT_CENTER,
                                text,
                                FontId::proportional(12.0),
                                theme::TEXT,
                            );
                        }
                        response.on_hover_text(format!("{} — {}", row.extension, row.kind));
                    }
                },
            );
        });
    }
}

fn type_name(extension: &str) -> String {
    match extension {
        ".jpg" | ".jpeg" | ".png" | ".webp" | ".gif" | ".raw" | ".svg" => tr!("image"),
        ".mp4" | ".mkv" | ".mov" | ".avi" | ".webm" => tr!("video"),
        ".mp3" | ".flac" | ".wav" | ".ogg" | ".m4a" => tr!("audio"),
        ".zip" | ".7z" | ".gz" | ".tar" | ".rar" => tr!("archive"),
        ".exe" | ".dll" | ".so" | ".dylib" => tr!("application-library"),
        ".iso" | ".dmg" | ".vhd" | ".vhdx" => tr!("disk-image"),
        ".rs" | ".js" | ".ts" | ".py" | ".cpp" | ".h" => tr!("source-code"),
        ".txt" | ".md" | ".pdf" | ".docx" => tr!("document"),
        ".json" | ".yaml" | ".toml" | ".xml" | ".csv" => tr!("data-configuration"),
        ".blend" | ".fbx" | ".obj" | ".gltf" | ".glb" => tr!("asset-3d"),
        ".pak" => tr!("game-archive"),
        ".bin" => tr!("binary-data"),
        ".fig" => tr!("design-document"),
        "(none)" => tr!("no-extension"),
        _ => tr!("file"),
    }
}

fn summarize(tree: &Tree, scope: NodeId, cancel: &AtomicBool) -> Option<Summary> {
    let _span = crate::perf::span("worker.file_types");
    let mut counts = HashMap::<String, (u64, u64)>::new();
    let mut stack = vec![scope];
    let mut total = 0_u64;
    while let Some(id) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let node = tree.node(id);
        if node.has(flags::REMOVED) {
            continue;
        }
        if node.is_dir() {
            stack.extend(node.children.iter().copied());
            continue;
        }
        let extension = Path::new(&node.name)
            .extension()
            .filter(|ext| !ext.is_empty())
            .map_or_else(|| "(none)".to_owned(), |ext| format!(".{}", ext.to_string_lossy().to_lowercase()));
        let entry = counts.entry(extension).or_default();
        entry.0 = entry.0.saturating_add(node.size);
        entry.1 += 1;
        total = total.saturating_add(node.size);
    }
    let mut entries: Vec<_> = counts.into_iter().collect();
    entries.sort_unstable_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let rows = entries
        .into_iter()
        .map(|(extension, (bytes, count))| Row {
            kind: type_name(&extension),
            extension,
            size: format::size(bytes),
            files: format::count(count),
            share: format::percent(bytes, total),
            fraction: if total == 0 { 0.0 } else { bytes as f32 / total as f32 },
        })
        .collect();
    Some(Summary { rows, name: tree.node(scope).name_lossy().into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{Kind, ROOT, tree::NewEntry};

    #[test]
    fn totals_follow_scope_normalize_extensions_and_skip_removed_files() {
        let mut tree = Tree::new(Path::new("demo"));
        let entry = |name: &str, kind, size| NewEntry {
            name: name.into(),
            kind,
            size,
            len: size,
            mtime: 0,
            flags: 0,
            file_id: None,
        };
        let folder = tree.add_children(ROOT, vec![entry("Photos", Kind::Dir, 0)]).start;
        tree.add_children(ROOT, vec![entry("outside.zip", Kind::File, 900)]);
        let ids = tree.add_children(
            folder,
            vec![
                entry("one.JPG", Kind::File, 30),
                entry("two.jpg", Kind::File, 70),
                entry(".gitignore", Kind::File, 10),
                entry("removed.jpg", Kind::File, 500),
            ],
        );
        tree.node_mut(ids.end - 1).flags |= flags::REMOVED;
        let summary = summarize(&tree, folder, &AtomicBool::new(false)).expect("summary");
        assert_eq!(summary.rows.len(), 2);
        assert_eq!(summary.rows[0].extension, ".jpg");
        assert_eq!(summary.rows[0].files, "2");
        assert_eq!(summary.rows[0].size, format::size(100));
        assert_eq!(summary.rows[0].share, format::percent(100, 110));
        assert_eq!(summary.rows[1].extension, "(none)");
        assert!(summarize(&tree, ROOT, &AtomicBool::new(true)).is_none());
    }
}
