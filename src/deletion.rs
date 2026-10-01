//! Bounded, polled progress for a background Recycle Bin operation.
use clawback_core::scan::lock;
#[cfg(windows)]
use std::time::Instant;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Preparing,
    Recycling,
    Updating,
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub total: u32,
    pub done: u32,
    pub current: String,
    #[cfg(windows)]
    pub error: Option<String>,
}
#[derive(Default)]
pub struct Progress {
    state: Mutex<Snapshot>,
    #[cfg(windows)]
    last_item: Mutex<Option<Instant>>,
}
impl Progress {
    pub fn snapshot(&self) -> Snapshot {
        lock(&self.state).clone()
    }
    pub fn phase(&self, phase: Phase) {
        lock(&self.state).phase = phase;
    }
}
pub fn trash(path: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    let _span = crate::perf::span("delete.recycle");
    #[cfg(windows)]
    {
        windows::recycle(path, progress).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        progress.phase(Phase::Recycling);
        trash::delete(path).map_err(|error| error.to_string())
    }
}
