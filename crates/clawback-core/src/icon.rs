//! Shared pixel-aligned treemap artwork for native application icons.
/// Rectangles on a 64-unit grid, from back to front.
pub const BOXES: [(u32, u32, u32, u32, [u8; 3]); 6] = [
    (4, 4, 56, 56, [24, 24, 24]),
    (8, 8, 48, 6, [68, 138, 255]),
    (8, 18, 27, 38, [124, 77, 255]),
    (39, 18, 17, 21, [0, 191, 165]),
    (39, 43, 7, 13, [224, 64, 251]),
    (50, 43, 6, 13, [255, 215, 64]),
];

/// Render at the requested physical size, snapping all edges to whole pixels.
pub fn pixels(size: u32) -> Vec<u8> {
    let mut rgba = vec![0; (size * size * 4) as usize];
    for (x, y, w, h, color) in BOXES {
        let edge = |v| (v * size + 32) / 64;
        for yy in edge(y)..edge(y + h) {
            for xx in edge(x)..edge(x + w) {
                let offset = ((yy * size + xx) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    rgba
}
