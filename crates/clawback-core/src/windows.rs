//! Read-only Windows metadata and volume I/O. Opening a reparse point never
//! follows its target or reads file content (including cloud placeholders).
use crate::tree::FileId;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::path::{Path, PathBuf};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(file: *mut c_void, info: *mut u32) -> i32;
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
}

pub(crate) struct Metadata {
    pub size: u64,
    pub len: u64,
    pub id: FileId,
}

fn metadata_handle(path: &Path) -> io::Result<File> {
    OpenOptions::new().access_mode(0).share_mode(7).custom_flags(0x0200_0000 | 0x0020_0000).open(path)
}

fn identity(file: &File) -> io::Result<FileId> {
    // BY_HANDLE_FILE_INFORMATION is thirteen DWORDs, including FILETIMEs.
    let mut info = [0u32; 13];
    // SAFETY: file owns a live handle and info has the Win32 structure's layout.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(info[7]), (u64::from(info[11]) << 32) | u64::from(info[12])))
}

fn query<const N: usize>(file: &File, class: i32) -> io::Result<[u64; N]> {
    let mut info = [0u64; N];
    // SAFETY: the output is aligned, writable and its full size is supplied;
    // callers use the documented sizes of FILE_STANDARD/COMPRESSION_INFO.
    if unsafe {
        GetFileInformationByHandleEx(file.as_raw_handle(), class, info.as_mut_ptr().cast(), size_of_val(&info) as u32)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

pub(crate) fn metadata(path: &Path, apparent: bool, ntfs_cluster: Option<u64>) -> io::Result<Metadata> {
    let file = metadata_handle(path)?;
    let id = identity(&file)?;
    let standard = query::<3>(&file, 1)?;
    let len = standard[1];
    let size = if apparent {
        len
    } else if resident_allocation(&file, standard[0], ntfs_cluster)? {
        // NTFS returns the inline value's padded length for resident data.
        // It occupies the MFT record, not separately allocated file clusters.
        0
    } else {
        use std::os::windows::fs::MetadataExt;
        let attributes = file.metadata()?.file_attributes();
        if attributes & (0x200 | 0x800) != 0 {
            // FileCompressionInfo reports physical storage for sparse and
            // compressed streams; AllocationSize alone can include holes.
            query::<2>(&file, 8)?[0]
        } else {
            standard[0]
        }
    };
    Ok(Metadata { size, len, id })
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
        match control(file, 0x0009_0073, &0u64.to_le_bytes(), &mut mapping) {
            Err(error) if error.raw_os_error() == Some(38) => return Ok(true), // ERROR_HANDLE_EOF
            Err(error) if error.raw_os_error() != Some(234) => return Err(error),
            _ => {} // More extents than fit still proves nonresident storage.
        }
    }
    Ok(false)
}

pub(crate) fn ntfs_cluster(path: &Path) -> Option<u64> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = vec![0u16; 32768];
    // SAFETY: path is terminated and mount has the declared writable capacity.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return None;
    }
    if !local_ntfs(&mount) {
        return None;
    }
    let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
    // SAFETY: all output pointers are live DWORDs and mount is terminated.
    if unsafe { GetDiskFreeSpaceW(mount.as_ptr(), &raw mut sectors, &raw mut bytes, &raw mut free, &raw mut total) }
        == 0
    {
        return None;
    }
    let cluster = u64::from(sectors) * u64::from(bytes);
    (cluster >= 512).then_some(cluster)
}

/// One volume query per scan; file sizes come from directory entries.
pub(crate) fn cluster_size(path: &Path) -> u64 {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = vec![0u16; 32768];
    // SAFETY: input is terminated and output has the supplied capacity.
    let found = unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } != 0;
    let (mut sectors, mut bytes, mut free, mut total) = (0, 0, 0, 0);
    // SAFETY: a successful lookup terminated mount; all outputs are live DWORDs.
    let ok = found
        && unsafe {
            GetDiskFreeSpaceW(mount.as_ptr(), &raw mut sectors, &raw mut bytes, &raw mut free, &raw mut total)
        } != 0;
    let cluster = u64::from(sectors) * u64::from(bytes);
    if ok && cluster > 0 { cluster } else { 4096 }
}

fn local_disk(drive_type: u32) -> bool {
    // DRIVE_REMOVABLE, DRIVE_FIXED, DRIVE_RAMDISK. A mapped network drive
    // remains DRIVE_REMOTE even when addressed by a local-looking drive letter.
    matches!(drive_type, 2 | 3 | 6)
}

fn ntfs_name(filesystem: &[u16]) -> bool {
    filesystem.starts_with(&[78, 84, 70, 83, 0])
}

fn local_ntfs(mount: &[u16]) -> bool {
    // SAFETY: only terminated paths returned by GetVolumePathNameW reach here.
    if !local_disk(unsafe { GetDriveTypeW(mount.as_ptr()) }) {
        return false;
    }
    let mut filesystem = [0u16; 32];
    // SAFETY: mount is terminated; unused output pointers are optional and the
    // filesystem output buffer has the specified capacity.
    if unsafe {
        GetVolumeInformationW(
            mount.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
        || !ntfs_name(&filesystem)
    {
        return false;
    }
    true
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
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = vec![0u16; 32768];
    // SAFETY: wide is terminated and mount has the declared capacity.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return Ok(None);
    }
    if !local_ntfs(&mount) {
        return Ok(None);
    }
    let len = mount.iter().position(|&c| c == 0).unwrap_or(mount.len());
    use std::os::windows::ffi::OsStringExt;
    let mount_path = PathBuf::from(std::ffi::OsString::from_wide(&mount[..len]));
    if std::fs::canonicalize(mount_path)? != root {
        return Ok(None);
    }
    let mut name = [0u16; 64];
    // SAFETY: mount is terminated by the API and name is a writable buffer.
    if unsafe { GetVolumeNameForVolumeMountPointW(mount.as_ptr(), name.as_mut_ptr(), name.len() as u32) } == 0 {
        return Ok(None);
    }
    let len = name.iter().position(|&c| c == 0).ok_or_else(|| io::Error::other("Invalid volume name"))?;
    let path = PathBuf::from(std::ffi::OsString::from_wide(&name[..len.saturating_sub(1)]));
    let serial = identity(&metadata_handle(&root)?)?.0;
    // Raw volumes are noncached. The MFT reader supplies aligned I/O buffers.
    let file = OpenOptions::new().read(true).share_mode(7).custom_flags(0x2000_0000).open(path)?;
    Ok(Some((file, serial)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{ScanOptions, scan};
    use std::io::{Seek, SeekFrom, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "profiling")]
    #[test]
    #[ignore = "Read-only per-call metadata profiling; set CLAWBACK_PROFILE_ROOT"]
    fn profile_metadata_calls() {
        use std::os::windows::fs::MetadataExt;
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
        let cluster = ntfs_cluster(&root);
        println!("files,wall_s,open_s,identity_s,standard_s,allocation_s,attributes_s,compression_s,close_s");
        for _ in 0..3 {
            let mut times = [Duration::ZERO; 7];
            let wall = Instant::now();
            for path in &paths {
                let start = Instant::now();
                let file = metadata_handle(path).unwrap();
                times[0] += start.elapsed();
                let start = Instant::now();
                std::hint::black_box(identity(&file).unwrap());
                times[1] += start.elapsed();
                let start = Instant::now();
                let standard = query::<3>(&file, 1).unwrap();
                times[2] += start.elapsed();
                let start = Instant::now();
                let resident = resident_allocation(&file, standard[0], cluster).unwrap();
                times[3] += start.elapsed();
                if !resident {
                    let start = Instant::now();
                    let attributes = file.metadata().unwrap().file_attributes();
                    times[4] += start.elapsed();
                    if attributes & (0x200 | 0x800) != 0 {
                        let start = Instant::now();
                        std::hint::black_box(query::<2>(&file, 8).unwrap());
                        times[5] += start.elapsed();
                    }
                }
                let start = Instant::now();
                drop(file);
                times[6] += start.elapsed();
            }
            print!("{},{:.6}", paths.len(), wall.elapsed().as_secs_f64());
            for time in times {
                print!(",{:.6}", time.as_secs_f64());
            }
            println!();
        }
    }

    fn metadata(path: &Path, apparent: bool) -> io::Result<Metadata> {
        super::metadata(path, apparent, ntfs_cluster(path))
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
        assert!(ntfs_name(&[78, 84, 70, 83, 0]));
    }

    #[test]
    fn resident_data_does_not_claim_separate_clusters() {
        let dir = Temp::new();
        let path = dir.0.join("tiny");
        std::fs::write(&path, [1; 100]).unwrap();
        assert!(ntfs_cluster(&path).is_some());
        assert_eq!(metadata(&path, false).unwrap().size, 0);
        assert_eq!(metadata(&path, true).unwrap().size, 100);
        std::fs::write(&path, [1; 512]).unwrap();
        let file = metadata_handle(&path).unwrap();
        // Exercise the ambiguous small-cluster branch on any NTFS test volume.
        assert!(resident_allocation(&file, 512, Some(512)).unwrap());
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "clawback-allocation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn directory_scans_estimate_sparse_files_and_count_each_link() {
        let dir = Temp::new();
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
        let dir = Temp::new();
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
        let dir = Temp::new();
        let path = dir.0.join("locked");
        std::fs::write(&path, [1; 100]).unwrap();
        let _exclusive = OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let result = scan::scan(&dir.0, ScanOptions::default()).unwrap();
        assert_eq!(result.bytes, cluster_size(&dir.0));
        assert!(result.skipped.is_empty());
        let node = result.tree.find_path(&path).unwrap();
        assert!(!result.tree.node(node).has(crate::tree::flags::PARTIAL));
    }

    #[test]
    fn folders_are_ineligible_for_raw_volume_scans() {
        let dir = Temp::new();
        assert!(volume(&dir.0).unwrap().is_none());
    }

    #[test]
    fn directory_accounting_does_not_reopen_files() {
        let dir = Temp::new();
        let path = dir.0.join("listing-only");
        std::fs::write(&path, [1; 100]).unwrap();
        let md = std::fs::read_dir(&dir.0).unwrap().next().unwrap().unwrap().metadata().unwrap();
        let accounting = scan::FileAccounting::new(&dir.0);
        std::fs::remove_file(&path).unwrap();
        // Metadata from the listing remains sufficient after the name is gone.
        assert_eq!(accounting.measure(&path, &md, false).unwrap(), (cluster_size(&dir.0), 100, None));
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
