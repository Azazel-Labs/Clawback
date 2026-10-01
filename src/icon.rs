//! Runtime and About icons use the same artwork as native packages.
use eframe::egui::{self, IconData, Rect, pos2, vec2};

pub fn icon() -> IconData {
    IconData { rgba: clawback_core::icon::pixels(256), width: 256, height: 256 }
}

pub fn paint(painter: &egui::Painter, rect: Rect) {
    let scale = rect.width() / 64.0;
    for (x, y, w, h, c) in clawback_core::icon::BOXES {
        let r = Rect::from_min_size(
            pos2(rect.min.x + x as f32 * scale, rect.min.y + y as f32 * scale),
            vec2(w as f32 * scale, h as f32 * scale),
        );
        painter.rect_filled(r, 0.0, crate::theme::rgb(c));
    }
}
