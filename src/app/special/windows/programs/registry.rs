//! Windows configured folders and the same uninstall registrations used by Installed Apps.
use super::{Action, Application, Inventory};
use crate::platform::wide;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    System::{
        Com::CoTaskMemFree,
        Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RRF_RT_REG_DWORD,
            RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
        },
    },
    UI::{
        Shell::{
            FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX64, FOLDERID_ProgramFilesX86, FOLDERID_System,
            KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath, ShellExecuteW,
        },
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns an opened registry handle.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

impl Key {
    fn open(root: HKEY, name: &str, view: u32) -> Option<Self> {
        let name = wide(OsStr::new(name));
        let mut key = std::ptr::null_mut();
        // SAFETY: valid predefined/open root, NUL-terminated name and writable handle output.
        if unsafe { RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ | view, &raw mut key) } == 0 {
            Some(Self(key))
        } else {
            None
        }
    }

    fn string(&self, name: &str) -> Option<String> {
        let name = wide(OsStr::new(name));
        let mut bytes = 0;
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        // SAFETY: the first query obtains the required capacity, including expanded strings.
        if unsafe {
            RegGetValueW(
                self.0,
                std::ptr::null(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut bytes,
            )
        } != 0
            || bytes > 65_536
        {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        // SAFETY: buffer has the requested capacity, names and handle remain live.
        if unsafe {
            RegGetValueW(
                self.0,
                std::ptr::null(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &raw mut bytes,
            )
        } != 0
        {
            return None;
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end]).ok()
    }

    fn dword(&self, name: &str) -> Option<u32> {
        let name = wide(OsStr::new(name));
        let (mut value, mut bytes) = (0u32, 4u32);
        // SAFETY: DWORD output with the matching capacity and valid handle/value name.
        (unsafe {
            RegGetValueW(
                self.0,
                std::ptr::null(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut value).cast(),
                &raw mut bytes,
            )
        } == 0)
            .then_some(value)
    }

    fn children(&self) -> Vec<String> {
        let mut result = Vec::new();
        for index in 0..65_536 {
            let mut name = [0u16; 256];
            let mut length = 255;
            // SAFETY: sized output buffer, all optional outputs null.
            if unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } != 0
            {
                break;
            }
            if let Ok(name) = String::from_utf16(&name[..length as usize]) {
                result.push(name);
            }
        }
        result
    }
}

fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    let mut value = std::ptr::null_mut();
    // SAFETY: valid folder ID and output pointer. Do not request the default path;
    // the configured location is what matters, even when it has been relocated.
    let status = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY as u32, std::ptr::null_mut(), &raw mut value) };
    let result = if status >= 0 && !value.is_null() {
        let mut length = 0;
        // SAFETY: a successful API result is a NUL-terminated allocated UTF-16 string.
        while unsafe { *value.wrapping_add(length) } != 0 {
            length += 1;
        }
        // SAFETY: length was measured within the returned string.
        let units = unsafe { std::slice::from_raw_parts(value, length) };
        use std::os::windows::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_wide(units)))
    } else {
        None
    };
    // SAFETY: the API requires freeing the output even on failure; null is permitted.
    unsafe {
        CoTaskMemFree(value.cast());
    }
    result
}

pub(super) fn steam_path() -> Option<PathBuf> {
    Key::open(HKEY_CURRENT_USER, r"Software\Valve\Steam", 0)
        .and_then(|key| key.string("SteamPath"))
        .or_else(|| {
            Key::open(HKEY_LOCAL_MACHINE, r"Software\Valve\Steam", KEY_WOW64_32KEY)
                .and_then(|key| key.string("InstallPath"))
        })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub(super) fn program_roots() -> &'static [PathBuf] {
    static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_ProgramFilesX64]
            .iter()
            .filter_map(known_folder)
            .collect()
    })
}

pub(super) fn inventory() -> Inventory {
    let mut inventory = Inventory { program_roots: program_roots().to_vec(), ..Default::default() };
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Some(key) = Key::open(root, r"Software\Microsoft\Windows\CurrentVersion\Uninstall", view) else {
                continue;
            };
            for name in key.children() {
                let Some(app) = Key::open(key.0, &name, view) else { continue };
                let Some(display_name) = app.string("DisplayName").filter(|s| !s.trim().is_empty()) else { continue };
                let Some(location) = app.string("InstallLocation").map(|s| PathBuf::from(s.trim().trim_matches('"')))
                else {
                    continue;
                };
                // Empty locations and drive roots cannot establish application ownership.
                if !location.is_absolute()
                    || location.parent().is_none()
                    || location.components().any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    continue;
                }
                let action = if app.dword("NoRemove") == Some(1) || app.dword("SystemComponent") == Some(1) {
                    None
                } else if let Some(id) =
                    name.strip_prefix("Steam App ").and_then(|s| s.parse::<u32>().ok()).filter(|&id| id != 0)
                {
                    Some(Action::Steam(id))
                } else if app.dword("WindowsInstaller") == Some(1) && product_code(&name) {
                    known_folder(&FOLDERID_System).map(|system| Action::Uninstall {
                        executable: system.join("msiexec.exe"),
                        arguments: format!("/x {name}"),
                    })
                } else {
                    app.string("UninstallString").as_deref().and_then(uninstall_command)
                };
                inventory.applications.push(Application {
                    name: display_name,
                    publisher: app.string("Publisher").unwrap_or_default(),
                    version: app.string("DisplayVersion").unwrap_or_default(),
                    location,
                    action: action.filter(|action| match action {
                        Action::Uninstall { executable, .. } => executable.is_file(),
                        Action::Steam(_) => true,
                    }),
                });
            }
        }
    }
    inventory
}

fn product_code(value: &str) -> bool {
    value.len() == 38
        && value.starts_with('{')
        && value.ends_with('}')
        && value.bytes().enumerate().all(|(i, c)| match i {
            0 | 37 => true,
            9 | 14 | 19 | 24 => c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Preserve registered arguments verbatim; never hand the command to cmd.exe or
/// guess an ambiguous unquoted executable containing spaces. Installed Apps is the fallback.
fn uninstall_command(command: &str) -> Option<Action> {
    let command = command.trim();
    if command.contains('\0') {
        return None;
    }
    let (file, arguments) = if let Some(quoted) = command.strip_prefix('"') {
        let end = quoted.find('"')?;
        let tail = &quoted[end + 1..];
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return None;
        }
        (&quoted[..end], tail.trim_start())
    } else {
        command.split_once(char::is_whitespace).unwrap_or((command, ""))
    };
    let file = Path::new(file);
    if !file.is_absolute() || !file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
        return None;
    }
    Some(Action::Uninstall { executable: file.to_owned(), arguments: arguments.to_owned() })
}

pub(super) fn launch(executable: &Path, arguments: &str) -> Result<(), String> {
    if !executable.is_file() {
        return Err(crate::i18n::tr!("uninstaller-missing"));
    }
    let file = wide(executable.as_os_str());
    let args = wide(OsStr::new(arguments));
    // SAFETY: separate NUL-terminated executable/argument buffers remain valid for the call.
    // ShellExecute honors the registered installer's manifest, including Windows elevation.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            file.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as usize;
    if result > 32 { Ok(()) } else { Err(crate::i18n::tr!("uninstaller-launch-error", code = (result as u32))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uninstall_commands_require_an_unambiguous_absolute_executable() {
        assert_eq!(
            uninstall_command(r#""D:\Moved Apps\Example\uninstall.exe" /uninstall "two words""#),
            Some(Action::Uninstall {
                executable: r"D:\Moved Apps\Example\uninstall.exe".into(),
                arguments: r#"/uninstall "two words""#.into(),
            })
        );
        for command in [r"D:\Moved Apps\uninstall.exe /S", "uninstall.exe", r#""D:\app.exe"extra"#, "cmd /c del", ""] {
            assert!(uninstall_command(command).is_none(), "{command}");
        }
        assert!(product_code("{12345678-1234-5678-90AB-123456789ABC}"));
        assert!(!product_code("{12345678-1234-5678-90AB-123456789ABZ}"));
    }
}
