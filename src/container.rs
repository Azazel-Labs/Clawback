//! Container mode: the Linux build streamed to a browser from Docker, over volumes mapped
//! into one folder.
//!
//! The image turns it on through the environment, so desktop installs never see it:
//! `CLAWBACK_CONTAINER` enables it, `CLAWBACK_DATA` names the folder holding the volumes
//! (`/data` by default), and `CLAWBACK_ALLOW_DELETE=false` removes deletion entirely.
//! There is no Trash to recover from inside a container, so every delete is permanent and
//! confirmed. Volumes mounted read-only are browsed but never offered for deletion.
use crate::platform::DiskInfo;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Config {
    root: PathBuf,
    allow_delete: bool,
    /// Mapped volumes at or under `root`. A container's mounts are fixed when it starts.
    mounts: Vec<Mount>,
}

#[derive(Debug, PartialEq, Eq)]
struct Mount {
    path: PathBuf,
    fs: String,
    read_only: bool,
}

fn config() -> Option<&'static Config> {
    static CONFIG: OnceLock<Option<Config>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            if !std::env::var_os("CLAWBACK_CONTAINER").is_some_and(|value| enabled_value(&value)) {
                return None;
            }
            let root = std::env::var_os("CLAWBACK_DATA").map_or_else(|| PathBuf::from("/data"), PathBuf::from);
            let allow_delete = std::env::var_os("CLAWBACK_ALLOW_DELETE").is_none_or(|value| enabled_value(&value));
            let table = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
            Some(Config { mounts: parse_mounts(&table, &root), root, allow_delete })
        })
        .as_ref()
}

/// Anything but empty, `0`, `false`, `no` or `off` turns a switch on.
fn enabled_value(value: &OsStr) -> bool {
    let value = value.to_string_lossy();
    !["", "0", "false", "no", "off"].contains(&value.trim().to_ascii_lowercase().as_str())
}

pub fn enabled() -> bool {
    config().is_some()
}

/// The folder holding the mapped volumes, opened at startup.
pub fn root() -> Option<&'static Path> {
    config().map(|config| config.root.as_path())
}

/// Whether container mode removed deletion altogether.
pub fn delete_forbidden() -> bool {
    config().is_some_and(|config| !config.allow_delete)
}

/// Whether `path` may be deleted: always on the desktop; in a container, only when deletion
/// is allowed and the volume holding it is writable.
pub fn can_delete(path: &Path) -> bool {
    config().is_none_or(|config| {
        config.allow_delete
            && !config
                .mounts
                .iter()
                .filter(|mount| path.starts_with(&mount.path))
                .max_by_key(|mount| mount.path.as_os_str().len())
                .is_some_and(|mount| mount.read_only)
    })
}

/// Whether `mount` is one of the mapped volumes and read-only.
pub fn read_only(mount: &Path) -> bool {
    config().is_some_and(|config| config.mounts.iter().any(|m| m.path == mount && m.read_only))
}

/// The mapped volumes as drives, with current capacity. Network shares are included, so this
/// can block on an unresponsive server; call it off the UI thread.
pub fn drives() -> Vec<DiskInfo> {
    let Some(config) = config() else { return Vec::new() };
    config
        .mounts
        .iter()
        .map(|mount| {
            let (total, free) = space(&mount.path).unwrap_or_default();
            DiskInfo {
                name: if mount.path == config.root {
                    String::new()
                } else {
                    mount.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
                },
                mount: mount.path.clone(),
                fs: mount.fs.clone(),
                total,
                free,
                removable: false,
                kind: clawback_core::adaptive::StorageKind::Unknown,
            }
        })
        .collect()
}

/// Mounts at or under `root` from a `/proc/self/mounts` table; a later mount on the same
/// path hides the earlier one.
fn parse_mounts(table: &str, root: &Path) -> Vec<Mount> {
    let mut mounts: Vec<Mount> = Vec::new();
    for line in table.lines() {
        let mut fields = line.split_ascii_whitespace();
        let (Some(_device), Some(path), Some(fs), Some(options)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = PathBuf::from(unescape(path));
        if !path.starts_with(root) {
            continue;
        }
        mounts.retain(|mount| mount.path != path);
        mounts.push(Mount { path, fs: fs.to_owned(), read_only: options.split(',').any(|option| option == "ro") });
    }
    mounts.sort_by(|a, b| a.path.cmp(&b.path));
    mounts
}

/// Undo the kernel's octal escapes (`\040` for a space) in a mount path.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && let Some(code) = bytes.get(i + 1..i + 4)
            && code.iter().all(|digit| (b'0'..=b'7').contains(digit))
        {
            out.push(code.iter().fold(0u8, |value, digit| value.wrapping_mul(8) + (digit - b'0')));
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Total and available bytes on the filesystem holding `path`.
#[cfg(target_os = "linux")]
fn space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a NUL-terminated path and a properly sized, writable output struct.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded, so it filled in the struct.
    let stat = unsafe { stat.assume_init() };
    // The field widths differ between 32- and 64-bit targets.
    #[allow(clippy::useless_conversion)]
    let (block, blocks, available) = (u64::from(stat.f_frsize), u64::from(stat.f_blocks), u64::from(stat.f_bavail));
    Some((block * blocks, block * available))
}

#[cfg(not(target_os = "linux"))]
fn space(_path: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_volumes_come_from_the_mount_table() {
        let table = "\
overlay / overlay rw,relatime,lowerdir=/a 0 0
/dev/sda1 /etc/hosts ext4 rw,relatime 0 0
/dev/sdb1 /data/media ext4 rw,relatime 0 0
nas:/backups /data/old\\040backups nfs4 ro,relatime,vers=4.2 0 0
/dev/sdc1 /data/media ext4 ro,relatime 0 0
/dev/sdd1 /data/media/inner xfs rw 0 0
/dev/sde1 /database ext4 rw 0 0
truncated
";
        let mounts = parse_mounts(table, Path::new("/data"));
        let mount = |path: &str, fs: &str, read_only| Mount { path: PathBuf::from(path), fs: fs.to_owned(), read_only };
        assert_eq!(
            mounts,
            [
                mount("/data/media", "ext4", true),
                mount("/data/media/inner", "xfs", false),
                mount("/data/old backups", "nfs4", true),
            ]
        );
    }

    #[test]
    fn switches_and_escapes() {
        for value in ["1", "true", "YES", "on", "anything"] {
            assert!(enabled_value(OsStr::new(value)), "{value}");
        }
        for value in ["", " ", "0", "false", "No", "OFF"] {
            assert!(!enabled_value(OsStr::new(value)), "{value}");
        }
        assert_eq!(unescape(r"/data/a\040b\011c\d\04"), "/data/a b\tc\\d\\04");
    }
}
