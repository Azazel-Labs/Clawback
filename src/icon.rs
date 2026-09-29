//! The application icon, drawn procedurally: a tiny SpaceMonger map.

use clawback_core::palette::Rgb;
use eframe::egui::{self, IconData, Rect, pos2, vec2};

const N: u32 = 64;

/// (x, y, w, h, depth) on a 64x64 grid.
const BOXES: [(u32, u32, u32, u32, i32); 7] = [
    (1, 1, 62, 62, 0), // drive root folder with its title band
    (4, 12, 32, 48, 1),
    (7, 22, 26, 22, 2),
    (7, 43, 13, 15, 3),
    (19, 43, 14, 15, 4),
    (35, 12, 25, 29, 1),
    (35, 40, 25, 20, 5),
];

/// Draw one SpaceMonger box through `fill(x, y, w, h, colour)`.
fn sm_box(fill: &mut impl FnMut(u32, u32, u32, u32, Rgb), (x, y, w, h, depth): (u32, u32, u32, u32, i32)) {
    const COLORS: [Rgb; 6] = [[20, 27, 39], [42, 94, 112], [63, 75, 130], [94, 230, 195], [96, 64, 118], [58, 104, 89]];
    fill(x, y, w, h, [13, 18, 27]);
    fill(x + 1, y + 1, w - 2, h - 2, COLORS[depth.rem_euclid(6) as usize]);
}

/// 64x64 RGBA pixels.
pub fn pixels() -> Vec<u8> {
    let mut px = vec![0u8; (N * N * 4) as usize];
    let mut fill = |x: u32, y: u32, w: u32, h: u32, c: Rgb| {
        for yy in y..(y + h).min(N) {
            for xx in x..(x + w).min(N) {
                let i = ((yy * N + xx) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    };
    for b in BOXES {
        sm_box(&mut fill, b);
    }
    px
}

pub fn icon() -> IconData {
    IconData { rgba: pixels(), width: N, height: N }
}

/// Paint the icon into `rect` (used by the About box).
pub fn paint(painter: &egui::Painter, rect: Rect) {
    let scale = rect.width() / N as f32;
    let mut fill = |x: u32, y: u32, w: u32, h: u32, c: Rgb| {
        let r = Rect::from_min_size(
            pos2(rect.min.x + x as f32 * scale, rect.min.y + y as f32 * scale),
            vec2(w as f32 * scale, h as f32 * scale),
        );
        painter.rect_filled(r, 0.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
    };
    for b in BOXES {
        sm_box(&mut fill, b);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_is_opaque_where_drawn() {
        let px = super::pixels();
        assert_eq!(px.len(), 64 * 64 * 4);
        assert_eq!(px[(10 * 64 + 10) * 4 + 3], 255);
        assert_eq!(px[3], 0); // corner pixel stays transparent
    }
}
