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
pub const DEFAULT_MAP_SCHEME: usize = 16;

/// The palettes offered to users, alphabetically. Other schemes are kept only so saved IDs stay
/// stable; settings that name one fall back to [`DEFAULT_MAP_SCHEME`].
pub const MAP_PRESETS: [(usize, &str); 11] = [
    (18, "Arcade"),
    (17, "Aurora"),
    (22, "Blackbody Radiation"),
    (12, "Candy"),
    (19, "Citrus"),
    (16, "Electric"),
    (21, "Gemstone"),
    (14, "Lagoon"),
    (0, "Material"),
    (20, "Orchid"),
    (13, "Sunset"),
];

pub fn is_preset(scheme: usize) -> bool {
    MAP_PRESETS.iter().any(|&(id, _)| id == scheme)
}

/// Apply a restrained saturation reduction while retaining palette identity.
/// Shared by the cached desktop mesh, label contrast, previews, and terminal.
pub fn display_color(scheme: usize, depth: i32, muted: bool) -> Rgb {
    let rgb = map_color(scheme, depth);
    if !muted {
        return rgb;
    }
    let gray = (u32::from(rgb[0]) * 54 + u32::from(rgb[1]) * 183 + u32::from(rgb[2]) * 19 + 128) / 256;
    rgb.map(|channel| ((u32::from(channel) * 2 + gray * 3 + 2) / 5) as u8)
}
fn from_hex(colors: [u32; 8]) -> [Rgb; 8] {
    colors.map(|rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}

pub fn map_color(scheme: usize, depth: i32) -> Rgb {
    let colors = match scheme {
        16 => from_hex([
            0x0044_8AFF,
            0x0000_BFA5,
            0x007C_4DFF,
            0x00E0_40FB,
            0x00FF_4081,
            0x00FF_6E40,
            0x00FF_D740,
            0x0076_FF03,
        ]),
        17 => from_hex([
            0x003B_82F6,
            0x0022_D3EE,
            0x002D_D4BF,
            0x00A3_E635,
            0x0081_8CF8,
            0x00C0_84FC,
            0x00E8_79F9,
            0x0038_BDF8,
        ]),
        18 => from_hex([
            0x0000_D9FF,
            0x00FF_3DAE,
            0x009D_5CFF,
            0x00FF_E14A,
            0x0000_E6A8,
            0x00FF_784F,
            0x005B_8CFF,
            0x00D4_FF42,
        ]),
        19 => from_hex([
            0x00FF_B300,
            0x00FF_7043,
            0x00F0_6292,
            0x00AB_47BC,
            0x0029_B6F6,
            0x0026_C6DA,
            0x0066_BB6A,
            0x00D4_E157,
        ]),
        20 => from_hex([
            0x008B_5CF6,
            0x00C0_84FC,
            0x00F4_72B6,
            0x00FB_7185,
            0x00FD_BA74,
            0x00FD_E68A,
            0x0067_E8F9,
            0x0060_A5FA,
        ]),
        21 => from_hex([
            0x0025_63EB,
            0x000D_9488,
            0x007C_3AED,
            0x00BE_185D,
            0x00DC_2626,
            0x00EA_580C,
            0x00CA_8A04,
            0x0016_A34A,
        ]),
        // Blackbody Radiation: rising colour temperature, from a dull ~800 K glow through
        // 1000, 1500, 2000, 3000 and 4500 K to daylight (~6500 K) and ~10000 K blue-white.
        22 => from_hex([
            0x008B_1A00,
            0x00D6_3A00,
            0x00FF_6A00,
            0x00FF_9A2E,
            0x00FF_C27A,
            0x00FF_E4C4,
            0x00F4_F1FF,
            0x00B9_CCFF,
        ]),
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
pub const SCHEME_NAMES: [&str; 23] = [
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
    "Electric",
    "Aurora",
    "Arcade",
    "Citrus",
    "Orchid",
    "Gemstone",
    "Blackbody Radiation",
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
    fn muting_reduces_saturation_without_changing_channel_order() {
        let spread = |rgb: Rgb| rgb.iter().max().expect("RGB") - rgb.iter().min().expect("RGB");
        for (scheme, _) in MAP_PRESETS {
            for depth in 0..8 {
                let original = map_color(scheme, depth);
                assert_eq!(display_color(scheme, depth, false), original);
                let muted = display_color(scheme, depth, true);
                assert!(spread(muted) < spread(original));
                for a in 0..3 {
                    for b in 0..3 {
                        if original[a] > original[b] {
                            assert!(muted[a] >= muted[b]);
                        }
                    }
                }
            }
        }
        assert_eq!(display_color(2, 0, true), map_color(2, 0)); // white stays neutral
    }
    #[test]
    fn depth_wraps_every_eight() {
        for (scheme, name) in MAP_PRESETS {
            assert_eq!(SCHEME_NAMES[scheme], name);
            assert_eq!(map_color(scheme, 0), map_color(scheme, 8));
            assert_eq!(map_color(scheme, -1), map_color(scheme, 7));
        }
        assert_eq!(shades(0, 0), shades(0, 8));
        assert_eq!(shades(0, 3).color, [0x7F, 0xFF, 0x7F]);
        assert_eq!(shades(11, 0).color, [0xFF, 0x7F, 0xFF]);
        assert_eq!(shades(1, 5), BoxShades { color: FACE, bright: HILIGHT, dark: SHADOW });
    }
}
