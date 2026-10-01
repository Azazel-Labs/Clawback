//! Opt-in core-only profiling; normal builds get zero-sized no-op timers.
#[cfg(not(feature = "profiling"))]
pub(crate) use disabled::*;
#[cfg(feature = "profiling")]
pub use enabled::*;

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

#[cfg(not(feature = "profiling"))]
mod disabled {
    use super::Phase;

    pub(crate) struct Metrics;

    pub(crate) struct Timer;

    // Dropping a timer ends its phase, as in profiling builds.
    impl Drop for Timer {
        fn drop(&mut self) {}
    }

    #[allow(clippy::unused_self)]
    impl Metrics {
        pub(crate) fn new() -> Self {
            Self
        }

        #[inline]
        pub(crate) fn timer(&self, _phase: Phase) -> Timer {
            Timer
        }

        #[cfg_attr(not(windows), allow(dead_code))]
        #[inline]
        pub(crate) fn fallback(&self, _error: &std::io::Error) {}
    }
}

#[cfg(feature = "profiling")]
mod enabled {
    use super::Phase;
    use crate::ScanOptions;
    use crate::scan::{self, Shared, lock, nanos};
    use std::fmt::{self, Write};
    use std::io;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    const COUNT: usize = Phase::Publish as usize + 1;
    const NAMES: [&str; COUNT] =
        ["volume", "bootstrap", "read", "parse", "merge", "index", "assemble", "sort", "metadata", "publish"];

    #[derive(Default)]
    pub(crate) struct Metrics {
        nanos: [AtomicU64; COUNT],
        calls: [AtomicU64; COUNT],
        fallback: Mutex<Option<String>>,
    }

    pub(crate) struct Timer<'a> {
        metrics: &'a Metrics,
        phase: usize,
        start: Instant,
    }

    impl Metrics {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn timer(&self, phase: Phase) -> Timer<'_> {
            Timer { metrics: self, phase: phase as usize, start: Instant::now() }
        }

        /// Why the MFT scan fell back to directory traversal.
        #[cfg_attr(not(windows), allow(dead_code))]
        pub(crate) fn fallback(&self, error: &io::Error) {
            *lock(&self.fallback) = Some(error.to_string());
        }

        pub(crate) fn fields(&self) -> String {
            let mut row = String::new();
            for (nanos, calls) in self.nanos.iter().zip(&self.calls) {
                let _ =
                    write!(row, ",{:.6},{}", nanos.load(Ordering::Relaxed) as f64 / 1e9, calls.load(Ordering::Relaxed));
            }
            row
        }
    }

    impl Drop for Timer<'_> {
        fn drop(&mut self) {
            self.metrics.nanos[self.phase].fetch_add(nanos(self.start.elapsed()), Ordering::Relaxed);
            self.metrics.calls[self.phase].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// `Mft` is strict: an unavailable or rejected MFT scan is never directory work.
    /// `Directory` bypasses MFT entirely; `Auto` uses the production selection.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Mode {
        Auto,
        Directory,
        Mft,
    }

    impl FromStr for Mode {
        type Err = io::Error;
        fn from_str(s: &str) -> io::Result<Self> {
            match s {
                "auto" => Ok(Self::Auto),
                "directory" => Ok(Self::Directory),
                "mft" => Ok(Self::Mft),
                _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "mode must be auto, directory or mft")),
            }
        }
    }

    impl fmt::Display for Mode {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Self::Auto => "auto",
                Self::Directory => "directory",
                Self::Mft => "mft",
            })
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

    /// Run one scan in `mode`, cancelling it after `timeout`, and return a CSV row.
    pub fn run(root: &Path, mode: Mode, options: &ScanOptions, timeout: Duration) -> io::Result<String> {
        let root = std::path::absolute(root)?;
        let md = std::fs::metadata(&root)?;
        if !md.is_dir() {
            return Err(io::Error::other("profile root is not a directory"));
        }
        let shared = Shared::new(root, options.clone());
        let start = Instant::now();
        let mut peak_workers = 0;
        let result = std::thread::scope(|s| {
            let task = s.spawn(|| -> io::Result<Duration> {
                let worker_start = Instant::now();
                match mode {
                    Mode::Auto => scan::run(&shared, &md, false),
                    Mode::Directory => scan::run_directory(&shared, &md, false),
                    Mode::Mft => {
                        #[cfg(windows)]
                        if !crate::ntfs::scan(&shared)? {
                            return Err(io::Error::other("root is not eligible for MFT scanning"));
                        }
                        #[cfg(not(windows))]
                        return Err(io::Error::new(io::ErrorKind::Unsupported, "MFT requires Windows"));
                    }
                }
                Ok(worker_start.elapsed())
            });
            while !task.is_finished() {
                peak_workers = peak_workers.max(shared.progress.workers.load(Ordering::Relaxed));
                if start.elapsed() >= timeout {
                    shared.progress.cancel();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            task.join().map_err(|_| io::Error::other("scan worker panicked"))
        })?;
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
            shared.progress.is_cancelled(),
            result.is_err(),
            fallback.is_some()
        );
        row.push_str(&shared.profile.fields());
        let backend = if mode == Mode::Mft && result.is_ok() {
            "NtfsMft".to_owned()
        } else if result.is_err() {
            "Failed".to_owned()
        } else {
            format!("{:?}", *lock(&shared.backend))
        };
        let _ = write!(row, ",{backend},{:?},{peak_workers}", options.storage);
        Ok(row)
    }
}
