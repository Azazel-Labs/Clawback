//! Read-only Windows metadata and volume I/O. Opening a reparse point never
//! follows its target or reads file content (including cloud placeholders).
use crate::tree::FileId;
use std::ffi::{OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    fs::OpenOptionsExt,
    io::AsRawHandle,
};
use std::path::{Path, PathBuf};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(file: *mut c_void, info: *mut ByHandleFileInformation) -> i32;
    fn GetFileInformationByHandleEx(file: *mut c_void, class: i32, info: *mut c_void, size: u32) -> i32;
    fn DeviceIoControl(
        file: *mut c_void,
        code: u32,
        input: *const c_void,
        input_len: u32,
        output: *mut c_void,
        output_len: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetVolumePathNameW(path: *const u16, volume: *mut u16, len: u32) -> i32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetVolumeNameForVolumeMountPointW(path: *const u16, volume: *mut u16, len: u32) -> i32;
    fn GetVolumeInformationW(
        path: *const u16,
        label: *mut u16,
        label_len: u32,
        serial: *mut u32,
        component_len: *mut u32,
        flags: *mut u32,
        filesystem: *mut u16,
        filesystem_len: u32,
    ) -> i32;
    fn GetDiskFreeSpaceW(path: *const u16, sectors: *mut u32, bytes: *mut u32, free: *mut u32, total: *mut u32) -> i32;
    fn FileTimeToLocalFileTime(utc: *const FileTime, local: *mut FileTime) -> i32;
}

// CreateFileW share modes and flags.
const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4; // read, write, delete
const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
// FILE_INFO_BY_HANDLE_CLASS values.
const FILE_STANDARD_INFO: i32 = 1;
const FILE_COMPRESSION_INFO: i32 = 8;
const FSCTL_GET_RETRIEVAL_POINTERS: u32 = 0x0009_0073;
const ERROR_HANDLE_EOF: i32 = 38;
const ERROR_MORE_DATA: i32 = 234;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_RAMDISK: u32 = 6;
/// Longest extended-length path, in UTF-16 units.
const MAX_PATH_WIDE: usize = 32768;
/// Directory estimates assume this cluster size when the volume cannot say.
const DEFAULT_CLUSTER: u64 = 4096;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
#[derive(Default)]
#[allow(dead_code)] // Win32 layout
struct ByHandleFileInformation {
    attributes: u32,
    created: FileTime,
    accessed: FileTime,
    written: FileTime,
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}

impl ByHandleFileInformation {
    fn id(&self) -> FileId {
        (u64::from(self.volume_serial), (u64::from(self.index_high) << 32) | u64::from(self.index_low))
    }
}

#[repr(C)]
#[derive(Default)]
#[allow(dead_code)] // Win32 layout
struct FileStandardInfo {
    allocation_size: u64,
    end_of_file: u64,
    links: u32,
    delete_pending: u8,
    directory: u8,
}

#[repr(C)]
#[derive(Default)]
#[allow(dead_code)] // Win32 layout
struct FileCompressionInfo {
    compressed_size: u64,
    format: u16,
    unit_shift: u8,
    chunk_shift: u8,
    cluster_shift: u8,
    reserved: [u8; 3],
}

/// NUL-terminated UTF-16 for Win32 calls.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The text before the first NUL.
fn from_wide_nul(buffer: &[u16]) -> OsString {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..len])
}

/// Convert UTC FILETIME ticks to local time.
pub(crate) fn local_filetime(ticks: u64) -> Option<u64> {
    let utc = FileTime { low: ticks as u32, high: (ticks >> 32) as u32 };
    let mut local = FileTime::default();
    // SAFETY: both pointers refer to valid FILETIME structs.
    if unsafe { FileTimeToLocalFileTime(&raw const utc, &raw mut local) } == 0 {
        return None;
    }
    Some((u64::from(local.high) << 32) | u64::from(local.low))
}

pub(crate) struct Metadata {
    pub size: u64,
    pub len: u64,
    pub id: FileId,
}

fn metadata_handle(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn by_handle(file: &File) -> io::Result<ByHandleFileInformation> {
    let mut info = ByHandleFileInformation::default();
    // SAFETY: file owns a live handle and info has the Win32 structure's layout.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn query<T: Default>(file: &File, class: i32) -> io::Result<T> {
    let mut info = T::default();
    // SAFETY: the output is aligned, writable and its full size is supplied;
    // callers pair each class with its documented #[repr(C)] structure.
    if unsafe {
        GetFileInformationByHandleEx(file.as_raw_handle(), class, (&raw mut info).cast(), size_of::<T>() as u32)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

pub(crate) fn metadata(path: &Path, apparent: bool, ntfs_cluster: Option<u64>) -> io::Result<Metadata> {
    let file = metadata_handle(path)?;
    let info = by_handle(&file)?;
    let standard: FileStandardInfo = query(&file, FILE_STANDARD_INFO)?;
    let len = standard.end_of_file;
    let size = if apparent {
        len
    } else if resident_allocation(&file, standard.allocation_size, ntfs_cluster)? {
        // NTFS returns the inline value's padded length for resident data.
        // It occupies the MFT record, not separately allocated file clusters.
        0
    } else if info.attributes & (FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED) != 0 {
        // FileCompressionInfo reports physical storage for sparse and
        // compressed streams; AllocationSize alone can include holes.
        query::<FileCompressionInfo>(&file, FILE_COMPRESSION_INFO)?.compressed_size
    } else {
        standard.allocation_size
    };
    Ok(Metadata { size, len, id: info.id() })
}

fn resident_allocation(file: &File, allocated: u64, ntfs_cluster: Option<u64>) -> io::Result<bool> {
    let Some(cluster) = ntfs_cluster else { return Ok(false) };
    if allocated < cluster || !allocated.is_multiple_of(cluster) {
        return Ok(true);
    }
    // With unusually small clusters a resident value can be exactly one or
    // several clusters long. EOF from retrieval pointers identifies inline
    // data without reading content. Normal 4 KiB+ clusters avoid this ioctl.
    if cluster < 4096 && allocated <= 65536 {
        let mut mapping = [0u8; 32];
        match control(file, FSCTL_GET_RETRIEVAL_POINTERS, &0u64.to_le_bytes(), &mut mapping) {
            Err(error) if error.raw_os_error() == Some(ERROR_HANDLE_EOF) => return Ok(true),
            Err(error) if error.raw_os_error() != Some(ERROR_MORE_DATA) => return Err(error),
            _ => {} // More extents than fit still proves nonresident storage.
        }
    }
    Ok(false)
}

/// The NUL-terminated mount point of the volume holding `path`.
fn mount_point(path: &Path) -> Option<Vec<u16>> {
    let wide = wide(path);
    let mut mount = vec![0u16; MAX_PATH_WIDE];
    // SAFETY: path is terminated and mount has the declared writable capacity.
    let found = unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } != 0;
    found.then_some(mount)
}

fn cluster_bytes(mount: &[u16]) -> Option<u64> {
    let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
    // SAFETY: all output pointers are live DWORDs and mount is terminated.
    if unsafe { GetDiskFreeSpaceW(mount.as_ptr(), &raw mut sectors, &raw mut bytes, &raw mut free, &raw mut total) }
        == 0
    {
        return None;
    }
    Some(u64::from(sectors) * u64::from(bytes))
}

/// One volume query per scan; file sizes come from directory entries. Returns
/// the cluster size used for estimates, and the cluster size of a local NTFS
/// volume for detecting resident data.
pub(crate) fn clusters(path: &Path) -> (u64, Option<u64>) {
    let Some(mount) = mount_point(path) else { return (DEFAULT_CLUSTER, None) };
    let cluster = cluster_bytes(&mount).filter(|&c| c > 0);
    let ntfs = cluster.filter(|&c| c >= 512 && local_ntfs(&mount).is_some());
    (cluster.unwrap_or(DEFAULT_CLUSTER), ntfs)
}

fn local_disk(drive_type: u32) -> bool {
    // A mapped network drive remains DRIVE_REMOTE even when addressed by a
    // local-looking drive letter.
    matches!(drive_type, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK)
}

fn ntfs_name(filesystem: &[u16]) -> bool {
    "NTFS\0".encode_utf16().eq(filesystem.iter().copied().take(5))
}

/// Whether the volume holding `path` is one the MFT reader accepts: local
/// (fixed, removable or RAM disk) and NTFS. Scanning it raw also requires
/// administrator rights and the scan root being the volume's mount point.
pub fn raw_volume_eligible(path: &Path) -> bool {
    mount_point(path).is_some_and(|mount| local_ntfs(&mount).is_some())
}

/// The serial number of a local NTFS volume.
fn local_ntfs(mount: &[u16]) -> Option<u32> {
    // SAFETY: only terminated paths returned by GetVolumePathNameW reach here.
    if !local_disk(unsafe { GetDriveTypeW(mount.as_ptr()) }) {
        return None;
    }
    let mut serial = 0;
    let mut filesystem = [0u16; 32];
    // SAFETY: mount is terminated; unused output pointers are optional and the
    // serial and filesystem outputs are live, with the specified capacity.
    let ok = unsafe {
        GetVolumeInformationW(
            mount.as_ptr(),
            std::ptr::null_mut(),
            0,
            &raw mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } != 0;
    (ok && ntfs_name(&filesystem)).then_some(serial)
}

pub(crate) fn control(file: &File, code: u32, input: &[u8], output: &mut [u8]) -> io::Result<usize> {
    let mut returned = 0;
    // SAFETY: buffers remain live for this synchronous call, sizes match, and
    // no OVERLAPPED operation is started. Production callers use read controls.
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            input.as_ptr().cast(),
            input.len() as u32,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &raw mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(returned as usize)
}

/// Only whole local volumes are eligible. Folder scans retain traversal.
pub(crate) fn volume(root: &Path) -> io::Result<Option<(File, u64)>> {
    let root = std::fs::canonicalize(root)?;
    let Some(mount) = mount_point(&root) else { return Ok(None) };
    let Some(serial) = local_ntfs(&mount) else { return Ok(None) };
    if std::fs::canonicalize(PathBuf::from(from_wide_nul(&mount)))? != root {
        return Ok(None);
    }
    let mut name = [0u16; 64];
    // SAFETY: mount is terminated by the API and name is a writable buffer.
    if unsafe { GetVolumeNameForVolumeMountPointW(mount.as_ptr(), name.as_mut_ptr(), name.len() as u32) } == 0 {
        return Ok(None);
    }
    let invalid = || io::Error::other("Invalid volume name");
    let len = name.iter().position(|&c| c == 0).ok_or_else(invalid)?;
    // `\\?\Volume{GUID}\` names the root directory; without the slash it is the device.
    let device = name[..len].strip_suffix(&[u16::from(b'\\')]).ok_or_else(invalid)?;
    // Raw volumes are noncached. The MFT reader supplies aligned I/O buffers.
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_NO_BUFFERING)
        .open(PathBuf::from(OsString::from_wide(device)))?;
    Ok(Some((file, u64::from(serial))))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use crate::{ScanOptions, scan};
    use std::io::{Seek, SeekFrom, Write};

    #[cfg(feature = "profiling")]
    #[test]
    #[ignore = "Read-only per-call metadata profiling; set CLAWBACK_PROFILE_ROOT"]
    fn profile_metadata_calls() {
        use std::time::{Duration, Instant};
        let root = PathBuf::from(std::env::var_os("CLAWBACK_PROFILE_ROOT").expect("set CLAWBACK_PROFILE_ROOT"));
        let mut paths = Vec::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let ft = entry.file_type().unwrap();
                if ft.is_dir() {
                    dirs.push(entry.path());
                } else if ft.is_file() {
                    paths.push(entry.path());
                }
            }
        }
        let cluster = clusters(&root).1;
        println!("files,wall_s,open_s,identity_s,standard_s,allocation_s,compression_s,close_s");
        for _ in 0..3 {
            let mut times = [Duration::ZERO; 6];
            let wall = Instant::now();
            for path in &paths {
                let start = Instant::now();
                let file = metadata_handle(path).unwrap();
                times[0] += start.elapsed();
                let start = Instant::now();
                let info = std::hint::black_box(by_handle(&file).unwrap());
                times[1] += start.elapsed();
                let start = Instant::now();
                let standard: FileStandardInfo = query(&file, FILE_STANDARD_INFO).unwrap();
                times[2] += start.elapsed();
                let start = Instant::now();
                let resident = resident_allocation(&file, standard.allocation_size, cluster).unwrap();
                times[3] += start.elapsed();
                if !resident && info.attributes & (FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED) != 0 {
                    let start = Instant::now();
                    std::hint::black_box(query::<FileCompressionInfo>(&file, FILE_COMPRESSION_INFO).unwrap());
                    times[4] += start.elapsed();
                }
                let start = Instant::now();
                drop(file);
                times[5] += start.elapsed();
            }
            print!("{},{:.6}", paths.len(), wall.elapsed().as_secs_f64());
            for time in times {
                print!(",{:.6}", time.as_secs_f64());
            }
            println!();
        }
    }

    fn metadata(path: &Path, apparent: bool) -> io::Result<Metadata> {
        super::metadata(path, apparent, clusters(path).1)
    }

    #[test]
    fn raw_scan_policy_excludes_remote_unknown_and_non_ntfs_volumes() {
        for kind in [0, 1, 4, 5, 7] {
            assert!(!local_disk(kind), "drive type {kind} must use traversal");
        }
        for kind in [2, 3, 6] {
            assert!(local_disk(kind));
        }
        for name in ["FAT", "FAT32", "exFAT", "ReFS", "NTFSremote", ""] {
            let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
            assert!(!ntfs_name(&name));
        }
        assert!(ntfs_name(&"NTFS\0".encode_utf16().collect::<Vec<_>>()));
    }

    #[test]
    fn volume_serial_matches_file_identities() {
        let dir = TempDir::new("serial");
        let file = metadata_handle(&dir.0).unwrap();
        let mount = mount_point(&dir.0).unwrap();
        if let Some(serial) = local_ntfs(&mount) {
            assert_eq!(by_handle(&file).unwrap().id().0, u64::from(serial));
        }
    }

    #[test]
    fn resident_data_does_not_claim_separate_clusters() {
        let dir = TempDir::new("allocation");
        let path = dir.0.join("tiny");
        std::fs::write(&path, [1; 100]).unwrap();
        assert!(clusters(&path).1.is_some());
        assert_eq!(metadata(&path, false).unwrap().size, 0);
        assert_eq!(metadata(&path, true).unwrap().size, 100);
        std::fs::write(&path, [1; 512]).unwrap();
        let file = metadata_handle(&path).unwrap();
        // Exercise the ambiguous small-cluster branch on any NTFS test volume.
        assert!(resident_allocation(&file, 512, Some(512)).unwrap());
    }

    #[test]
    fn directory_scans_estimate_sparse_files_and_count_each_link() {
        let dir = TempDir::new("allocation");
        let path = dir.0.join("sparse");
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        control(&file, 0x0009_00c4, &[], &mut []).unwrap(); // FSCTL_SET_SPARSE, fixture only
        file.set_len(16 * 1024 * 1024).unwrap();
        drop(file);
        let empty = metadata(&path, false).unwrap();
        assert_eq!(empty.size, 0);
        assert_eq!(empty.len, 16 * 1024 * 1024);
        file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(1024 * 1024)).unwrap();
        file.write_all(&[1; 8192]).unwrap();
        file.sync_all().unwrap();
        drop(file);
        std::fs::hard_link(&path, dir.0.join("alias")).unwrap();
        let physical = metadata(&path, false).unwrap();
        assert!(physical.size >= 8192 && physical.size < physical.len, "size={}, len={}", physical.size, physical.len);
        let result = scan::scan(&dir.0, ScanOptions::default()).unwrap();
        assert_eq!(result.backend, scan::ScanBackend::Directory);
        assert!(result.skipped.is_empty());
        assert_eq!(result.files, 2);
        assert_eq!(result.bytes, 2 * physical.len);
        let counted_twice =
            scan::scan(&dir.0, ScanOptions { dedupe_hardlinks: false, ..ScanOptions::default() }).unwrap();
        assert_eq!(counted_twice.bytes, 2 * physical.len);
        let logical = scan::scan(&dir.0, ScanOptions { apparent_size: true, ..ScanOptions::default() }).unwrap();
        assert_eq!(logical.bytes, 2 * physical.len);
        let mut tree = result.tree;
        let mut refresh = crate::live::Refresh::new(&tree, ScanOptions::default());
        std::fs::write(&path, [4; 16384]).unwrap();
        refresh.path(&mut tree, &path, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        refresh.path(&mut tree, &dir.0.join("alias"), &std::sync::atomic::AtomicBool::new(false)).unwrap();
        assert_eq!(tree.root().size, 2 * 16384);
    }

    #[test]
    fn windows_compressed_file_uses_physical_bytes() {
        let dir = TempDir::new("allocation");
        let path = dir.0.join("compressed");
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        control(&file, 0x0009_c040, &2u16.to_le_bytes(), &mut []).unwrap(); // FSCTL_SET_COMPRESSION
        file.write_all(&vec![42; 1024 * 1024]).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let physical = metadata(&path, false).unwrap();
        assert_eq!(physical.len, 1024 * 1024);
        assert!(physical.size > 0 && physical.size < physical.len, "size={}, len={}", physical.size, physical.len);
        assert_eq!(metadata(&path, true).unwrap().size, physical.len);
        assert_eq!(scan::scan(&dir.0, ScanOptions::default()).unwrap().bytes, physical.len);
    }

    #[test]
    fn directory_estimates_work_without_read_sharing() {
        let dir = TempDir::new("allocation");
        let path = dir.0.join("locked");
        std::fs::write(&path, [1; 100]).unwrap();
        let _exclusive = OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let result = scan::scan(&dir.0, ScanOptions::default()).unwrap();
        assert_eq!(result.bytes, clusters(&dir.0).0);
        assert!(result.skipped.is_empty());
        let node = result.tree.find_path(&path).unwrap();
        assert!(!result.tree.node(node).has(crate::tree::flags::PARTIAL));
    }

    #[test]
    fn folders_are_ineligible_for_raw_volume_scans() {
        let dir = TempDir::new("allocation");
        assert!(volume(&dir.0).unwrap().is_none());
    }

    #[test]
    fn directory_accounting_does_not_reopen_files() {
        let dir = TempDir::new("allocation");
        let path = dir.0.join("listing-only");
        std::fs::write(&path, [1; 100]).unwrap();
        let md = std::fs::read_dir(&dir.0).unwrap().next().unwrap().unwrap().metadata().unwrap();
        let accounting = scan::FileAccounting::new(&dir.0, false);
        std::fs::remove_file(&path).unwrap();
        // Metadata from the listing remains sufficient after the name is gone.
        assert_eq!(accounting.measure(&path, &md, false).unwrap(), (clusters(&dir.0).0, 100, None));
        assert_eq!(accounting.measure(&path, &md, true).unwrap(), (100, 100, None));
    }

    #[test]
    fn local_volume_root_is_recognized_even_without_raw_read_privileges() {
        let current = std::env::current_dir().unwrap();
        let root = current.ancestors().last().unwrap();
        // Recognition should reach the volume open. A non-elevated process
        // receives Access Denied there, triggering the scanner's fallback.
        match volume(root) {
            Ok(Some(_)) => {}
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
            other => panic!("Local volume root was not recognized: {other:?}"),
        }
    }
}
