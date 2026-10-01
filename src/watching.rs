//! Bounded notifications and background-only incremental reconciliation.
use crate::platform::{self, DiskInfo};
use clawback_core::{Scan, ScanOptions, Skipped, Tree, live::Refresh};
use std::{
    collections::BTreeSet,
    io,
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
const WATCHING: &str = "Live · Watching for changes";

fn stopped(error: impl std::fmt::Display) -> Update {
    Update::Status(format!("Live stopped: {error} · Rescan to reconnect"))
}

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
    pub fn start(root: &Path) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(CAPACITY);
        let inbox = Inbox { tx, overflow: Arc::new(AtomicBool::new(false)), failed: Arc::new(Mutex::new(None)) };
        #[cfg(windows)]
        let watcher = crate::watch_windows::Watcher::start(root, inbox.clone())?;
        #[cfg(not(windows))]
        let watcher = {
            use notify::Watcher;
            let events = inbox.clone();
            // Native backends (notably FSEvents) report resolved paths. Keep
            // events in the same namespace as the scanner, including aliases
            // such as /var -> /private/var. Resolve only the root, never an
            // individual event: deleted and renamed paths may no longer exist.
            let scan_root = std::path::absolute(root)?;
            let watched_root = root.canonicalize()?;
            let event_root = watched_root.clone();
            let mut watcher = notify::RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| match event {
                    Ok(event) if event.need_rescan() => events.send(Change::Rescan),
                    Ok(event) => {
                        if matches!(event.kind, notify::EventKind::Access(_)) {
                            return;
                        }
                        for path in event.paths {
                            if let Some(path) = remap_event_path(&path, &event_root, &scan_root) {
                                events.send(Change::Path(path));
                            }
                        }
                    }
                    Err(error) => events.send(Change::Failed(error.to_string())),
                },
                notify::Config::default().with_follow_symlinks(false),
            )
            .map_err(io::Error::other)?;
            watcher.watch(&watched_root, notify::RecursiveMode::Recursive).map_err(io::Error::other)?;
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
        let baseline = Baseline { tree, options, skipped, disk };
        match std::thread::Builder::new().name("clawback-live".into()).spawn(move || {
            self.run(baseline, &cancel, &tx, &*repaint);
        }) {
            Ok(_) => Started { live: Some(Live { rx, inbox, stop }), status: WATCHING.into() },
            Err(error) => Started::unavailable(error),
        }
    }

    fn run(self, baseline: Baseline, stop: &AtomicBool, tx: &mpsc::SyncSender<Update>, repaint: &dyn Fn()) {
        let Baseline { mut tree, options, skipped, mut disk } = baseline;
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
                publish(stopped(error));
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
                        publish(stopped(error));
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
                let status = if pending.is_empty() { WATCHING } else { "Live · Catching up" };
                if !publish(Update::Status(status.into())) {
                    return;
                }
            }
        }
    }
}

/// What the initial scan hands to the live worker.
struct Baseline {
    tree: Tree,
    options: ScanOptions,
    skipped: Vec<Skipped>,
    disk: Option<DiskInfo>,
}

#[cfg(any(not(windows), test))]
fn remap_event_path(path: &Path, watched_root: &Path, scan_root: &Path) -> Option<PathBuf> {
    path.strip_prefix(watched_root)
        .or_else(|_| path.strip_prefix(scan_root))
        .ok()
        .map(|relative| scan_root.join(relative))
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
    fn receive(live: &Live, stage: &str, condition: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match live
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("live update deadline during {stage}: {error}"))
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
        #[cfg(unix)]
        let root = {
            let actual = temp.0.join("actual");
            fs::create_dir_all(actual.join("watched")).unwrap();
            let alias = temp.0.join("alias");
            std::os::unix::fs::symlink(&actual, &alias).unwrap();
            alias.join("watched")
        };
        #[cfg(not(unix))]
        let root = temp.0.clone();
        let options =
            ScanOptions { apparent_size: true, dedupe_hardlinks: false, threads: 1, ..ScanOptions::default() };
        fs::write(root.join("initial"), [1; 10]).unwrap();
        let watcher = Watch::start(&root).unwrap();
        let result = clawback_core::scan::scan(&root, options.clone()).unwrap();
        // This event occurs after the baseline scan, before the live worker starts.
        fs::write(root.join("during-scan"), [2; 20]).unwrap();
        let live = watcher.live(result.tree, options, Vec::new(), None, Arc::new(|| {})).live.unwrap();
        let first = receive(&live, "scan-gap creation", |s| s.tree.root().size == 30);
        let initial_id = first.tree.find_path(&root.join("initial")).unwrap();
        fs::write(root.join("initial"), [3; 100]).unwrap();
        let edited = receive(&live, "file edit", |s| s.tree.root().size == 120);
        assert!(!edited.reset);
        assert_eq!(edited.tree.find_path(&root.join("initial")), Some(initial_id));
        assert_eq!(first.tree.root().size, 30);
        fs::rename(root.join("during-scan"), root.join("renamed")).unwrap();
        let renamed = receive(&live, "rename", |s| {
            s.tree.find_path(&root.join("renamed")).is_some() && s.tree.find_path(&root.join("during-scan")).is_none()
        });
        assert_eq!(renamed.tree.root().size, 120);
        #[cfg(windows)]
        {
            fs::rename(root.join("renamed"), root.join("RENAMED")).unwrap();
            let renamed_case = receive(&live, "case-only rename", |s| {
                s.tree.find_path(&root.join("RENAMED")).is_some() && s.tree.find_path(&root.join("renamed")).is_none()
            });
            assert_eq!(renamed_case.tree.root().files, 2);
            assert_eq!(renamed_case.tree.root().size, 120);
        }
        fs::create_dir_all(root.join("new/nested")).unwrap();
        fs::write(root.join("new/nested/data"), [4; 55]).unwrap();
        let added = receive(&live, "new nested directory", |s| s.tree.root().size == 175);
        assert_eq!(added.dirs, 3);
        fs::remove_dir_all(root.join("new")).unwrap();
        fs::remove_file(root.join("initial")).unwrap();
        let removed = receive(&live, "deletion", |s| s.tree.root().size == 20);
        assert_eq!(removed.tree.root().files, 1);
        assert_eq!(removed.dirs, 1);
        // Simulate the native zero-byte completion / OS overflow signal.
        live.inbox.send(Change::Rescan);
        let recovered = receive(&live, "overflow recovery", |s| s.reset);
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
    fn native_paths_map_to_scan_alias_even_after_deletion() {
        let native = Path::new("/private/var/demo");
        let scanned = Path::new("/var/demo");
        assert_eq!(remap_event_path(&native.join("gone/file"), native, scanned), Some(scanned.join("gone/file")));
        assert_eq!(remap_event_path(native, native, scanned), Some(scanned.to_path_buf()));
        assert_eq!(remap_event_path(&scanned.join("file"), native, scanned), Some(scanned.join("file")));
        assert_eq!(remap_event_path(Path::new("/private/var/demo-other/file"), native, scanned), None);
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
