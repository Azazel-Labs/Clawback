//! Opt-in core-only profiling; no timers or counters exist in normal builds.
use crate::ScanOptions;
use crate::scan::{self, Shared, lock};
use std::fmt::Write;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Phase {
    Volume,
    Bootstrap,
    Read,
    Parse,
    Merge,
    Index,
    Assemble,
    Sort,
    Metadata,
    Publish,
}
const NAMES: [&str; 10] =
    ["volume", "bootstrap", "read", "parse", "merge", "index", "assemble", "sort", "metadata", "publish"];

#[derive(Default)]
pub(crate) struct Metrics {
    nanos: [AtomicU64; 10],
    calls: [AtomicU64; 10],
    pub(crate) fallback: Mutex<Option<String>>,
}

pub(crate) struct Timer<'a> {
    metrics: &'a Metrics,
    phase: usize,
    start: Instant,
}
impl Metrics {
    pub(crate) fn timer(&self, phase: Phase) -> Timer<'_> {
        Timer { metrics: self, phase: phase as usize, start: Instant::now() }
    }

    pub(crate) fn fields(&self) -> String {
        let mut row = String::new();
        for index in 0..NAMES.len() {
            let _ = write!(
                row,
                ",{:.6},{}",
                self.nanos[index].load(Ordering::Relaxed) as f64 / 1e9,
                self.calls[index].load(Ordering::Relaxed)
            );
        }
        row
    }
}
impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.metrics.nanos[self.phase]
            .fetch_add(self.start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
        self.metrics.calls[self.phase].fetch_add(1, Ordering::Relaxed);
    }
}

/// CSV output separates wall time from summed worker time and reports errors.
pub fn header() -> String {
    let mut result = "mode,threads,apparent,wall_s,files,dirs,bytes,denied,cancelled,failed,fallback".to_owned();
    for name in NAMES {
        let _ = write!(result, ",{name}_s,{name}_calls");
    }
    result.push_str(",backend,storage,peak_workers");
    result
}

/// `mft` is strict: an unavailable or rejected MFT scan is never directory work.
/// `directory` bypasses MFT entirely; `auto` uses the production selection.
pub fn run(root: &Path, mode: &str, options: &ScanOptions, timeout: Duration) -> io::Result<String> {
    if !["auto", "directory", "mft"].contains(&mode) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "mode must be auto, directory or mft"));
    }
    let root = std::path::absolute(root)?;
    let md = std::fs::metadata(&root)?;
    if !md.is_dir() {
        return Err(io::Error::other("profile root is not a directory"));
    }
    let shared = Arc::new(Shared::new(root, options.clone()));
    let worker = shared.clone();
    let selected = mode.to_owned();
    let start = Instant::now();
    let task = std::thread::spawn(move || -> io::Result<Duration> {
        let worker_start = Instant::now();
        match selected.as_str() {
            "auto" => scan::run(&worker, &md),
            "directory" => scan::run_directory(&worker, &md),
            _ => {
                #[cfg(windows)]
                if !crate::ntfs::scan(&worker)? {
                    return Err(io::Error::other("root is not eligible for MFT scanning"));
                }
                #[cfg(not(windows))]
                return Err(io::Error::new(io::ErrorKind::Unsupported, "MFT requires Windows"));
            }
        }
        Ok(worker_start.elapsed())
    });
    let mut peak_workers = 0;
    while !task.is_finished() {
        peak_workers = peak_workers.max(shared.progress.workers.load(Ordering::Relaxed));
        if start.elapsed() >= timeout {
            shared.progress.cancel.store(true, Ordering::Relaxed);
            shared.progress.set_paused(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = task.join().map_err(|_| io::Error::other("scan worker panicked"))?;
    let wall = result.as_ref().copied().unwrap_or_else(|_| start.elapsed()).as_secs_f64();
    if let Err(error) = &result {
        eprintln!("{mode}: {error}");
    }
    let fallback = lock(&shared.profile.fallback);
    if let Some(error) = &*fallback {
        eprintln!("MFT fallback: {error}");
    }
    let p = shared.progress.snapshot();
    let mut row = format!(
        "{mode},{},{},{wall:.6},{},{},{},{},{},{},{}",
        options.threads,
        options.apparent_size,
        p.files,
        p.dirs,
        p.bytes,
        p.denied,
        shared.progress.cancel.load(Ordering::Relaxed),
        result.is_err(),
        fallback.is_some()
    );
    row.push_str(&shared.profile.fields());
    let backend = if mode == "mft" && result.is_ok() {
        "NtfsMft".to_owned()
    } else if result.is_err() {
        "Failed".to_owned()
    } else {
        format!("{:?}", *lock(&shared.backend))
    };
    let _ = write!(row, ",{backend},{:?},{peak_workers}", options.storage);
    Ok(row)
}
