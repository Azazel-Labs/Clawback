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
use crate::tree::{Kind, NewEntry, NodeId, ROOT, Tree, flags};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Lock a mutex, recovering from poisoning (a panicked worker must not take
/// the whole UI down with it).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    /// Do not descend into directories that live on a different filesystem
    /// (mount points, other volumes). Unix only; Windows never follows
    /// mounted-folder junctions anyway.
    pub one_filesystem: bool,
    /// Report file lengths instead of space actually allocated on disk.
    /// (Allocated size is only available on Unix; Windows always uses length.)
    pub apparent_size: bool,
    /// Count hard-linked files only once. Unix only.
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
    pub cancel: AtomicBool,
    pub done: AtomicBool,
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

/// State shared between the scan threads and whoever is watching.
pub struct Shared {
    pub root: PathBuf,
    pub options: ScanOptions,
    pub started: Instant,
    /// The tree being built. Lock briefly to draw a live view.
    pub tree: Mutex<Tree>,
    pub progress: Progress,
    pub skipped: Mutex<Vec<Skipped>>,
    elapsed: Mutex<Option<Duration>>,
}

pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// A running (or finished) scan.
pub struct Scan {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct ScanResult {
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

impl Scan {
    /// Start scanning `root` on background threads. `notify` is called once
    /// when the scan finishes (e.g. to wake a GUI event loop).
    pub fn start(root: impl AsRef<Path>, options: ScanOptions, notify: Option<Notify>) -> io::Result<Scan> {
        let root = std::path::absolute(root.as_ref())?;
        let md = fs::metadata(&root)?;
        if !md.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a folder", root.display())));
        }
        let shared = Arc::new(Shared {
            tree: Mutex::new(Tree::new(&root)),
            root: root.clone(),
            options,
            started: Instant::now(),
            progress: Progress::default(),
            skipped: Mutex::new(Vec::new()),
            elapsed: Mutex::new(None),
        });
        let ctx_shared = shared.clone();
        let thread = std::thread::Builder::new().name("clawback-scan".into()).spawn(move || {
            run(&ctx_shared, &md);
            *lock(&ctx_shared.elapsed) = Some(ctx_shared.started.elapsed());
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
        self.shared.progress.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared.progress.cancel.load(Ordering::Relaxed)
    }

    /// Block until the scan is complete and take the result.
    pub fn wait(mut self) -> ScanResult {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let s = &self.shared;
        let snap = s.progress.snapshot();
        let elapsed = lock(&s.elapsed).unwrap_or_else(|| s.started.elapsed());
        ScanResult {
            root: s.root.clone(),
            tree: std::mem::replace(&mut *lock(&s.tree), Tree::placeholder()),
            skipped: std::mem::take(&mut *lock(&s.skipped)),
            cancelled: s.progress.cancel.load(Ordering::Relaxed),
            elapsed,
            files: snap.files,
            dirs: snap.dirs,
            bytes: snap.bytes,
            denied: snap.denied,
        }
    }
}

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

struct Ctx<'a> {
    shared: &'a Shared,
    queue: Mutex<Queue>,
    wake: Condvar,
    finished: Condvar,
    measured_entries: AtomicU64,
    work_nanos: AtomicU64,
    #[cfg_attr(not(unix), allow(dead_code))]
    allowed_devs: Vec<u64>,
    excludes: Vec<PathBuf>,
    #[cfg_attr(not(unix), allow(dead_code))]
    hardlinks: Mutex<HashSet<(u64, u64)>>,
    #[cfg_attr(unix, allow(dead_code))]
    cluster: u64,
}

fn run(shared: &Shared, root_md: &fs::Metadata) {
    lock(&shared.tree).node_mut(ROOT).mtime = mtime_secs(root_md);
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
        allowed_devs: allowed_devices(&shared.root, root_md),
        excludes: if shared.options.skip_virtual { virtual_paths(&shared.root) } else { Vec::new() },
        hardlinks: Mutex::new(HashSet::new()),
        cluster: cluster_size(&shared.root),
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
            if shared.progress.cancel.load(Ordering::Relaxed) {
                queue.outstanding -= queue.jobs.len();
                queue.jobs.clear();
                ctx.wake.notify_all();
                if queue.outstanding == 0 {
                    break;
                }
            }
            queue = ctx
                .finished
                .wait_timeout(queue, Duration::from_secs(1))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
            let now = Instant::now();
            let next_entries = ctx.measured_entries.load(Ordering::Relaxed);
            let next_work = ctx.work_nanos.load(Ordering::Relaxed);
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
    lock(&shared.tree).sort_all();
}

fn worker(ctx: &Ctx<'_>) {
    loop {
        let job = {
            let mut q = lock(&ctx.queue);
            loop {
                if ctx.shared.progress.cancel.load(Ordering::Relaxed) {
                    q.outstanding -= q.jobs.len();
                    q.jobs.clear();
                }
                if q.active < q.limit
                    && let Some(j) = q.jobs.pop()
                {
                    q.active += 1;
                    break j;
                }
                if q.outstanding == 0 {
                    ctx.wake.notify_all();
                    ctx.finished.notify_one();
                    return;
                }
                q = ctx.wake.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let cancelled = ctx.shared.progress.cancel.load(Ordering::Relaxed);
        let mut new_jobs = if cancelled { Vec::new() } else { process(ctx, &job) };
        let mut q = lock(&ctx.queue);
        q.active -= 1;
        if ctx.shared.progress.cancel.load(Ordering::Relaxed) {
            // Drain: everything still queued is abandoned.
            q.outstanding -= q.jobs.len();
            q.jobs.clear();
            new_jobs.clear();
        }
        let added = new_jobs.len();
        q.jobs.extend(new_jobs);
        q.outstanding = q.outstanding + added - 1;
        if q.outstanding == 0 {
            ctx.wake.notify_all();
            ctx.finished.notify_one();
        } else if !q.jobs.is_empty() {
            // This worker takes the next job itself. Wake only additional
            // available slots, not every parked thread on each directory.
            let slots = q.limit.saturating_sub(q.active).min(q.jobs.len());
            for _ in 1..slots {
                ctx.wake.notify_one();
            }
        }
    }
}

fn process(ctx: &Ctx<'_>, job: &Job) -> Vec<Job> {
    let shared = ctx.shared;
    if let Ok(mut cur) = shared.progress.current.try_lock() {
        cur.clone_from(&job.path);
    }
    let opened = Instant::now();
    let rd = fs::read_dir(&job.path);
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

    let mut entries: Vec<NewEntry> = Vec::new();
    let mut subdirs: Vec<(usize, PathBuf)> = Vec::new();
    let mut partial = false;
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut new_jobs = Vec::new();
    let mut batch_started = Instant::now();

    for ent in rd {
        if shared.progress.cancel.load(Ordering::Relaxed) {
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
        let mtime = md.as_ref().map_or(i64::MIN, mtime_secs);
        if ft.is_dir() {
            let path = ent.path();
            let mut f = 0;
            if !ctx.excludes.is_empty() && ctx.excludes.iter().any(|e| e == &path) {
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
                subdirs.push((entries.len(), path));
            }
            entries.push(NewEntry { name, kind: Kind::Dir, size: 0, len: 0, mtime, flags: f, file_id: None });
        } else {
            let (size, len, f) = if let Some(md) = &md {
                ctx.file_size(md)
            } else {
                partial = true;
                (0, 0, 0)
            };
            let kind = if ft.is_symlink() {
                Kind::Symlink
            } else if ft.is_file() {
                Kind::File
            } else {
                Kind::Other
            };
            files += 1;
            bytes += size;
            entries.push(NewEntry { name, kind, size, len, mtime, flags: f, file_id: md.as_ref().and_then(file_id) });
        }
        if entries.len() >= 256
            || (entries.len().is_multiple_of(16) && batch_started.elapsed() >= Duration::from_millis(100))
        {
            let count = entries.len();
            let range = lock(&shared.tree).add_children(job.node, std::mem::take(&mut entries));
            new_jobs.extend(subdirs.drain(..).map(|(i, path)| Job { node: range.start + i as NodeId, path }));
            shared.progress.files.fetch_add(files, Ordering::Relaxed);
            shared.progress.bytes.fetch_add(bytes, Ordering::Relaxed);
            files = 0;
            bytes = 0;
            ctx.record_work(count, batch_started.elapsed());
            batch_started = Instant::now();
        }
    }

    let count = entries.len();
    let range = {
        let mut tree = lock(&shared.tree);
        if partial {
            tree.node_mut(job.node).flags |= flags::PARTIAL;
        }
        tree.add_children(job.node, entries)
    };
    if count > 0 {
        ctx.record_work(count, batch_started.elapsed());
    }
    let p = &shared.progress;
    p.dirs.fetch_add(1, Ordering::Relaxed);
    p.files.fetch_add(files, Ordering::Relaxed);
    p.bytes.fetch_add(bytes, Ordering::Relaxed);

    new_jobs.extend(subdirs.into_iter().map(|(i, path)| Job { node: range.start + i as NodeId, path }));
    new_jobs
}

impl Ctx<'_> {
    fn record_work(&self, entries: usize, elapsed: Duration) {
        if self.shared.options.threads == 0 {
            self.measured_entries.fetch_add(entries as u64, Ordering::Relaxed);
            self.work_nanos.fetch_add(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
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
    fn same_filesystem(&self, md: Option<&fs::Metadata>) -> bool {
        use std::os::unix::fs::MetadataExt;
        if !self.shared.options.one_filesystem {
            return true;
        }
        // Unknown metadata: let read_dir report the real error.
        md.is_none_or(|md| self.allowed_devs.contains(&md.dev()))
    }

    #[cfg(not(unix))]
    #[allow(clippy::unused_self)] // Same method interface as the Unix device check.
    fn same_filesystem(&self, _md: Option<&fs::Metadata>) -> bool {
        true
    }

    /// (layout size, actual length, flags) for a non-directory entry.
    #[cfg(unix)]
    fn file_size(&self, md: &fs::Metadata) -> (u64, u64, u8) {
        use std::os::unix::fs::MetadataExt;
        let opts = &self.shared.options;
        if opts.dedupe_hardlinks && md.nlink() > 1 && !lock(&self.hardlinks).insert((md.dev(), md.ino())) {
            return (0, md.len(), flags::HARDLINK_DUP);
        }
        let size = if opts.apparent_size { md.len() } else { md.blocks() * 512 };
        (size, md.len(), 0)
    }

    /// (layout size, actual length, flags) for a non-directory entry.
    /// Like SpaceMonger, on-disk size is the length rounded up to whole clusters.
    #[cfg(not(unix))]
    fn file_size(&self, md: &fs::Metadata) -> (u64, u64, u8) {
        let len = md.len();
        if self.shared.options.apparent_size || self.cluster <= 1 {
            return (len, len, 0);
        }
        (len.div_ceil(self.cluster) * self.cluster, len, 0)
    }
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)] // The Windows implementation has no inode identity.
pub(crate) fn file_id(md: &fs::Metadata) -> Option<crate::tree::FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((md.dev(), md.ino()))
}

#[cfg(not(unix))]
pub(crate) fn file_id(_md: &fs::Metadata) -> Option<crate::tree::FileId> {
    None
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

#[cfg(not(unix))]
pub(crate) fn allowed_devices(_root: &Path, _root_md: &fs::Metadata) -> Vec<u64> {
    Vec::new()
}

/// Allocation unit of the volume holding `path` (Windows only; Unix reports
/// allocated blocks directly).
#[cfg(windows)]
pub(crate) fn cluster_size(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetVolumePathNameW(file: *const u16, volume: *mut u16, len: u32) -> i32;
        fn GetDiskFreeSpaceW(root: *const u16, spc: *mut u32, bps: *mut u32, free: *mut u32, total: *mut u32) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut vol = [0u16; 1024];
    let (mut spc, mut bps, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
    // SAFETY: both buffers are NUL-terminated and sized as declared.
    let found = unsafe { GetVolumePathNameW(wide.as_ptr(), vol.as_mut_ptr(), vol.len() as u32) } != 0;
    // SAFETY: a successful lookup populated vol with a NUL-terminated path;
    // each output pointer refers to a live, writable u32.
    let ok = found
        && unsafe { GetDiskFreeSpaceW(vol.as_ptr(), &raw mut spc, &raw mut bps, &raw mut free, &raw mut total) } != 0;
    let c = u64::from(spc) * u64::from(bps);
    if ok && c > 0 { c } else { 4096 }
}

#[cfg(not(windows))]
pub(crate) fn cluster_size(_path: &Path) -> u64 {
    1
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
    use std::sync::atomic::AtomicUsize;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "clawback-test-{}-{}-{}",
                std::process::id(),
                tag,
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(self.0.join("locked"), fs::Permissions::from_mode(0o755));
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(p: &Path, len: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; len]).unwrap();
    }

    fn apparent() -> ScanOptions {
        ScanOptions { apparent_size: true, ..Default::default() }
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
