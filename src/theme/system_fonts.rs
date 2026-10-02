//! CJK faces installed with the operating system, for builds without the
//! bundled Noto fonts. Each region takes the first candidate that is a valid
//! font; a region with none falls back to egui's own fonts, which show boxes.
#![cfg_attr(bundled_cjk_fonts, allow(dead_code))]
use super::Region;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A face read once and kept for the life of the process, so switching
/// languages does not read the files again.
#[derive(Clone, Copy, Debug)]
pub(super) struct Face {
    pub bytes: &'static [u8],
    /// The face within a font collection (`.ttc`).
    pub index: u32,
}

pub(super) fn cjk(region: Region) -> Option<Face> {
    static FACES: OnceLock<[Option<Face>; 4]> = OnceLock::new();
    FACES.get_or_init(|| {
        let _span = crate::perf::span("startup.system_fonts");
        // Regions often share a collection file; read each file once.
        let mut files: Vec<(PathBuf, &'static [u8])> = Vec::new();
        Region::ALL.map(|region| {
            candidates(region).into_iter().find_map(|(path, index)| {
                let bytes = if let Some(&(_, bytes)) = files.iter().find(|(read, _)| *read == path) {
                    bytes
                } else {
                    let bytes = std::fs::read(&path).ok().filter(|bytes| is_font(bytes))?;
                    let bytes: &'static [u8] = bytes.leak();
                    files.push((path, bytes));
                    bytes
                };
                let index = regional_index(bytes, index, region)?;
                Some(Face { bytes, index })
            })
        })
    })[region as usize]
}

fn is_font(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some(b"ttcf" | [0, 1, 0, 0] | b"OTTO" | b"true"))
}

/// Whether `bytes` holds a font (`.ttf`, `.otf` or `.ttc`) with face `index`.
fn has_face(bytes: &[u8], index: u32) -> bool {
    ttf_parser::Face::parse(bytes, index).is_ok()
}

fn regional_index(bytes: &[u8], index: u32, region: Region) -> Option<u32> {
    // PingFang contains multiple weights and both Chinese regional families.
    // Collection order is not a stable way to select SC versus TC.
    #[cfg(target_os = "macos")]
    if matches!(region, Region::Sc | Region::Tc) {
        let family = if region == Region::Sc { "PingFang SC" } else { "PingFang TC" };
        for face_index in 0..ttf_parser::fonts_in_collection(bytes).unwrap_or(1) {
            let Ok(face) = ttf_parser::Face::parse(bytes, face_index) else { continue };
            if face.names().into_iter().any(|name| {
                matches!(name.name_id, ttf_parser::name_id::FAMILY | ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                    && name.to_string().is_some_and(|name| name == family)
            }) {
                return Some(face_index);
            }
        }
    }
    let _ = region;
    has_face(bytes, index).then_some(index)
}

/// Fonts shipped with Windows, best first.
#[cfg(windows)]
fn candidates(region: Region) -> Vec<(PathBuf, u32)> {
    let fonts = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts");
    let names: &[&str] = match region {
        Region::Sc => &["msyh.ttc", "Deng.ttf", "simsun.ttc"],
        Region::Tc => &["msjh.ttc", "mingliu.ttc"],
        Region::Jp => &["YuGothR.ttc", "meiryo.ttc", "msgothic.ttc"],
        Region::Kr => &["malgun.ttf", "gulim.ttc"],
    };
    names.iter().map(|name| (fonts.join(name), 0)).collect()
}

/// Fonts shipped with macOS, best first. Hiragino Sans GB covers Traditional
/// characters too, so it is the fallback for both Chinese regions.
#[cfg(target_os = "macos")]
fn candidates(region: Region) -> Vec<(PathBuf, u32)> {
    let names: &[&str] = match region {
        Region::Sc | Region::Tc => &["PingFang.ttc", "Hiragino Sans GB.ttc", "STHeiti Light.ttc"],
        Region::Jp => &["ヒラギノ角ゴシック W3.ttc", "Hiragino Sans GB.ttc"],
        Region::Kr => &["AppleSDGothicNeo.ttc"],
    };
    ["/System/Library/Fonts", "/Library/Fonts"]
        .iter()
        .flat_map(|dir| names.iter().map(move |name| (PathBuf::from(dir).join(name), 0)))
        .collect()
}

/// Ask fontconfig for the system's choice for the region's language, then try
/// where distributions install Noto Sans CJK, whose collection holds the JP,
/// KR, SC and TC faces in that order.
#[cfg(not(any(windows, target_os = "macos")))]
fn candidates(region: Region) -> Vec<(PathBuf, u32)> {
    let (language, index) = match region {
        Region::Jp => ("ja", 0),
        Region::Kr => ("ko", 1),
        Region::Sc => ("zh-cn", 2),
        Region::Tc => ("zh-tw", 3),
    };
    let mut found: Vec<(PathBuf, u32)> = fontconfig(language).into_iter().collect();
    for dir in ["opentype/noto", "noto-cjk", "google-noto-cjk", "opentype/noto-cjk"] {
        found.push((PathBuf::from("/usr/share/fonts").join(dir).join("NotoSansCJK-Regular.ttc"), index));
    }
    found
}

/// fontconfig's best sans-serif face for `language`, if it actually supports it
/// (with nothing suitable installed, fontconfig still answers with some font).
#[cfg(not(any(windows, target_os = "macos")))]
fn fontconfig(language: &str) -> Option<(PathBuf, u32)> {
    let output = std::process::Command::new("fc-match")
        .args(["--format", "%{file}\n%{index}\n%{lang}", &format!("sans-serif:lang={language}")])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let (file, index, languages) = (lines.next()?, lines.next()?, lines.next()?);
    let supported = languages.split('|').any(|supported| supported == language);
    supported.then_some((PathBuf::from(file), index.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_faces_are_accepted() {
        let mut collection = b"ttcf\0\x02\0\0\0\0\0\x02".to_vec();
        collection.extend([0; 8]);
        assert!(!has_face(&collection, 0), "a collection header alone is not a usable font");
        assert!(!has_face(&collection, 2), "the collection holds two faces");
        assert!(!has_face(b"OTTO....", 0) && !has_face(&[0, 1, 0, 0, 9], 0));
        assert!(!has_face(b"OTTO....", 1), "a single font has only face 0");
        assert!(!has_face(b"<html>", 0) && !has_face(b"", 0));
    }

    #[test]
    fn system_faces_that_are_found_are_valid() {
        for region in Region::ALL {
            if let Some(face) = cjk(region) {
                assert!(has_face(face.bytes, face.index), "{region:?}");
            }
        }
    }
}
