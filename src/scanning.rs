//! Background scan orchestration. The UI only receives prepared snapshots;
//! it never locks the scanner tree, probes a disk, or joins a scan thread.

use crate::platform::{self, DiskInfo};
use crate::watching::{Started, Watch};
use clawback_core::scan::{ProgressSnapshot, ScanOptions, ScanResult, lock};
use clawback_core::{Scan, Tree};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub struct Preview {
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
    pub root: PathBuf,
    pub progress: ProgressSnapshot,
    pub current: PathBuf,
    pub disk: Option<DiskInfo>,
    pub is_mount: bool,
    pub rx: mpsc::Receiver<Update>,
    cancel: Arc<AtomicBool>,
}

impl Running {
    pub fn start(root: PathBuf, options: ScanOptions, repaint: eframe::egui::Context) -> std::io::Result<Self> {
        Self::start_with_disk(
            root,
            options,
            move || repaint.request_repaint(),
            |path| platform::disk_for(path, &platform::all_disks()),
        )
    }

    pub fn start_terminal(root: PathBuf, options: ScanOptions) -> std::io::Result<Self> {
        Self::start_with_disk(root, options, || {}, |path| platform::disk_for(path, &platform::all_disks()))
    }

    fn start_with_disk(
        root: PathBuf,
        options: ScanOptions,
        repaint: impl Fn() + Send + Sync + 'static,
        find_disk: impl FnOnce(&std::path::Path) -> Option<DiskInfo> + Send + 'static,
    ) -> std::io::Result<Self> {
        // One pending preview at most: a minimized window cannot accumulate trees.
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let path = root.clone();
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(repaint);
        std::thread::Builder::new().name("clawback-preview".into()).spawn(move || {
            // Arm before any scan work so edits during the scan are retained.
            let watcher = Watch::start(&path);
            let disk = find_disk(&path);
            let mut options = options;
            if let Some(disk) = &disk {
                options.storage = disk.kind;
            }
            let is_mount = disk.as_ref().is_some_and(|d| platform::same_path(&d.mount, &path));
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
                if stop.load(Ordering::Relaxed) {
                    scan.cancel();
                }
                if scan.is_finished() {
                    break;
                }
                let shared = scan.shared();
                let progress = shared.progress.snapshot();
                if published == Some(progress) {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                let preview = Preview {
                    tree: lock(&shared.tree).preview(4096),
                    progress,
                    current: shared.progress.current_path(),
                    disk: disk.clone(),
                    is_mount,
                };
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
            let result = scan.wait();
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
            current: root.clone(),
            root,
            progress: ProgressSnapshot::default(),
            disk: None,
            is_mount: false,
            rx,
            cancel,
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
