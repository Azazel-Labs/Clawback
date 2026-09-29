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

/// Colour scheme names, in SpaceMonger's order. Index 0 is "Rainbow"
/// (colour by depth), 1 is the system 3D colours, 2.. are fixed colours.
pub const SCHEME_NAMES: [&str; 12] = [
    "Rainbow",
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
