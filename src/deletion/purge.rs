//! Fast permanent deletion: files in parallel, then folders deepest-first.
//!
//! Links (symlinks, junctions) are removed themselves and never followed.
use super::Progress;
use clawback_core::scan::lock;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

/// Directories with more entries than this are split across workers.
const BATCH: usize = 256;

/// What a purge could not remove. Nothing here means everything went.
#[derive(Debug, Default)]
pub struct Report {
    pub failed: u64,
    pub first_error: Option<String>,
    pub cancelled: bool,
}

/// What to delete: `root` itself, or only its contents (keeping `keep` names at the top level).
pub struct Target<'a> {
    pub root: &'a Path,
    pub contents_only: bool,
    pub keep: &'a [&'a str],
}

enum Task {
    Dir(PathBuf, usize),
    Files(Vec<(PathBuf, bool)>),
}

#[derive(Default)]
struct Queue {
    tasks: Vec<Task>,
    /// Workers currently running a task.
    busy: usize,
}

struct Work<'a> {
    queue: Mutex<Queue>,
    ready: Condvar,
    dirs: Mutex<Vec<(usize, PathBuf)>>,
    failed: AtomicU64,
    first_error: Mutex<Option<String>>,
    progress: &'a Progress,
    target: &'a Target<'a>,
}

pub fn run(target: &Target<'_>, threads: usize, progress: &Progress) -> Report {
    let root = target.root;
    let work = Work {
        queue: Mutex::default(),
        ready: Condvar::new(),
        dirs: Mutex::new(Vec::new()),
        failed: AtomicU64::new(0),
        first_error: Mutex::new(None),
        progress,
        target,
    };
    match std::fs::symlink_metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Report::default(),
        Err(error) => work.fail(root, &error),
        // A file or link: one removal, nothing to walk.
        Ok(md) if !md.is_dir() || md.file_type().is_symlink() => {
            if !target.contents_only {
                work.remove(root, link_is_dir(&md));
            }
        }
        Ok(_) => {
            if !target.contents_only {
                lock(&work.dirs).push((0, root.to_owned()));
            }
            lock(&work.queue).tasks.push(Task::Dir(root.to_owned(), 0));
            std::thread::scope(|scope| {
                for _ in 0..threads.max(1) {
                    scope.spawn(|| work.drain());
                }
            });
            work.remove_dirs(threads);
        }
    }
    Report {
        failed: work.failed.load(Ordering::Relaxed),
        first_error: lock(&work.first_error).take(),
        cancelled: progress.cancelled(),
    }
}

impl Work<'_> {
    /// Phase one: walk every folder, deleting files and links as they are found.
    fn drain(&self) {
        loop {
            let task = {
                let mut queue = lock(&self.queue);
                loop {
                    if self.progress.cancelled() {
                        queue.tasks.clear();
                    }
                    if let Some(task) = queue.tasks.pop() {
                        queue.busy += 1;
                        break task;
                    }
                    if queue.busy == 0 {
                        self.ready.notify_all();
                        return;
                    }
                    queue = self.ready.wait(queue).unwrap_or_else(std::sync::PoisonError::into_inner);
                }
            };
            match task {
                Task::Dir(dir, depth) => self.walk(&dir, depth),
                Task::Files(files) => {
                    for (file, link_dir) in files {
                        if self.progress.cancelled() {
                            break;
                        }
                        self.remove(&file, link_dir);
                    }
                }
            }
            let mut queue = lock(&self.queue);
            queue.busy -= 1;
            self.ready.notify_all();
        }
    }

    fn walk(&self, dir: &Path, depth: usize) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => return self.fail(dir, &error),
        };
        let (mut files, mut subdirs) = (Vec::new(), Vec::new());
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.fail(dir, &error);
                    continue;
                }
            };
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    self.fail(&entry.path(), &error);
                    continue;
                }
            };
            if depth == 0 && self.target.contents_only && self.target.keep.iter().any(|k| entry.file_name() == *k) {
                continue;
            }
            if kind.is_dir() && !kind.is_symlink() {
                subdirs.push(entry.path());
            } else {
                files.push((entry.path(), link_kind_is_dir(kind)));
            }
        }
        {
            let mut found = lock(&self.dirs);
            found.extend(subdirs.iter().map(|d| (depth + 1, d.clone())));
        }
        let mut queue = lock(&self.queue);
        queue.tasks.extend(subdirs.into_iter().map(|d| Task::Dir(d, depth + 1)));
        // Hand most of a huge folder to other workers, keeping the last batch here.
        while files.len() > BATCH {
            let rest = files.split_off(BATCH);
            queue.tasks.push(Task::Files(std::mem::replace(&mut files, rest)));
        }
        self.ready.notify_all();
        drop(queue);
        for (file, link_dir) in files {
            if self.progress.cancelled() {
                return;
            }
            self.remove(&file, link_dir);
        }
    }

    /// Phase two: remove folders, deepest first, each level in parallel.
    fn remove_dirs(&self, threads: usize) {
        let mut dirs = std::mem::take(&mut *lock(&self.dirs));
        dirs.sort_unstable_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        for level in dirs.chunk_by(|a, b| a.0 == b.0) {
            if self.progress.cancelled() {
                return;
            }
            let next = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for _ in 0..threads.max(1).min(level.len()) {
                    scope.spawn(|| {
                        while let Some((_, dir)) = level.get(next.fetch_add(1, Ordering::Relaxed)) {
                            if self.progress.cancelled() {
                                return;
                            }
                            self.remove_dir(dir);
                        }
                    });
                }
            });
        }
    }

    fn remove(&self, path: &Path, link_dir: bool) {
        self.progress.note(path);
        match remove(path, link_dir) {
            Ok(()) => self.progress.file_done(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.progress.file_done(),
            Err(error) => self.fail(path, &error),
        }
    }

    fn remove_dir(&self, dir: &Path) {
        match remove(dir, true) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            // Something inside could not be deleted; that failure is already counted.
            Err(_) if std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some()) => {}
            Err(error) => self.fail(dir, &error),
        }
    }

    fn fail(&self, path: &Path, error: &io::Error) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        lock(&self.first_error).get_or_insert_with(|| format!("{}: {error}", path.display()));
    }
}

#[cfg(windows)]
fn link_kind_is_dir(kind: std::fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt;
    kind.is_symlink_dir()
}
#[cfg(not(windows))]
fn link_kind_is_dir(_: std::fs::FileType) -> bool {
    false
}
fn link_is_dir(md: &std::fs::Metadata) -> bool {
    md.is_dir() || link_kind_is_dir(md.file_type())
}

/// Delete one file, link or (empty) folder. Read-only and in-use files go too: POSIX
/// semantics unlink the name immediately while other handles stay valid.
#[cfg(windows)]
fn remove(path: &Path, dir: bool) -> io::Result<()> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
        Storage::FileSystem::{
            DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfoEx,
            SetFileInformationByHandle,
        },
    };
    // std adds the long-path prefix itself without a lossy round trip. The link itself is opened.
    let file = std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: live handle opened with DELETE access; the structure matches the class.
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    drop(file);
    // FAT/exFAT and older systems lack the extended disposition: clear read-only and retry plainly.
    let unsupported = [ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED, ERROR_INVALID_PARAMETER];
    if error.raw_os_error().is_some_and(|code| unsupported.contains(&(code as u32))) {
        if let Ok(md) = std::fs::symlink_metadata(path) {
            let mut permissions = md.permissions();
            if permissions.readonly() {
                #[allow(clippy::permissions_set_readonly_false)] // Windows: clears the read-only attribute only.
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(path, permissions);
            }
        }
        return if dir { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
    }
    Err(error)
}

#[cfg(not(windows))]
fn remove(path: &Path, dir: bool) -> io::Result<()> {
    if dir { std::fs::remove_dir(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("purge-tests");
        let root = base.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        for i in 0..600 {
            std::fs::write(root.join("a").join(format!("f{i}.txt")), b"x").unwrap();
        }
        std::fs::write(root.join("a/b/c/deep.txt"), b"x").unwrap();
        let readonly = root.join("a/b/readonly.txt");
        std::fs::write(&readonly, b"x").unwrap();
        let mut permissions = std::fs::metadata(&readonly).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&readonly, permissions).unwrap();
        std::fs::write(root.join("desktop.ini"), b"x").unwrap();
        root
    }

    #[test]
    fn deletes_a_tree_including_read_only_files() {
        let root = fixture("all");
        let progress = Progress::default();
        let report = run(&Target { root: &root, contents_only: false, keep: &[] }, 4, &progress);
        assert_eq!(report.failed, 0, "{:?}", report.first_error);
        assert!(!root.exists());
        assert_eq!(progress.snapshot().done, 603);
    }

    #[test]
    fn contents_only_keeps_the_root_and_named_files() {
        let root = fixture("contents");
        let progress = Progress::default();
        let report = run(&Target { root: &root, contents_only: true, keep: &["desktop.ini"] }, 4, &progress);
        assert_eq!(report.failed, 0, "{:?}", report.first_error);
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["desktop.ini"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn links_are_removed_without_touching_their_targets() {
        let root = fixture("links");
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"x").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, root.join("link"))
            .or_else(|_| junction(&outside, &root.join("link")));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("link"));
        linked.expect("create a directory link");
        let report = run(&Target { root: &root, contents_only: false, keep: &[] }, 2, &Progress::default());
        assert_eq!(report.failed, 0, "{:?}", report.first_error);
        assert!(!root.exists());
        assert!(outside.join("keep.txt").exists(), "a link's target must survive");
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn names_that_are_not_unicode_are_deleted() {
        use std::os::windows::ffi::OsStringExt;
        let root = fixture("surrogate");
        // An unpaired surrogate: valid on NTFS, but not representable as a `str`.
        let name = std::ffi::OsString::from_wide(&[u16::from(b'x'), 0xD800]);
        std::fs::write(root.join("a/b").join(&name), b"x").unwrap();
        let report = run(&Target { root: &root, contents_only: false, keep: &[] }, 2, &Progress::default());
        assert_eq!(report.failed, 0, "{:?}", report.first_error);
        assert!(!root.exists());
    }

    /// Junctions need no privilege, unlike symlinks without Developer Mode.
    #[cfg(windows)]
    fn junction(target: &Path, link: &Path) -> io::Result<()> {
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("mklink /J failed")) }
    }

    #[cfg(windows)]
    #[test]
    fn files_open_in_another_program_are_still_deleted() {
        let root = fixture("open");
        // Programs normally share delete access; POSIX semantics unlink the name at once.
        let held = std::fs::File::open(root.join("a/b/c/deep.txt")).unwrap();
        let report = run(&Target { root: &root, contents_only: false, keep: &[] }, 4, &Progress::default());
        assert_eq!(report.failed, 0, "{:?}", report.first_error);
        assert!(!root.exists());
        drop(held);
    }

    #[test]
    fn a_single_file_or_a_missing_path_needs_no_walk() {
        let root = fixture("single");
        let file = root.join("desktop.ini");
        let progress = Progress::default();
        let report = run(&Target { root: &file, contents_only: false, keep: &[] }, 4, &progress);
        assert_eq!((report.failed, file.exists()), (0, false));
        assert_eq!(progress.snapshot().done, 1);
        let gone = run(&Target { root: &file, contents_only: false, keep: &[] }, 4, &Progress::default());
        assert_eq!(gone.failed, 0, "already deleted is not a failure");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancellation_stops_early() {
        let root = fixture("cancel");
        let progress = Progress::default();
        progress.cancel();
        let report = run(&Target { root: &root, contents_only: false, keep: &[] }, 4, &progress);
        assert!(report.cancelled);
        assert!(root.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
