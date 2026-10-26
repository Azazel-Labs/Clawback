//! Steam achievements, compiled only into Steam builds (any OS). Elsewhere every call does nothing.
//!
//! The API names must match the achievements configured in Steamworks; see `docs/steam.md`.
use clawback_core::{Tree, tree::flags};

const GB: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Achievement {
    FirstScan,
    WholeDrive,
    Terabyte,
    MillionFiles,
    AbsoluteUnit,
    NodeModules,
    FirstDelete,
    SparkJoy,
    TrashDay,
    TempClean,
    WslDiet,
    DontTouch,
    Spelunker,
    GottaGoFast,
    GameOver,
    DripCheck,
    DeleteWindows,
    DeleteLinux,
    DeleteMacos,
    WorldTour,
}

#[cfg_attr(not(feature = "steam"), allow(dead_code))] // Read by the Steam connection and tests.
impl Achievement {
    pub const ALL: [Self; 20] = [
        Self::FirstScan,
        Self::WholeDrive,
        Self::Terabyte,
        Self::MillionFiles,
        Self::AbsoluteUnit,
        Self::NodeModules,
        Self::FirstDelete,
        Self::SparkJoy,
        Self::TrashDay,
        Self::TempClean,
        Self::WslDiet,
        Self::DontTouch,
        Self::Spelunker,
        Self::GottaGoFast,
        Self::GameOver,
        Self::DripCheck,
        Self::DeleteWindows,
        Self::DeleteLinux,
        Self::DeleteMacos,
        Self::WorldTour,
    ];

    pub const fn api_name(self) -> &'static str {
        match self {
            Self::FirstScan => "FIRST_SCAN",
            Self::WholeDrive => "WHOLE_DRIVE",
            Self::Terabyte => "TERABYTE",
            Self::MillionFiles => "MILLION_FILES",
            Self::AbsoluteUnit => "ABSOLUTE_UNIT",
            Self::NodeModules => "NODE_MODULES",
            Self::FirstDelete => "FIRST_DELETE",
            Self::SparkJoy => "SPARK_JOY",
            Self::TrashDay => "TRASH_DAY",
            Self::TempClean => "TEMP_CLEAN",
            Self::WslDiet => "WSL_DIET",
            Self::DontTouch => "DONT_TOUCH",
            Self::Spelunker => "SPELUNKER",
            Self::GottaGoFast => "GOTTA_GO_FAST",
            Self::GameOver => "GAME_OVER",
            Self::DripCheck => "DRIP_CHECK",
            Self::DeleteWindows => "DELETE_WINDOWS",
            Self::DeleteLinux => "DELETE_LINUX",
            Self::DeleteMacos => "DELETE_MACOS",
            Self::WorldTour => "WORLD_TOUR",
        }
    }
}

/// Connect to Steam when the app was started by it. Call once, early and from the desktop
/// app only: the Turbo service runs the same executable and must never load the Steam API.
pub fn init() {
    #[cfg(feature = "steam")]
    steam::init();
}

/// Let the Steam API deliver its callbacks; call once per frame.
pub fn pump() {
    #[cfg(feature = "steam")]
    steam::pump();
}

pub fn unlock(achievement: Achievement) {
    #[cfg(feature = "steam")]
    steam::unlock(achievement);
    #[cfg(not(feature = "steam"))]
    let _ = achievement;
}

fn enabled() -> bool {
    #[cfg(feature = "steam")]
    return steam::connected();
    #[cfg(not(feature = "steam"))]
    false
}

/// Achievements earned by a finished scan.
pub fn scanned(tree: &Tree, whole_drive: bool, turbo: bool) {
    if !enabled() {
        return;
    }
    for achievement in from_scan(tree, whole_drive, turbo) {
        unlock(achievement);
    }
}

fn from_scan(tree: &Tree, whole_drive: bool, turbo: bool) -> Vec<Achievement> {
    let root = tree.root();
    let mut earned = vec![Achievement::FirstScan];
    earned.extend(whole_drive.then_some(Achievement::WholeDrive));
    earned.extend(turbo.then_some(Achievement::GottaGoFast));
    earned.extend((root.size >= 1024 * GB).then_some(Achievement::Terabyte));
    earned.extend((root.files >= 1_000_000).then_some(Achievement::MillionFiles));
    let (mut unit, mut black_hole) = (false, false);
    for id in 0..tree.len() as clawback_core::NodeId {
        let node = tree.node(id);
        if node.has(flags::REMOVED) {
            continue;
        }
        if node.is_dir() {
            black_hole |= node.size >= GB && node.name_lossy() == "node_modules";
        } else {
            unit |= node.size >= 50 * GB;
        }
    }
    earned.extend(unit.then_some(Achievement::AbsoluteUnit));
    earned.extend(black_hole.then_some(Achievement::NodeModules));
    earned
}

/// Achievements for space given back by a delete or cleanup of `bytes`.
pub fn deleted(bytes: u64) {
    unlock(Achievement::FirstDelete);
    if bytes >= 10 * GB {
        unlock(Achievement::SparkJoy);
    }
    let platform = if cfg!(windows) {
        Achievement::DeleteWindows
    } else if cfg!(target_os = "macos") {
        Achievement::DeleteMacos
    } else {
        Achievement::DeleteLinux
    };
    unlock(platform);
    // Achievements belong to the Steam account, so the others may come from other computers.
    #[cfg(feature = "steam")]
    if [Achievement::DeleteWindows, Achievement::DeleteLinux, Achievement::DeleteMacos].into_iter().all(steam::achieved) {
        unlock(Achievement::WorldTour);
    }
}

#[cfg(feature = "steam")]
mod steam {
    use super::Achievement;
    use std::sync::{
        OnceLock,
        atomic::{AtomicU32, Ordering},
    };

    static CLIENT: OnceLock<Option<steamworks::Client>> = OnceLock::new();
    /// Achievements already reported this session, by index in [`Achievement::ALL`].
    static REPORTED: AtomicU32 = AtomicU32::new(0);
    const _: () = assert!(Achievement::ALL.len() <= 32);

    pub fn init() {
        CLIENT.get_or_init(|| {
            // The Windows DLL is delay-loaded (see build.rs): calling into a missing one would
            // crash, and only the copy shipped beside the executable may be loaded.
            #[cfg(windows)]
            if !std::env::current_exe().ok()?.with_file_name("steam_api64.dll").is_file() {
                return None;
            }
            steamworks::Client::init().ok()
        });
    }

    pub fn connected() -> bool {
        CLIENT.get().is_some_and(Option::is_some)
    }

    pub fn pump() {
        if let Some(Some(client)) = CLIENT.get() {
            client.run_callbacks();
        }
    }

    pub fn achieved(achievement: Achievement) -> bool {
        let Some(Some(client)) = CLIENT.get() else { return false };
        client.user_stats().achievement(achievement.api_name()).get() == Ok(true)
    }

    pub fn unlock(achievement: Achievement) {
        let Some(Some(client)) = CLIENT.get() else { return };
        let index = Achievement::ALL.iter().position(|&a| a == achievement).unwrap_or_default();
        if REPORTED.fetch_or(1 << index, Ordering::Relaxed) & (1 << index) != 0 {
            return;
        }
        let stats = client.user_stats();
        let entry = stats.achievement(achievement.api_name());
        if entry.get() == Ok(false) && entry.set().is_ok() {
            let _ = stats.store_stats();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{ROOT, tree::NewEntry};
    use std::path::Path;

    #[test]
    fn api_names_are_unique_constants() {
        for (index, achievement) in Achievement::ALL.iter().enumerate() {
            let name = achievement.api_name();
            assert!(name.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'), "{name}");
            assert!(Achievement::ALL[..index].iter().all(|other| other.api_name() != name));
        }
    }

    #[test]
    fn scans_earn_size_achievements() {
        let mut tree = Tree::new(Path::new("C:\\"));
        let ids = tree.add_children(ROOT, vec![NewEntry::dir("node_modules"), NewEntry::file("disk.vhdx", 60 * GB)]);
        tree.add_children(ids.start, vec![NewEntry::file("left-pad.js", 2 * GB)]);
        tree.sort_all();
        let earned = from_scan(&tree, true, false);
        for wanted in [Achievement::FirstScan, Achievement::WholeDrive, Achievement::AbsoluteUnit, Achievement::NodeModules] {
            assert!(earned.contains(&wanted), "{wanted:?}");
        }
        assert!(!earned.contains(&Achievement::Terabyte));
        assert!(!earned.contains(&Achievement::GottaGoFast));
    }
}
