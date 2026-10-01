use super::{COMMON, FILE, Metadata, Record, parse_batch};
use crate::Kind;
use std::collections::{HashSet, VecDeque};
use std::ffi::{OsString, c_void};
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
struct AttrList {
    count: u16,
    reserved: u16,
    common: u32,
    volume: u32,
    directory: u32,
    file: u32,
    fork: u32,
}

unsafe extern "C" {
    fn getattrlistbulk(fd: i32, attrs: *mut AttrList, buffer: *mut c_void, size: usize, options: u64) -> i32;
}

#[repr(C, align(8))]
struct Buffer([u8; 64 * 1024]);

struct Bulk {
    directory: fs::File,
    buffer: Box<Buffer>,
    pending: VecDeque<Record>,
    #[cfg(test)]
    fail_next_batch: bool,
}

impl Bulk {
    fn open(path: &Path) -> io::Result<Self> {
        // Darwin O_DIRECTORY | O_NOFOLLOW: never traverse a directory replaced
        // by a symlink between enumeration and opening the child.
        let directory = fs::OpenOptions::new().read(true).custom_flags(0x0010_0000 | 0x100).open(path)?;
        Ok(Self {
            directory,
            // SAFETY: Buffer contains only bytes, for which all-zero is valid.
            buffer: unsafe { Box::<Buffer>::new_zeroed().assume_init() },
            pending: VecDeque::new(),
            #[cfg(test)]
            fail_next_batch: false,
        })
    }

    fn next(&mut self) -> io::Result<Option<Record>> {
        if let Some(record) = self.pending.pop_front() {
            return Ok(Some(record));
        }
        #[cfg(test)]
        if self.fail_next_batch {
            return Err(io::Error::other("injected bulk read failure"));
        }
        let mut attrs =
            AttrList { count: 5, reserved: 0, common: COMMON, volume: 0, directory: 0, file: FILE, fork: 0 };
        self.buffer.0.fill(0);
        // SAFETY: directory owns a live fd; attrs has Darwin's C layout and
        // buffer is writable, eight-byte aligned, and valid for its full size.
        let count = unsafe {
            getattrlistbulk(
                self.directory.as_raw_fd(),
                &raw mut attrs,
                self.buffer.0.as_mut_ptr().cast(),
                self.buffer.0.len(),
                0,
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        self.pending = parse_batch(&self.buffer.0, count as usize)?.into();
        Ok(self.pending.pop_front())
    }
}

enum Source {
    Bulk(Bulk),
    Standard(fs::ReadDir),
    Done,
}

pub(crate) struct ReadDir<'a> {
    path: PathBuf,
    source: Source,
    // Needed only for a mid-stream fallback. A fresh read_dir has its own fd
    // and skips names already delivered, avoiding duplicate tree accounting.
    seen: HashSet<OsString>,
    cancel: &'a AtomicBool,
}

pub(crate) fn read_dir<'a>(path: &Path, cancel: &'a AtomicBool) -> io::Result<ReadDir<'a>> {
    let source = match Bulk::open(path) {
        Ok(bulk) => Source::Bulk(bulk),
        Err(_) => Source::Standard(fs::read_dir(path)?),
    };
    Ok(ReadDir { path: path.into(), source, seen: HashSet::new(), cancel })
}

impl Iterator for ReadDir<'_> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel.load(Ordering::Relaxed) {
            return None;
        }
        if let Source::Bulk(bulk) = &mut self.source {
            match bulk.next() {
                Ok(Some(record)) => {
                    let name = OsString::from_vec(record.name);
                    let path = self.path.join(&name);
                    self.seen.insert(name);
                    return Some(match record.metadata {
                        Some(metadata) => Ok(Entry::Bulk { path, metadata }),
                        None => fs::symlink_metadata(&path).map(|metadata| Entry::Stat { path, metadata }),
                    });
                }
                Ok(None) => {
                    self.source = Source::Done;
                    return None;
                }
                Err(_) => {
                    // Unsupported filesystems, entry errors without a usable
                    // name, and malformed batches all retry through read_dir.
                    match fs::read_dir(&self.path) {
                        Ok(rd) => self.source = Source::Standard(rd),
                        Err(error) => {
                            self.source = Source::Done;
                            return Some(Err(error));
                        }
                    }
                }
            }
        }
        let Source::Standard(rd) = &mut self.source else { return None };
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return None;
            }
            match rd.next()? {
                Ok(entry) if self.seen.contains(&entry.file_name()) => {}
                result => return Some(result.map(|entry| Entry::Standard(Box::new(entry)))),
            }
        }
    }
}

pub(crate) enum Entry {
    Standard(Box<fs::DirEntry>),
    Bulk { path: PathBuf, metadata: Metadata },
    Stat { path: PathBuf, metadata: fs::Metadata },
}

pub(crate) enum FileType {
    Regular,
    Standard(fs::FileType),
}

impl FileType {
    pub(crate) fn is_dir(&self) -> bool {
        matches!(self, Self::Standard(t) if t.is_dir())
    }
    #[cfg(test)]
    fn is_symlink(&self) -> bool {
        matches!(self, Self::Standard(t) if t.is_symlink())
    }
}

impl From<FileType> for Kind {
    fn from(ft: FileType) -> Self {
        match ft {
            FileType::Regular => Kind::File,
            FileType::Standard(t) => Kind::from(t),
        }
    }
}

impl Entry {
    pub(crate) fn file_name(&self) -> OsString {
        match self {
            Self::Standard(e) => e.file_name(),
            Self::Bulk { path, .. } | Self::Stat { path, .. } => path.file_name().unwrap_or_default().to_owned(),
        }
    }
    pub(crate) fn path(&self) -> PathBuf {
        match self {
            Self::Standard(e) => e.path(),
            Self::Bulk { path, .. } | Self::Stat { path, .. } => path.clone(),
        }
    }
    pub(crate) fn file_type(&self) -> io::Result<FileType> {
        match self {
            Self::Standard(e) => e.file_type().map(FileType::Standard),
            Self::Bulk { .. } => Ok(FileType::Regular),
            Self::Stat { metadata, .. } => Ok(FileType::Standard(metadata.file_type())),
        }
    }
    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        match self {
            Self::Standard(e) => e.metadata().map(|md| Metadata::from(&md)),
            Self::Bulk { metadata, .. } => Ok(*metadata),
            Self::Stat { metadata, .. } => Ok(Metadata::from(metadata)),
        }
    }
}

impl From<&fs::Metadata> for Metadata {
    fn from(md: &fs::Metadata) -> Self {
        Self {
            device: md.dev(),
            inode: md.ino(),
            modified: md.mtime(),
            allocated: md.blocks() * 512,
            length: md.len(),
            links: md.nlink(),
        }
    }
}

impl Metadata {
    pub(crate) fn dev(&self) -> u64 {
        self.device
    }
    pub(crate) fn ino(&self) -> u64 {
        self.inode
    }
    pub(crate) fn mtime(&self) -> i64 {
        self.modified
    }
    pub(crate) fn allocated(&self) -> u64 {
        self.allocated
    }
    pub(crate) fn len(&self) -> u64 {
        self.length
    }
    pub(crate) fn nlink(&self) -> u64 {
        self.links
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    fn temp() -> TempDir {
        TempDir::new("bulk")
    }

    fn bulk<'a>(rd: &'a mut ReadDir<'_>) -> &'a mut Bulk {
        let Source::Bulk(bulk) = &mut rd.source else { panic!("native bulk reader") };
        bulk
    }

    fn uses_fallback(rd: &ReadDir<'_>) -> bool {
        matches!(rd.source, Source::Standard(_))
    }

    #[test]
    fn native_bulk_matches_stat_including_sparse_files_and_hardlinks() {
        let dir = temp();
        fs::write(dir.0.join("regular"), vec![7; 12345]).unwrap();
        fs::write(dir.0.join("regular/..namedfork/rsrc"), b"resource fork allocation").unwrap();
        fs::File::create(dir.0.join("sparse")).unwrap().set_len(8 * 1024 * 1024).unwrap();
        fs::hard_link(dir.0.join("regular"), dir.0.join("alias")).unwrap();
        fs::create_dir(dir.0.join("child")).unwrap();
        std::os::unix::fs::symlink(&dir.0, dir.0.join("loop")).unwrap();
        // APFS rejects names that are not valid UTF-8; fall back to a
        // multibyte name there so raw name bytes are still round-tripped.
        let name = OsString::from_vec(vec![b'n', 0xff]);
        if let Err(error) = fs::write(dir.0.join(&name), b"bytes") {
            assert_eq!(error.raw_os_error(), Some(92), "{error}"); // EILSEQ
            fs::write(dir.0.join("n\u{e9}"), b"bytes").unwrap();
        }
        let mut bulk = Bulk::open(&dir.0).unwrap();
        let mut count = 0;
        while let Some(record) = bulk.next().unwrap() {
            let path = dir.0.join(OsString::from_vec(record.name));
            let md = fs::symlink_metadata(path).unwrap();
            if md.is_file() {
                assert_eq!(record.metadata, Some(Metadata::from(&md)));
            } else {
                assert!(record.metadata.is_none());
            }
            count += 1;
        }
        assert_eq!(count, 6);
        let cancel = AtomicBool::new(false);
        for entry in read_dir(&dir.0, &cancel).unwrap() {
            let entry = entry.unwrap();
            let md = fs::symlink_metadata(entry.path()).unwrap();
            assert_eq!(entry.file_type().unwrap().is_symlink(), md.is_symlink());
            assert_eq!(entry.file_type().unwrap().is_dir(), md.is_dir());
            assert_eq!(entry.metadata().unwrap(), Metadata::from(&md));
        }
    }

    #[test]
    fn unsupported_bulk_call_falls_back_before_first_entry() {
        let dir = temp();
        fs::write(dir.0.join("file"), b"hello").unwrap();
        let cancel = AtomicBool::new(false);
        let mut rd = read_dir(&dir.0, &cancel).unwrap();
        bulk(&mut rd).fail_next_batch = true;
        let entry = rd.next().unwrap().unwrap();
        assert_eq!(entry.file_name(), "file");
        assert_eq!(entry.metadata().unwrap().len(), 5);
        assert!(uses_fallback(&rd));
        assert!(rd.next().is_none());
    }

    #[test]
    fn fallback_after_a_published_batch_does_not_duplicate_entries() {
        let dir = temp();
        // More than one 64 KiB batch.
        for i in 0..1500 {
            fs::write(dir.0.join(format!("file-{i:04}")), b"x").unwrap();
        }
        let cancel = AtomicBool::new(false);
        let mut rd = read_dir(&dir.0, &cancel).unwrap();
        let first = rd.next().unwrap().unwrap();
        let bulk = bulk(&mut rd);
        bulk.fail_next_batch = true;
        let mut names = HashSet::from([first.file_name()]);
        for entry in rd.by_ref() {
            assert!(names.insert(entry.unwrap().file_name()));
        }
        assert!(uses_fallback(&rd), "must exercise fallback");
        assert_eq!(names.len(), 1500);
    }

    #[test]
    fn cancellation_stops_before_another_batch_or_fallback() {
        let dir = temp();
        fs::write(dir.0.join("file"), b"x").unwrap();
        let cancel = AtomicBool::new(true);
        let mut rd = read_dir(&dir.0, &cancel).unwrap();
        assert!(rd.next().is_none());
        assert!(!uses_fallback(&rd));
    }
}
