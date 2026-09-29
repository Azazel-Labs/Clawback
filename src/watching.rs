//! Bounded notifications and background-only incremental reconciliation.
use crate::platform::{self, DiskInfo};
use clawback_core::{Scan, ScanOptions, Skipped, Tree, live::Refresh};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const CAPACITY: usize = 4096;
const BATCH: Duration = Duration::from_secs(1);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

pub enum Change {
    Path(PathBuf),
    Rescan,
    Failed(String),
}

#[derive(Clone)]
pub struct Inbox {
    tx: mpsc::SyncSender<Change>,
    overflow: Arc<AtomicBool>,
    failed: Arc<Mutex<Option<String>>>,
}
impl Inbox {
    pub fn send(&self, change: Change) {
        if let Change::Failed(error) = &change {
            *clawback_core::scan::lock(&self.failed) = Some(error.clone());
        }
        if let Err(mpsc::TrySendError::Full(_)) = self.tx.try_send(change) {
            self.overflow.store(true, Ordering::Release);
        }
    }
}

pub struct Snapshot {
    pub tree: Arc<Tree>,
    pub dirs: u64,
    pub skipped: Arc<Vec<Skipped>>,
    pub disk: Option<DiskInfo>,
    pub reset: bool,
}

pub enum Update {
    Snapshot(Snapshot),
    Status(String),
}

pub struct Live {
    pub rx: mpsc::Receiver<Update>,
    inbox: Inbox,
    stop: Arc<AtomicBool>,
}
impl Live {
    pub fn invalidate(&self, path: PathBuf) {
        self.inbox.send(Change::Path(path));
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct Started {
    pub live: Option<Live>,
    pub status: String,
}
impl Started {
    pub fn unavailable(error: impl std::fmt::Display) -> Self {
        Self { live: None, status: format!("Live unavailable: {error} · Rescan to refresh") }
    }
}

pub struct Watch {
    #[cfg(windows)]
    _watcher: crate::watch_windows::Watcher,
    #[cfg(not(windows))]
    _watcher: notify::RecommendedWatcher,
    inbox: Inbox,
    rx: mpsc::Receiver<Change>,
}

impl Watch {
    /// Called before the initial scan, exclusively on the coordinator thread.
    pub fn start(root: &Path) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(CAPACITY);
        let inbox = Inbox { tx, overflow: Arc::new(AtomicBool::new(false)), failed: Arc::new(Mutex::new(None)) };
        #[cfg(windows)]
        let watcher = crate::watch_windows::Watcher::start(root, inbox.clone()).map_err(|e| e.to_string())?;
        #[cfg(not(windows))]
        let watcher = {
            use notify::Watcher;
            let events = inbox.clone();
            let mut watcher = notify::RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| match event {
                    Ok(event) if event.need_rescan() => events.send(Change::Rescan),
                    Ok(event) => {
                        if matches!(event.kind, notify::EventKind::Access(_)) {
                            return;
                        }
                        for path in event.paths {
                            events.send(Change::Path(path));
                        }
                    }
                    Err(error) => events.send(Change::Failed(error.to_string())),
                },
                notify::Config::default().with_follow_symlinks(false),
            )
            .map_err(|e| e.to_string())?;
            watcher.watch(root, notify::RecursiveMode::Recursive).map_err(|e| e.to_string())?;
            watcher
        };
        Ok(Self { _watcher: watcher, inbox, rx })
    }

    pub fn live(
        self,
        tree: Tree,
        options: ScanOptions,
        skipped: Vec<Skipped>,
        disk: Option<DiskInfo>,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Started {
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let inbox = self.inbox.clone();
        match std::thread::Builder::new().name("clawback-live".into()).spawn(move || {
            self.run(tree, &options, skipped, disk, &cancel, &tx, &*repaint);
        }) {
            Ok(_) => Started { live: Some(Live { rx, inbox, stop }), status: "Live · Watching for changes".into() },
            Err(error) => Started::unavailable(error),
        }
    }

    #[allow(clippy::too_many_arguments)] // Worker inputs are transferred once at startup.
    fn run(
        self,
        mut tree: Tree,
        options: &ScanOptions,
        skipped: Vec<Skipped>,
        mut disk: Option<DiskInfo>,
        stop: &AtomicBool,
        tx: &mpsc::SyncSender<Update>,
        repaint: &dyn Fn(),
    ) {
        let publish = |update| {
            let sent = tx.send(update).is_ok();
            if sent {
                repaint();
            }
            sent
        };
        let root = tree.root_path().to_path_buf();
        let mut refresh = Refresh::new(&tree, options.clone());
        let mut skipped = Arc::new(skipped);
        let mut pending = Pending::default();
        let mut last_recovery: Option<Instant> = None;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if pending.is_empty() {
                match self.rx.recv_timeout(BATCH) {
                    Ok(change) => pending.add(change, &root),
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                }
            }
            // Fixed window: sustained activity cannot postpone updates forever.
            let deadline = Instant::now() + BATCH;
            while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                match self
                    .rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)))
                {
                    Ok(change) => pending.add(change, &root),
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if self.inbox.overflow.swap(false, Ordering::AcqRel) {
                pending.rescan = true;
            }
            if let Some(error) = clawback_core::scan::lock(&self.inbox.failed).take().or_else(|| pending.failed.take())
            {
                publish(Update::Status(format!("Live stopped: {error} · Rescan to reconnect")));
                return;
            }
            let mut changed = false;
            let mut reset = false;
            if pending.rescan {
                if last_recovery.is_some_and(|last| last.elapsed() < RECOVERY_INTERVAL) {
                    continue;
                }
                if !publish(Update::Status("Live · Reconciling changes in background".into())) {
                    return;
                }
                pending.paths.clear();
                pending.rescan = false;
                let mut opts = options.clone();
                opts.threads = 1;
                let result = Scan::start(&root, opts, None).map(|scan| {
                    while !scan.is_finished() {
                        if stop.load(Ordering::Relaxed) {
                            scan.cancel();
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    scan.wait()
                });
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match result {
                    Ok(result) if !result.cancelled => {
                        last_recovery = Some(Instant::now());
                        tree = result.tree;
                        skipped = Arc::new(result.skipped);
                        refresh = Refresh::new(&tree, options.clone());
                        changed = true;
                        reset = true;
                    }
                    Ok(_) => return,
                    Err(error) => {
                        publish(Update::Status(format!("Live stopped: {error} · Rescan to reconnect")));
                        return;
                    }
                }
            } else {
                let budget = Instant::now();
                for _ in 0..64 {
                    let Some(path) = pending.paths.pop_first() else { break };
                    if let Ok(updated) = refresh.path(&mut tree, &path, stop) {
                        changed |= updated;
                    } else {
                        pending.rescan = true;
                        break;
                    }
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if budget.elapsed() >= Duration::from_millis(50) {
                        break;
                    }
                }
            }
            if changed {
                if let Some(disk) = &mut disk {
                    platform::refresh_disk_space(disk);
                }
                // Page sharing keeps snapshots proportional to changed storage.
                if !publish(Update::Snapshot(Snapshot {
                    tree: Arc::new(tree.clone()),
                    dirs: refresh.dirs,
                    skipped: skipped.clone(),
                    disk: disk.clone(),
                    reset,
                })) {
                    return;
                }
            }
            // Bound detached arena storage after sustained create/delete churn.
            if tree.len() as u64 > (tree.root().files + refresh.dirs).max(4096) * 2 {
                pending.rescan = true;
            }
            if changed || pending.rescan {
                let status = if pending.is_empty() { "Live · Watching for changes" } else { "Live · Catching up" };
                if !publish(Update::Status(status.into())) {
                    return;
                }
            }
        }
    }
}

#[derive(Default)]
struct Pending {
    paths: BTreeSet<PathBuf>,
    rescan: bool,
    failed: Option<String>,
}
impl Pending {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.rescan && self.failed.is_none()
    }
    fn add(&mut self, change: Change, root: &Path) {
        match change {
            Change::Path(path) if path.starts_with(root) && !self.rescan => {
                // Retain individual descendant events: an existing directory
                // notification only reconciles its immediate children.
                self.paths.insert(path);
                if self.paths.len() > CAPACITY {
                    self.paths.clear();
                    self.rescan = true;
                }
            }
            Change::Rescan => {
                self.paths.clear();
                self.rescan = true;
            }
            Change::Failed(error) => self.failed = Some(error),
            Change::Path(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::AtomicU64};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "clawback-watch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[track_caller]
    fn receive(live: &Live, condition: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match live
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("live update deadline")
            {
                Update::Snapshot(snapshot) if condition(&snapshot) => return snapshot,
                Update::Status(status) => assert!(!status.starts_with("Live stopped:"), "{status}"),
                Update::Snapshot(snapshot) => {
                    eprintln!(
                        "Waiting: size={}, names={:?}",
                        snapshot.tree.root().size,
                        snapshot
                            .tree
                            .root()
                            .children
                            .iter()
                            .map(|&id| snapshot.tree.node(id).name_lossy())
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    #[test]
    fn native_notifications_cover_scan_gap_edit_rename_delete_and_recovery() {
        let temp = Temp::new();
        let options =
            ScanOptions { apparent_size: true, dedupe_hardlinks: false, threads: 1, ..ScanOptions::default() };
        fs::write(temp.0.join("initial"), [1; 10]).unwrap();
        let watcher = Watch::start(&temp.0).unwrap();
        let result = clawback_core::scan::scan(&temp.0, options.clone()).unwrap();
        // This event occurs after the baseline scan, before the live worker starts.
        fs::write(temp.0.join("during-scan"), [2; 20]).unwrap();
        let live = watcher.live(result.tree, options, Vec::new(), None, Arc::new(|| {})).live.unwrap();
        let first = receive(&live, |s| s.tree.root().size == 30);
        let initial_id = first.tree.find_path(&temp.0.join("initial")).unwrap();
        fs::write(temp.0.join("initial"), [3; 100]).unwrap();
        let edited = receive(&live, |s| s.tree.root().size == 120);
        assert!(!edited.reset);
        assert_eq!(edited.tree.find_path(&temp.0.join("initial")), Some(initial_id));
        assert_eq!(first.tree.root().size, 30);
        fs::rename(temp.0.join("during-scan"), temp.0.join("renamed")).unwrap();
        let renamed = receive(&live, |s| {
            s.tree.find_path(&temp.0.join("renamed")).is_some()
                && s.tree.find_path(&temp.0.join("during-scan")).is_none()
        });
        assert_eq!(renamed.tree.root().size, 120);
        #[cfg(windows)]
        {
            fs::rename(temp.0.join("renamed"), temp.0.join("RENAMED")).unwrap();
            let renamed_case = receive(&live, |s| {
                s.tree.find_path(&temp.0.join("RENAMED")).is_some()
                    && s.tree.find_path(&temp.0.join("renamed")).is_none()
            });
            assert_eq!(renamed_case.tree.root().files, 2);
            assert_eq!(renamed_case.tree.root().size, 120);
        }
        fs::create_dir_all(temp.0.join("new/nested")).unwrap();
        fs::write(temp.0.join("new/nested/data"), [4; 55]).unwrap();
        let added = receive(&live, |s| s.tree.root().size == 175);
        assert_eq!(added.dirs, 3);
        fs::remove_dir_all(temp.0.join("new")).unwrap();
        fs::remove_file(temp.0.join("initial")).unwrap();
        let removed = receive(&live, |s| s.tree.root().size == 20);
        assert_eq!(removed.tree.root().files, 1);
        assert_eq!(removed.dirs, 1);
        // Simulate the native zero-byte completion / OS overflow signal.
        live.inbox.send(Change::Rescan);
        let recovered = receive(&live, |s| s.reset);
        assert_eq!(recovered.tree.root().size, 20);
        // Drain the final status; idle watching must not publish periodic trees.
        while live.rx.recv_timeout(Duration::from_millis(1300)).is_ok() {}
        let worker = Arc::downgrade(&live.stop);
        drop(live);
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.upgrade().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(worker.upgrade().is_none(), "closing the document stops its worker");
    }

    #[test]
    fn burst_storage_is_bounded_and_duplicates_coalesce() {
        let root = Path::new("root");
        let mut pending = Pending::default();
        for _ in 0..100_000 {
            pending.add(Change::Path(root.join("same")), root);
        }
        assert_eq!(pending.paths.len(), 1);
        for i in 0..=CAPACITY {
            pending.add(Change::Path(root.join(i.to_string())), root);
        }
        assert!(pending.rescan);
        assert!(pending.paths.is_empty());
        let (tx, rx) = mpsc::sync_channel(2);
        let inbox = Inbox { tx, overflow: Arc::new(AtomicBool::new(false)), failed: Arc::new(Mutex::new(None)) };
        for _ in 0..1000 {
            inbox.send(Change::Path(root.join("same")));
        }
        inbox.send(Change::Failed("lost watcher".into()));
        assert_eq!(rx.try_iter().count(), 2);
        assert!(inbox.overflow.load(Ordering::Acquire));
        assert_eq!(clawback_core::scan::lock(&inbox.failed).as_deref(), Some("lost watcher"));
    }
}
