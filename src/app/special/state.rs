use super::windows::{programs, wsl_disks};
use crate::app::QueuedDelete;
use std::{collections::VecDeque, path::PathBuf};

pub(in crate::app) struct Compacted {
    pub path: PathBuf,
    pub info: wsl_disks::Info,
    pub disk: Option<crate::platform::DiskInfo>,
}

#[derive(Default)]
pub struct State {
    pub(in crate::app) wsl_disk: Option<PathBuf>,
    pub(in crate::app) wsl_info: Option<wsl_disks::Info>,
    pub(in crate::app) wsl_info_job: Option<crate::background::Job<(), wsl_disks::Info>>,
    pub(in crate::app) wsl_confirm_compact: bool,
    pub(in crate::app) removal_checks: Vec<(QueuedDelete, crate::background::Job<(), Option<programs::Removal>>)>,
    pub(in crate::app) removals: VecDeque<programs::Removal>,
    pub(in crate::app) compacting: Option<crate::background::Job<u64, Result<Compacted, String>>>,
}
