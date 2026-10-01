//! Operating-system integration: volumes, opening files, revealing them in
//! the file manager, the trash, the folder picker and file attributes.
use crate::i18n::tr;

use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskInfo {
    pub name: String,
    pub mount: PathBuf,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
    pub kind: clawback_core::adaptive::StorageKind,
}

impl DiskInfo {
    /// A short label: "Macintosh HD (/)", "Local Disk (C:\)", "/home".
    pub fn label(&self) -> String {
        let mount = self.mount.display().to_string();
        if self.name.is_empty() || self.name == mount || self.name.starts_with("/dev/") {
            mount
        } else {
            format!("{} ({mount})", self.name)
        }
    }
}

/// Pseudo and container filesystems that are never useful to scan.
const HIDDEN_FS: &[&str] = &[
    "squashfs",
    "overlay",
    "tmpfs",
    "devtmpfs",
    "ramfs",
    "autofs",
    "proc",
    "sysfs",
    "nsfs",
    "efivarfs",
    "devfs",
    "fuse.snapfuse",
    "fuse.portal",
];

fn hidden_mount(m: &Path) -> bool {
    let s = m.to_string_lossy();
    // macOS: the Data volume is reached from "/" through firmlinks, and the
    // other /System/Volumes entries are system internals.
    s.starts_with("/System/Volumes/")
        || s.starts_with("/private/var/vm")
        || s.starts_with("/snap/")
        || s.starts_with("/var/lib/docker/")
        || s.starts_with("/run/")
}

/// Every mounted volume, including ones hidden from the drive list.
pub fn all_disks() -> Vec<DiskInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut v: Vec<DiskInfo> = disks
        .list()
        .iter()
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount: d.mount_point().to_path_buf(),
            fs: d.file_system().to_string_lossy().into_owned(),
            total: d.total_space(),
            free: d.available_space(),
            removable: d.is_removable(),
            kind: match d.kind() {
                sysinfo::DiskKind::HDD => clawback_core::adaptive::StorageKind::Rotational,
                sysinfo::DiskKind::SSD => clawback_core::adaptive::StorageKind::SolidState,
                sysinfo::DiskKind::Unknown(_) => clawback_core::adaptive::StorageKind::Unknown,
            },
        })
        .collect();
    v.sort_by(|a, b| a.mount.cmp(&b.mount));
    v.dedup_by(|a, b| a.mount == b.mount);
    v
}

/// Refresh capacity for the watched volume without enumerating other drives.
pub fn refresh_disk_space(disk: &mut DiskInfo) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let path: Vec<u16> = disk.mount.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: NUL-terminated path and valid output pointers; total-free is optional.
        if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &raw mut free, &raw mut total, std::ptr::null_mut()) } != 0 {
            disk.free = free;
            disk.total = total;
        }
    }
    #[cfg(not(windows))]
    if let Some(current) = all_disks().into_iter().find(|d| d.mount == disk.mount) {
        disk.free = current.free;
        disk.total = current.total;
    }
}

/// Volumes worth offering in the "Select Drive to View" dialog.
pub fn drive_list() -> Vec<DiskInfo> {
    all_disks()
        .into_iter()
        .filter(|d| d.total > 0 && !HIDDEN_FS.contains(&d.fs.as_str()) && !hidden_mount(&d.mount))
        .collect()
}

fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) { s.to_lowercase().trim_end_matches(['\\', '/']).to_owned() } else { s.into_owned() }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    normalize(a) == normalize(b) || a == b
}

/// The volume containing `path` (longest matching mount point).
pub fn disk_for(path: &Path, disks: &[DiskInfo]) -> Option<DiskInfo> {
    let path = std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), strip_verbatim);
    let p = normalize(&path);
    disks
        .iter()
        .filter(|d| {
            let m = normalize(&d.mount);
            path.starts_with(&d.mount) || (cfg!(windows) && p.starts_with(&m))
        })
        .max_by_key(|d| d.mount.as_os_str().len())
        .cloned()
}

/// `canonicalize` on Windows returns `\\?\C:\...`; drop the prefix again.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

/// Open a file with its default application, or a folder in the file manager
/// (SpaceMonger's "Run / Open").
pub fn open(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::shell_execute(path)
    }
    #[cfg(target_os = "macos")]
    {
        spawn("open", &[path.as_os_str()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        spawn("xdg-open", &[path.as_os_str()])
    }
}

/// Show the item selected in the system file manager.
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("explorer: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        spawn("open", &[std::ffi::OsStr::new("-R"), path.as_os_str()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // The freedesktop FileManager1 interface selects the item in
        // Nautilus, Dolphin, Nemo, Thunar, ...; fall back to opening the parent.
        let uri = format!("array:string:{}", file_uri(path));
        let ok = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(uri)
            .arg("string:")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok { Ok(()) } else { spawn("xdg-open", &[path.parent().unwrap_or(path).as_os_str()]) }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn file_uri(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            let _ = write!(s, "%{b:02X}");
        }
    }
    s
}

#[cfg(unix)]
fn spawn(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;
    // Reap it in the background so it doesn't linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Native folder picker.
pub fn pick_folder(start: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(tr!("select-a-folder-to-view"));
    if let Some(s) = start {
        d = d.set_directory(s);
    }
    d.pick_folder()
}

/// SpaceMonger-style attribute names for the info tip and properties.
pub fn attributes(path: &Path) -> Vec<String> {
    let Ok(md) = std::fs::symlink_metadata(path) else { return Vec::new() };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let names = [
            (0x20, tr!("arch")),
            (0x800, tr!("compress")),
            (0x10, tr!("folder-2")),
            (0x4000, tr!("encrypt")),
            (0x2, tr!("hidden")),
            (0x1000, tr!("offline")),
            (0x1, tr!("read-only")),
            (0x400, tr!("reparse-pt")),
            (0x200, tr!("sparse")),
            (0x4, tr!("system")),
            (0x100, tr!("temp")),
        ];
        let a = md.file_attributes();
        names.into_iter().filter(|(bit, _)| a & bit != 0).map(|(_, n)| n).collect()
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut v = Vec::new();
        if md.is_dir() {
            v.push(tr!("folder-2"));
        }
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            v.push(tr!("hidden"));
        }
        if md.permissions().readonly() {
            v.push(tr!("read-only"));
        }
        if md.file_type().is_symlink() {
            v.push(tr!("symlink"));
        }
        v.push(permission_string(md.permissions().mode()));
        v
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = md;
        Vec::new()
    }
}

/// `rwxr-xr-x`.
#[cfg(any(unix, test))]
pub fn permission_string(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

/// Let `clawback --report` print to the terminal it was started from, even
/// though release builds use the Windows GUI subsystem.
pub fn attach_console() {
    #[cfg(windows)]
    windows::attach_console();
}

/// Platform-specific advice shown with the list of unreadable folders.
pub fn permission_hint() -> String {
    if cfg!(target_os = "macos") {
        tr!("permission-hint-macos")
    } else if cfg!(windows) {
        tr!("permission-hint-windows")
    } else {
        tr!("permission-hint-unix")
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut c_void,
            op: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> *mut c_void;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub(super) fn shell_execute(path: &Path) -> Result<(), String> {
        const SW_SHOWDEFAULT: i32 = 10;
        let file = wide(path);
        let dir = path.parent().map(wide);
        let dir_ptr = dir.as_ref().map_or(std::ptr::null(), Vec::as_ptr);
        // SAFETY: all strings are NUL-terminated UTF-16 that outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                file.as_ptr(),
                std::ptr::null(),
                dir_ptr,
                SW_SHOWDEFAULT,
            )
        };
        // ShellExecute returns a value greater than 32 on success.
        if r as usize > 32 { Ok(()) } else { Err(format!("Windows could not open the file (error {})", r as usize)) }
    }

    pub(super) fn attach_console() {
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: plain Win32 call with no pointers.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_strings() {
        assert_eq!(permission_string(0o755), "rwxr-xr-x");
        assert_eq!(permission_string(0o640), "rw-r-----");
    }

    #[test]
    fn disk_lookup_picks_longest_mount() {
        let d = |m: &str| DiskInfo {
            name: String::new(),
            mount: m.into(),
            fs: String::new(),
            total: 1,
            free: 1,
            removable: false,
            kind: clawback_core::adaptive::StorageKind::Unknown,
        };
        let disks = [d("/"), d("/home"), d("/home/azazel-labs/media")];
        let found = disk_for(Path::new("/home/azazel-labs/media/x/y-that-does-not-exist"), &disks).unwrap();
        assert_eq!(found.mount, PathBuf::from("/home/azazel-labs/media"));
        assert!(same_path(Path::new("/home"), Path::new("/home")));
    }
}
