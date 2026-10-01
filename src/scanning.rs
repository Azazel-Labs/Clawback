//! Background scan orchestration. The UI only receives prepared snapshots;
//! it never locks the scanner tree, probes a disk, or joins a scan thread.

use crate::platform::{self, DiskInfo};
use crate::watching::{Started, Watch};
use clawback_core::scan::{ProgressSnapshot, ScanOptions, ScanResult, lock};
use clawback_core::{Scan, Tree};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub struct Preview {
    #[cfg(windows)]
    pub turbo_available: bool,
    pub tree: Tree,
    pub progress: ProgressSnapshot,
    pub current: PathBuf,
    pub disk: Option<DiskInfo>,
    pub is_mount: bool,
}

pub enum Update {
    Preview(Preview),
    Finished(ScanResult, Option<DiskInfo>, bool, Started),
    Failed(String),
    Cancelled,
}

pub struct Running {
    #[cfg(windows)]
    pub turbo_available: bool,
    pub root: PathBuf,
    pub progress: ProgressSnapshot,
    pub current: PathBuf,
    pub disk: Option<DiskInfo>,
    pub is_mount: bool,
    pub rx: mpsc::Receiver<Update>,
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    #[cfg(windows)]
    pub turbo: crate::turbo::Control,
    /// The screenshot fixture's own end of `rx`, standing in for a scan thread.
    #[cfg(feature = "screenshots")]
    demo: Option<mpsc::SyncSender<Update>>,
}

impl Running {
    /// A scan-shaped fixture for screenshots: no filesystem access or elevation.
    #[cfg(feature = "screenshots")]
    pub fn demo(paused: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        Self {
            root: PathBuf::from("Demo Drive"),
            current: PathBuf::from("Demo Drive/Projects/Lunar Garden/Assets"),
            progress: ProgressSnapshot { files: 184_302, dirs: 12_408, bytes: 94_983_340_321, workers: 8, denied: 0 },
            disk: Some(crate::demo::disk()),
            is_mount: true,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(paused)),
            #[cfg(windows)]
            turbo_available: true,
            #[cfg(windows)]
            turbo: crate::turbo::Control::default(),
            demo: Some(tx),
        }
    }
    pub fn start(root: PathBuf, options: ScanOptions, repaint: eframe::egui::Context) -> std::io::Result<Self> {
        Self::start_with_disk(root, options, move || repaint.request_repaint(), find_disk)
    }

    pub fn start_terminal(root: PathBuf, options: ScanOptions) -> std::io::Result<Self> {
        Self::start_with_disk(root, options, || {}, find_disk)
    }

    fn start_with_disk(
        root: PathBuf,
        options: ScanOptions,
        repaint: impl Fn() + Send + Sync + 'static,
        find_disk: impl FnOnce(&Path) -> Option<DiskInfo> + Send + 'static,
    ) -> std::io::Result<Self> {
        // One pending preview at most: a minimized window cannot accumulate trees.
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let pause_requested = paused.clone();
        let path = root.clone();
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(repaint);
        let mut race = TurboRace::new();
        #[cfg(windows)]
        let turbo = race.control.clone();
        std::thread::Builder::new().name("clawback-preview".into()).spawn(move || {
            let _span = crate::perf::span("worker.scan_lifetime");
            // Arm before any scan work so edits during the scan are retained.
            let setup_span = crate::perf::span("worker.scan_setup");
            let watcher = Watch::start(&path);
            let disk = find_disk(&path);
            drop(setup_span);
            let mut options = options;
            if let Some(disk) = &disk {
                options.storage = disk.kind;
            }
            let is_mount = disk.as_ref().is_some_and(|d| platform::same_path(&d.mount, &path));
            race.offer(disk.as_ref(), is_mount);
            while pause_requested.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if stop.load(Ordering::Relaxed) {
                let _ = tx.send(Update::Cancelled);
                repaint();
                return;
            }
            let scan = match Scan::start(&path, options.clone(), None) {
                Ok(scan) => scan,
                Err(error) => {
                    let _ = tx.send(Update::Failed(error.to_string()));
                    repaint();
                    return;
                }
            };
            let mut published = None;
            loop {
                // Once Turbo leads, park traversal workers to avoid competing
                // for CPU/disk. Retain their tree so a failed helper can resume.
                let turbo_leads = race.leads(scan.shared().progress.snapshot());
                scan.set_paused(pause_requested.load(Ordering::Relaxed) || turbo_leads);
                let stopping = stop.load(Ordering::Relaxed);
                if stopping {
                    scan.cancel();
                    race.cancel();
                }
                if scan.is_finished() || (!stopping && race.poll(&path, &options, &pause_requested)) {
                    break;
                }
                let shared = scan.shared();
                let progress = shared.progress.snapshot();
                if published == Some(progress) {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                let preview_span = crate::perf::span("worker.scan_preview");
                let preview = Preview {
                    #[cfg(windows)]
                    turbo_available: race.available,
                    tree: lock(&shared.tree).preview(4096),
                    progress,
                    current: shared.progress.current_path(),
                    disk: disk.clone(),
                    is_mount,
                };
                drop(preview_span);
                match tx.try_send(Update::Preview(preview)) {
                    Ok(()) => {
                        published = Some(progress);
                        repaint();
                    }
                    Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // Final sorting, joining and ownership transfer all happen here.
            let result = match race.finish() {
                Some(result) => {
                    drop(scan); // cooperatively cancel directory workers, never join them on the UI
                    result
                }
                None => scan.wait(),
            };
            let started = if result.cancelled || stop.load(Ordering::Relaxed) {
                Started::unavailable("scan cancelled")
            } else {
                match watcher {
                    Ok(watcher) => watcher.live(
                        result.tree.clone(),
                        options,
                        result.skipped.clone(),
                        disk.clone(),
                        repaint.clone(),
                    ),
                    Err(error) => Started::unavailable(error),
                }
            };
            let _ = tx.send(Update::Finished(result, disk, is_mount, started));
            repaint();
        })?;
        Ok(Self {
            #[cfg(windows)]
            turbo_available: false,
            current: root.clone(),
            root,
            progress: ProgressSnapshot::default(),
            disk: None,
            is_mount: false,
            rx,
            cancel,
            paused,
            #[cfg(windows)]
            turbo,
            #[cfg(feature = "screenshots")]
            demo: None,
        })
    }

    /// Show whichever scanner has assembled more entries. MFT records alone
    /// are not comparable: they include extensions and filesystem metadata.
    pub fn display_progress(&self) -> ProgressSnapshot {
        #[cfg(windows)]
        if let Some(progress) = self.turbo.progress().and_then(|p| turbo_lead(self.progress, p)) {
            return progress;
        }
        self.progress
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        #[cfg(feature = "screenshots")]
        if let Some(demo) = &self.demo {
            let _ = demo.try_send(Update::Cancelled);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn toggle_pause(&self) {
        self.paused.fetch_xor(true, Ordering::Relaxed);
    }
}

fn find_disk(path: &Path) -> Option<DiskInfo> {
    platform::disk_for(path, &platform::all_disks())
}

/// The optional elevated MFT read racing the directory walk: started on the
/// user's request, it parks the walk once ahead and wins if it finishes first.
#[cfg(windows)]
#[derive(Default)]
struct TurboRace {
    control: crate::turbo::Control,
    available: bool,
    attempt: Option<crate::turbo::Attempt>,
    result: Option<ScanResult>,
}

#[cfg(windows)]
impl TurboRace {
    fn new() -> Self {
        Self::default()
    }
    fn offer(&mut self, disk: Option<&DiskInfo>, is_mount: bool) {
        self.available = crate::turbo::eligible(disk, is_mount);
    }
    fn leads(&self, walk: ProgressSnapshot) -> bool {
        self.control.progress().and_then(|p| turbo_lead(walk, p)).is_some()
    }
    /// Cancels a running helper, including pending consent.
    fn cancel(&mut self) {
        self.attempt = None;
    }
    /// Starts a requested helper and collects a finished one; true once Turbo won.
    fn poll(&mut self, root: &Path, options: &ScanOptions, paused: &Arc<AtomicBool>) -> bool {
        if self.available && self.attempt.is_none() && self.control.take_request() {
            match crate::turbo::Attempt::start(root.to_owned(), options.clone(), paused.clone(), self.control.clone()) {
                Ok(started) => self.attempt = Some(started),
                Err(error) => self.control.failed(&error),
            }
        }
        match self.attempt.as_mut().and_then(crate::turbo::Attempt::poll) {
            Some(Ok(result)) => self.result = Some(result),
            Some(Err(_)) => self.attempt = None, // status already records decline/failure
            None => {}
        }
        self.result.is_some()
    }
    /// The winning snapshot, if any; a losing helper is cancelled.
    fn finish(self) -> Option<ScanResult> {
        self.result
    }
}

/// Turbo is Windows-only; elsewhere the walk always runs alone.
#[cfg(not(windows))]
struct TurboRace;

#[cfg(not(windows))]
#[allow(clippy::unused_self)] // mirrors the Windows race
impl TurboRace {
    fn new() -> Self {
        Self
    }
    fn offer(&mut self, _: Option<&DiskInfo>, _: bool) {}
    fn leads(&self, _: ProgressSnapshot) -> bool {
        false
    }
    fn cancel(&mut self) {}
    fn poll(&mut self, _: &Path, _: &ScanOptions, _: &Arc<AtomicBool>) -> bool {
        false
    }
    fn finish(self) -> Option<ScanResult> {
        None
    }
}

#[cfg(windows)]
fn turbo_lead(normal: ProgressSnapshot, mft: clawback_core::scan::MftProgress) -> Option<ProgressSnapshot> {
    (mft.phase >= 2 && mft.files.saturating_add(mft.dirs) > normal.files.saturating_add(normal.dirs))
        .then_some(ProgressSnapshot { files: mft.files, dirs: mft.dirs, bytes: mft.bytes, denied: 0, workers: 1 })
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn turbo_overtakes_only_with_assembled_entries() {
        use clawback_core::scan::MftProgress;
        let normal = ProgressSnapshot { files: 100, dirs: 10, ..Default::default() };
        let mut mft = MftProgress { records: 1_000_000, ..Default::default() };
        assert!(turbo_lead(normal, mft).is_none());
        mft.phase = 2;
        mft.files = 99;
        mft.dirs = 11;
        assert!(turbo_lead(normal, mft).is_none());
        mft.files = 101;
        mft.bytes = 8192;
        let lead = turbo_lead(normal, mft).unwrap();
        assert_eq!((lead.files, lead.dirs, lead.bytes, lead.workers), (101, 11, 8192, 1));
        assert!(turbo_lead(ProgressSnapshot { files: 200, ..normal }, mft).is_none());
    }

    #[test]
    fn slow_disk_discovery_does_not_block_start_or_cancel() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let running = Running::start_with_disk(
            PathBuf::from("missing"),
            ScanOptions::default(),
            || {},
            move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                None
            },
        )
        .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(running.rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        running.toggle_pause();
        assert!(running.is_paused());
        #[cfg(windows)]
        {
            running.turbo.request();
            assert!(running.turbo.take_request());
            running
                .turbo
                .failed(&std::io::Error::from_raw_os_error(windows_sys::Win32::Foundation::ERROR_CANCELLED as i32));
            assert_eq!(running.turbo.status(), crate::turbo::Status::Declined);
            assert!(!running.cancel.load(Ordering::Relaxed));
            assert!(running.is_paused());
        }
        running.cancel();
        release_tx.send(()).unwrap();
        assert!(matches!(running.rx.recv_timeout(Duration::from_secs(5)), Ok(Update::Cancelled)));
    }

    #[test]
    fn startup_errors_are_delivered_asynchronously() {
        let running = Running::start_with_disk(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            ScanOptions::default(),
            || {},
            |_| None,
        )
        .unwrap();
        assert!(matches!(running.rx.recv_timeout(Duration::from_secs(5)), Ok(Update::Failed(_))));
    }
}
