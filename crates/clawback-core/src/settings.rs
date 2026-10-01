//! User settings, mirroring SpaceMonger's Setup dialog, stored as a small
//! `key = value` file in the platform's configuration directory.

use crate::layout::LayoutSettings;
use crate::scan::ScanOptions;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Allowed `directory_split` range, in thousandths.
pub const DIRECTORY_SPLIT: (u32, u32) = (100, 650);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// GUI catalog code, or "auto" for OS preferences. Unknown codes use English.
    pub language: String,
    // File layout
    pub density: i32,
    pub bias: i32,
    // Display colours (indices into `palette::SCHEME_NAMES`)
    pub file_color: usize,
    pub folder_color: usize,
    pub mute_palette: bool,
    // Tool tips
    pub show_name_tips: bool,
    pub nametip_delay_ms: u32,
    pub show_info_tips: bool,
    pub infotip_delay_ms: u32,
    pub tip_path: bool,
    pub tip_icon: bool,
    pub tip_date: bool,
    pub tip_size: bool,
    pub tip_attrib: bool,
    // Miscellaneous
    pub auto_rescan: bool,
    pub disable_delete: bool,
    pub animated_zoom: bool,
    pub save_pos: bool,
    /// Height of the folder and file-type panels, in thousandths of the window below the menu bar.
    pub directory_split: u32,
    pub rollover_box: bool,
    pub show_free: bool,
    // Scanning (not in SpaceMonger; needed off Windows)
    pub one_filesystem: bool,
    pub apparent_size: bool,
    pub dedupe_hardlinks: bool,
    // Recently opened roots, most recent first
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    /// Clawback defaults, based on SpaceMonger with the Electric map palette.
    fn default() -> Self {
        Settings {
            language: "auto".into(),
            density: 0,
            bias: 0,
            file_color: crate::palette::DEFAULT_MAP_SCHEME,
            folder_color: crate::palette::DEFAULT_MAP_SCHEME,
            mute_palette: false,
            show_name_tips: true,
            nametip_delay_ms: 125,
            show_info_tips: true,
            infotip_delay_ms: 250,
            tip_path: false,
            tip_icon: true,
            tip_date: true,
            tip_size: true,
            tip_attrib: false,
            auto_rescan: false,
            disable_delete: false,
            animated_zoom: true,
            save_pos: false,
            directory_split: 300,
            rollover_box: false,
            show_free: true,
            one_filesystem: true,
            apparent_size: false,
            dedupe_hardlinks: true,
            recent: Vec::new(),
        }
    }
}

const MAX_RECENT: usize = 8;

impl Settings {
    pub fn layout(&self) -> LayoutSettings {
        LayoutSettings { density: self.density, bias: self.bias }
    }

    pub fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            one_filesystem: self.one_filesystem,
            apparent_size: self.apparent_size,
            dedupe_hardlinks: self.dedupe_hardlinks,
            ..ScanOptions::default()
        }
    }

    pub fn push_recent(&mut self, p: &Path) {
        self.recent.retain(|r| r != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// Clamp everything into the ranges the UI offers (as `Load()` did).
    pub fn sanitize(&mut self) {
        if self.language.is_empty() || !self.language.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            self.language = "auto".into();
        }
        self.density = self.density.clamp(-3, 3);
        self.bias = self.bias.clamp(-20, 20);
        // ID 15 was a standalone Muted preset. Retired single-hue and grey schemes use the default.
        for scheme in [&mut self.file_color, &mut self.folder_color] {
            if *scheme == 15 {
                *scheme = crate::palette::DEFAULT_MAP_SCHEME;
                self.mute_palette = true;
            } else if !crate::palette::is_preset(*scheme) {
                *scheme = crate::palette::DEFAULT_MAP_SCHEME;
            }
        }
        self.nametip_delay_ms = self.nametip_delay_ms.min(99_999);
        self.infotip_delay_ms = self.infotip_delay_ms.min(99_999);
        self.directory_split = self.directory_split.clamp(DIRECTORY_SPLIT.0, DIRECTORY_SPLIT.1);
        self.recent.truncate(MAX_RECENT);
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("# Clawback settings\n");
        let mut kv = |k: &str, v: &dyn std::fmt::Display| {
            let _ = writeln!(s, "{k} = {v}");
        };
        kv("density", &self.density);
        kv("language", &self.language);
        kv("bias", &self.bias);
        kv("file_color", &self.file_color);
        kv("folder_color", &self.folder_color);
        kv("mute_palette", &self.mute_palette);
        kv("show_name_tips", &self.show_name_tips);
        kv("nametip_delay", &self.nametip_delay_ms);
        kv("show_info_tips", &self.show_info_tips);
        kv("infotip_delay", &self.infotip_delay_ms);
        kv("tip_path", &self.tip_path);
        kv("tip_icon", &self.tip_icon);
        kv("tip_date", &self.tip_date);
        kv("tip_size", &self.tip_size);
        kv("tip_attrib", &self.tip_attrib);
        kv("auto_rescan", &self.auto_rescan);
        kv("disable_delete", &self.disable_delete);
        kv("animated_zoom", &self.animated_zoom);
        kv("save_pos", &self.save_pos);
        kv("directory_split", &self.directory_split);
        kv("rollover_box", &self.rollover_box);
        kv("show_free", &self.show_free);
        kv("one_filesystem", &self.one_filesystem);
        kv("apparent_size", &self.apparent_size);
        kv("dedupe_hardlinks", &self.dedupe_hardlinks);
        for r in &self.recent {
            if let Some(p) = r.to_str() {
                kv("recent", &p);
            }
        }
        s
    }

    /// Parse settings text. Unknown keys and bad values are ignored.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let flag = || v.parse::<bool>().ok();
            let int = || v.parse::<i32>().ok();
            let ms = || v.parse::<u32>().ok();
            match k {
                "language" => v.clone_into(&mut s.language),
                "density" => set(&mut s.density, int()),
                "bias" => set(&mut s.bias, int()),
                "file_color" => set(&mut s.file_color, v.parse().ok()),
                "folder_color" => set(&mut s.folder_color, v.parse().ok()),
                "mute_palette" => set(&mut s.mute_palette, flag()),
                "show_name_tips" => set(&mut s.show_name_tips, flag()),
                "nametip_delay" => set(&mut s.nametip_delay_ms, ms()),
                "show_info_tips" => set(&mut s.show_info_tips, flag()),
                "infotip_delay" => set(&mut s.infotip_delay_ms, ms()),
                "tip_path" => set(&mut s.tip_path, flag()),
                "tip_icon" => set(&mut s.tip_icon, flag()),
                "tip_date" => set(&mut s.tip_date, flag()),
                "tip_size" => set(&mut s.tip_size, flag()),
                "tip_attrib" => set(&mut s.tip_attrib, flag()),
                "auto_rescan" => set(&mut s.auto_rescan, flag()),
                "disable_delete" => set(&mut s.disable_delete, flag()),
                "animated_zoom" => set(&mut s.animated_zoom, flag()),
                "save_pos" => set(&mut s.save_pos, flag()),
                "directory_split" => set(&mut s.directory_split, ms()),
                "rollover_box" => set(&mut s.rollover_box, flag()),
                "show_free" => set(&mut s.show_free, flag()),
                "one_filesystem" => set(&mut s.one_filesystem, flag()),
                "apparent_size" => set(&mut s.apparent_size, flag()),
                "dedupe_hardlinks" => set(&mut s.dedupe_hardlinks, flag()),
                "recent" if !v.is_empty() => s.recent.push(PathBuf::from(v)),
                _ => {}
            }
        }
        s.sanitize();
        s
    }

    /// Load from the default location (defaults if missing or unreadable).
    pub fn load() -> Settings {
        config_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_else(Settings::default, |t| Settings::from_text(&t))
    }

    /// Save to the default location.
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file().ok_or_else(|| std::io::Error::other("no configuration directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

fn set<T>(slot: &mut T, v: Option<T>) {
    if let Some(v) = v {
        *slot = v;
    }
}

/// Platform configuration directory for Clawback.
pub fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").map(|p| p.join("Clawback"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support/Clawback"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config"))).map(|p| p.join("clawback"))
    }
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings.ini"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_preserves_selected_palettes_and_migrates_legacy_preset() {
        let settings = Settings { file_color: 18, folder_color: 20, mute_palette: true, ..Settings::default() };
        assert_eq!(Settings::from_text(&settings.to_text()), settings);
        let migrated = Settings::from_text("file_color=15\nfolder_color=18\n");
        assert_eq!(migrated.file_color, crate::palette::DEFAULT_MAP_SCHEME);
        assert_eq!(migrated.folder_color, 18);
        assert!(migrated.mute_palette);
        assert!(!Settings::from_text("file_color=18\n").mute_palette);
    }

    #[test]
    fn map_presets_survive_save_and_reload() {
        for (scheme, _) in crate::palette::MAP_PRESETS {
            let settings = Settings { file_color: scheme, folder_color: scheme, ..Settings::default() };
            let restored = Settings::from_text(&settings.to_text());
            assert_eq!((restored.file_color, restored.folder_color), (scheme, scheme));
        }
    }

    #[test]
    fn retired_schemes_fall_back_and_presets_are_alphabetical() {
        let s = Settings::from_text(
            "file_color = 2
folder_color = 10
",
        );
        assert_eq!(
            (s.file_color, s.folder_color),
            (crate::palette::DEFAULT_MAP_SCHEME, crate::palette::DEFAULT_MAP_SCHEME)
        );
        let names: Vec<_> = crate::palette::MAP_PRESETS.iter().map(|(_, name)| *name).collect();
        assert!(names.is_sorted());
    }

    #[test]
    fn language_defaults_to_os_but_preserves_explicit_choices() {
        assert_eq!(Settings::from_text("").language, "auto");
        assert_eq!(Settings::from_text("language = en\n").language, "en");
        assert_eq!(Settings::from_text("language = fr-FR\n").language, "fr-FR");
        let defaults = Settings::default();
        assert_eq!(Settings::from_text(&defaults.to_text()).language, "auto");
    }

    #[test]
    fn round_trips() {
        let mut s = Settings {
            language: "pt-BR".into(),
            density: -2,
            bias: 7,
            folder_color: 19,
            rollover_box: true,
            nametip_delay_ms: 10,
            ..Settings::default()
        };
        s.push_recent(Path::new("/a"));
        s.push_recent(Path::new("/b"));
        s.push_recent(Path::new("/a"));
        assert_eq!(s.recent, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(Settings::from_text(&s.to_text()), s);
    }

    #[test]
    fn tolerates_garbage_and_clamps() {
        let s = Settings::from_text("density = 99\nbias=-99\nnonsense\nfile_color = 50\nshow_free = maybe\n");
        assert_eq!(s.density, 3);
        assert_eq!(s.bias, -20);
        assert_eq!(s.file_color, crate::palette::DEFAULT_MAP_SCHEME);
        assert!(s.show_free);
    }
}
