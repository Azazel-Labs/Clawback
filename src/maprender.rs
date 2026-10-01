//! Prepare static map geometry off-thread, then amortize font work across frames.
use crate::theme;
use clawback_core::{
    Tree, format,
    layout::{DisplayBox, Item},
    palette,
};
use eframe::egui::{self, Color32, FontId, Mesh, Pos2, Rect, Shape, Vec2, vec2};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

/// Pick the higher-contrast ink once on the layout worker, not each frame.
fn label_color(rgb: [u8; 3]) -> Color32 {
    if palette::dark_ink(rgb) { Color32::BLACK } else { Color32::WHITE }
}

const MAX_LABELS: usize = 384;
// Cheap labels should not hold a completed map back for dozens of frames.
// The elapsed-time budget remains the primary bound on UI work.
const LABELS_PER_FRAME: usize = 64;
const LABEL_BUDGET: Duration = Duration::from_millis(1);

#[derive(Clone, PartialEq, Eq, Hash)]
struct LabelKey {
    color: Color32,
    folder: bool,
    lines: Vec<String>,
}

#[derive(Default)]
pub struct LabelCache {
    scale: Option<u32>,
    shaped: HashMap<LabelKey, Vec<Arc<egui::Galley>>>,
}

struct LabelSpec {
    color: Color32,
    rect: Rect,
    folder: bool,
    lines: Vec<String>,
}

struct Label {
    clip: Rect,
    lines: Vec<(Pos2, Arc<egui::Galley>)>,
}

pub struct PreparedMap {
    mesh: Arc<Mesh>,
    pending: VecDeque<LabelSpec>,
    labels: Vec<Label>,
}

impl PreparedMap {
    #[cfg(test)]
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }
    /// Called by the layout worker. No font/context locks are taken here.
    pub fn build(
        tree: &Tree,
        boxes: &[DisplayBox],
        origin: Pos2,
        schemes: [usize; 2],
        muted: bool,
        free: [u64; 2],
    ) -> Self {
        let _span = crate::perf::span("worker.map_geometry");
        let mut mesh = Mesh::default();
        mesh.reserve_vertices(boxes.len() * 8);
        mesh.reserve_triangles(boxes.len() * 4);
        let rect_of = |b: &DisplayBox| {
            Rect::from_min_size(origin + vec2(b.x as f32, b.y as f32), vec2((b.w + 1) as f32, (b.h + 1) as f32))
        };
        for b in boxes {
            let rect = rect_of(b).shrink(1.0);
            if !rect.is_positive() {
                continue;
            }
            let scheme = schemes[usize::from(b.folder)];
            let base = if b.item == Item::Free { [29, 30, 30] } else { palette::display_color(scheme, b.depth, muted) };
            mesh.add_colored_rect(rect, theme::BORDER);
            let rect = rect.shrink(0.6);
            if !rect.is_positive() {
                continue;
            }
            // Broad directional light, baked once into the cached mesh. Tiny
            // cells and free space stay quiet rather than becoming shiny beads.
            let strength = if b.item == Item::Free || rect.width().min(rect.height()) < 10.0 { 0.025 } else { 0.14 };
            let top = base.map(|c| (f32::from(c) + (255.0 - f32::from(c)) * strength) as u8);
            let middle = base;
            let bottom = base.map(|c| (f32::from(c) * (1.0 - strength)) as u8);
            let start = mesh.vertices.len() as u32;
            for (pos, color) in [
                (rect.left_top(), top),
                (rect.right_top(), middle),
                (rect.right_bottom(), bottom),
                (rect.left_bottom(), middle),
            ] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos,
                    uv: egui::epaint::WHITE_UV,
                    color: Color32::from_rgb(color[0], color[1], color[2]),
                });
            }
            mesh.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
        // At high density most cells cannot carry legible labels. Keep every
        // cell interactive but prioritize the largest readable labels.
        let mut candidates: Vec<_> =
            boxes.iter().filter(|b| b.item != Item::Filler && b.w >= 48 && b.h >= 14).collect();
        candidates.sort_by_key(|b| std::cmp::Reverse(i64::from(b.w) * i64::from(b.h)));
        candidates.truncate(MAX_LABELS);
        let pending = candidates
            .into_iter()
            .map(|b| {
                let mut lines = Vec::new();
                if b.item == Item::Free {
                    lines.push(format!("Free space · {}", format::percent(free[0], free[1])));
                    if b.h >= 42 {
                        lines.push(format::size(free[0]));
                    }
                } else if let Some(id) = b.node() {
                    let node = tree.node(id);
                    // Bound cold text shaping even for unusually long file names.
                    let max_chars = ((b.w / 5) as usize).clamp(8, 180);
                    let name = node.name_lossy();
                    let mut chars = name.chars();
                    let mut short: String = chars.by_ref().take(max_chars).collect();
                    if chars.next().is_some() {
                        short.push('…');
                    }
                    lines.push(short);
                    if !b.folder && b.h >= 56 && b.w >= 88 {
                        lines.push(format::size(node.len));
                        lines.push(format::date(node.mtime));
                    }
                }
                LabelSpec {
                    rect: rect_of(b),
                    folder: b.folder,
                    lines,
                    color: if b.item == Item::Free {
                        theme::TEXT
                    } else {
                        label_color(palette::display_color(schemes[usize::from(b.folder)], b.depth, muted))
                    },
                }
            })
            .collect();
        Self { mesh: Arc::new(mesh), pending, labels: Vec::new() }
    }

    pub fn is_ready(&self) -> bool {
        self.pending.is_empty()
    }

    /// Prepare a replacement without exposing partially populated labels.
    /// Reuse shaped text across redraws; only cache misses spend the font budget.
    pub fn prepare(&mut self, painter: &egui::Painter, cache: &mut LabelCache) {
        let _span = crate::perf::span("ui.map_labels");
        crate::perf::counter("map.pending_labels", self.pending.len() as f64);
        let scale = painter.ctx().pixels_per_point().to_bits();
        if cache.scale != Some(scale) {
            cache.shaped.clear();
            cache.scale = Some(scale);
        }
        let started = Instant::now();
        let mut shaped = 0;
        while started.elapsed() < LABEL_BUDGET {
            let Some(spec) = self.pending.front() else {
                break;
            };
            let key = LabelKey { color: spec.color, folder: spec.folder, lines: spec.lines.clone() };
            let galleys = if let Some(galleys) = cache.shaped.get(&key) {
                galleys.clone()
            } else {
                if shaped >= LABELS_PER_FRAME {
                    break;
                }
                let font = FontId::proportional(if spec.folder { 10.0 } else { 12.0 });
                let galleys: Vec<_> = spec
                    .lines
                    .iter()
                    .map(|text| painter.layout_no_wrap(text.clone(), font.clone(), spec.color))
                    .collect();
                if cache.shaped.len() >= 1024 {
                    cache.shaped.clear();
                }
                cache.shaped.insert(key, galleys.clone());
                shaped += 1;
                galleys
            };
            let spec = self.pending.pop_front().expect("pending label");
            let mut lines = Vec::new();
            let mut y =
                if spec.folder { spec.rect.min.y + 1.0 } else { spec.rect.center().y - galleys.len() as f32 * 7.0 };
            for galley in galleys {
                let x = if spec.folder {
                    spec.rect.min.x + 3.0
                } else {
                    (spec.rect.center().x - galley.size().x * 0.5).max(spec.rect.min.x + 2.0)
                };
                lines.push((egui::pos2(x.floor(), y.floor()), galley));
                y += 14.0;
            }
            self.labels.push(Label { clip: spec.rect.shrink2(Vec2::splat(1.0)), lines });
        }
        if !self.is_ready() {
            painter.ctx().request_repaint();
        }
    }

    pub fn paint(&self, painter: &egui::Painter) {
        let _span = crate::perf::span("ui.map_paint");
        painter.add(Shape::Mesh(self.mesh.clone()));
        for label in &self.labels {
            let clipped = painter.with_clip_rect(label.clip.intersect(painter.clip_rect()));
            for (pos, galley) in &label.lines {
                clipped.galley(*pos, galley.clone(), theme::TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{
        ROOT,
        tree::{Kind, NewEntry},
    };
    use std::path::Path;

    #[test]
    fn dense_labels_are_bounded_and_cached_across_repaints() {
        let mut tree = Tree::new(Path::new("/labels"));
        let ids = tree.add_children(
            ROOT,
            (0..1000)
                .map(|i| NewEntry {
                    name: format!("file-{i}.bin").into(),
                    kind: Kind::File,
                    size: 10,
                    len: 10,
                    mtime: 0,
                    flags: 0,
                    file_id: None,
                })
                .collect(),
        );
        let boxes: Vec<_> = ids
            .map(|id| DisplayBox {
                x: (id % 20) as i32 * 90,
                y: (id / 20) as i32 * 60,
                w: 89,
                h: 59,
                item: Item::Node(id),
                depth: 0,
                folder: false,
            })
            .collect();
        let mut prepared = PreparedMap::build(&tree, &boxes, Pos2::ZERO, [0, 0], false, [0, 0]);
        assert_eq!(prepared.pending.len(), MAX_LABELS);
        let mesh = prepared.mesh.clone();
        let ctx = egui::Context::default();
        let mut cache = LabelCache::default();
        let mut frame = |prepared: &mut PreparedMap| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let before = prepared.labels.len();
                prepared.prepare(ui.painter(), &mut cache);
                prepared.paint(ui.painter());
                assert!(prepared.labels.len() - before <= LABELS_PER_FRAME);
            });
            output.textures_delta.clear();
        };
        frame(&mut prepared);
        assert!(!prepared.pending.is_empty(), "a cold frame must not shape all labels");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !prepared.pending.is_empty() {
            assert!(Instant::now() < deadline);
            frame(&mut prepared);
        }
        assert_eq!(prepared.labels.len(), MAX_LABELS);
        let galley = prepared.labels[0].lines[0].1.clone();
        frame(&mut prepared);
        assert!(Arc::ptr_eq(&mesh, &prepared.mesh));
        assert!(Arc::ptr_eq(&galley, &prepared.labels[0].lines[0].1));
    }
}
