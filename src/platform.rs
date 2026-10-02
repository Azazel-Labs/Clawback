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
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let path = wide(disk.mount.as_os_str());
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
    let path = std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), |p| strip_verbatim(&p));
    // Windows paths are case-insensitive; either way, compare whole components.
    let fold = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    let folded = cfg!(windows).then(|| fold(&path));
    disks
        .iter()
        .filter(|d| path.starts_with(&d.mount) || folded.as_ref().is_some_and(|f| f.starts_with(fold(&d.mount))))
        .max_by_key(|d| d.mount.as_os_str().len())
        .cloned()
}

/// `\\?\C:\x` becomes `C:\x` and `\\?\UNC\server\share\x` becomes `\\server\share\x`:
/// the forms mount points and the shell use. Other paths are returned unchanged.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else { return path.to_path_buf() };
    let mut plain = std::ffi::OsString::new();
    match prefix.kind() {
        Prefix::VerbatimDisk(drive) => plain.push(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            plain.push(r"\\");
            plain.push(server);
            plain.push(r"\");
            plain.push(share);
        }
        _ => return path.to_path_buf(),
    }
    plain.push(components.as_path());
    plain.into()
}

/// A NUL-terminated UTF-16 copy of `value` for Win32 calls.
#[cfg(windows)]
pub(crate) fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
pub(crate) use self::windows::Apartment;

/// Open a file with its default application, or a folder in the file manager.
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

pub fn file_manager_label() -> String {
    if cfg!(windows) {
        tr!("open-in-explorer")
    } else if cfg!(target_os = "macos") {
        tr!("open-in-finder")
    } else {
        tr!("open-in-file-manager")
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

/// Human-readable attribute names for the info tip and properties.
pub fn attributes(path: &Path) -> Vec<String> {
    let Ok(md) = std::fs::symlink_metadata(path) else { return Vec::new() };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_ENCRYPTED,
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_ATTRIBUTE_SPARSE_FILE, FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_TEMPORARY,
        };
        let names = [
            (FILE_ATTRIBUTE_ARCHIVE, tr!("attribute-backup-flag")),
            (FILE_ATTRIBUTE_COMPRESSED, tr!("attribute-compressed")),
            (FILE_ATTRIBUTE_DIRECTORY, tr!("folder-2")),
            (FILE_ATTRIBUTE_ENCRYPTED, tr!("attribute-encrypted")),
            (FILE_ATTRIBUTE_HIDDEN, tr!("hidden")),
            (FILE_ATTRIBUTE_OFFLINE, tr!("offline")),
            (FILE_ATTRIBUTE_READONLY, tr!("read-only")),
            (FILE_ATTRIBUTE_REPARSE_POINT, tr!("attribute-reparse-point")),
            (FILE_ATTRIBUTE_SPARSE_FILE, tr!("sparse")),
            (FILE_ATTRIBUTE_SYSTEM, tr!("system")),
            (FILE_ATTRIBUTE_TEMPORARY, tr!("attribute-temporary")),
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
    use super::wide;
    use ::windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use std::{marker::PhantomData, path::Path};
    use windows_sys::Win32::{
        System::Console::{ATTACH_PARENT_PROCESS, AttachConsole},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWDEFAULT},
    };

    /// Single-threaded COM on the current thread until dropped. Not `Send`:
    /// it must be released on the thread that entered it.
    pub struct Apartment(PhantomData<*const ()>);
    impl Apartment {
        pub fn enter() -> ::windows::core::Result<Self> {
            // SAFETY: plain initialization; any success (including S_FALSE) is balanced on drop.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
            Ok(Self(PhantomData))
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: balances the successful initialization on this same thread.
            unsafe { CoUninitialize() };
        }
    }

    pub(super) fn shell_execute(path: &Path) -> Result<(), String> {
        let file = wide(path.as_os_str());
        let dir = path.parent().map(|p| wide(p.as_os_str()));
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

    #[cfg(windows)]
    #[test]
    fn disk_lookup_matches_whole_components_ignoring_case() {
        let d = |m: &str| DiskInfo {
            name: String::new(),
            mount: m.into(),
            fs: String::new(),
            total: 1,
            free: 1,
            removable: false,
            kind: clawback_core::adaptive::StorageKind::Unknown,
        };
        let disks = [d(r"Q:\"), d(r"Q:\Mnt\X")];
        let found = |p: &str| disk_for(Path::new(p), &disks).unwrap().mount;
        assert_eq!(found(r"q:\mnt\x\file"), PathBuf::from(r"Q:\Mnt\X"));
        assert_eq!(found(r"q:\mnt\xyz\file"), PathBuf::from(r"Q:\"));
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefixes_are_stripped() {
        let strip = |p: &str| strip_verbatim(Path::new(p));
        assert_eq!(strip(r"\\?\C:\Users\x"), PathBuf::from(r"C:\Users\x"));
        assert_eq!(strip(r"\\?\C:\"), PathBuf::from(r"C:\"));
        assert_eq!(strip(r"\\?\UNC\server\share\dir"), PathBuf::from(r"\\server\share\dir"));
        assert_eq!(strip(r"\\?\Volume{1}\x"), PathBuf::from(r"\\?\Volume{1}\x"));
        assert_eq!(strip(r"C:\plain"), PathBuf::from(r"C:\plain"));
    }
}
