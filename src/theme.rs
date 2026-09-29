//! Shared visual language for the shell, dialogs and disk map.
use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

pub const BG: Color32 = Color32::from_rgb(13, 18, 27);
pub const SURFACE: Color32 = Color32::from_rgb(20, 27, 39);
pub const BORDER: Color32 = Color32::from_rgb(42, 54, 72);
pub const TEXT: Color32 = Color32::from_rgb(230, 237, 247);
pub const MUTED: Color32 = Color32::from_rgb(143, 160, 182);
pub const ACCENT: Color32 = Color32::from_rgb(94, 230, 195);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = Color32::from_rgb(25, 34, 48);
    v.override_text_color = Some(TEXT);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = Color32::from_rgb(32, 91, 83);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(7);
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
    }
    v.widgets.inactive.weak_bg_fill = SURFACE;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(37, 52, 67);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(80, 125, 132));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(35, 83, 77);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 7.0);
    style.spacing.interact_size = vec2(32.0, 30.0);
    style.animation_time = 0.16;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(23.0));
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn frame() -> egui::Frame {
    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(10).inner_margin(14)
}
