//! Known WSL virtual disks have a reclaim-space action rather than being opened.
use std::path::{Path, PathBuf};
#[cfg(windows)]
mod registry;

#[derive(Clone, Debug)]
struct Registration {
    name: String,
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskKind {
    DockerData,
    DockerSystem,
    Linux,
}

pub struct Info {
    pub kind: DiskKind,
    pub distribution: Option<String>,
    pub package: Option<String>,
    pub file_size: Option<u64>,
    pub size_on_disk: Option<u64>,
    pub modified: Option<i64>,
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").trim_start_matches("\\\\?\\").to_lowercase()
}

fn registration<'a>(path: &Path, registrations: &'a [Registration]) -> Option<&'a Registration> {
    let path = normalized(path);
    registrations.iter().find(|entry| normalized(&entry.path) == path)
}

fn identify(path: &Path, registrations: &[Registration]) -> Info {
    let distribution = registration(path, registrations).map(|entry| entry.name.clone());
    let normalized = normalized(path);
    let kind = if normalized.ends_with("\\docker\\wsl\\main\\ext4.vhdx")
        || distribution.as_deref() == Some("docker-desktop")
    {
        DiskKind::DockerSystem
    } else if normalized.ends_with("\\docker\\wsl\\disk\\docker_data.vhdx")
        || normalized.ends_with("\\docker\\wsl\\data\\ext4.vhdx")
        || distribution.as_deref() == Some("docker-desktop-data")
    {
        DiskKind::DockerData
    } else {
        DiskKind::Linux
    };
    let components: Vec<_> = path.components().collect();
    let package = components
        .windows(2)
        .find(|pair| pair[0].as_os_str().eq_ignore_ascii_case("Packages"))
        .map(|pair| pair[1].as_os_str().to_string_lossy().into_owned());
    Info { kind, distribution, package, file_size: None, size_on_disk: None, modified: None }
}

/// Details are read on a worker without mounting the disk or starting Linux.
pub fn inspect(path: &Path) -> Info {
    #[cfg(windows)]
    let registrations = registry::read();
    #[cfg(not(windows))]
    let registrations = Vec::new();
    let mut info = identify(path, &registrations);
    if let Ok(metadata) = std::fs::metadata(path) {
        info.file_size = Some(metadata.len());
        info.modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);
    }
    #[cfg(windows)]
    {
        info.size_on_disk = windows::allocated_size(path);
    }
    info
}

/// Recognize standard Docker Desktop and WSL installation layouts, not arbitrary VHDs.
pub fn recognized(path: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    if standard_path(&path.to_string_lossy()) {
        return true;
    }
    #[cfg(windows)]
    {
        if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("vhdx")) {
            return false;
        }
        // Recognition is cached for this session; identifying details refresh on each click.
        static REGISTRATIONS: std::sync::OnceLock<Vec<Registration>> = std::sync::OnceLock::new();
        registration(path, REGISTRATIONS.get_or_init(registry::read)).is_some()
    }
    #[cfg(not(windows))]
    false
}

fn standard_path(path: &str) -> bool {
    let path = path.replace('/', "\\").to_ascii_lowercase();
    let docker = path.ends_with("\\docker\\wsl\\disk\\docker_data.vhdx")
        || path.ends_with("\\docker\\wsl\\data\\ext4.vhdx")
        || path.ends_with("\\docker\\wsl\\main\\ext4.vhdx");
    let distro = path.contains("\\appdata\\local\\packages\\") && path.ends_with("\\localstate\\ext4.vhdx");
    let modern = path.contains("\\appdata\\local\\wsl\\") && path.ends_with("\\ext4.vhdx");
    docker || distro || modern
}

#[cfg(windows)]
pub use windows::{compact, open_docker, worker_entry};

#[cfg(windows)]
mod windows {
    use super::recognized;
    use crate::platform::{Apartment, wide};
    use std::{
        ffi::OsStr,
        io,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::Path,
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        Storage::Vhd::{
            COMPACT_VIRTUAL_DISK_FLAG_NONE, CompactVirtualDisk, OPEN_VIRTUAL_DISK_FLAG_NONE,
            OPEN_VIRTUAL_DISK_PARAMETERS, OPEN_VIRTUAL_DISK_VERSION_1, OpenVirtualDisk, VIRTUAL_DISK_ACCESS_METAOPS,
            VIRTUAL_STORAGE_TYPE, VIRTUAL_STORAGE_TYPE_DEVICE_VHDX, VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
        },
        System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
        UI::{
            Shell::{
                SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
            },
            WindowsAndMessaging::SW_HIDE,
        },
    };

    pub(super) fn allocated_size(path: &Path) -> Option<u64> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, SetLastError},
            Storage::FileSystem::GetCompressedFileSizeW,
        };
        let path = wide(path.as_os_str());
        let mut high = 0;
        // SAFETY: resets only the current thread's error value.
        unsafe {
            SetLastError(0);
        }
        // SAFETY: valid NUL-terminated path and writable high-word storage.
        let low = unsafe { GetCompressedFileSizeW(path.as_ptr(), &raw mut high) };
        // SAFETY: reads the current thread's last error from the preceding Win32 call.
        if low == u32::MAX && unsafe { GetLastError() } != 0 {
            None
        } else {
            Some((u64::from(high) << 32) | u64::from(low))
        }
    }

    pub fn open_docker() -> Result<(), String> {
        let candidates = [
            std::env::var_os("ProgramFiles")
                .map(|base| std::path::PathBuf::from(base).join("Docker/Docker/Docker Desktop.exe")),
            std::env::var_os("LOCALAPPDATA")
                .map(|base| std::path::PathBuf::from(base).join("Programs/DockerDesktop/Docker Desktop.exe")),
        ];
        let path = candidates
            .into_iter()
            .flatten()
            .find(|path| path.is_file())
            .ok_or_else(|| crate::i18n::tr!("docker-desktop-not-found"))?;
        crate::platform::open(&path)
    }

    /// Run compaction in a dedicated elevated process. No command shell or script.
    pub fn compact(path: &Path) -> io::Result<()> {
        if !recognized(path) || !path.is_absolute() {
            return Err(io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER as i32));
        }
        // Run as the current user: WSL registrations belong to that user, not
        // necessarily the account used to approve the elevation prompt.
        shutdown_wsl()?;
        let executable = wide(std::env::current_exe()?.as_os_str());
        // Encode the path as UTF-16 hex: spaces, quotes and shell characters never become arguments.
        use std::fmt::Write;
        use std::os::windows::ffi::OsStrExt;
        let mut encoded = String::new();
        for unit in path.as_os_str().encode_wide() {
            write!(encoded, "{unit:04x}").expect("write to string");
        }
        let arguments = wide(OsStr::new(&format!("--compact-wsl-disk {encoded}")));
        let verb = wide(OsStr::new("runas"));
        let _apartment = Apartment::enter().ok();
        // SAFETY: Win32 shell structure permits zero initialization before setting its size.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.lpVerb = verb.as_ptr();
        info.lpFile = executable.as_ptr();
        info.lpParameters = arguments.as_ptr();
        info.nShow = SW_HIDE;
        // SAFETY: structure and NUL-terminated strings live through the synchronous shell call.
        if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.hProcess.is_null() {
            return Err(io::Error::other("Compaction helper returned no process handle"));
        }
        // SAFETY: the shell transfers this process handle to the caller.
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
        // SAFETY: live owned process handle; waiting takes place on a background thread.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        // SAFETY: live process handle and writable exit-code storage.
        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        status(code)
    }

    fn status(code: u32) -> io::Result<()> {
        if code == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(code as i32)) }
    }

    fn shutdown_wsl() -> io::Result<()> {
        use std::{
            os::windows::process::CommandExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let root = std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("Windows directory unavailable"))?;
        let mut child = Command::new(std::path::PathBuf::from(root).join("System32/wsl.exe"))
            .arg("--shutdown")
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(result) = child.try_wait()? {
                return if result.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("WSL shutdown failed: {result}")))
                };
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "WSL did not stop within 60 seconds"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn worker_entry() -> Option<io::Result<()>> {
        let mut args = std::env::args_os().skip(1);
        if args.next()?.as_os_str() != OsStr::new("--compact-wsl-disk") {
            return None;
        }
        Some((|| {
            use std::os::windows::ffi::OsStringExt;
            let invalid = || io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER as i32);
            let encoded = args.next().ok_or_else(invalid)?;
            let encoded = encoded.to_str().ok_or_else(invalid)?;
            if args.next().is_some() || encoded.len() % 4 != 0 || !encoded.is_ascii() {
                return Err(invalid());
            }
            let units: Vec<u16> = (0..encoded.len())
                .step_by(4)
                .map(|offset| u16::from_str_radix(&encoded[offset..offset + 4], 16).map_err(|_| invalid()))
                .collect::<io::Result<_>>()?;
            if units.contains(&0) {
                return Err(invalid());
            }
            let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&units));
            if !path.is_absolute() || !recognized(&path) {
                return Err(invalid());
            }
            let path = wide(path.as_os_str());
            let storage = VIRTUAL_STORAGE_TYPE {
                DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
                VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
            };
            // SAFETY: structure is a Win32 tagged union initialized before selecting V1.
            let mut parameters: OPEN_VIRTUAL_DISK_PARAMETERS = unsafe { std::mem::zeroed() };
            parameters.Version = OPEN_VIRTUAL_DISK_VERSION_1;
            parameters.Anonymous.Version1.RWDepth = 1;
            let mut handle = std::ptr::null_mut();
            // SAFETY: initialized type/parameters, NUL-terminated path and writable handle storage.
            status(unsafe {
                OpenVirtualDisk(
                    &raw const storage,
                    path.as_ptr(),
                    VIRTUAL_DISK_ACCESS_METAOPS,
                    OPEN_VIRTUAL_DISK_FLAG_NONE,
                    &raw const parameters,
                    &raw mut handle,
                )
            })?;
            // SAFETY: successful OpenVirtualDisk transfers ownership of this handle.
            let disk = unsafe { OwnedHandle::from_raw_handle(handle) };
            // SAFETY: live disk handle with METAOPS access; optional parameters and overlapped are null.
            // Windows rejects disks that are in use. Never detach or shut down a running guest here.
            status(unsafe {
                CompactVirtualDisk(
                    disk.as_raw_handle(),
                    COMPACT_VIRTUAL_DISK_FLAG_NONE,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            })
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskKind, Registration, identify};
    use std::path::{Path, PathBuf};

    #[test]
    fn registered_custom_disk_uses_distribution_name_and_exact_path() {
        let registrations =
            vec![Registration { name: "Ubuntu-24.04".into(), path: PathBuf::from("D:/Linux/Work/my-disk.vhdx") }];
        let info = identify(Path::new("d:/linux/work/MY-DISK.vhdx"), &registrations);
        assert_eq!(info.distribution.as_deref(), Some("Ubuntu-24.04"));
        assert_eq!(info.kind, DiskKind::Linux);
        assert!(identify(Path::new("D:/Linux/Backup/my-disk.vhdx"), &registrations).distribution.is_none());
    }

    #[test]
    fn docker_data_and_system_disks_are_distinct() {
        assert_eq!(
            identify(Path::new("C:/Users/NickD/AppData/Local/Docker/wsl/disk/docker_data.vhdx"), &[]).kind,
            DiskKind::DockerData
        );
        assert_eq!(
            identify(Path::new("C:/Users/NickD/AppData/Local/Docker/wsl/main/ext4.vhdx"), &[]).kind,
            DiskKind::DockerSystem
        );
        let registrations =
            vec![Registration { name: "docker-desktop-data".into(), path: PathBuf::from("D:/Docker/ext4.vhdx") }];
        assert_eq!(identify(Path::new("D:/Docker/ext4.vhdx"), &registrations).kind, DiskKind::DockerData);
    }

    #[test]
    fn unregistered_store_disk_keeps_package_without_guessing_distribution() {
        let info = identify(
            Path::new("C:/Users/NickD/AppData/Local/Packages/CanonicalGroupLimited.Ubuntu_abc/LocalState/ext4.vhdx"),
            &[],
        );
        assert_eq!(info.kind, DiskKind::Linux);
        assert!(info.distribution.is_none());
        assert_eq!(info.package.as_deref(), Some("CanonicalGroupLimited.Ubuntu_abc"));
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_paths_match_registrations() {
        let registrations =
            vec![Registration { name: "Debian".into(), path: PathBuf::from(r"\\?\D:\Linux\ext4.vhdx") }];
        assert_eq!(identify(Path::new("D:/Linux/ext4.vhdx"), &registrations).distribution.as_deref(), Some("Debian"));
    }

    #[cfg(windows)]
    #[test]
    fn current_user_registrations_can_be_identified_without_starting_wsl() {
        for entry in super::registry::read() {
            let info = super::inspect(&entry.path);
            assert_eq!(info.distribution.as_deref(), Some(entry.name.as_str()));
            assert!(super::recognized(&entry.path));
            if entry.path.is_file() {
                assert!(info.file_size.is_some());
                assert!(info.size_on_disk.is_some());
            }
        }
    }

    #[test]
    fn standard_layouts_only() {
        for path in [
            "C:/Users/NickD/AppData/Local/Docker/wsl/disk/docker_data.vhdx",
            "C:/Users/NickD/AppData/Local/Packages/CanonicalGroupLimited.Ubuntu/LocalState/ext4.vhdx",
            "C:/Users/NickD/AppData/Local/wsl/{guid}/ext4.vhdx",
        ] {
            assert!(super::standard_path(path));
        }
        for path in
            ["C:/VMs/ext4.vhdx", "C:/VMs/docker_data.vhdx", "C:/Users/NickD/AppData/Local/Docker/wsl/disk/backup.vhdx"]
        {
            assert!(!super::standard_path(path));
        }
    }
}
