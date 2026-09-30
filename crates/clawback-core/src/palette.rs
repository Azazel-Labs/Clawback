//! SpaceMonger's colour tables and colour schemes.

pub type Rgb = [u8; 3];

/// Depth colours: 8 base colours, then 8 bright, then 8 dark
/// (`BoxColors` in the original source).
pub const BOX_COLORS: [Rgb; 24] = [
    [0xFF, 0x7F, 0x7F],
    [0xFF, 0xBF, 0x7F],
    [0xFF, 0xFF, 0x00],
    [0x7F, 0xFF, 0x7F],
    [0x7F, 0xFF, 0xFF],
    [0xBF, 0xBF, 0xFF],
    [0xBF, 0xBF, 0xBF],
    [0xFF, 0x7F, 0xFF],
    //
    [0xFF, 0xBF, 0xBF],
    [0xFF, 0xDF, 0xBF],
    [0xFF, 0xFF, 0xBF],
    [0xBF, 0xFF, 0xBF],
    [0xDF, 0xFF, 0xFF],
    [0xDF, 0xDF, 0xFF],
    [0xDF, 0xDF, 0xDF],
    [0xFF, 0xBF, 0xFF],
    //
    [0xBF, 0x7F, 0x7F],
    [0xBF, 0x9F, 0x5F],
    [0xBF, 0xBF, 0x3F],
    [0x7F, 0xBF, 0x7F],
    [0x7F, 0xBF, 0xBF],
    [0x9F, 0x9F, 0xFF],
    [0x9F, 0x9F, 0x9F],
    [0xBF, 0x7F, 0xBF],
];

/// Desktop map presets. Existing scheme IDs remain stable in saved settings.
pub const MAP_PRESETS: [(usize, &str); 5] =
    [(0, "Material"), (12, "Candy"), (13, "Sunset"), (14, "Lagoon"), (15, "Muted")];

pub fn map_color(scheme: usize, depth: i32) -> Rgb {
    let colors = match scheme {
        // Google Material Design 400 swatches: blue, teal, indigo, purple,
        // pink, orange, amber, green. https://m1.material.io/style/color.html
        0 => [
            [0x42, 0xA5, 0xF5],
            [0x26, 0xA6, 0x9A],
            [0x5C, 0x6B, 0xC0],
            [0xAB, 0x47, 0xBC],
            [0xEC, 0x40, 0x7A],
            [0xFF, 0xA7, 0x26],
            [0xFF, 0xCA, 0x28],
            [0x66, 0xBB, 0x6A],
        ],
        12 => [
            [245, 117, 170],
            [169, 134, 245],
            [106, 165, 249],
            [78, 211, 204],
            [168, 220, 116],
            [252, 202, 108],
            [250, 150, 115],
            [214, 123, 219],
        ],
        13 => [
            [104, 72, 183],
            [151, 67, 178],
            [202, 65, 141],
            [235, 83, 103],
            [246, 125, 70],
            [247, 170, 69],
            [229, 199, 104],
            [184, 92, 134],
        ],
        14 => [
            [29, 122, 188],
            [27, 163, 208],
            [26, 188, 187],
            [54, 201, 148],
            [147, 208, 101],
            [54, 154, 173],
            [70, 117, 200],
            [110, 97, 211],
        ],
        15 => [
            [43, 63, 71],
            [51, 57, 78],
            [67, 51, 74],
            [77, 54, 62],
            [76, 65, 48],
            [46, 68, 59],
            [45, 64, 77],
            [57, 57, 76],
        ],
        _ => return shades(scheme, depth).color.map(|c| (u16::from(c) * 2 / 5 + 16) as u8),
    };
    colors[depth.rem_euclid(8) as usize]
}

/// Choose black or white text using relative luminance.
pub fn dark_ink(rgb: Rgb) -> bool {
    let linear = rgb.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722 > 0.179
}

/// Fixed colours: 10 base, 10 bright, 10 dark (`FixedColors`).
const FIXED_COLORS: [Rgb; 30] = [
    [0xFF, 0xFF, 0xFF],
    [0xBF, 0xBF, 0xBF],
    [0x7F, 0x7F, 0x7F],
    [0xFF, 0x7F, 0x7F],
    [0xFF, 0xBF, 0x7F],
    [0xFF, 0xFF, 0x00],
    [0x7F, 0xFF, 0x7F],
    [0x7F, 0xFF, 0xFF],
    [0xBF, 0xBF, 0xFF],
    [0xFF, 0x7F, 0xFF],
    //
    [0xFF, 0xFF, 0xFF],
    [0xFF, 0xFF, 0xFF],
    [0xBF, 0xBF, 0xBF],
    [0xFF, 0x9F, 0x9F],
    [0xFF, 0xDF, 0xBF],
    [0xFF, 0xFF, 0xBF],
    [0xBF, 0xFF, 0xBF],
    [0xDF, 0xFF, 0xFF],
    [0xDF, 0xDF, 0xFF],
    [0xFF, 0xBF, 0xFF],
    //
    [0xBF, 0xBF, 0xBF],
    [0x7F, 0x7F, 0x7F],
    [0x3F, 0x3F, 0x3F],
    [0xBF, 0x7F, 0x7F],
    [0xBF, 0x9F, 0x9F],
    [0xBF, 0xBF, 0x3F],
    [0x7F, 0xBF, 0x7F],
    [0x7F, 0xBF, 0xBF],
    [0x9F, 0x9F, 0xFF],
    [0xBF, 0x7F, 0xBF],
];

/// Classic Windows 3D face / highlight / shadow ("Windows Colors").
pub const FACE: Rgb = [0xC0, 0xC0, 0xC0];
pub const HILIGHT: Rgb = [0xFF, 0xFF, 0xFF];
pub const SHADOW: Rgb = [0x80, 0x80, 0x80];
pub const BLACK: Rgb = [0, 0, 0];
pub const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

/// Colour scheme names, preserving legacy IDs. Index 0 is "Material"
/// (colour by depth), 1 is the system 3D colours, 2.. are fixed colours.
pub const SCHEME_NAMES: [&str; 16] = [
    "Material",
    "Windows Colors",
    "White",
    "Light Gray",
    "Dark Gray",
    "Red",
    "Orange",
    "Yellow",
    "Green",
    "Aqua",
    "Blue",
    "Violet",
    "Candy",
    "Sunset",
    "Lagoon",
    "Muted",
];

/// Base, bright (top-left bevel) and dark (bottom-right bevel) colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxShades {
    pub color: Rgb,
    pub bright: Rgb,
    pub dark: Rgb,
}

/// Shades for a box at `depth` in colour scheme `scheme`.
pub fn shades(scheme: usize, depth: i32) -> BoxShades {
    let d = (depth & 7) as usize;
    match scheme {
        0 => BoxShades { color: BOX_COLORS[d], bright: BOX_COLORS[d + 8], dark: BOX_COLORS[d + 16] },
        1 => BoxShades { color: FACE, bright: HILIGHT, dark: SHADOW },
        n => {
            let i = (n - 2).min(9);
            BoxShades { color: FIXED_COLORS[i], bright: FIXED_COLORS[i + 10], dark: FIXED_COLORS[i + 20] }
        }
    }
}

/// Apply the "rollover box" look: boxes are drawn dark and flat, and the ones
/// under the cursor light up.
pub fn rollover(s: BoxShades, lit: bool) -> BoxShades {
    if lit {
        BoxShades { color: s.bright, bright: WHITE, dark: s.color }
    } else {
        BoxShades { color: s.dark, bright: s.color, dark: s.dark }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_wraps_every_eight() {
        assert_eq!(shades(0, 0), shades(0, 8));
        assert_eq!(shades(0, 3).color, [0x7F, 0xFF, 0x7F]);
        assert_eq!(shades(11, 0).color, [0xFF, 0x7F, 0xFF]);
        assert_eq!(shades(1, 5), BoxShades { color: FACE, bright: HILIGHT, dark: SHADOW });
    }
}
