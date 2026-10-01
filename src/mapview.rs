//! The folder map: SpaceMonger's `CFolderView`, rebuilt on egui.
//!
//! Interaction, as in SpaceMonger 1.4:
//! * **Left button down** selects the box under the cursor (clicking a
//!   folder's frame or title band selects the folder; clicking free space or
//!   an unnamed filler box clears the selection).
//! * **Double-click** a bucket to zoom into its folder; files zoom to their parent.
//! * **Right-click** selects the box and pops up the command menu.
//! * Resting the mouse shows a *name tip* over truncated labels and an
//!   *info tip* near the cursor; any mouse or keyboard input hides them.
//! * Zooming, resizing, changing settings, or switching away from the app
//!   clears the selection.
//! * Folder navigation is immediate; completed redraws swap atomically.
use crate::i18n::tr;

use crate::{
    background::retire,
    maprender::{LabelCache, PreparedMap},
    theme,
};
use clawback_core::layout::{self, DisplayBox, LayoutSettings};
use clawback_core::{NodeId, Settings, Tree, format};
use eframe::egui::{
    self, Color32, FontId, Id, LayerId, Order, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Font used for box labels. SpaceMonger used the 9px "Small Fonts".
const LABEL_FONT: f32 = 10.0;
/// Vertical distance between label lines.
const LINE: f32 = 14.0;
/// Info tips appear this far below-right of the cursor.
const TIP_OFFSET: f32 = 16.0;
/// Folder frames take precedence; otherwise choose the deepest containing bucket.
fn zoom_target(boxes: &[DisplayBox], x: i32, y: i32) -> Option<NodeId> {
    if let Some(index) = layout::hit_test(boxes, x, y)
        && boxes[index].folder
    {
        return boxes[index].node();
    }
    boxes.iter().rev().find(|b| b.folder && b.contains(x, y)).and_then(DisplayBox::node)
}

const INFO_BG: Color32 = theme::SURFACE;

/// What the map asks the application to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    ZoomTo(NodeId),
    ZoomPath(std::path::PathBuf),
    Back,
    ZoomOut,
    ZoomFull,
    RunOpen(NodeId),
    Delete(NodeId),
    OpenDrive,
    Rescan,
    ToggleFree,
    Properties(NodeId),
}

/// Everything the map needs to draw one frame.
#[derive(Clone, Copy)]
pub struct MapInput<'a> {
    pub scanning: bool,
    pub tree: &'a Arc<Tree>,
    pub view: NodeId,
    pub generation: u64,
    pub settings: &'a Settings,
    /// The free-space pseudo entry, when it should be shown.
    pub free: Option<u64>,
    pub disk_free: u64,
    pub disk_total: u64,
}

impl MapInput<'_> {
    fn zoomed(&self) -> bool {
        self.view != clawback_core::ROOT
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    view: NodeId,
    w: i32,
    h: i32,
    generation: u64,
    layout: LayoutSettings,
    free: Option<u64>,
    origin: [u32; 2],
    pixels_per_point: u32,
    schemes: [usize; 2],
    muted: bool,
}

// Generations may advance while labels prepare. A complete older snapshot is
// still coherent and avoids starving live scans; geometry/view changes are not.
fn same_view(a: LayoutKey, b: LayoutKey) -> bool {
    LayoutKey { generation: b.generation, ..a } == b
}

/// Cached info-tip text for the hovered node.
struct InfoTip {
    node: NodeId,
    lines: Vec<String>,
    folder: bool,
}

type LayoutResult = (Arc<Tree>, Vec<DisplayBox>, PreparedMap);

pub struct MapView {
    layout_started: Option<std::time::Instant>,
    layout_rx: Option<(LayoutKey, mpsc::Receiver<LayoutResult>)>,
    display_tree: Option<Arc<Tree>>,
    prepared: Option<PreparedMap>,
    boxes: Vec<DisplayBox>,
    key: Option<LayoutKey>,
    selected: Option<usize>,
    /// Box (and pointer position) the last context menu was opened for.
    last_input: f64,
    staging: Option<(LayoutKey, LayoutResult)>,
    label_cache: LabelCache,
    queued: Vec<Command>,
    focused: Option<bool>,
    rect: Rect,
    info: Option<InfoTip>,
    info_rx: Option<mpsc::Receiver<InfoTip>>,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            layout_started: None,
            layout_rx: None,
            display_tree: None,
            prepared: None,
            boxes: Vec::new(),
            key: None,
            selected: None,
            last_input: 0.0,
            staging: None,
            label_cache: LabelCache::default(),
            queued: Vec::new(),
            focused: None,
            rect: Rect::ZERO,
            info: None,
            info_rx: None,
        }
    }
}

impl MapView {
    pub fn selected_node(&self) -> Option<NodeId> {
        self.selected.and_then(|i| self.boxes.get(i)).and_then(DisplayBox::node)
    }

    pub fn selected_is_folder(&self) -> bool {
        self.selected.and_then(|i| self.boxes.get(i)).is_some_and(|b| b.folder)
    }

    /// Forget the layout (new document, or none).
    pub fn reset(&mut self) {
        self.layout_started = None;
        if let Some(pending) = self.layout_rx.take() {
            retire(pending);
        }
        if let Some(prepared) = self.prepared.take() {
            retire(prepared);
        }
        if let Some(tree) = self.display_tree.take() {
            retire(tree);
        }
        self.boxes.clear();
        self.key = None;
        self.selected = None;
        if let Some(staging) = self.staging.take() {
            retire(staging);
        }
        self.info = None;
        self.info_rx = None;
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    pub fn zoom_in(&mut self) {
        if let Some(node) =
            self.selected.and_then(|i| self.boxes.get(i)).filter(|b| b.folder).and_then(DisplayBox::node)
        {
            self.queued.push(Command::ZoomTo(node));
        }
    }
    pub fn zoom_out(&mut self) {
        self.queued.push(Command::ZoomOut);
    }
    pub fn zoom_full(&mut self) {
        self.queued.push(Command::ZoomFull);
    }

    fn box_rect(&self, b: &DisplayBox) -> Rect {
        Rect::from_min_size(
            self.display_origin() + vec2(b.x as f32, b.y as f32),
            vec2((b.w + 1) as f32, (b.h + 1) as f32),
        )
    }

    fn display_origin(&self) -> Pos2 {
        self.key.map_or(self.rect.min, |key| pos2(f32::from_bits(key.origin[0]), f32::from_bits(key.origin[1])))
    }

    fn hit(&self, pos: Option<Pos2>) -> Option<usize> {
        let p = pos?;
        let origin = self.display_origin();
        let (x, y) = ((p.x - origin.x).floor() as i32, (p.y - origin.y).floor() as i32);
        layout::hit_test(&self.boxes, x, y)
    }

    /// Draw the map and handle input. Returns commands for the application.
    pub fn ui(&mut self, ui: &mut Ui, input: Option<&MapInput<'_>>) -> Vec<Command> {
        let _span = crate::perf::span("ui.map");
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        self.rect = rect;
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let mut out = std::mem::take(&mut self.queued);

        let painter = ui.painter_at(rect);
        let Some(input) = input else {
            painter.rect_filled(rect, 8.0, theme::BG);
            self.reset();
            return out;
        };

        // Switching away from (or back to) the app clears the selection.
        let focused = ctx.input(|i| i.focused);
        if self.focused.is_some_and(|f| f != focused) {
            self.selected = None;
        }
        self.focused = Some(focused);

        // Rebuild the layout when anything that affects it changes. Like
        // SpaceMonger, this also drops the selection.
        let key = LayoutKey {
            view: input.view,
            w: rect.width().floor() as i32,
            h: rect.height().floor() as i32,
            generation: input.generation,
            layout: input.settings.layout(),
            free: input.free,
            origin: [rect.min.x.to_bits(), rect.min.y.to_bits()],
            pixels_per_point: ctx.pixels_per_point().to_bits(),
            schemes: [input.settings.file_color, input.settings.folder_color],
            muted: input.settings.mute_palette,
        };
        if let Some((pending_key, rx)) = &self.layout_rx {
            match rx.try_recv() {
                Ok(result) => {
                    if same_view(*pending_key, key) {
                        self.staging = Some((*pending_key, result));
                    } else {
                        retire(result);
                    }
                    self.layout_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.layout_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.staging.as_ref().is_some_and(|(staged_key, _)| !same_view(*staged_key, key)) {
            retire(self.staging.take());
        }
        if let Some((_, (_, _, prepared))) = &mut self.staging {
            prepared.prepare(&painter, &mut self.label_cache);
        }
        if self.staging.as_ref().is_some_and(|(_, (_, _, prepared))| prepared.is_ready()) {
            crate::perf::instant("map.ready");
            if let Some(started) = self.layout_started.take() {
                crate::perf::counter("map.request_to_ready_ms", started.elapsed().as_secs_f64() * 1000.0);
            }
            let (ready_key, (tree, boxes, prepared)) = self.staging.take().expect("ready map");
            if let Some(old) = self.display_tree.replace(tree) {
                retire(old);
            }
            if let Some(old) = self.prepared.replace(prepared) {
                retire(old);
            }
            self.boxes = boxes;
            self.key = Some(ready_key);
            self.selected = None;
            self.info = None;
            self.info_rx = None;
        }
        if self.key != Some(key) && self.layout_rx.is_none() && self.staging.is_none() {
            self.layout_started = crate::perf::enabled().then(std::time::Instant::now);
            crate::perf::instant("map.requested");
            let tree = Arc::clone(input.tree);
            let repaint = ctx.clone();
            let (tx, rx) = mpsc::channel();
            let origin = rect.min;
            let free = [input.disk_free, input.disk_total];
            std::thread::spawn(move || {
                let _span = crate::perf::span("worker.map_layout");
                let boxes = layout::build(&tree, key.view, key.w, key.h, &key.layout, key.free);
                crate::perf::counter("map.boxes", boxes.len() as f64);
                let prepared = PreparedMap::build(&tree, &boxes, origin, key.schemes, key.muted, free);
                let _ = tx.send((tree, boxes, prepared));
                repaint.request_repaint();
            });
            self.layout_rx = Some((key, rx));
        }

        // Keep painting the previous complete snapshot until its replacement
        // is ready. Tree and boxes must always refer to the same node ids.
        let displayed = self.display_tree.clone();
        let input = &MapInput { tree: displayed.as_ref().unwrap_or(input.tree), ..*input };
        let interactive = !input.scanning && self.key == Some(key);

        // Any mouse or keyboard activity hides tips and restarts their timers.
        let active = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::Key { .. }
                        | egui::Event::MouseWheel { .. }
                )
            })
        });
        if active {
            self.last_input = now;
        }

        let pointer = response.hover_pos();
        if response.double_clicked() {
            let origin = self.display_origin();
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = ((pos.x - origin.x).floor() as i32, (pos.y - origin.y).floor() as i32);
                // Use the displayed snapshot, whose IDs may differ from a new scan preview.
                if let Some(node) = zoom_target(&self.boxes, x, y) {
                    out.push(Command::ZoomPath(input.tree.path(node)));
                }
            }
        }
        if interactive {
            self.handle_mouse(&ctx, &response, pointer, input, &mut out);
        }

        self.paint(&painter, input, pointer);

        if interactive && pointer.is_some() && !ctx.any_popup_open() {
            self.paint_tips(&ctx, input, pointer, now);
        }
        out.append(&mut self.queued);
        out
    }

    fn handle_mouse(
        &mut self,
        ctx: &egui::Context,
        response: &Response,
        pointer: Option<Pos2>,
        input: &MapInput<'_>,
        out: &mut Vec<Command>,
    ) {
        let press = ctx.input(|i| i.pointer.primary_pressed() || i.pointer.secondary_pressed());
        if press && response.hovered() {
            self.selected = self.hit(pointer);
        }
        if response.secondary_clicked() {
            self.selected = self.hit(response.interact_pointer_pos());
        }
        response.context_menu(|ui| self.context_menu(ui, input, out));
    }

    /// The right-click menu (`CFolderView::OnRButtonUp`).
    fn context_menu(&mut self, ui: &mut Ui, input: &MapInput<'_>, out: &mut Vec<Command>) {
        let cur = self.selected.map(|i| self.boxes[i]);
        let node = cur.and_then(|b| b.node());
        let folder = cur.is_some_and(|b| b.folder);
        let s = input.settings;
        ui.set_min_width(150.0);

        if item(ui, tr!("zoom-in"), folder, folder) {
            self.zoom_in();
        }
        if item(ui, tr!("zoom-out"), cur.is_some() && input.zoomed(), false) {
            self.zoom_out();
        }
        if item(ui, tr!("zoom-full"), cur.is_some() && input.zoomed(), false) {
            self.zoom_full();
        }
        ui.separator();
        if item(ui, tr!("run-open"), node.is_some(), cur.is_some() && !folder)
            && let Some(n) = node
        {
            out.push(Command::RunOpen(n));
        }
        if item(ui, tr!("delete"), node.is_some() && !s.disable_delete, false)
            && let Some(n) = node
        {
            out.push(Command::Delete(n));
        }
        ui.separator();
        if item(ui, tr!("open-drive"), true, false) {
            out.push(Command::OpenDrive);
        }
        if item(ui, tr!("rescan-drive"), true, false) {
            out.push(Command::Rescan);
        }
        let mut free = s.show_free;
        if ui.checkbox(&mut free, tr!("show-free-space")).clicked() {
            out.push(Command::ToggleFree);
            ui.close();
        }
        ui.separator();
        if item(ui, tr!("properties-2"), node.is_some(), false)
            && let Some(n) = node
        {
            out.push(Command::Properties(n));
        }
    }

    fn paint(&mut self, painter: &egui::Painter, input: &MapInput<'_>, pointer: Option<Pos2>) {
        painter.rect_filled(self.rect, 8.0, theme::BG);
        if let Some(prepared) = &mut self.prepared {
            prepared.paint(painter);
        }
        let hovered = self.hit(pointer);
        let fade = painter.ctx().animate_bool(Id::new("map-hover-glow"), hovered.is_some());
        // Only changed interactive cells need extra drawing, not every box.
        for (index, alpha) in [(self.selected, 1.0), (hovered.filter(|h| Some(*h) != self.selected), fade)] {
            let Some(b) = index.and_then(|i| self.boxes.get(i)) else {
                continue;
            };
            let rect = self.box_rect(b).shrink(1.0);
            if input.settings.rollover_box {
                painter.rect_filled(rect, 1.0, theme::ACCENT.gamma_multiply(alpha * 0.08));
            }
            painter.rect_stroke(
                rect,
                2.0,
                Stroke::new(5.0, theme::ACCENT.gamma_multiply(alpha * 0.08)),
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                rect,
                2.0,
                Stroke::new(1.3, theme::ACCENT.gamma_multiply(alpha * 0.9)),
                StrokeKind::Inside,
            );
        }
    }

    fn paint_tips(&mut self, ctx: &egui::Context, input: &MapInput<'_>, pointer: Option<Pos2>, now: f64) {
        let Some(i) = self.hit(pointer) else {
            self.info = None;
            return;
        };
        let b = self.boxes[i];
        let Some(node) = b.node() else { return };
        let idle = now - self.last_input;
        let s = input.settings;
        let tips = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("clawback-map-tips")));
        let screen = ctx.content_rect();

        let name_delay = f64::from(s.nametip_delay_ms) / 1000.0;
        let info_delay = f64::from(s.infotip_delay_ms) / 1000.0;
        let mut wake = f64::INFINITY;

        if s.show_name_tips {
            if idle >= name_delay {
                self.paint_name_tip(&tips, screen, &b, i, input);
            } else {
                wake = wake.min(name_delay - idle);
            }
        }
        if s.show_info_tips {
            if let Some(rx) = &self.info_rx {
                match rx.try_recv() {
                    Ok(tip) => {
                        self.info = Some(tip);
                        self.info_rx = None;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => self.info_rx = None,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
            if idle >= info_delay && self.info_rx.is_none() && self.info.as_ref().is_none_or(|t| t.node != node) {
                let tree = input.tree.clone();
                let settings = s.clone();
                let repaint = ctx.clone();
                let (tx, rx) = mpsc::channel();
                self.info_rx = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(info_tip(&tree, node, &settings));
                    repaint.request_repaint();
                });
            }
            if idle >= info_delay {
                if let (Some(tip), Some(p)) = (self.info.as_ref().filter(|tip| tip.node == node), pointer) {
                    paint_info_tip(&tips, screen, tip, p, s.tip_icon);
                }
            } else {
                wake = wake.min(info_delay - idle);
            }
        }
        if wake.is_finite() {
            ctx.request_repaint_after(Duration::from_secs_f64(wake.max(0.0) + 0.005));
        }
    }

    /// `CFolderView::SetupNameTip`: show the full name over a truncated label.
    fn paint_name_tip(
        &self,
        tips: &egui::Painter,
        screen: Rect,
        tile: &DisplayBox,
        index: usize,
        input: &MapInput<'_>,
    ) {
        let Some(node) = tile.node() else { return };
        let origin = self.rect.min;
        let (x, y, w, h) = (tile.x as f32, tile.y as f32, (tile.w + 1) as f32, (tile.h + 1) as f32);
        let selected = self.selected == Some(index);
        let text_color = if selected { theme::ACCENT } else { theme::TEXT };
        let galley = tips.layout_no_wrap(
            input.tree.node(node).name_lossy().into_owned(),
            FontId::proportional(if tile.folder { LABEL_FONT } else { 12.0 }),
            text_color,
        );
        let size = galley.size();
        let mut fits = 0;
        let tx = if size.x > w - 2.0 {
            x + 2.0
        } else {
            fits += 1;
            if tile.folder { x + 2.0 } else { x + (w - size.x) / 2.0 }
        };
        let mut ty = if size.y > h - 2.0 {
            y + 1.0
        } else {
            fits += 1;
            if tile.folder { y + 1.0 } else { y + (h - size.y) / 2.0 }
        };
        if fits == 2 {
            return; // the label is fully visible already
        }
        if !tile.folder && h >= 56.0 && w >= 88.0 {
            ty -= LINE;
        }
        let bg = theme::SURFACE;
        let bounds = Rect::from_min_size(origin + vec2(tx - 2.0, ty - 1.0), size + vec2(4.0, 2.0));
        let bounds = push_on_screen(bounds, screen);
        tips.rect_filled(bounds, 0.0, bg);
        tips.rect_stroke(bounds, 0.0, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
        tips.galley(bounds.min + vec2(2.0, 1.0), galley, text_color);
    }
}

/// A menu entry; the default action is drawn bold, like `MFS_DEFAULT`.
fn item(ui: &mut Ui, label: impl Into<String>, enabled: bool, bold: bool) -> bool {
    let text = if bold { RichText::new(label).strong() } else { RichText::new(label) };
    let clicked = ui.add_enabled(enabled, egui::Button::new(text)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn info_tip(tree: &Tree, node: NodeId, s: &Settings) -> InfoTip {
    let n = tree.node(node);
    let path = tree.path(node);
    let mut lines = Vec::new();

    let mut first = String::new();
    if s.tip_path
        && let Some(parent) = path.parent()
    {
        first.push_str(&parent.display().to_string());
        if !first.ends_with(std::path::MAIN_SEPARATOR) {
            first.push(std::path::MAIN_SEPARATOR);
        }
    }
    if s.tip_name {
        first.push_str(&n.name_lossy());
    }
    if s.tip_path || s.tip_name {
        lines.push(first);
    }

    let mut second = String::new();
    if s.tip_size {
        second.push_str(&format::size(n.display_len()));
    }
    if s.tip_attrib {
        let attrs = crate::platform::attributes(&path);
        if s.tip_size && !attrs.is_empty() {
            second.push_str("  /  ");
        }
        second.push_str(&attrs.join(" "));
    }
    if s.tip_size || s.tip_attrib {
        lines.push(second);
    }
    if s.tip_date {
        lines.push(format::date(n.mtime));
    }
    InfoTip { node, lines, folder: n.is_dir() }
}

fn paint_info_tip(tips: &egui::Painter, screen: Rect, tip: &InfoTip, pointer: Pos2, icon: bool) {
    let font = FontId::proportional(12.0);
    let galleys: Vec<_> = tip
        .lines
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| tips.layout_no_wrap(l.clone(), font.clone(), theme::TEXT))
        .collect();
    if galleys.is_empty() && !icon {
        return;
    }
    let pad = 10.0;
    let icon_w = if icon { 32.0 + pad } else { 0.0 };
    let text_w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let text_h: f32 = galleys.iter().map(|g| g.size().y).sum();
    let size = vec2(icon_w + text_w + 2.0 * pad, text_h.max(if icon { 32.0 } else { 0.0 }) + 2.0 * pad);
    let mut pos = pointer;
    pos.x = pos.x.min(screen.max.x - size.x - TIP_OFFSET);
    pos.y = pos.y.min(screen.max.y - size.y - TIP_OFFSET);
    let r = Rect::from_min_size(pos + Vec2::splat(TIP_OFFSET), size);
    tips.rect_filled(r, 8.0, INFO_BG);
    tips.rect_stroke(r, 8.0, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
    if icon {
        paint_icon(tips, Rect::from_min_size(r.min + Vec2::splat(pad), Vec2::splat(32.0)), tip.folder);
    }
    let mut y = r.min.y + pad;
    let x = r.min.x + pad + icon_w;
    for g in galleys {
        let h = g.size().y;
        tips.galley(pos2(x, y), g, theme::TEXT);
        y += h;
    }
}

/// A small folder or document glyph for the info tip.
fn paint_icon(p: &egui::Painter, r: Rect, folder: bool) {
    let outline = Stroke::new(1.0, Color32::from_gray(60));
    if folder {
        let tab = Rect::from_min_size(r.min + vec2(2.0, 5.0), vec2(12.0, 5.0));
        let body = Rect::from_min_max(r.min + vec2(2.0, 9.0), r.max - vec2(2.0, 5.0));
        let yellow = Color32::from_rgb(0xF5, 0xD0, 0x5A);
        p.rect_filled(tab, 1.0, yellow);
        p.rect_stroke(tab, 1.0, outline, StrokeKind::Inside);
        p.rect_filled(body, 1.0, yellow);
        p.rect_stroke(body, 1.0, outline, StrokeKind::Inside);
    } else {
        let page = Rect::from_min_max(r.min + vec2(6.0, 2.0), r.max - vec2(6.0, 2.0));
        p.rect_filled(page, 0.0, Color32::WHITE);
        p.rect_stroke(page, 0.0, outline, StrokeKind::Inside);
        for k in 0..5 {
            let yy = page.min.y + 7.0 + k as f32 * 4.0;
            p.line_segment(
                [pos2(page.min.x + 3.0, yy), pos2(page.max.x - 3.0, yy)],
                Stroke::new(1.0, Color32::from_gray(170)),
            );
        }
    }
}

fn push_on_screen(mut r: Rect, screen: Rect) -> Rect {
    if r.max.x > screen.max.x {
        r = r.translate(vec2(screen.max.x - r.max.x, 0.0));
    }
    if r.max.y > screen.max.y {
        r = r.translate(vec2(0.0, screen.max.y - r.max.y));
    }
    if r.min.x < screen.min.x {
        r = r.translate(vec2(screen.min.x - r.min.x, 0.0));
    }
    if r.min.y < screen.min.y {
        r = r.translate(vec2(0.0, screen.min.y - r.min.y));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{
        ROOT,
        tree::{Kind, NewEntry},
    };
    use std::{path::Path, time::Instant};

    #[test]
    fn pending_layout_keeps_rendering_matching_snapshot_without_waiting() {
        let mut tree = Tree::new(Path::new("/preview"));
        tree.add_children(
            ROOT,
            vec![NewEntry {
                name: "visible.bin".into(),
                kind: Kind::File,
                size: 100,
                len: 100,
                mtime: 0,
                flags: 0,
                file_id: None,
            }],
        );
        let tree = Arc::new(tree);
        let settings = Settings::default();
        let input = MapInput {
            scanning: true,
            tree: &tree,
            view: ROOT,
            generation: 0,
            settings: &settings,
            free: None,
            disk_free: 0,
            disk_total: 100,
        };
        let ctx = egui::Context::default();
        let mut map = MapView::default();
        let frame = |map: &mut MapView, input: &MapInput<'_>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
                    ..Default::default()
                },
                |ui| {
                    map.ui(ui, Some(input));
                },
            );
            output.textures_delta.clear();
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while map.boxes.is_empty() {
            assert!(Instant::now() < deadline, "background layout did not complete");
            frame(&mut map, &input);
            std::thread::yield_now();
        }
        let old_boxes = map.boxes.clone();
        let old_key = map.key.unwrap();
        // Hold the next layout pending indefinitely. The replacement tree has
        // no child #1: painting old boxes against it would panic.
        let (_tx, rx) = mpsc::channel();
        map.layout_rx = Some((LayoutKey { generation: 1, ..old_key }, rx));
        let replacement = Arc::new(Tree::new(Path::new("/replacement")));
        frame(&mut map, &MapInput { tree: &replacement, generation: 1, ..input });
        assert_eq!(map.boxes, old_boxes);
        assert!(Arc::ptr_eq(map.display_tree.as_ref().unwrap(), &tree));
        assert!(map.layout_rx.is_some());
    }
    #[test]
    fn replacement_keeps_old_labels_until_atomic_swap() {
        let make_tree = |prefix: &str, count| {
            let mut tree = Tree::new(Path::new("/redraw"));
            tree.add_children(
                ROOT,
                (0..count)
                    .map(|i| NewEntry {
                        name: format!("{prefix}-{i}.txt").into(),
                        kind: Kind::File,
                        size: 100,
                        len: 100,
                        mtime: 0,
                        flags: 0,
                        file_id: None,
                    })
                    .collect(),
            );
            Arc::new(tree)
        };
        let old = make_tree("old", 1);
        let replacement = make_tree("new", 500);
        let settings = Settings::default();
        let ctx = egui::Context::default();
        let mut map = MapView::default();
        let frame = |map: &mut MapView, tree: &Arc<Tree>, generation| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
                    ..Default::default()
                },
                |ui| {
                    map.ui(
                        ui,
                        Some(&MapInput {
                            tree,
                            generation,
                            view: ROOT,
                            settings: &settings,
                            scanning: false,
                            free: None,
                            disk_free: 0,
                            disk_total: tree.root().size,
                        }),
                    );
                },
            );
            output.textures_delta.clear();
            assert!(map.prepared.as_ref().is_none_or(PreparedMap::is_ready), "never show partial labels");
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while map.prepared.is_none() {
            assert!(Instant::now() < deadline);
            frame(&mut map, &old, 0);
            std::thread::yield_now();
        }
        let key = LayoutKey { generation: 1, ..map.key.unwrap() };
        let boxes: Vec<_> = (1..=500)
            .map(|node| DisplayBox {
                x: (node % 10) as i32 * 60,
                y: (node / 10) as i32 * 60,
                w: 59,
                h: 59,
                item: layout::Item::Node(node),
                depth: 0,
                folder: false,
            })
            .collect();
        let prepared = PreparedMap::build(&replacement, &boxes, map.rect.min, [0, 0], false, [0, 0]);
        map.staging = Some((key, (replacement.clone(), boxes, prepared)));
        frame(&mut map, &replacement, 1);
        assert!(Arc::ptr_eq(map.display_tree.as_ref().unwrap(), &old));
        assert_eq!(map.prepared.as_ref().unwrap().label_count(), 1);
        while map.staging.is_some() {
            assert!(Instant::now() < deadline);
            frame(&mut map, &replacement, 1);
        }
        assert!(Arc::ptr_eq(map.display_tree.as_ref().unwrap(), &replacement));
        assert_eq!(map.prepared.as_ref().unwrap().label_count(), 384);
    }

    #[test]
    fn double_click_targets_folder_frames_and_file_containers() {
        let boxes = vec![
            DisplayBox { x: 0, y: 0, w: 200, h: 200, item: layout::Item::Node(1), depth: 0, folder: true },
            DisplayBox { x: 10, y: 30, w: 100, h: 100, item: layout::Item::Node(2), depth: 1, folder: true },
            DisplayBox { x: 20, y: 60, w: 60, h: 60, item: layout::Item::Node(3), depth: 2, folder: false },
        ];
        assert_eq!(zoom_target(&boxes, 1, 1), Some(1));
        assert_eq!(zoom_target(&boxes, 11, 31), Some(2));
        assert_eq!(zoom_target(&boxes, 50, 90), Some(2));
        assert_eq!(zoom_target(&boxes, 300, 300), None);
    }

    #[test]
    fn folder_navigation_has_no_animation_delay() {
        let mut map = MapView::default();
        map.boxes.push(DisplayBox { x: 0, y: 0, w: 100, h: 100, item: layout::Item::Node(1), depth: 0, folder: true });
        map.selected = Some(0);
        map.zoom_in();
        map.zoom_out();
        map.zoom_full();
        assert_eq!(map.queued, vec![Command::ZoomTo(1), Command::ZoomOut, Command::ZoomFull]);
    }

    /// Run with `cargo test frame_cost_dense_map -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual frame-time measurement"]
    fn frame_cost_dense_map() {
        measure_scene(64, 128);
        measure_scene(32, 32);
    }

    fn measure_scene(groups: u64, files: u64) {
        let mut tree = Tree::new(Path::new("/benchmark"));
        for group in 0..groups {
            let dir = tree.add_children(
                ROOT,
                vec![NewEntry {
                    name: format!("folder-{group}").into(),
                    kind: Kind::Dir,
                    size: 0,
                    len: 0,
                    mtime: 0,
                    flags: 0,
                    file_id: None,
                }],
            );
            tree.add_children(
                dir.start,
                (0..files)
                    .map(|file| NewEntry {
                        name: format!("document-{group}-{file}-with-a-descriptive-name.bin").into(),
                        kind: Kind::File,
                        size: 128 + file * 32,
                        len: 128 + file * 32,
                        mtime: 1_700_000_000,
                        flags: 0,
                        file_id: None,
                    })
                    .collect(),
            );
        }
        let tree = Arc::new(tree);
        let settings = Settings { density: 3, ..Settings::default() };
        let input = MapInput {
            scanning: false,
            tree: &tree,
            view: ROOT,
            generation: 0,
            settings: &settings,
            free: None,
            disk_free: 0,
            disk_total: tree.root().size,
        };
        let ctx = egui::Context::default();
        theme::apply(&ctx, "en");
        let mut map = MapView::default();
        let frame = |map: &mut MapView, pointer: Option<Pos2>| {
            let started = Instant::now();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))),
                    events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                    ..Default::default()
                },
                |ui| {
                    map.ui(ui, Some(&input));
                },
            );
            let _ = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            output.textures_delta.clear();
            started.elapsed().as_secs_f64() * 1000.0
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while map.boxes.is_empty() {
            assert!(Instant::now() < deadline);
            frame(&mut map, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..80 {
            frame(&mut map, None);
        }
        let mut times: Vec<_> = (0..60).map(|_| frame(&mut map, None)).collect();
        times.sort_by(f64::total_cmp);
        eprintln!(
            "dense map: {} boxes; frame CPU p50={:.2}ms p95={:.2}ms max={:.2}ms",
            map.boxes.len(),
            times[30],
            times[57],
            times[59]
        );
        let mut hovering: Vec<_> =
            (0..60).map(|i| frame(&mut map, Some(pos2(30.0 + i as f32 * 29.0, 500.0)))).collect();
        hovering.sort_by(f64::total_cmp);
        eprintln!(
            "hover: {} cached labels; CPU p50={:.2}ms p95={:.2}ms max={:.2}ms",
            map.prepared.as_ref().unwrap().label_count(),
            hovering[30],
            hovering[57],
            hovering[59]
        );
    }
}
