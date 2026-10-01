//! Parallel filesystem scanner.
//!
//! A pool of worker threads pulls directories from a shared LIFO queue, lists
//! them, and appends the entries to a shared [`Tree`] under a short-lived lock.
//! Because sizes are propagated to ancestors immediately, the tree can be
//! displayed while the scan is running.
//!
//! Symlinks (and Windows junctions) are never followed. Directories that
//! cannot be read are recorded in the skip list rather than aborting the scan.

use crate::adaptive::{Controller, Sample, StorageKind};
use crate::profiling::Phase;
use crate::tree::{FileId, Kind, NewEntry, NodeId, ROOT, Tree, flags};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use crate::sync::lock;

/// Saturating nanoseconds for atomic counters.
pub(crate) fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    /// Do not descend into directories that live on a different filesystem
    /// (mount points, other volumes). Unix only; Windows never follows
    /// mounted-folder junctions anyway.
    pub one_filesystem: bool,
    /// Report file lengths instead of allocation (estimated by cluster rounding
    /// for Windows directory traversal).
    pub apparent_size: bool,
    /// Count hard-linked files only once when the backend supplies identities.
    pub dedupe_hardlinks: bool,
    /// Skip pseudo filesystems such as `/proc`, `/sys` and `/dev`.
    pub skip_virtual: bool,
    /// Worker threads. 0 = adaptive; a nonzero value fixes concurrency.
    pub threads: usize,
    /// Initial concurrency hint. Measured performance controls later changes.
    pub storage: StorageKind,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            one_filesystem: true,
            apparent_size: false,
            dedupe_hardlinks: true,
            skip_virtual: true,
            threads: 0,
            storage: StorageKind::Unknown,
        }
    }
}

impl ScanOptions {
    /// Maximum pool size. Automatic scans enable only a measured subset.
    pub fn thread_count(&self) -> usize {
        if self.threads > 0 {
            return self.threads;
        }
        // CPU availability bounds the pool, not the initial active count.
        let cpus = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        (cpus * 2).clamp(2, 32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The directory could not be listed because of permissions.
    PermissionDenied,
    /// A mount point / other volume, not scanned because of `one_filesystem`.
    OtherFilesystem,
    /// Any other I/O error.
    Error,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::PermissionDenied => "Permission denied",
            SkipReason::OtherFilesystem => "Other filesystem",
            SkipReason::Error => "Error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
    pub detail: String,
}

/// Live counters, readable from any thread while the scan runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
    pub workers: AtomicU64,
    cancelled: AtomicBool,
    pub done: AtomicBool,
    paused: AtomicBool,
    pause_lock: Mutex<()>,
    resume: Condvar,
    current: Mutex<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub denied: u64,
    /// Current concurrency limit, including workers waiting for directories.
    pub workers: u64,
}

impl Progress {
    /// Pause cooperatively between filesystem operations, preserving all work.
    pub fn set_paused(&self, paused: bool) {
        let _guard = lock(&self.pause_lock);
        self.paused.store(paused, Ordering::Relaxed);
        if !paused {
            self.resume.notify_all();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Stop cooperatively, releasing any paused workers.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.set_paused(false);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Wait without holding tree or queue locks. Cancellation always wins.
    pub(crate) fn wait_if_paused(&self) {
        if !self.is_paused() {
            return;
        }
        let guard = lock(&self.pause_lock);
        let _guard = self
            .resume
            .wait_while(guard, |()| self.is_paused() && !self.is_cancelled())
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// A pause point that fails once the scan is cancelled.
    #[cfg(windows)]
    pub(crate) fn checkpoint(&self) -> io::Result<()> {
        self.wait_if_paused();
        if self.is_cancelled() { Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled")) } else { Ok(()) }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            denied: self.denied.load(Ordering::Relaxed),
            workers: self.workers.load(Ordering::Relaxed),
        }
    }
    /// The directory most recently picked up by a worker.
    pub fn current_path(&self) -> PathBuf {
        lock(&self.current).clone()
    }
}

/// The stages of an MFT scan, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum MftPhase {
    #[default]
    Reading,
    /// Resolving file names.
    Resolving,
    Assembling,
    Sorting,
    /// The finished tree is being sent to another process.
    Transferring,
}

impl TryFrom<u64> for MftPhase {
    type Error = u64;
    fn try_from(value: u64) -> Result<Self, u64> {
        Ok(match value {
            0 => Self::Reading,
            1 => Self::Resolving,
            2 => Self::Assembling,
            3 => Self::Sorting,
            4 => Self::Transferring,
            _ => return Err(value),
        })
    }
}

/// Provisional MFT telemetry, separate from the validated tree and scan totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MftProgress {
    pub phase: MftPhase,
    pub read: u64,
    pub total: u64,
    pub records: u64,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

/// State shared between the scan threads and whoever is watching.
pub struct Shared {
    pub(crate) profile: crate::profiling::Metrics,
    pub root: PathBuf,
    pub options: ScanOptions,
    pub started: Instant,
    /// The tree being built. Lock briefly to draw a live view.
    pub tree: Mutex<Tree>,
    pub progress: Progress,
    pub mft_progress: Mutex<MftProgress>,
    pub skipped: Mutex<Vec<Skipped>>,
    elapsed: OnceLock<Duration>,
    pub(crate) backend: Mutex<ScanBackend>,
}

impl Shared {
    pub(crate) fn new(root: PathBuf, options: ScanOptions) -> Self {
        Self {
            profile: crate::profiling::Metrics::new(),
            tree: Mutex::new(Tree::new(&root)),
            root,
            options,
            started: Instant::now(),
            progress: Progress::default(),
            mft_progress: Mutex::default(),
            skipped: Mutex::new(Vec::new()),
            elapsed: OnceLock::new(),
            backend: Mutex::new(ScanBackend::Directory),
        }
    }

    /// Move the tree and totals out of a finished scan.
    fn take_result(&self) -> ScanResult {
        let snap = self.progress.snapshot();
        ScanResult {
            backend: *lock(&self.backend),
            root: self.root.clone(),
            tree: std::mem::replace(&mut *lock(&self.tree), Tree::placeholder()),
            skipped: std::mem::take(&mut *lock(&self.skipped)),
            cancelled: self.progress.is_cancelled(),
            elapsed: self.elapsed.get().copied().unwrap_or_else(|| self.started.elapsed()),
            files: snap.files,
            dirs: snap.dirs,
            bytes: snap.bytes,
            denied: snap.denied,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanBackend {
    Directory,
    NtfsMft,
}

pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// A running (or finished) scan.
pub struct Scan {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct ScanResult {
    pub backend: ScanBackend,
    pub root: PathBuf,
    pub tree: Tree,
    pub skipped: Vec<Skipped>,
    pub cancelled: bool,
    pub elapsed: Duration,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub denied: u64,
}

const MAX_SKIPPED_RECORDED: usize = 100_000;

/// Read-only, strict MFT scan for an elevated Windows helper. Never falls back
/// to directory traversal. The shared progress supports cooperative controls.
#[cfg(windows)]
pub struct MftScan {
    shared: Arc<Shared>,
}

#[cfg(windows)]
impl MftScan {
    pub fn new(root: &Path, options: ScanOptions) -> io::Result<Self> {
        Ok(Self { shared: Arc::new(Shared::new(std::path::absolute(root)?, options)) })
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub fn run(self) -> io::Result<ScanResult> {
        let s = &self.shared;
        let check = || {
            if s.progress.is_cancelled() {
                Err(io::Error::new(io::ErrorKind::Interrupted, "Turbo cancelled"))
            } else {
                Ok(())
            }
        };
        check()?;
        if !crate::ntfs::scan(s)? {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Turbo requires a whole local NTFS volume"));
        }
        check()?;
        *lock(&s.backend) = ScanBackend::NtfsMft;
        Ok(s.take_result())
    }
}

impl Scan {
    /// Start scanning `root` on background threads. `notify` is called once
    /// when the scan finishes (e.g. to wake a GUI event loop).
    pub fn start(root: impl AsRef<Path>, options: ScanOptions, notify: Option<Notify>) -> io::Result<Scan> {
        Self::start_with_accounting(root, options, notify, false)
    }

    /// Exact per-file queries are reserved for newly added subtrees in an
    /// existing MFT document, never ordinary directory scans.
    pub(crate) fn start_with_accounting(
        root: impl AsRef<Path>,
        options: ScanOptions,
        notify: Option<Notify>,
        exact: bool,
    ) -> io::Result<Scan> {
        let root = std::path::absolute(root.as_ref())?;
        let md = fs::metadata(&root)?;
        if !md.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a folder", root.display())));
        }
        let shared = Arc::new(Shared::new(root, options));
        let ctx_shared = shared.clone();
        let thread = std::thread::Builder::new().name("clawback-scan".into()).spawn(move || {
            run(&ctx_shared, &md, exact);
            let _ = ctx_shared.elapsed.set(ctx_shared.started.elapsed());
            ctx_shared.progress.done.store(true, Ordering::Release);
            if let Some(n) = notify {
                n();
            }
        })?;
        Ok(Scan { shared, thread: Some(thread) })
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub fn root(&self) -> &Path {
        &self.shared.root
    }

    pub fn is_finished(&self) -> bool {
        self.shared.progress.done.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.shared.progress.cancel();
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.progress.set_paused(paused);
    }

    /// Block until the scan is complete and take the result.
    pub fn wait(mut self) -> ScanResult {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.shared.take_result()
    }

    /// Like [`wait`](Self::wait), but cancel as soon as `stop` is set.
    pub fn wait_or_stop(self, stop: &AtomicBool) -> ScanResult {
        while !self.is_finished() {
            if stop.load(Ordering::Relaxed) {
                self.cancel();
                break;
            }
            std::thread::sleep(STOP_POLL);
        }
        self.wait()
    }
}

/// How often [`Scan::wait_or_stop`] checks its stop flag.
const STOP_POLL: Duration = Duration::from_millis(20);

impl Drop for Scan {
    fn drop(&mut self) {
        if self.thread.is_some() {
            // Dropped without waiting: stop the workers promptly.
            self.cancel();
        }
    }
}

/// Scan synchronously.
pub fn scan(root: impl AsRef<Path>, options: ScanOptions) -> io::Result<ScanResult> {
    Ok(Scan::start(root, options, None)?.wait())
}

struct Job {
    node: NodeId,
    path: PathBuf,
}

struct Queue {
    jobs: Vec<Job>,
    /// Jobs queued plus jobs being processed. Zero means we're done.
    outstanding: usize,
    active: usize,
    limit: usize,
}

impl Queue {
    /// Drop everything still queued; used once the scan is cancelled.
    fn abandon(&mut self) {
        self.outstanding -= self.jobs.len();
        self.jobs.clear();
    }
}

/// Listings are published in batches of up to `FLUSH_ENTRIES`, or sooner once
/// a slow batch is `FLUSH_INTERVAL` old (checked every `FLUSH_CLOCK_STRIDE`).
const FLUSH_ENTRIES: usize = 256;
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const FLUSH_CLOCK_STRIDE: usize = 16;

#[cfg(not(target_os = "macos"))]
type EntryMetadata = fs::Metadata;
#[cfg(target_os = "macos")]
type EntryMetadata = crate::macos::Metadata;

struct Ctx<'a> {
    shared: &'a Shared,
    queue: Mutex<Queue>,
    wake: Condvar,
    finished: Condvar,
    measured_entries: AtomicU64,
    work_nanos: AtomicU64,
    #[cfg(unix)]
    allowed_devs: Vec<u64>,
    excludes: Vec<PathBuf>,
    hardlinks: Mutex<HashSet<FileId>>,
    #[cfg(not(target_os = "macos"))]
    accounting: FileAccounting,
}

/// `exact` asks traversal for exact per-file accounting; see [`FileAccounting::new`].
pub(crate) fn run(shared: &Shared, root_md: &fs::Metadata, exact: bool) {
    shared.progress.wait_if_paused();
    if shared.progress.is_cancelled() {
        return;
    }
    lock(&shared.tree).node_mut(ROOT).mtime = mtime_secs(root_md);
    #[cfg(windows)]
    {
        let attempt = crate::ntfs::scan(shared);
        if let Err(error) = &attempt {
            shared.profile.fallback(error);
        }
        if attempt.unwrap_or(false) {
            *lock(&shared.backend) = ScanBackend::NtfsMft;
            return;
        }
        // MFT ingestion publishes only after validation. A failed attempt has
        // no partial tree to merge or double count; cancellation never retries.
        if shared.progress.is_cancelled() {
            return;
        }
    }
    run_directory(shared, root_md, exact);
}

pub(crate) fn run_directory(shared: &Shared, root_md: &fs::Metadata, exact: bool) {
    #[cfg(target_os = "macos")]
    let _ = exact; // Bulk listings carry their own allocation and identity.
    #[cfg(not(unix))]
    let _ = root_md; // Only Unix keeps scans to the root's devices.
    let maximum = shared.options.thread_count();
    let mut controller = (shared.options.threads == 0).then(|| Controller::new(shared.options.storage, maximum));
    let initial = controller.as_ref().map_or(maximum, Controller::limit);
    shared.progress.workers.store(initial as u64, Ordering::Relaxed);
    let ctx = Ctx {
        shared,
        queue: Mutex::new(Queue {
            jobs: vec![Job { node: ROOT, path: shared.root.clone() }],
            outstanding: 1,
            active: 0,
            limit: initial,
        }),
        wake: Condvar::new(),
        finished: Condvar::new(),
        measured_entries: AtomicU64::new(0),
        work_nanos: AtomicU64::new(0),
        #[cfg(unix)]
        allowed_devs: allowed_devices(&shared.root, root_md),
        excludes: if shared.options.skip_virtual { virtual_paths(&shared.root) } else { Vec::new() },
        hardlinks: Mutex::new(HashSet::new()),
        #[cfg(not(target_os = "macos"))]
        accounting: FileAccounting::new(&shared.root, exact),
    };
    std::thread::scope(|s| {
        let mut spawned = 0;
        for i in 0..initial {
            let ctx = &ctx;
            if std::thread::Builder::new()
                .name(format!("clawback-worker-{i}"))
                .spawn_scoped(s, move || worker(ctx))
                .is_ok()
            {
                spawned += 1;
            }
        }
        if spawned == 0 {
            lock(&ctx.queue).limit = 1;
            shared.progress.workers.store(1, Ordering::Relaxed);
            worker(&ctx);
            return;
        }
        let mut last = Instant::now();
        let (mut entries, mut work) = (0, 0);
        let mut queue = lock(&ctx.queue);
        if spawned < initial {
            if let Some(controller) = &mut controller {
                controller.restrict(spawned);
            }
            queue.limit = spawned;
            shared.progress.workers.store(spawned as u64, Ordering::Relaxed);
        }
        loop {
            if queue.outstanding == 0 {
                break;
            }
            if shared.progress.is_cancelled() {
                queue.abandon();
                ctx.wake.notify_all();
                if queue.outstanding == 0 {
                    break;
                }
            }
            queue = ctx.finished.wait_timeout(queue, Duration::from_secs(1)).unwrap_or_else(PoisonError::into_inner).0;
            let now = Instant::now();
            let next_entries = ctx.measured_entries.load(Ordering::Relaxed);
            let next_work = ctx.work_nanos.load(Ordering::Relaxed);
            if shared.progress.is_paused() {
                last = now;
                entries = next_entries;
                work = next_work;
                continue;
            }
            if let Some(controller) = &mut controller {
                let limit = controller.observe(Sample {
                    entries: next_entries.saturating_sub(entries),
                    work_nanos: next_work.saturating_sub(work),
                    elapsed: now.duration_since(last),
                    saturated: !queue.jobs.is_empty() && queue.active >= queue.limit,
                });
                // Create extra threads only when a measured trial needs them.
                // Previously created workers park when the limit falls.
                while spawned < limit {
                    let ctx_ref = &ctx;
                    if std::thread::Builder::new()
                        .name(format!("clawback-worker-{spawned}"))
                        .spawn_scoped(s, move || worker(ctx_ref))
                        .is_err()
                    {
                        controller.restrict(spawned);
                        break;
                    }
                    spawned += 1;
                }
                let previous_limit = queue.limit;
                queue.limit = limit.min(spawned);
                shared.progress.workers.store(queue.limit as u64, Ordering::Relaxed);
                for _ in previous_limit..queue.limit {
                    ctx.wake.notify_one();
                }
            }
            last = now;
            entries = next_entries;
            work = next_work;
        }
    });
    let _phase = shared.profile.timer(Phase::Sort);
    lock(&shared.tree).sort_all();
}

fn worker(ctx: &Ctx<'_>) {
    loop {
        let job = {
            let mut q = lock(&ctx.queue);
            loop {
                if ctx.shared.progress.is_cancelled() {
                    q.abandon();
                }
                if q.active < q.limit
                    && let Some(j) = q.jobs.pop()
                {
                    q.active += 1;
                    break j;
                }
                if ctx.finish_if_done(&q) {
                    return;
                }
                q = ctx.wake.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
        };
        ctx.shared.progress.wait_if_paused();
        let cancelled = ctx.shared.progress.is_cancelled();
        let mut new_jobs = if cancelled { Vec::new() } else { process(ctx, &job) };
        let mut q = lock(&ctx.queue);
        q.active -= 1;
        if ctx.shared.progress.is_cancelled() {
            q.abandon();
            new_jobs.clear();
        }
        let added = new_jobs.len();
        q.jobs.extend(new_jobs);
        q.outstanding = q.outstanding + added - 1;
        if !ctx.finish_if_done(&q) && !q.jobs.is_empty() {
            // This worker takes the next job itself. Wake only additional
            // available slots, not every parked thread on each directory.
            let slots = q.limit.saturating_sub(q.active).min(q.jobs.len());
            for _ in 1..slots {
                ctx.wake.notify_one();
            }
        }
    }
}

/// Listed entries not yet added to the shared tree.
struct Batch {
    entries: Vec<NewEntry>,
    /// Subdirectories to queue, by index into `entries`.
    subdirs: Vec<(usize, PathBuf)>,
    files: u64,
    bytes: u64,
    started: Instant,
}

impl Batch {
    fn new() -> Self {
        Self { entries: Vec::new(), subdirs: Vec::new(), files: 0, bytes: 0, started: Instant::now() }
    }

    fn is_due(&self) -> bool {
        let len = self.entries.len();
        len >= FLUSH_ENTRIES || (len.is_multiple_of(FLUSH_CLOCK_STRIDE) && self.started.elapsed() >= FLUSH_INTERVAL)
    }

    /// Add the entries under `node` and queue their subdirectories as jobs.
    fn flush(&mut self, ctx: &Ctx<'_>, node: NodeId, partial: bool, jobs: &mut Vec<Job>) {
        let shared = ctx.shared;
        let count = self.entries.len();
        let range = {
            let mut tree = lock(&shared.tree);
            if partial {
                tree.node_mut(node).flags |= flags::PARTIAL;
            }
            tree.add_children(node, std::mem::take(&mut self.entries))
        };
        jobs.extend(self.subdirs.drain(..).map(|(i, path)| Job { node: range.start + i as NodeId, path }));
        shared.progress.files.fetch_add(std::mem::take(&mut self.files), Ordering::Relaxed);
        shared.progress.bytes.fetch_add(std::mem::take(&mut self.bytes), Ordering::Relaxed);
        if count > 0 {
            ctx.record_work(count, self.started.elapsed());
        }
        self.started = Instant::now();
    }
}

/// Size accounting for one non-directory entry.
struct Measured {
    size: u64,
    len: u64,
    flags: u8,
    file_id: Option<FileId>,
}

impl Measured {
    const UNKNOWN: Self = Self { size: 0, len: 0, flags: flags::PARTIAL, file_id: None };
}

fn process(ctx: &Ctx<'_>, job: &Job) -> Vec<Job> {
    let shared = ctx.shared;
    if let Ok(mut cur) = shared.progress.current.try_lock() {
        cur.clone_from(&job.path);
    }
    let opened = Instant::now();
    #[cfg(not(target_os = "macos"))]
    let rd = fs::read_dir(&job.path);
    #[cfg(target_os = "macos")]
    let rd = crate::macos::read_dir(&job.path, &shared.progress.cancelled);
    ctx.record_work(1, opened.elapsed());
    let rd = match rd {
        Ok(rd) => rd,
        Err(e) => {
            if job.node != ROOT || e.kind() == io::ErrorKind::PermissionDenied {
                ctx.skip(&job.path, &e);
            }
            lock(&shared.tree).node_mut(job.node).flags |= flags::DENIED;
            shared.progress.dirs.fetch_add(1, Ordering::Relaxed);
            return Vec::new();
        }
    };

    let mut batch = Batch::new();
    let mut partial = false;
    let mut new_jobs = Vec::new();
    for ent in rd {
        if shared.progress.is_paused() {
            let paused_at = Instant::now();
            shared.progress.wait_if_paused();
            // Paused time is not filesystem latency for adaptive tuning.
            batch.started += paused_at.elapsed();
        }
        if shared.progress.is_cancelled() {
            break;
        }
        let Ok(ent) = ent else {
            partial = true;
            continue;
        };
        let Ok(ft) = ent.file_type() else {
            partial = true;
            continue;
        };
        let name = ent.file_name();
        // DirEntry::metadata never follows symlinks. On Windows it is free
        // (it comes from the directory listing); on Unix it is one lstat.
        let md = ent.metadata().ok();
        #[cfg(not(target_os = "macos"))]
        let mtime = md.as_ref().map_or(i64::MIN, mtime_secs);
        #[cfg(target_os = "macos")]
        let mtime = md.as_ref().map_or(i64::MIN, crate::macos::Metadata::mtime);
        if ft.is_dir() {
            let path = ent.path();
            let mut f = 0;
            if ctx.excludes.contains(&path) {
                f |= flags::VIRTUAL;
            } else if !ctx.same_filesystem(md.as_ref()) {
                f |= flags::OTHER_FS;
                ctx.record(Skipped {
                    path: path.clone(),
                    reason: SkipReason::OtherFilesystem,
                    detail: "mount point / other volume".into(),
                });
            }
            if f == 0 {
                batch.subdirs.push((batch.entries.len(), path));
            }
            batch.entries.push(NewEntry { name, kind: Kind::Dir, size: 0, len: 0, mtime, flags: f, file_id: None });
        } else {
            let m = md.as_ref().map_or(Measured::UNKNOWN, |md| ctx.measure(|| ent.path(), md));
            partial |= m.flags & flags::PARTIAL != 0;
            batch.files += 1;
            batch.bytes += m.size;
            batch.entries.push(NewEntry {
                name,
                kind: Kind::from(ft),
                size: m.size,
                len: m.len,
                mtime,
                flags: m.flags,
                file_id: m.file_id,
            });
        }
        if batch.is_due() {
            batch.flush(ctx, job.node, false, &mut new_jobs);
        }
    }
    batch.flush(ctx, job.node, partial, &mut new_jobs);
    shared.progress.dirs.fetch_add(1, Ordering::Relaxed);
    new_jobs
}

impl Ctx<'_> {
    /// Wake everyone once no work remains; true when the scan is done.
    fn finish_if_done(&self, queue: &Queue) -> bool {
        let done = queue.outstanding == 0;
        if done {
            self.wake.notify_all();
            self.finished.notify_one();
        }
        done
    }

    /// Size, identity and hard-link deduplication for a non-directory entry.
    fn measure(&self, path: impl Fn() -> PathBuf, md: &EntryMetadata) -> Measured {
        let options = &self.shared.options;
        #[cfg(not(target_os = "macos"))]
        let measured = {
            let exact_path = self.accounting.exact().then(&path);
            let _phase = self.shared.profile.timer(Phase::Metadata);
            self.accounting.measure(exact_path.as_deref().unwrap_or_else(|| Path::new("")), md, options.apparent_size)
        };
        #[cfg(target_os = "macos")]
        let measured: io::Result<_> =
            Ok((if options.apparent_size { md.len() } else { md.allocated() }, md.len(), Some((md.dev(), md.ino()))));
        match measured {
            Ok((size, len, file_id)) => {
                let duplicate = options.dedupe_hardlinks
                    && may_have_aliases(md)
                    && file_id.is_some_and(|key| !lock(&self.hardlinks).insert(key));
                Measured {
                    size: if duplicate { 0 } else { size },
                    len,
                    flags: if duplicate { flags::HARDLINK_DUP } else { 0 },
                    file_id,
                }
            }
            Err(error) => {
                self.skip(&path(), &error);
                Measured {
                    size: if options.apparent_size { md.len() } else { 0 },
                    len: md.len(),
                    flags: flags::PARTIAL,
                    file_id: None,
                }
            }
        }
    }

    fn record_work(&self, entries: usize, elapsed: Duration) {
        if self.shared.options.threads == 0 {
            self.measured_entries.fetch_add(entries as u64, Ordering::Relaxed);
            self.work_nanos.fetch_add(nanos(elapsed), Ordering::Relaxed);
        }
    }

    fn skip(&self, path: &Path, e: &io::Error) {
        let reason =
            if e.kind() == io::ErrorKind::PermissionDenied { SkipReason::PermissionDenied } else { SkipReason::Error };
        self.shared.progress.denied.fetch_add(1, Ordering::Relaxed);
        self.record(Skipped { path: path.to_path_buf(), reason, detail: e.to_string() });
    }

    fn record(&self, s: Skipped) {
        let mut list = lock(&self.shared.skipped);
        if list.len() < MAX_SKIPPED_RECORDED {
            list.push(s);
        }
    }

    #[cfg(unix)]
    fn same_filesystem(&self, md: Option<&EntryMetadata>) -> bool {
        #[cfg(not(target_os = "macos"))]
        use std::os::unix::fs::MetadataExt;
        // Unknown metadata: let read_dir report the real error.
        !self.shared.options.one_filesystem || md.is_none_or(|md| self.allowed_devs.contains(&md.dev()))
    }

    #[cfg(not(unix))]
    #[allow(clippy::unused_self)] // Same method interface as the Unix device check.
    fn same_filesystem(&self, _md: Option<&EntryMetadata>) -> bool {
        true
    }
}

/// Only files with several links can be counted more than once.
#[cfg(unix)]
fn may_have_aliases(md: &EntryMetadata) -> bool {
    #[cfg(not(target_os = "macos"))]
    use std::os::unix::fs::MetadataExt;
    md.nlink() > 1
}

#[cfg(not(unix))]
fn may_have_aliases(_md: &EntryMetadata) -> bool {
    true
}

/// Per-file size accounting. Volume geometry is resolved once per scan, not
/// once per file.
#[cfg(windows)]
pub(crate) struct FileAccounting {
    ntfs_cluster: Option<u64>,
    cluster: u64,
    exact: bool,
}

#[cfg(not(windows))]
pub(crate) struct FileAccounting;

#[cfg(windows)]
impl FileAccounting {
    /// `exact` queries each file's identity and allocation. Only incremental
    /// edits to a tree built from the MFT need that to preserve its totals.
    pub(crate) fn new(root: &Path, exact: bool) -> Self {
        let (cluster, ntfs_cluster) = crate::windows::clusters(root);
        Self { ntfs_cluster, cluster, exact }
    }

    pub(crate) fn exact(&self) -> bool {
        self.exact
    }

    /// Allocated/estimated (or apparent) bytes, length, and optional identity.
    pub(crate) fn measure(
        &self,
        path: &Path,
        md: &fs::Metadata,
        apparent: bool,
    ) -> io::Result<(u64, u64, Option<FileId>)> {
        if self.exact {
            let info = crate::windows::metadata(path, apparent, self.ntfs_cluster)?;
            return Ok((info.size, info.len, Some(info.id)));
        }
        let len = md.len();
        let size = if apparent { len } else { len.div_ceil(self.cluster).saturating_mul(self.cluster) };
        Ok((size, len, None))
    }
}

#[cfg(not(windows))]
#[allow(clippy::unused_self)] // Same interface as the Windows accounting.
impl FileAccounting {
    pub(crate) fn new(_root: &Path, _exact: bool) -> Self {
        Self
    }

    pub(crate) fn exact(&self) -> bool {
        false
    }

    /// Allocated (or apparent) bytes, length, and optional identity.
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn measure(
        &self,
        _path: &Path,
        md: &fs::Metadata,
        apparent: bool,
    ) -> io::Result<(u64, u64, Option<FileId>)> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok((if apparent { md.len() } else { md.blocks() * 512 }, md.len(), file_id(md)))
        }
        #[cfg(not(unix))]
        {
            let _ = apparent;
            Ok((md.len(), md.len(), None))
        }
    }
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)] // Matches the optional identity in the shared tree.
pub(crate) fn file_id(md: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((md.dev(), md.ino()))
}

#[cfg(unix)]
pub(crate) fn mtime_secs(md: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    md.mtime()
}

#[cfg(not(unix))]
pub(crate) fn mtime_secs(md: &fs::Metadata) -> i64 {
    use std::time::UNIX_EPOCH;
    match md.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        },
        Err(_) => i64::MIN,
    }
}

/// Filesystems considered "the same" as the root for `one_filesystem`.
#[cfg(unix)]
pub(crate) fn allowed_devices(root: &Path, root_md: &fs::Metadata) -> Vec<u64> {
    use std::os::unix::fs::MetadataExt;
    #[allow(unused_mut)]
    let mut devs = vec![root_md.dev()];
    // On macOS the boot disk is split into a read-only System volume mounted
    // at "/" and a Data volume whose folders (/Users, /Applications, ...) are
    // stitched in with firmlinks. Scanning "/" should show both.
    #[cfg(target_os = "macos")]
    if root == Path::new("/")
        && let Ok(md) = fs::metadata("/System/Volumes/Data")
    {
        devs.push(md.dev());
    }
    let _ = root;
    devs
}

/// Pseudo filesystems and double-mounted paths that should never be scanned.
pub(crate) fn virtual_paths(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if cfg!(any(target_os = "linux", target_os = "android")) {
        v.extend(["/proc", "/sys", "/dev"].map(PathBuf::from));
    } else if cfg!(target_os = "macos") {
        v.push(PathBuf::from("/dev"));
        if root == Path::new("/") {
            // The Data volume is reached through firmlinks; its real mount
            // point would count everything twice.
            v.push(PathBuf::from("/System/Volumes"));
        }
    } else if cfg!(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")) {
        v.extend(["/proc", "/dev"].map(PathBuf::from));
    }
    // Never exclude the root itself.
    v.retain(|p| p != root);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use std::sync::atomic::AtomicUsize;

    fn write(p: &Path, len: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; len]).unwrap();
    }

    fn apparent() -> ScanOptions {
        ScanOptions { apparent_size: true, ..Default::default() }
    }

    #[test]
    fn paused_scan_resumes_without_losing_files() {
        let d = TempDir::new("pause_resume");
        for i in 0..32 {
            write(&d.0.join(format!("dir{i}/file")), 100);
        }
        let shared = Arc::new(Shared::new(d.0.clone(), ScanOptions { threads: 4, ..apparent() }));
        shared.progress.set_paused(true);
        let worker_shared = shared.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            run(&worker_shared, &fs::metadata(&worker_shared.root).unwrap(), false);
            tx.send(()).unwrap();
        });
        let blocked =
            matches!(rx.recv_timeout(Duration::from_millis(100)), Err(std::sync::mpsc::RecvTimeoutError::Timeout));
        let before = shared.progress.snapshot();
        // Resume before asserting so a failed assertion cannot leave parked workers.
        shared.progress.set_paused(false);
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        thread.join().unwrap();
        assert!(blocked);
        assert_eq!(before.files, 0);
        assert_eq!(shared.progress.snapshot().files, 32);
        assert_eq!(lock(&shared.tree).root().size, 3200);
        assert!(!shared.progress.is_cancelled());
    }

    #[test]
    fn cancellation_releases_paused_scan() {
        let d = TempDir::new("pause_cancel");
        write(&d.0.join("file"), 100);
        let shared = Arc::new(Shared::new(d.0.clone(), apparent()));
        shared.progress.set_paused(true);
        let worker_shared = shared.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            run(&worker_shared, &fs::metadata(&worker_shared.root).unwrap(), false);
            tx.send(()).unwrap();
        });
        shared.progress.cancel();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        thread.join().unwrap();
        assert_eq!(shared.progress.snapshot().files, 0);
    }

    #[test]
    fn scans_nested_tree_with_exact_sizes() {
        let d = TempDir::new("nested");
        write(&d.0.join("a/one.bin"), 1000);
        write(&d.0.join("a/b/two.bin"), 2000);
        write(&d.0.join("a/b/c/three.bin"), 3000);
        write(&d.0.join("top.txt"), 10);
        fs::create_dir_all(d.0.join("empty")).unwrap();
        // Many directories to exercise the parallel queue.
        for i in 0..200 {
            write(&d.0.join(format!("many/d{i}/f")), i);
        }
        let r = scan(&d.0, ScanOptions { threads: 8, ..apparent() }).unwrap();
        let expect_many: u64 = (0..200).sum();
        assert!(!r.cancelled);
        assert_eq!(r.tree.root().size, 6010 + expect_many);
        assert_eq!(r.tree.root().files, 204);
        assert_eq!(r.files, 204);
        assert_eq!(r.dirs, 1 + 3 + 1 + 1 + 200); // root, a, a/b, a/b/c, empty, many, many/d*
        let b = r.tree.find_path(&d.0.join("a/b")).unwrap();
        assert_eq!(r.tree.node(b).size, 5000);
        // Children sorted largest-first.
        let sizes: Vec<u64> = r.tree.root().children.iter().map(|&c| r.tree.node(c).size).collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    }

    #[test]
    fn rejects_non_directory_root() {
        let d = TempDir::new("file");
        write(&d.0.join("f"), 1);
        assert!(scan(d.0.join("f"), apparent()).is_err());
        assert!(scan(d.0.join("missing"), apparent()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks_and_dedupes_hardlinks() {
        let d = TempDir::new("links");
        write(&d.0.join("real/big.bin"), 50_000);
        std::os::unix::fs::symlink(d.0.join("real"), d.0.join("loop")).unwrap();
        std::os::unix::fs::symlink(&d.0, d.0.join("real/cycle")).unwrap();
        fs::hard_link(d.0.join("real/big.bin"), d.0.join("hard.bin")).unwrap();

        let r = scan(&d.0, apparent()).unwrap();
        let link = r.tree.find_path(&d.0.join("loop")).unwrap();
        assert_eq!(r.tree.node(link).kind, Kind::Symlink);
        let real = r.tree.find_path(&d.0.join("real")).unwrap();
        let hard = r.tree.find_path(&d.0.join("hard.bin")).unwrap();
        let big = r.tree.find_path(&d.0.join("real/big.bin")).unwrap();
        // Exactly one of the two hard links carries the bytes.
        let counted = r.tree.node(hard).size + r.tree.node(big).size;
        assert_eq!(counted, 50_000);
        assert!(r.tree.node(hard).has(flags::HARDLINK_DUP) ^ r.tree.node(big).has(flags::HARDLINK_DUP));
        assert!(r.tree.node(real).size <= 50_000 + 4096);

        let no_dedupe = scan(&d.0, ScanOptions { dedupe_hardlinks: false, ..apparent() }).unwrap();
        let hard = no_dedupe.tree.find_path(&d.0.join("hard.bin")).unwrap();
        assert_eq!(no_dedupe.tree.node(hard).size, 50_000);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_is_skipped_not_fatal() {
        use std::os::unix::fs::PermissionsExt;
        let d = TempDir::new("perm");
        write(&d.0.join("locked/secret.bin"), 100);
        write(&d.0.join("open/ok.bin"), 200);
        fs::set_permissions(d.0.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let can_read_anyway = fs::read_dir(d.0.join("locked")).is_ok(); // e.g. running as root
        let r = scan(&d.0, apparent()).unwrap();
        fs::set_permissions(d.0.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        let locked = r.tree.find_path(&d.0.join("locked")).unwrap();
        if can_read_anyway {
            assert_eq!(r.tree.root().size, 300);
        } else {
            assert_eq!(r.tree.root().size, 200);
            assert!(r.tree.node(locked).has(flags::DENIED));
            assert_eq!(r.denied, 1);
            assert_eq!(r.skipped.len(), 1);
            assert_eq!(r.skipped[0].reason, SkipReason::PermissionDenied);
            assert_eq!(r.skipped[0].path, d.0.join("locked"));
        }
    }

    #[test]
    fn cancel_stops_early_and_live_tree_is_readable() {
        let d = TempDir::new("cancel");
        for i in 0..300 {
            write(&d.0.join(format!("x{i}/y/z/f")), 10);
        }
        for threads in [0, 8] {
            let s = Scan::start(&d.0, ScanOptions { threads, ..apparent() }, None).unwrap();
            // Reading the tree mid-scan and cancelling with parked workers is safe.
            let _ = lock(&s.shared().tree).root().size;
            s.cancel();
            let r = s.wait();
            assert!(r.cancelled);
            assert!(r.tree.root().size <= 3000);
        }
    }

    #[test]
    fn adaptive_and_fixed_scans_produce_identical_totals() {
        let d = TempDir::new("adaptive");
        for i in 0..48 {
            write(&d.0.join(format!("dir{i}/nested/file")), i + 1);
        }
        let fixed = Scan::start(&d.0, ScanOptions { threads: 1, ..apparent() }, None).unwrap();
        let shared = fixed.shared().clone();
        let fixed = fixed.wait();
        assert_eq!(shared.progress.snapshot().workers, 1, "explicit background limit remains fixed");
        for storage in [StorageKind::Unknown, StorageKind::Rotational, StorageKind::SolidState] {
            let adaptive = scan(&d.0, ScanOptions { storage, ..apparent() }).unwrap();
            assert_eq!((adaptive.files, adaptive.dirs, adaptive.bytes), (fixed.files, fixed.dirs, fixed.bytes));
            assert_eq!(adaptive.tree.root().size, fixed.tree.root().size);
        }
    }

    #[test]
    fn large_directory_batches_keep_all_files_and_subdirectories() {
        let d = TempDir::new("batches");
        for i in 0..600 {
            write(&d.0.join(format!("f{i}")), 3);
        }
        for i in 0..12 {
            write(&d.0.join(format!("dir{i}/nested")), 7);
        }
        let result = scan(&d.0, apparent()).unwrap();
        assert_eq!(result.files, 612);
        assert_eq!(result.dirs, 13);
        assert_eq!(result.bytes, 600 * 3 + 12 * 7);
        assert_eq!(result.tree.root().children.len(), 612);
        assert_eq!(result.tree.root().size, result.bytes);
        for i in 0..12 {
            assert!(result.tree.find_path(&d.0.join(format!("dir{i}/nested"))).is_some());
        }
    }

    #[test]
    fn notify_fires_once_on_completion() {
        let d = TempDir::new("notify");
        write(&d.0.join("f"), 1);
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let s = Scan::start(
            &d.0,
            apparent(),
            Some(Arc::new(move || {
                h.fetch_add(1, Ordering::SeqCst);
            })),
        )
        .unwrap();
        let r = s.wait();
        assert_eq!(r.tree.root().size, 1);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
