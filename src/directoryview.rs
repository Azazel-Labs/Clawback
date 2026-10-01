//! A virtualized directory navigator. Flattening and formatting run off-thread;
//! the UI only draws the rows currently inside the scroll viewport.
use crate::i18n::tr;
use crate::{background::retire, theme};
use clawback_core::{NodeId, ROOT, Tree, format, tree::flags};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Ui, vec2};
use std::{
    collections::HashSet,
    sync::{Arc, mpsc},
};

const ROW_HEIGHT: f32 = 22.0;

struct Row {
    node: NodeId,
    depth: usize,
    expandable: bool,
    name: String,
    size: String,
    share: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    document: u64,
    generation: u64,
    revision: u64,
    scanning: bool,
}

struct Rows {
    rows: Vec<Row>,
    focus: Option<usize>,
}

#[derive(Default)]
pub struct DirectoryView {
    #[cfg(test)]
    drawn_rows: usize,
    document: Option<u64>,
    expanded: HashSet<NodeId>,
    revision: u64,
    focused: Option<NodeId>,
    scanning: bool,
    displayed: Option<Key>,
    rows: Vec<Row>,
    pending: Option<(Key, mpsc::Receiver<Rows>)>,
    reveal: bool,
    scroll_to: Option<f32>,
}

impl DirectoryView {
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        tree: &Arc<Tree>,
        document: u64,
        generation: u64,
        view: NodeId,
        scanning: bool,
    ) -> Option<NodeId> {
        let _span = crate::perf::span("ui.directories");
        if self.document != Some(document) || (self.scanning && !scanning) {
            self.document = Some(document);
            self.expanded.clear();
            self.expanded.insert(ROOT);
            self.focused = None;
            self.revision += 1;
            self.displayed = None;
            retire(std::mem::take(&mut self.rows));
            if let Some(pending) = self.pending.take() {
                retire(pending);
            }
        }
        self.scanning = scanning;
        if self.focused != Some(view) {
            self.expanded.extend(tree.chain(view));
            self.focused = Some(view);
            self.revision += 1;
            self.reveal = true;
        }
        let key = Key { document, generation, revision: self.revision, scanning };
        if let Some((pending_key, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    let compatible = *pending_key == key || (scanning && Key { generation, ..*pending_key } == key);
                    if compatible {
                        retire(std::mem::replace(&mut self.rows, result.rows));
                        self.displayed = Some(*pending_key);
                        if self.reveal {
                            self.scroll_to = result.focus.map(|index| index as f32 * ROW_HEIGHT);
                            self.reveal = false;
                        }
                    } else {
                        retire(result);
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.displayed != Some(key) && self.pending.is_none() {
            let tree = tree.clone();
            let expanded = self.expanded.clone();
            let repaint = ui.ctx().clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _span = crate::perf::span("worker.directories");
                let rows = flatten(&tree, &expanded);
                let focus = rows.iter().position(|r| r.node == view);
                let _ = tx.send(Rows { rows, focus });
                repaint.request_repaint();
            });
            self.pending = Some((key, rx));
        }

        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.strong(tr!("directories"));
            ui.weak(if scanning { tr!("discovering-folders") } else { tr!("expand-to-browse-click-a-folder-to-view") });
        });
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(header, 3.0, theme::BG);
        let font = FontId::proportional(11.0);
        ui.painter().text(
            header.left_center() + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            tr!("folder"),
            font.clone(),
            theme::MUTED,
        );
        ui.painter().text(
            header.right_center() - vec2(80.0, 0.0),
            Align2::RIGHT_CENTER,
            tr!("size"),
            font.clone(),
            theme::MUTED,
        );
        ui.painter().text(
            header.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            tr!("of-scan"),
            font,
            theme::MUTED,
        );
        let enabled = !scanning && self.displayed == Some(key);
        let mut selected = None;
        let mut toggle = None;
        let mut scroll = egui::ScrollArea::vertical().id_salt("directory-navigator").auto_shrink([false, false]);
        if let Some(offset) = self.scroll_to.take() {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        #[cfg(test)]
        {
            self.drawn_rows = 0;
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            scroll.show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
                for index in range {
                    let row = &self.rows[index];
                    #[cfg(test)]
                    {
                        self.drawn_rows += 1;
                    }
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(rect, 0.0, if index % 2 == 0 { theme::NAVIGATOR } else { theme::ROW_ALT });
                    if row.node == view {
                        p.rect_filled(rect, 3.0, egui::Color32::from_rgb(43, 61, 78));
                        p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 1.0, theme::ACCENT);
                    } else if response.hovered() {
                        p.rect_filled(rect, 3.0, theme::PANEL_EDGE);
                    }
                    let indent = (row.depth as f32 * 14.0).min((rect.width() - 220.0).max(0.0));
                    let arrow = Rect::from_min_size(rect.min + vec2(indent + 5.0, 0.0), vec2(18.0, ROW_HEIGHT));
                    if row.expandable {
                        let center = arrow.center();
                        let points = if self.expanded.contains(&row.node) {
                            [center + vec2(-4.0, -2.0), center + vec2(0.0, 2.0), center + vec2(4.0, -2.0)]
                        } else {
                            [center + vec2(-2.0, -4.0), center + vec2(2.0, 0.0), center + vec2(-2.0, 4.0)]
                        };
                        p.add(egui::Shape::line(points.to_vec(), egui::Stroke::new(1.5, theme::TEXT)));
                    }
                    let text = Rect::from_min_max(
                        Pos2::new(arrow.max.x, rect.min.y),
                        Pos2::new((rect.max.x - 168.0).max(arrow.max.x), rect.max.y),
                    );
                    p.with_clip_rect(text.intersect(ui.clip_rect())).text(
                        text.left_center(),
                        Align2::LEFT_CENTER,
                        &row.name,
                        FontId::proportional(12.0),
                        theme::TEXT,
                    );
                    p.text(
                        rect.right_center() - vec2(80.0, 0.0),
                        Align2::RIGHT_CENTER,
                        &row.size,
                        FontId::proportional(12.0),
                        theme::TEXT,
                    );
                    p.text(
                        rect.right_center() - vec2(8.0, 0.0),
                        Align2::RIGHT_CENTER,
                        &row.share,
                        FontId::proportional(12.0),
                        theme::MUTED,
                    );
                    if enabled && response.clicked() {
                        if row.expandable && response.interact_pointer_pos().is_some_and(|pos| arrow.contains(pos)) {
                            toggle = Some(row.node);
                        } else {
                            selected = Some(row.node);
                        }
                    }
                    response.on_hover_text(&row.name);
                }
            });
        });
        if let Some(node) = toggle {
            if !self.expanded.remove(&node) {
                self.expanded.insert(node);
            }
            self.revision += 1;
            ui.ctx().request_repaint();
        }
        selected
    }
}

fn flatten(tree: &Tree, expanded: &HashSet<NodeId>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut stack = vec![(ROOT, 0)];
    while let Some((id, depth)) = stack.pop() {
        let node = tree.node(id);
        if !node.is_dir() || node.has(flags::REMOVED) {
            continue;
        }
        let expandable = node.children.iter().any(|&child| {
            let n = tree.node(child);
            n.is_dir() && !n.has(flags::REMOVED)
        });
        rows.push(Row {
            node: id,
            depth,
            expandable,
            name: node.name_lossy().into_owned(),
            size: format::size(node.size),
            share: format::percent(node.size, tree.root().size),
        });
        if expanded.contains(&id) {
            stack.extend(
                node.children.iter().rev().filter(|&&child| tree.node(child).is_dir()).map(|&child| (child, depth + 1)),
            );
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::tree::{Kind, NewEntry};
    use std::path::Path;

    #[test]
    fn large_directory_lists_only_render_visible_rows() {
        let mut tree = Tree::new(Path::new("/large"));
        tree.add_children(
            ROOT,
            (0..10_000)
                .map(|i| NewEntry {
                    name: format!("folder-{i}").into(),
                    kind: Kind::Dir,
                    size: 0,
                    len: 0,
                    mtime: 0,
                    flags: 0,
                    file_id: None,
                })
                .collect(),
        );
        let tree = Arc::new(tree);
        let mut directories = DirectoryView::default();
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            assert!(std::time::Instant::now() < deadline);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 300.0))),
                    ..Default::default()
                },
                |ui| {
                    directories.ui(ui, &tree, 1, 0, ROOT, false);
                },
            );
            output.textures_delta.clear();
            if directories.pending.is_none() {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(directories.rows.len(), 10_001);
        assert!(directories.drawn_rows > 0 && directories.drawn_rows < 20);
    }

    #[test]
    fn tree_rows_expand_only_directories_and_hide_removed_nodes() {
        let entry =
            |name: &str, kind| NewEntry { name: name.into(), kind, size: 0, len: 0, mtime: 0, flags: 0, file_id: None };
        let mut tree = Tree::new(Path::new("/root"));
        let ids =
            tree.add_children(ROOT, vec![entry("one", Kind::Dir), entry("file", Kind::File), entry("two", Kind::Dir)]);
        let nested = tree.add_children(ids.start, vec![entry("nested", Kind::Dir)]).start;
        let mut expanded = HashSet::from([ROOT]);
        let rows = flatten(&tree, &expanded);
        assert_eq!(rows.iter().map(|r| r.node).collect::<Vec<_>>(), vec![ROOT, ids.start, ids.start + 2]);
        expanded.insert(ids.start);
        assert_eq!(flatten(&tree, &expanded)[2].node, nested);
        assert_eq!(flatten(&tree, &expanded)[2].depth, 2);
        tree.remove(ids.start + 2);
        assert_eq!(flatten(&tree, &expanded).len(), 3);
        expanded.remove(&ROOT);
        assert_eq!(flatten(&tree, &expanded).len(), 1);
    }
}
