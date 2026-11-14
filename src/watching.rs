//! Bounded notifications and background-only incremental reconciliation.
use crate::platform::{self, DiskInfo};
use clawback_core::{Scan, ScanOptions, Skipped, Tree, live::Refresh};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Individual paths kept before falling back to relisting their directories.
const CAPACITY: usize = 4096;
/// Directories kept before falling back to a full rescan.
const DIRECTORIES: usize = 16_384;
const BATCH: Duration = Duration::from_secs(1);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

fn stopped(error: impl std::fmt::Display) -> Update {
    Update::Status(LiveStatus::Stopped(error.to_string()))
}

/// What live watching is doing; [`LiveStatus::text`] renders it for both frontends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveStatus {
    Watching,
    CatchingUp,
    Reconciling,
    /// The watcher failed; a rescan reconnects.
    Stopped(String),
    /// The live worker went away without reporting why.
    Disconnected,
    /// Watching could not start.
    Unavailable(String),
    /// The scan was cancelled, so there is no complete tree to keep live.
    Cancelled,
    /// The screenshot fixture's fictional data.
    #[cfg(feature = "screenshots")]
    Demo,
}
impl LiveStatus {
    /// Changes are being followed, as opposed to stopped or never started.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Watching | Self::CatchingUp | Self::Reconciling)
    }
    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped(_))
    }
    pub fn text(&self) -> String {
        use crate::i18n::tr;
        match self {
            Self::Watching => tr!("live-watching"),
            Self::CatchingUp => tr!("live-catching-up"),
            Self::Reconciling => tr!("live-reconciling"),
            Self::Stopped(error) => tr!("live-stopped", error = error.as_str()),
            Self::Disconnected => tr!("live-disconnected"),
            Self::Unavailable(error) => tr!("live-unavailable", error = error.as_str()),
            Self::Cancelled => tr!("live-scan-cancelled"),
            #[cfg(feature = "screenshots")]
            Self::Demo => tr!("live-demo"),
        }
    }
}

pub enum Change {
    Path(PathBuf),
    Rescan,
    Failed(String),
}

/// Changes not yet taken by the live worker, coalesced as they arrive. Nothing drains it
/// during the initial scan, so a queue of individual events would overflow on a busy disk
/// and force a full rescan, which holds a second complete tree while it runs.
#[derive(Clone)]
pub struct Inbox(Arc<Queue>);
struct Queue {
    root: PathBuf,
    pending: Mutex<Pending>,
    ready: Condvar,
}
impl Inbox {
    fn new(root: &Path) -> Self {
        Self(Arc::new(Queue { root: root.to_path_buf(), pending: Mutex::default(), ready: Condvar::new() }))
    }
    pub fn send(&self, change: Change) {
        clawback_core::scan::lock(&self.0.pending).add(change, &self.0.root);
        self.0.ready.notify_one();
    }
    /// Everything pending, waiting up to `timeout` for the first change.
    fn take(&self, timeout: Duration) -> Pending {
        let mut pending = clawback_core::scan::lock(&self.0.pending);
        if pending.is_empty() && !timeout.is_zero() {
            pending = self.0.ready.wait_timeout(pending, timeout).unwrap_or_else(PoisonError::into_inner).0;
        }
        std::mem::take(&mut *pending)
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
    Status(LiveStatus),
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
    pub status: LiveStatus,
}
impl Started {
    pub fn unavailable(error: impl std::fmt::Display) -> Self {
        Self { live: None, status: LiveStatus::Unavailable(error.to_string()) }
    }
    pub fn cancelled() -> Self {
        Self { live: None, status: LiveStatus::Cancelled }
    }
}

pub struct Watch {
    #[cfg(windows)]
    _watcher: crate::watch_windows::Watcher,
    #[cfg(not(windows))]
    _watcher: notify::RecommendedWatcher,
    inbox: Inbox,
}

impl Watch {
    /// Called before the initial scan, exclusively on the coordinator thread.
    pub fn start(root: &Path) -> io::Result<Self> {
        // In the scanner's namespace, which is where event paths are reported.
        let inbox = Inbox::new(&std::path::absolute(root)?);
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
        Ok(Self { _watcher: watcher, inbox })
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
            Ok(_) => Started { live: Some(Live { rx, inbox, stop }), status: LiveStatus::Watching },
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
                pending = self.inbox.take(BATCH);
                if pending.is_empty() {
                    continue;
                }
            }
            // Fixed window: sustained activity cannot postpone updates forever.
            let deadline = Instant::now() + BATCH;
            while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
            }
            if stop.load(Ordering::Relaxed) {
                return;
            }
            pending.merge(self.inbox.take(Duration::ZERO), &root);
            if let Some(error) = pending.failed.take() {
                publish(stopped(error));
                return;
            }
            let mut changed = false;
            let mut reset = false;
            if pending.rescan {
                if last_recovery.is_some_and(|last| last.elapsed() < RECOVERY_INTERVAL) {
                    continue;
                }
                if !publish(Update::Status(LiveStatus::Reconciling)) {
                    return;
                }
                pending.clear();
                // The app's snapshot keeps what it shows; release this copy during the scan.
                tree = Tree::new(&root);
                drop(std::mem::replace(&mut refresh, Refresh::new(&tree, options.clone())));
                let mut opts = options.clone();
                opts.threads = 1;
                let result = Scan::start(&root, opts, None).map(|scan| scan.wait_or_stop(stop));
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
                    let Some(path) = pending.pop() else { break };
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
            // Edits leave replaced nodes detached; drop them once they are a fifth of the tree.
            if tree.len() as u64 > (tree.root().files + refresh.dirs).max(4096) * 5 / 4 {
                tree = std::mem::replace(&mut tree, Tree::new(&root)).compacted();
                refresh = Refresh::new(&tree, options.clone());
                changed = true;
                reset = true;
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
            if changed || pending.rescan {
                let status = if pending.is_empty() { LiveStatus::Watching } else { LiveStatus::CatchingUp };
                if !publish(Update::Status(status)) {
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
    /// Paths as reported. An existing directory's own notification reconciles only its
    /// immediate children, so descendant events are retained individually.
    paths: BTreeSet<PathBuf>,
    /// Directories whose children are relisted in place of their individual events once
    /// there are too many of those. They are never coarsened again, as that would lose
    /// changes further down.
    dirs: BTreeSet<PathBuf>,
    rescan: bool,
    failed: Option<String>,
}
impl Pending {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.dirs.is_empty() && !self.rescan && self.failed.is_none()
    }
    fn add(&mut self, change: Change, root: &Path) {
        match change {
            Change::Path(path) if path.starts_with(root) => self.insert(path, root),
            Change::Rescan => {
                self.clear();
                self.rescan = true;
            }
            Change::Failed(error) => self.failed = Some(error),
            Change::Path(_) => {}
        }
    }
    fn insert(&mut self, path: PathBuf, root: &Path) {
        if self.rescan || self.dirs.contains(&path) {
            return;
        }
        self.paths.insert(path);
        if self.paths.len() <= CAPACITY {
            return;
        }
        for path in std::mem::take(&mut self.paths) {
            let dir = if path == root { path } else { path.parent().map_or(path.clone(), Path::to_path_buf) };
            self.dirs.insert(dir);
        }
        if self.dirs.len() > DIRECTORIES {
            self.clear();
            self.rescan = true;
        }
    }
    fn merge(&mut self, other: Self, root: &Path) {
        if other.rescan {
            self.clear();
            self.rescan = true;
        }
        self.failed = other.failed.or(self.failed.take());
        for dir in other.dirs {
            if !self.rescan {
                self.paths.remove(&dir);
                self.dirs.insert(dir);
            }
        }
        for path in other.paths {
            self.insert(path, root);
        }
        if self.dirs.len() > DIRECTORIES {
            self.clear();
            self.rescan = true;
        }
    }
    fn pop(&mut self) -> Option<PathBuf> {
        self.dirs.pop_first().or_else(|| self.paths.pop_first())
    }
    fn clear(&mut self) {
        self.paths.clear();
        self.dirs.clear();
        self.rescan = false;
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
                Update::Status(status) => assert!(!status.is_stopped(), "{status:?}"),
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
    fn bursts_coarsen_to_directories_before_rescanning() {
        let root = Path::new("root");
        let mut pending = Pending::default();
        for _ in 0..100_000 {
            pending.add(Change::Path(root.join("same")), root);
        }
        assert_eq!(pending.paths.len(), 1);
        // Many files in a few directories: relisting those directories covers them all.
        for i in 0..CAPACITY {
            pending.add(Change::Path(root.join(format!("d{}", i % 3)).join(i.to_string())), root);
        }
        assert!(!pending.rescan);
        assert!(pending.paths.is_empty());
        let dirs: BTreeSet<_> = [root.into(), root.join("d0"), root.join("d1"), root.join("d2")].into();
        assert_eq!(pending.dirs, dirs);
        pending.add(Change::Path(root.join("d1")), root);
        assert!(pending.paths.is_empty(), "already relisted");
        pending.add(Change::Path(root.join("d1/later")), root);
        assert_eq!(pending.paths.len(), 1, "coarsened directories are never coarsened again");
        // Directories are popped first: relisting them supersedes their children's events.
        assert_eq!(pending.pop(), Some(root.to_path_buf()));
        // Changes spread over too many directories fall back to a full rescan.
        for i in 0..=DIRECTORIES + CAPACITY {
            pending.add(Change::Path(root.join(format!("e{i}/file"))), root);
        }
        assert!(pending.rescan);
        assert!(pending.paths.is_empty() && pending.dirs.is_empty());
        pending.add(Change::Path(Path::new("elsewhere/file").into()), root);
        assert!(pending.paths.is_empty());
    }

    #[test]
    fn the_inbox_coalesces_until_taken_and_merges_into_the_backlog() {
        let root = Path::new("root");
        let inbox = Inbox::new(root);
        for _ in 0..1000 {
            inbox.send(Change::Path(root.join("same")));
        }
        inbox.send(Change::Path(Path::new("outside").into()));
        inbox.send(Change::Failed("lost watcher".into()));
        let taken = inbox.take(Duration::ZERO);
        assert_eq!(taken.paths.len(), 1);
        assert_eq!(taken.failed.as_deref(), Some("lost watcher"));
        assert!(inbox.take(Duration::from_millis(10)).is_empty());

        let mut backlog = Pending::default();
        backlog.add(Change::Path(root.join("d/file")), root);
        let mut burst = Pending::default();
        for i in 0..=CAPACITY {
            burst.add(Change::Path(root.join(format!("d/{i}"))), root);
        }
        backlog.merge(burst, root);
        assert_eq!(backlog.dirs.len(), 1);
        assert_eq!(backlog.paths.len(), 1);
        let mut rescan = Pending::default();
        rescan.add(Change::Rescan, root);
        backlog.merge(rescan, root);
        assert!(backlog.rescan && backlog.paths.is_empty() && backlog.dirs.is_empty());
    }
}
