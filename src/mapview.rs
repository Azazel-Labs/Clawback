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
use crate::app::special;
use crate::i18n::tr;

use crate::{
    background::{Job, retire},
    maprender::{self, LabelCache, PreparedMap},
    theme,
};
use clawback_core::layout::{self, DisplayBox, LayoutSettings};
use clawback_core::{NodeId, Settings, Tree, format};
use eframe::egui::{
    self, Color32, FontId, Id, LayerId, Order, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
};
use std::sync::Arc;
use std::time::Duration;

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

/// What the map asks the application to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    ZoomTo(NodeId),
    ZoomPath(std::path::PathBuf),
    Back,
    ZoomOut,
    ZoomFull,
    Reveal(NodeId),
    Open(NodeId),
    Delete(NodeId),
    Special(special::Kind, NodeId),
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
    /// Items queued for the Recycle Bin; `true` marks the one in progress.
    pub deleting: &'a [(NodeId, bool)],
    pub special: special::Inventory,
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
    special: special::Inventory,
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
    layout_rx: Option<Job<LayoutKey, LayoutResult>>,
    display_tree: Option<Arc<Tree>>,
    prepared: Option<PreparedMap>,
    boxes: Vec<DisplayBox>,
    key: Option<LayoutKey>,
    selected: Option<usize>,
    /// Time of the last mouse or keyboard input; tips wait for it to settle.
    last_input: f64,
    staging: Option<(LayoutKey, LayoutResult)>,
    label_cache: LabelCache,
    queued: Vec<Command>,
    special: special::Inventory,
    special_visuals: special::Visuals,
    focused: Option<bool>,
    rect: Rect,
    info: Option<InfoTip>,
    info_rx: Option<Job<(), InfoTip>>,
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
            special: special::Inventory::default(),
            special_visuals: special::Visuals::default(),
            focused: None,
            rect: Rect::ZERO,
            info: None,
            info_rx: None,
        }
    }
}

impl MapView {
    fn selected_box(&self) -> Option<&DisplayBox> {
        self.selected.and_then(|i| self.boxes.get(i))
    }

    pub fn selected_node(&self) -> Option<NodeId> {
        self.selected_box().and_then(DisplayBox::node)
    }

    pub fn selected_is_folder(&self) -> bool {
        self.selected_box().is_some_and(|b| b.folder)
    }

    /// Forget the layout (new document, or none).
    pub fn reset(&mut self) {
        self.layout_started = None;
        self.layout_rx = None;
        retire(self.prepared.take());
        retire(self.display_tree.take());
        retire(self.staging.take());
        self.boxes.clear();
        self.key = None;
        self.clear_transient();
    }

    /// Drop the selection and tips, which refer to the boxes being replaced.
    fn clear_transient(&mut self) {
        self.selected = None;
        self.info = None;
        self.info_rx = None;
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    pub fn zoom_in(&mut self) {
        if let Some(node) = self.selected_box().filter(|b| b.folder).and_then(DisplayBox::node) {
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
        maprender::screen_rect(b, self.display_origin())
    }

    fn display_origin(&self) -> Pos2 {
        self.key.map_or(self.rect.min, |key| pos2(f32::from_bits(key.origin[0]), f32::from_bits(key.origin[1])))
    }

    /// Screen position to the displayed layout's pixel grid.
    fn to_layout(&self, p: Pos2) -> (i32, i32) {
        let p = p - self.display_origin();
        (p.x.floor() as i32, p.y.floor() as i32)
    }

    fn hit(&self, pos: Option<Pos2>) -> Option<usize> {
        let (x, y) = self.to_layout(pos?);
        // A folder's interior belongs to its children; the collapsed Recycle Bin has none.
        layout::hit_test(&self.boxes, x, y).or_else(|| {
            self.special
                .collapsed()
                .find_map(|node| self.boxes.iter().position(|b| b.node() == Some(node) && b.contains(x, y)))
        })
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

        self.special = input.special;

        // Switching away from (or back to) the app clears the selection.
        let focused = ctx.input(|i| i.focused);
        if self.focused.is_some_and(|f| f != focused) {
            self.selected = None;
        }
        self.focused = Some(focused);

        let key = self.sync_layout(&ctx, &painter, rect, input);

        // Keep painting the previous complete snapshot until its replacement
        // is ready. Tree and boxes must always refer to the same node ids.
        let displayed = self.display_tree.clone();
        let input = &MapInput { tree: displayed.as_ref().unwrap_or(input.tree), ..*input };
        let interactive = !input.scanning && self.key == Some(key);

        self.note_activity(&ctx, now);

        let pointer = response.hover_pos();
        if interactive
            && let Some(node) = self.hit(pointer).and_then(|index| self.boxes[index].node())
            && let Some(kind) = input.special.classify(input.tree, node)
            && kind.action_enabled(input.settings.disable_delete)
        {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = self.to_layout(pos);
            // Use the displayed snapshot, whose IDs may differ from a new scan preview.
            if let Some(node) = zoom_target(&self.boxes, x, y) {
                out.push(Command::ZoomPath(input.tree.path(node)));
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

    /// Rebuild the layout off-thread when anything that affects it changes,
    /// and swap in a finished one once its labels are ready. Like SpaceMonger,
    /// a swap drops the selection. Returns the key the map should now show.
    fn sync_layout(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        input: &MapInput<'_>,
    ) -> LayoutKey {
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
            special: input.special,
        };
        if let Some((pending_key, result)) = Job::poll(&mut self.layout_rx) {
            if same_view(pending_key, key) {
                self.staging = Some((pending_key, result));
            } else {
                retire(result);
            }
        }
        if self.staging.as_ref().is_some_and(|(staged_key, _)| !same_view(*staged_key, key)) {
            retire(self.staging.take());
        }
        if let Some((_, (_, _, prepared))) = &mut self.staging {
            prepared.prepare(painter, &mut self.label_cache);
        }
        if self.staging.as_ref().is_some_and(|(_, (_, _, prepared))| prepared.is_ready()) {
            crate::perf::instant("map.ready");
            if let Some(started) = self.layout_started.take() {
                crate::perf::counter("map.request_to_ready_ms", started.elapsed().as_secs_f64() * 1000.0);
            }
            let (ready_key, (tree, boxes, prepared)) = self.staging.take().expect("ready map");
            retire(self.display_tree.replace(tree));
            retire(self.prepared.replace(prepared));
            self.boxes = boxes;
            self.key = Some(ready_key);
            self.clear_transient();
        }
        if self.key != Some(key) && self.layout_rx.is_none() && self.staging.is_none() {
            self.layout_started = crate::perf::enabled().then(std::time::Instant::now);
            crate::perf::instant("map.requested");
            let tree = Arc::clone(input.tree);
            let origin = rect.min;
            let free = [input.disk_free, input.disk_total];
            self.layout_rx = Some(Job::spawn(key, ctx, move || {
                let _span = crate::perf::span("worker.map_layout");
                let mut boxes = layout::build(&tree, key.view, key.w, key.h, &key.layout, key.free);
                key.special.collapse_layout(&mut boxes);
                crate::perf::counter("map.boxes", boxes.len() as f64);
                let prepared = PreparedMap::build(&tree, &boxes, origin, key.schemes, key.muted, free);
                (tree, boxes, prepared)
            }));
        }
        key
    }

    /// Any mouse or keyboard activity hides tips and restarts their timers.
    fn note_activity(&mut self, ctx: &egui::Context, now: f64) {
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
    }

    fn handle_mouse(
        &mut self,
        ctx: &egui::Context,
        response: &Response,
        pointer: Option<Pos2>,
        input: &MapInput<'_>,
        out: &mut Vec<Command>,
    ) {
        let hit = self.hit(pointer);
        let press = ctx.input(|i| i.pointer.primary_pressed() || i.pointer.secondary_pressed());
        if press && response.hovered() {
            self.selected = hit;
            ctx.request_repaint(); // The directory navigator is drawn before the map.
        }
        if response.clicked()
            && let Some(node) = hit.and_then(|i| self.boxes[i].node())
            && let Some(kind) = input.special.classify(input.tree, node)
            && kind.action_enabled(input.settings.disable_delete)
        {
            out.push(Command::Special(kind, node));
        }
        if response.secondary_clicked() {
            self.selected = self.hit(response.interact_pointer_pos());
        }
        response.context_menu(|ui| self.context_menu(ui, input, out));
    }

    /// The right-click menu (`CFolderView::OnRButtonUp`).
    fn context_menu(&mut self, ui: &mut Ui, input: &MapInput<'_>, out: &mut Vec<Command>) {
        let cur = self.selected_box().copied();
        let node = cur.and_then(|b| b.node());
        let folder = cur.is_some_and(|b| b.folder);
        let special = node.and_then(|n| input.special.classify(input.tree, n));
        // Ownership lookup is asynchronous; refresh the open menu when it completes.
        ui.ctx().request_repaint_after(Duration::from_millis(200));
        let s = input.settings;
        ui.set_min_width(184.0);
        ui.spacing_mut().button_padding = vec2(8.0, 3.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
        ui.spacing_mut().interact_size.y = 24.0;
        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));

        use egui_phosphor::regular as icon;
        if let Some(n) = node {
            if let Some(kind) = special {
                let (glyph, label) = kind.menu_entry();
                if item(ui, glyph, label, kind.action_enabled(s.disable_delete), true) {
                    out.push(Command::Special(kind, n));
                }
            }
            if item(ui, icon::FOLDER_OPEN, crate::platform::file_manager_label(), true, false) {
                out.push(Command::Reveal(n));
            }
            if item(ui, icon::INFO, tr!("properties-2"), true, false) {
                out.push(Command::Properties(n));
            }
            if special.is_none()
                && !s.disable_delete
                && !special::protects_from_raw_delete(&input.tree.path(n))
                && item(ui, icon::TRASH, tr!("delete"), true, false)
            {
                out.push(Command::Delete(n));
            }
            ui.separator();
        }
        if folder
            && input.special.allows_zoom(node.expect("folder has a node"))
            && item(ui, icon::MAGNIFYING_GLASS_PLUS, tr!("zoom-in"), true, false)
        {
            self.zoom_in();
        }
        if input.zoomed() {
            if item(ui, icon::MAGNIFYING_GLASS_MINUS, tr!("zoom-out"), true, false) {
                self.zoom_out();
            }
            if item(ui, icon::CORNERS_OUT, tr!("zoom-full"), true, false) {
                self.zoom_full();
            }
        }
        // Keep scan-wide controls available without crowding the item's actions.
        ui.menu_button(format!("{}  {}", icon::HARD_DRIVES, tr!("view")), |ui| {
            if item(ui, icon::ARROWS_CLOCKWISE, tr!("rescan"), true, false) {
                out.push(Command::Rescan);
            }
            if item(ui, icon::HARD_DRIVES, tr!("open-drive"), true, false) {
                out.push(Command::OpenDrive);
            }
            ui.separator();
            let mut free = s.show_free;
            if ui.checkbox(&mut free, tr!("show-free-space")).clicked() {
                out.push(Command::ToggleFree);
                ui.close();
            }
        });
    }

    fn paint(&mut self, painter: &egui::Painter, input: &MapInput<'_>, pointer: Option<Pos2>) {
        painter.rect_filled(self.rect, 8.0, theme::BG);
        if let Some(prepared) = &mut self.prepared {
            prepared.paint(painter);
        }
        let origin = self.display_origin();
        self.special_visuals.paint(painter, input.tree, input.special, &self.boxes, origin);

        for b in &self.boxes {
            if let layout::Item::Node(node) = b.item
                && let Some(&(_, active)) = input.deleting.iter().find(|(n, _)| *n == node)
            {
                paint_pending_delete(painter, self.box_rect(b).shrink(1.0), active);
            }
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
            if let Some(((), tip)) = Job::poll(&mut self.info_rx) {
                self.info = Some(tip);
            }
            if idle < info_delay {
                wake = wake.min(info_delay - idle);
            } else if self.info_rx.is_none() && self.info.as_ref().is_none_or(|t| t.node != node) {
                let tree = input.tree.clone();
                let settings = s.clone();
                let special = input.special.classify(&tree, node);
                self.info_rx = Some(Job::spawn((), ctx, move || {
                    let mut tip = info_tip(&tree, node, &settings);
                    if let Some(kind) = special
                        && let Some(hint) = kind.hint(settings.disable_delete)
                    {
                        tip.lines.push(hint);
                    }
                    tip
                }));
            } else if let (Some(tip), Some(p)) = (self.info.as_ref().filter(|tip| tip.node == node), pointer) {
                paint_info_tip(&tips, screen, tip, p, s.tip_icon);
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
        let text_color = if self.selected == Some(index) { theme::ACCENT } else { theme::TEXT };
        let galley = tips.layout_no_wrap(
            input.tree.node(node).name_lossy().into_owned(),
            maprender::label_font(tile.folder),
            text_color,
        );
        // Cover the map's own label exactly, or stand in where an unlabelled box would put it.
        let pos = match self.prepared.as_ref().and_then(|prepared| prepared.name_label(index)) {
            Some(label) if label.complete => return,
            Some(label) => label.pos,
            None => {
                let lines = if maprender::shows_details(tile) { 3 } else { 1 };
                maprender::line_pos(self.box_rect(tile), tile.folder, lines, 0, galley.size().x)
            }
        };
        let bounds = push_on_screen(Rect::from_min_size(pos, galley.size()).expand2(vec2(2.0, 1.0)), screen);
        tips.rect_filled(bounds, 0.0, theme::SURFACE);
        tips.rect_stroke(bounds, 0.0, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
        tips.galley(bounds.min + vec2(2.0, 1.0), galley, text_color);
    }
}

/// Compact, left-aligned menu rows; special actions carry the accent color.
fn item(ui: &mut Ui, icon: &str, label: impl Into<String>, enabled: bool, bold: bool) -> bool {
    let label = format!("{icon}  {}", label.into());
    let text = if bold { RichText::new(label).strong().color(theme::ACCENT) } else { RichText::new(label) };
    // A popup's available width can span the viewport; let its labels determine its width.
    let clicked = ui.add_enabled(enabled, egui::Button::new(text).min_size(vec2(0.0, 24.0))).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn info_tip(tree: &Tree, node: NodeId, s: &Settings) -> InfoTip {
    let n = tree.node(node);
    let path = tree.path(node);
    let mut lines = Vec::new();

    // The name always leads; the path option prefixes its folder.
    lines.push(if s.tip_path { path.display().to_string() } else { n.name_lossy().into_owned() });
    if s.tip_size || s.tip_attrib {
        let size = s.tip_size.then(|| format::size(n.display_len()));
        let attrs = s.tip_attrib.then(|| crate::platform::attributes(&path).join(" "));
        let parts: Vec<_> = [size, attrs].into_iter().flatten().filter(|part| !part.is_empty()).collect();
        lines.push(parts.join("  /  "));
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
    let r = push_on_screen(Rect::from_min_size(pointer + Vec2::splat(TIP_OFFSET), size), screen);
    tips.rect_filled(r, 8.0, theme::SURFACE);
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

/// Dim and hatch an item that is leaving for the Recycle Bin, so it reads as going away.
fn paint_pending_delete(painter: &egui::Painter, rect: Rect, active: bool) {
    painter.rect_filled(rect, 1.0, theme::BG.gamma_multiply(if active { 0.7 } else { 0.5 }));
    let hatch = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let stroke = Stroke::new(1.0, theme::DANGER.gamma_multiply(if active { 0.55 } else { 0.3 }));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        hatch.line_segment([pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())], stroke);
        x += 9.0;
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.3, theme::DANGER.gamma_multiply(0.85)), StrokeKind::Inside);
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

/// Slide `r` onto the screen, keeping its top-left visible if it cannot fit.
fn push_on_screen(r: Rect, screen: Rect) -> Rect {
    let r = r.translate((screen.max - r.max).min(Vec2::ZERO));
    r.translate((screen.min - r.min).max(Vec2::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{ROOT, tree::NewEntry};
    use std::{path::Path, time::Instant};

    #[cfg(windows)]
    #[test]
    fn temp_cleanup_cell_hides_children_and_hits_its_whole_interior() {
        let mut map = MapView {
            special: special::Inventory::fixture(special::Kind::TempFolder, 1),
            boxes: vec![
                DisplayBox { x: 0, y: 0, w: 200, h: 200, item: layout::Item::Node(1), depth: 0, folder: true },
                DisplayBox { x: 10, y: 30, w: 100, h: 100, item: layout::Item::Node(2), depth: 1, folder: false },
                DisplayBox { x: 220, y: 0, w: 100, h: 100, item: layout::Item::Node(3), depth: 0, folder: false },
            ],
            ..Default::default()
        };
        map.special.collapse_layout(&mut map.boxes);
        assert_eq!(map.boxes.len(), 2);
        assert_eq!(map.hit(Some(pos2(80.0, 80.0))), Some(0));
        assert_eq!(map.hit(Some(pos2(240.0, 40.0))), Some(1));
        assert_eq!(map.hit(Some(pos2(400.0, 40.0))), None);
    }

    #[test]
    fn pending_layout_keeps_rendering_matching_snapshot_without_waiting() {
        let mut tree = Tree::new(Path::new("/preview"));
        tree.add_children(ROOT, vec![NewEntry::file("visible.bin", 100)]);
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
            deleting: &[],
            special: special::Inventory::default(),
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
        let (_tx, job) = Job::manual(LayoutKey { generation: 1, ..old_key });
        map.layout_rx = Some(job);
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
            tree.add_children(ROOT, (0..count).map(|i| NewEntry::file(format!("{prefix}-{i}.txt"), 100)).collect());
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
                            deleting: &[],
                            special: special::Inventory::default(),
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
            let dir = tree.add_children(ROOT, vec![NewEntry::dir(format!("folder-{group}"))]);
            tree.add_children(
                dir.start,
                (0..files)
                    .map(|file| NewEntry {
                        mtime: 1_700_000_000,
                        ..NewEntry::file(
                            format!("document-{group}-{file}-with-a-descriptive-name.bin"),
                            128 + file * 32,
                        )
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
            deleting: &[],
            special: special::Inventory::default(),
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
