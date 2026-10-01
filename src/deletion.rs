//! Background deletion with bounded, polled progress: recycling, or a fast permanent purge.
use clawback_core::scan::lock;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
mod purge;
#[cfg(windows)]
mod windows;
pub use purge::{Report, Target};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Preparing,
    Recycling,
    /// Permanently deleting, after the user confirmed it in Clawback.
    Deleting,
    Updating,
}

/// How a Recycle Bin attempt ended without an error.
// Only the Windows shell declines or refuses items; other platforms always finish.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recycled {
    Done,
    /// The user answered no to a Windows prompt; nothing changed.
    Declined,
    /// Too large for the Recycle Bin, or the drive has none. Nothing was deleted.
    TooLarge,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub total: u64,
    pub done: u64,
    pub current: String,
}

#[derive(Default)]
pub struct Progress {
    state: Mutex<Snapshot>,
    last_item: Mutex<Option<Instant>>,
    /// Files removed by a purge; kept outside the lock for its many workers.
    files_done: AtomicU64,
    cancel: AtomicBool,
}

impl Progress {
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = lock(&self.state).clone();
        snapshot.done += self.files_done.load(Ordering::Relaxed);
        snapshot
    }
    pub fn phase(&self, phase: Phase) {
        lock(&self.state).phase = phase;
    }
    pub fn set_total(&self, total: u64) {
        lock(&self.state).total = total;
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn file_done(&self) {
        self.files_done.fetch_add(1, Ordering::Relaxed);
    }
    /// Whether to show the next item being worked on: at most ten times a second.
    fn due(&self) -> bool {
        let mut last = lock(&self.last_item);
        let due = last.is_none_or(|time| time.elapsed() >= Duration::from_millis(100));
        if due {
            *last = Some(Instant::now());
        }
        due
    }
    fn note(&self, path: &Path) {
        if self.due() {
            lock(&self.state).current = path.display().to_string();
        }
    }
}

pub fn trash(path: &Path, progress: &Arc<Progress>) -> Result<Recycled, String> {
    let _span = crate::perf::span("delete.recycle");
    #[cfg(windows)]
    {
        windows::recycle(path, progress)
    }
    #[cfg(not(windows))]
    {
        progress.phase(Phase::Recycling);
        trash::delete(path).map(|()| Recycled::Done).map_err(|error| error.to_string())
    }
}

/// Permanently delete `target`. Only call this after the user confirmed it.
pub fn purge(target: &Target<'_>, threads: usize, progress: &Progress) -> Report {
    let _span = crate::perf::span("delete.purge");
    progress.phase(Phase::Deleting);
    purge::run(target, threads, progress)
}
