//! Contents-only cleanup policies shared by the app's ordinary deletion queue.
use crate::{deletion, i18n::tr};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))] // Current cleanup providers are Windows-only.
pub enum Cleanup {
    RecycleBin,
    TempFolder,
}

pub struct Confirmation {
    pub title: String,
    pub body: String,
    pub button: String,
}

impl Cleanup {
    pub fn keep(self) -> &'static [&'static str] {
        match self {
            Self::RecycleBin => &["desktop.ini"],
            Self::TempFolder => &[],
        }
    }

    pub fn rescan_after_partial(self) -> bool {
        self == Self::TempFolder
    }

    pub fn confirmation(self, path: &Path, drive: &str) -> Confirmation {
        match self {
            Self::RecycleBin => Confirmation {
                title: tr!("empty-recycle-bin-title"),
                body: tr!("empty-recycle-bin-body", drive = drive),
                button: tr!("empty-recycle-bin-button"),
            },
            Self::TempFolder => Confirmation {
                title: tr!("empty-temp-title"),
                body: tr!("empty-temp-body", path = path.display().to_string()),
                button: tr!("empty-temp-button"),
            },
        }
    }

    pub fn progress_label(self) -> String {
        match self {
            Self::RecycleBin => tr!("emptying-recycle-bin"),
            Self::TempFolder => tr!("cleaning-temp-folder"),
        }
    }

    pub fn failure_message(self, count: u64, error: String) -> Option<String> {
        match self {
            Self::RecycleBin => Some(tr!("purge-failed", count = count, error = error)),
            Self::TempFolder => None,
        }
    }

    pub fn run(self, target: &deletion::Target<'_>, threads: usize, progress: &deletion::Progress) -> deletion::Report {
        let report = match self {
            Self::RecycleBin => deletion::purge(target, threads, progress),
            Self::TempFolder => deletion::purge_temp(target, threads, progress),
        };
        #[cfg(windows)]
        if self == Self::RecycleBin {
            super::windows::recycle_bin::notify_changed(target.root);
        }
        report
    }
}
