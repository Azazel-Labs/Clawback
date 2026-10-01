//! Treemap colour palettes.

pub type Rgb = [u8; 3];

pub const BLACK: Rgb = [0, 0, 0];
pub const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

/// Desktop map presets. Existing scheme IDs remain stable in saved settings.
pub const DEFAULT_MAP_SCHEME: usize = 16;

struct Palette {
    /// Saved scheme ID; retired IDs are not reused.
    id: usize,
    name: &'static str,
    /// Colours by depth, repeating every eight levels.
    colors: [Rgb; 8],
}

const fn hex(colors: [u32; 8]) -> [Rgb; 8] {
    let mut out = [[0; 3]; 8];
    let mut i = 0;
    while i < colors.len() {
        let c = colors[i];
        out[i] = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        i += 1;
    }
    out
}

/// The palettes offered to users, alphabetically.
const PALETTES: [Palette; 11] = [
    Palette {
        id: 18,
        name: "Arcade",
        colors: hex([
            0x0000_D9FF,
            0x00FF_3DAE,
            0x009D_5CFF,
            0x00FF_E14A,
            0x0000_E6A8,
            0x00FF_784F,
            0x005B_8CFF,
            0x00D4_FF42,
        ]),
    },
    Palette {
        id: 17,
        name: "Aurora",
        colors: hex([
            0x003B_82F6,
            0x0022_D3EE,
            0x002D_D4BF,
            0x00A3_E635,
            0x0081_8CF8,
            0x00C0_84FC,
            0x00E8_79F9,
            0x0038_BDF8,
        ]),
    },
    // Rising colour temperature, from a dull ~800 K glow through 1000, 1500,
    // 2000, 3000 and 4500 K to daylight (~6500 K) and ~10000 K blue-white.
    Palette {
        id: 22,
        name: "Blackbody Radiation",
        colors: hex([
            0x008B_1A00,
            0x00D6_3A00,
            0x00FF_6A00,
            0x00FF_9A2E,
            0x00FF_C27A,
            0x00FF_E4C4,
            0x00F4_F1FF,
            0x00B9_CCFF,
        ]),
    },
    Palette {
        id: 12,
        name: "Candy",
        colors: [
            [245, 117, 170],
            [169, 134, 245],
            [106, 165, 249],
            [78, 211, 204],
            [168, 220, 116],
            [252, 202, 108],
            [250, 150, 115],
            [214, 123, 219],
        ],
    },
    Palette {
        id: 19,
        name: "Citrus",
        colors: hex([
            0x00FF_B300,
            0x00FF_7043,
            0x00F0_6292,
            0x00AB_47BC,
            0x0029_B6F6,
            0x0026_C6DA,
            0x0066_BB6A,
            0x00D4_E157,
        ]),
    },
    Palette {
        id: 16,
        name: "Electric",
        colors: hex([
            0x0044_8AFF,
            0x0000_BFA5,
            0x007C_4DFF,
            0x00E0_40FB,
            0x00FF_4081,
            0x00FF_6E40,
            0x00FF_D740,
            0x0076_FF03,
        ]),
    },
    Palette {
        id: 21,
        name: "Gemstone",
        colors: hex([
            0x0025_63EB,
            0x000D_9488,
            0x007C_3AED,
            0x00BE_185D,
            0x00DC_2626,
            0x00EA_580C,
            0x00CA_8A04,
            0x0016_A34A,
        ]),
    },
    Palette {
        id: 14,
        name: "Lagoon",
        colors: [
            [29, 122, 188],
            [27, 163, 208],
            [26, 188, 187],
            [54, 201, 148],
            [147, 208, 101],
            [54, 154, 173],
            [70, 117, 200],
            [110, 97, 211],
        ],
    },
    // Google Material Design 400 swatches: blue, teal, indigo, purple,
    // pink, orange, amber, green. https://m1.material.io/style/color.html
    Palette {
        id: 0,
        name: "Material",
        colors: [
            [0x42, 0xA5, 0xF5],
            [0x26, 0xA6, 0x9A],
            [0x5C, 0x6B, 0xC0],
            [0xAB, 0x47, 0xBC],
            [0xEC, 0x40, 0x7A],
            [0xFF, 0xA7, 0x26],
            [0xFF, 0xCA, 0x28],
            [0x66, 0xBB, 0x6A],
        ],
    },
    Palette {
        id: 20,
        name: "Orchid",
        colors: hex([
            0x008B_5CF6,
            0x00C0_84FC,
            0x00F4_72B6,
            0x00FB_7185,
            0x00FD_BA74,
            0x00FD_E68A,
            0x0067_E8F9,
            0x0060_A5FA,
        ]),
    },
    Palette {
        id: 13,
        name: "Sunset",
        colors: [
            [104, 72, 183],
            [151, 67, 178],
            [202, 65, 141],
            [235, 83, 103],
            [246, 125, 70],
            [247, 170, 69],
            [229, 199, 104],
            [184, 92, 134],
        ],
    },
];

/// The palettes offered to users, alphabetically, as `(scheme ID, name)`.
/// Settings that name any other scheme fall back to [`DEFAULT_MAP_SCHEME`].
pub const MAP_PRESETS: [(usize, &str); PALETTES.len()] = {
    let mut out = [(0, ""); PALETTES.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = (PALETTES[i].id, PALETTES[i].name);
        i += 1;
    }
    out
};

pub fn is_preset(scheme: usize) -> bool {
    PALETTES.iter().any(|p| p.id == scheme)
}

fn palette(scheme: usize) -> &'static Palette {
    PALETTES.iter().find(|p| p.id == scheme).unwrap_or_else(|| palette(DEFAULT_MAP_SCHEME))
}

pub fn map_color(scheme: usize, depth: i32) -> Rgb {
    palette(scheme).colors[depth.rem_euclid(8) as usize]
}

/// The map colour, optionally [muted](mute). Shared by the cached desktop
/// mesh, label contrast, previews, and terminal.
pub fn display_color(scheme: usize, depth: i32, muted: bool) -> Rgb {
    let rgb = map_color(scheme, depth);
    if muted { mute(rgb) } else { rgb }
}

/// Perceptual grey level, 0..=255.
pub fn luma(rgb: Rgb) -> u8 {
    ((u32::from(rgb[0]) * 54 + u32::from(rgb[1]) * 183 + u32::from(rgb[2]) * 19 + 128) / 256) as u8
}

/// Apply a restrained saturation reduction while retaining palette identity.
pub fn mute(rgb: Rgb) -> Rgb {
    let gray = u32::from(luma(rgb));
    rgb.map(|channel| ((u32::from(channel) * 2 + gray * 3 + 2) / 5) as u8)
}

/// Choose black or white text using relative luminance.
pub fn dark_ink(rgb: Rgb) -> bool {
    let linear = rgb.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722 > 0.179
}

/// Legible text colour on `rgb`: [`BLACK`] or [`WHITE`].
pub fn ink(rgb: Rgb) -> Rgb {
    if dark_ink(rgb) { BLACK } else { WHITE }
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
        // Neutral colours stay put.
        assert_eq!(mute(WHITE), WHITE);
        assert_eq!(mute(BLACK), BLACK);
    }

    #[test]
    fn depth_wraps_every_eight_and_unknown_schemes_use_the_default() {
        for (scheme, _) in MAP_PRESETS {
            assert!(is_preset(scheme));
            assert_eq!(map_color(scheme, 0), map_color(scheme, 8));
            assert_eq!(map_color(scheme, -1), map_color(scheme, 7));
        }
        assert!(is_preset(DEFAULT_MAP_SCHEME));
        assert!(!is_preset(15));
        assert_eq!(map_color(15, 3), map_color(DEFAULT_MAP_SCHEME, 3));
        assert_eq!(map_color(16, 0), [0x44, 0x8A, 0xFF]);
    }

    #[test]
    fn ink_contrasts_with_its_background() {
        assert_eq!(ink(WHITE), BLACK);
        assert_eq!(ink(BLACK), WHITE);
        assert_eq!(ink([0xFF, 0xD7, 0x40]), BLACK);
    }
}
