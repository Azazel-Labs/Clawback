//! Windows configured folders and the same uninstall registrations used by Installed Apps.
//!
//! Most registrations leave `InstallLocation` empty, so a registration's folder is also
//! recognized from its icon, its uninstaller, the files Windows Installer recorded for it,
//! or a folder named after it.
use super::{Action, Application, Inventory, normalized, within};
use crate::platform::wide;
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    System::{
        Com::CoTaskMemFree,
        Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ,
            REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegEnumValueW,
            RegGetValueW, RegOpenKeyExW,
        },
    },
    UI::{
        Shell::{
            FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData, FOLDERID_Profile,
            FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesCommon, FOLDERID_ProgramFilesCommonX64,
            FOLDERID_ProgramFilesCommonX86, FOLDERID_ProgramFilesX64, FOLDERID_ProgramFilesX86,
            FOLDERID_RoamingAppData, FOLDERID_System, FOLDERID_UserProfiles, FOLDERID_UserProgramFiles,
            FOLDERID_Windows, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath, ShellExecuteW,
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

    /// Every string value as (name, data); values too long to be a path are skipped.
    fn strings(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        let mut name = [0u16; 256];
        let mut data = vec![0u16; 2048];
        for index in 0..65_536 {
            let mut length = name.len() as u32;
            let mut bytes = (data.len() * 2) as u32;
            let mut kind = 0;
            // SAFETY: sized name and data buffers with their capacities; reserved is null.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut length,
                    std::ptr::null(),
                    &raw mut kind,
                    data.as_mut_ptr().cast(),
                    &raw mut bytes,
                )
            };
            match status {
                0 => {}
                234 => continue, // ERROR_MORE_DATA: too long for a path.
                _ => break,
            }
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                continue;
            }
            let units = &data[..(bytes as usize / 2).min(data.len())];
            let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
            if let (Ok(name), Ok(value)) =
                (String::from_utf16(&name[..length as usize]), String::from_utf16(&units[..end]))
            {
                result.push((name, value));
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
    static ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_ProgramFilesX64]
            .iter()
            .filter_map(known_folder)
            .collect()
    })
}

/// Folders no single registration may claim.
struct Places {
    /// Shared by many applications: no application owns one of these, or anything above it.
    shared: Vec<PathBuf>,
    /// Nothing inside these belongs to one application.
    excluded: Vec<PathBuf>,
    /// Where installers put shortcuts and settings rather than the application itself.
    data: Vec<PathBuf>,
}

fn places() -> &'static Places {
    static PLACES: OnceLock<Places> = OnceLock::new();
    PLACES.get_or_init(|| {
        let mut shared: Vec<_> = [
            FOLDERID_ProgramFilesCommon,
            FOLDERID_ProgramFilesCommonX86,
            FOLDERID_ProgramFilesCommonX64,
            FOLDERID_ProgramData,
            FOLDERID_UserProfiles,
            FOLDERID_Profile,
            FOLDERID_LocalAppData,
            FOLDERID_RoamingAppData,
            FOLDERID_UserProgramFiles,
            FOLDERID_Desktop,
            FOLDERID_Documents,
            FOLDERID_Downloads,
        ]
        .iter()
        .filter_map(known_folder)
        .collect();
        shared.extend(program_roots().iter().cloned());
        let mut excluded: Vec<_> = known_folder(&FOLDERID_Windows).into_iter().collect();
        excluded.extend(known_folder(&FOLDERID_ProgramData).map(|data| data.join("Package Cache")));
        excluded.extend(known_folder(&FOLDERID_LocalAppData).map(|local| local.join("Temp")));
        excluded.push(std::env::temp_dir());
        let data = [FOLDERID_ProgramData, FOLDERID_UserProfiles].iter().filter_map(known_folder).collect();
        Places { shared, excluded, data }
    })
}

/// Whether `folder` could belong to a single application.
fn plausible(folder: &Path, places: &Places) -> bool {
    folder.is_absolute()
        && folder.parent().is_some()
        && !folder.components().any(|c| matches!(c, std::path::Component::ParentDir))
        && !places.shared.iter().any(|shared| within(shared, folder))
        && !places.excluded.iter().any(|excluded| within(folder, excluded))
}

/// The application folder holding `file`, stepping out of the binary and uninstaller
/// folders many applications keep their executables in.
fn folder_of(file: &Path) -> Option<PathBuf> {
    const NESTED: &[&str] = &[
        "bin",
        "bin32",
        "bin64",
        "32bit",
        "64bit",
        "x64",
        "x86",
        "amd64",
        "win32",
        "win64",
        "uninstall",
        "uninstaller",
        "uninst",
    ];
    let mut folder = file.parent()?;
    while let (Some(name), Some(parent)) = (folder.file_name(), folder.parent())
        && NESTED.iter().any(|nested| name.eq_ignore_ascii_case(nested))
    {
        folder = parent;
    }
    Some(folder.to_owned())
}

/// The file a `DisplayIcon` names, without its quotes or icon index.
fn icon_file(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let value = match value.rsplit_once(',') {
        Some((file, index)) if index.trim().parse::<i32>().is_ok() => file.trim(),
        _ => value,
    };
    let file = Path::new(value.trim_matches('"'));
    (file.is_absolute() && file.extension().is_some()).then(|| file.to_owned())
}

/// A folder named after the application: `Vendor\Product` or `Product` in Program Files.
fn named_folder(roots: &[PathBuf], publisher: &str, name: &str) -> Option<PathBuf> {
    const SUFFIXES: &[&str] = &[", Inc.", " Inc.", ", Inc", " Inc", " Corporation", " Corp.", " LLC", " Ltd.", " GmbH"];
    let segment = |value: &str| {
        let value = value.trim();
        let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
        (!value.is_empty() && value != "." && value != ".." && !value.contains(forbidden)).then(|| value.to_owned())
    };
    let name = segment(name)?;
    let mut vendors: Vec<String> = segment(publisher).into_iter().collect();
    if let Some(publisher) = vendors.first().cloned() {
        vendors.extend(SUFFIXES.iter().filter_map(|suffix| publisher.strip_suffix(suffix).and_then(segment)));
    }
    roots.iter().find_map(|root| {
        vendors
            .iter()
            .map(|vendor| root.join(vendor).join(&name))
            .chain(std::iter::once(root.join(&name)))
            .find(|folder| folder.is_dir())
    })
}

/// A product code in the packed form Windows Installer's own registry keys use.
fn packed(code: &str) -> Option<String> {
    if !product_code(code) {
        return None;
    }
    let hex: Vec<char> = code.chars().filter(char::is_ascii_hexdigit).map(|c| c.to_ascii_uppercase()).collect();
    let mut packed: String =
        hex[..8].iter().rev().chain(hex[8..12].iter().rev()).chain(hex[12..16].iter().rev()).collect();
    for pair in hex[16..].chunks(2) {
        packed.extend([pair[1], pair[0]]);
    }
    Some(packed)
}

/// The folder most of `folders` lie in. Above an application folder (`own` is false),
/// descend while at least four in five agree, so a stray shortcut or shared library does
/// not widen it to a common ancestor; inside one, descend only while all agree, so a
/// large subfolder does not narrow it to a part of the application.
fn dominant(folders: &[PathBuf], own: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let parts: Vec<Vec<String>> =
        folders.iter().map(|folder| normalized(folder).split('\\').map(str::to_owned).collect()).collect();
    let mut members: Vec<usize> = (0..parts.len()).collect();
    let mut depth = 0;
    loop {
        let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for &index in &members {
            if let Some(part) = parts[index].get(depth).filter(|part| !part.is_empty()) {
                groups.entry(part).or_default().push(index);
            }
        }
        let Some(best) = groups.into_values().max_by_key(Vec::len) else { break };
        let unanimous = best.len() == members.len();
        let settled = depth > 1 && own(&prefix(&folders[members[0]], depth));
        if (settled && !unanimous) || best.len() * 5 < parts.len() * 4 {
            break;
        }
        members = best;
        depth += 1;
    }
    // Keep the original spelling of the first agreeing folder.
    (depth > 1).then(|| prefix(&folders[members[0]], depth))
}

/// The first `depth` parts of `path`, counting the drive and its root as one.
fn prefix(path: &Path, depth: usize) -> PathBuf {
    let mut result = PathBuf::new();
    let mut taken = 0;
    for component in path.components() {
        if taken == depth {
            break;
        }
        result.push(component);
        if !matches!(component, std::path::Component::Prefix(_)) {
            taken += 1;
        }
    }
    result
}

type Folders = Arc<HashMap<String, PathBuf>>;

/// Each Windows Installer product's folder, from where its components' files are.
/// Reading every component takes seconds, so the result is kept and, once stale,
/// refreshed in the background while the previous one is still returned.
fn installer_folders() -> Folders {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CACHE: Mutex<Option<(Instant, Folders)>> = Mutex::new(None);
    static READING: Mutex<()> = Mutex::new(());
    static REFRESHING: AtomicBool = AtomicBool::new(false);
    let store = |folders: &Folders| *clawback_core::scan::lock(&CACHE) = Some((Instant::now(), folders.clone()));
    let cached = clawback_core::scan::lock(&CACHE).clone();
    if let Some((at, folders)) = cached {
        if at.elapsed() > Duration::from_secs(600) && !REFRESHING.swap(true, Ordering::AcqRel) {
            std::thread::spawn(move || {
                let folders = read_installer_folders();
                store(&folders);
                REFRESHING.store(false, Ordering::Release);
            });
        }
        return folders;
    }
    // The first callers wait for one reading rather than each starting their own.
    let _reading = clawback_core::scan::lock(&READING);
    if let Some((_, folders)) = clawback_core::scan::lock(&CACHE).as_ref() {
        return folders.clone();
    }
    let folders = read_installer_folders();
    store(&folders);
    folders
}

fn read_installer_folders() -> Folders {
    let places = places();
    let mut files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let users = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData";
    if let Some(users) = Key::open(HKEY_LOCAL_MACHINE, users, KEY_WOW64_64KEY) {
        for user in users.children() {
            let Some(components) = Key::open(users.0, &format!(r"{user}\Components"), KEY_WOW64_64KEY) else {
                continue;
            };
            for component in components.children() {
                let Some(key) = Key::open(components.0, &component, KEY_WOW64_64KEY) else { continue };
                for (product, value) in key.strings() {
                    // Registry key paths start with a hive number; `?` marks unusual entries.
                    let value = value.trim();
                    let bytes = value.as_bytes();
                    if product.len() != 32
                        || product.bytes().all(|c| c == b'0')
                        || value.contains('?')
                        || bytes.len() < 4
                        || !bytes[0].is_ascii_alphabetic()
                        || &bytes[1..3] != b":\\"
                    {
                        continue;
                    }
                    let folder = if value.ends_with('\\') {
                        PathBuf::from(value)
                    } else {
                        Path::new(value).parent().map(Path::to_owned).unwrap_or_default()
                    };
                    if places.excluded.iter().chain(&places.data).any(|place| within(&folder, place)) {
                        continue;
                    }
                    files.entry(product.to_ascii_uppercase()).or_default().push(folder);
                }
            }
        }
    }
    let folders: Folders = Arc::new(
        files
            .into_iter()
            .filter_map(|(product, folders)| {
                let own = |folder: &Path| plausible(folder, places);
                Some((product, dominant(&folders, &own).filter(|f| own(f))?))
            })
            .collect(),
    );
    folders
}

/// Read Windows Installer's records ahead of the first right-click that needs them.
pub(super) fn prepare() {
    std::thread::spawn(|| drop(installer_folders()));
}

/// Installed applications, kept briefly so one menu or dialog reads the registry once.
pub(super) fn inventory() -> Inventory {
    static CACHE: Mutex<Option<(Instant, Inventory)>> = Mutex::new(None);
    let mut cache = clawback_core::scan::lock(&CACHE);
    if let Some((at, inventory)) = cache.as_ref()
        && at.elapsed() < Duration::from_secs(15)
    {
        return inventory.clone();
    }
    let inventory = read_inventory();
    *cache = Some((Instant::now(), inventory.clone()));
    inventory
}

fn read_inventory() -> Inventory {
    let mut inventory = Inventory { program_roots: program_roots().to_vec(), ..Default::default() };
    let places = places();
    let installer = installer_folders();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Some(key) = Key::open(root, r"Software\Microsoft\Windows\CurrentVersion\Uninstall", view) else {
                continue;
            };
            for name in key.children() {
                let Some(app) = Key::open(key.0, &name, view) else { continue };
                let Some(display_name) = app.string("DisplayName").filter(|s| !s.trim().is_empty()) else { continue };
                let publisher = app.string("Publisher").unwrap_or_default();
                let uninstaller = app.string("UninstallString").as_deref().and_then(uninstall_command);
                let candidate = |folder: Option<PathBuf>| folder.filter(|folder| plausible(folder, places));
                // Folders shared by many applications and drive roots cannot establish ownership.
                let Some(location) =
                    candidate(app.string("InstallLocation").map(|s| PathBuf::from(s.trim().trim_matches('"'))))
                        .or_else(|| {
                            candidate(
                                app.string("DisplayIcon").as_deref().and_then(icon_file).and_then(|f| folder_of(&f)),
                            )
                        })
                        .or_else(|| match &uninstaller {
                            Some(Action::Uninstall { executable, .. }) => candidate(folder_of(executable)),
                            _ => None,
                        })
                        .or_else(|| packed(&name).and_then(|code| installer.get(&code).cloned()))
                        .or_else(|| named_folder(program_roots(), &publisher, &display_name))
                else {
                    continue;
                };
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
                    uninstaller
                };
                inventory.applications.push(Application {
                    name: display_name,
                    publisher,
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

fn uninstall_command(command: &str) -> Option<Action> {
    parse_uninstall_command(command, &|file| file.is_file())
}

/// Preserve registered arguments verbatim and never hand the command to cmd.exe. An
/// unquoted executable containing spaces resolves as `CreateProcess` would: the first
/// whitespace-separated prefix naming an existing absolute `.exe`.
fn parse_uninstall_command(command: &str, exists: &dyn Fn(&Path) -> bool) -> Option<Action> {
    let command = command.trim();
    if command.contains('\0') {
        return None;
    }
    let executable = |file: &str| {
        let file = Path::new(file);
        (file.is_absolute() && file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")))
            .then(|| file.to_owned())
    };
    if let Some(quoted) = command.strip_prefix('"') {
        let end = quoted.find('"')?;
        let tail = &quoted[end + 1..];
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return None;
        }
        let executable = executable(&quoted[..end])?;
        return Some(Action::Uninstall { executable, arguments: tail.trim_start().to_owned() });
    }
    let ends = command.char_indices().filter(|(_, c)| c.is_whitespace()).map(|(i, _)| i).chain([command.len()]);
    ends.filter_map(|end| Some((executable(&command[..end])?, command[end..].trim_start())))
        .find(|(file, _)| exists(file))
        .map(|(executable, arguments)| Action::Uninstall { executable, arguments: arguments.to_owned() })
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
    fn uninstall_commands_require_an_absolute_executable() {
        let installed = |file: &Path| file == Path::new(r"D:\Moved Apps\uninstall.exe");
        let parse = |command| parse_uninstall_command(command, &installed);
        let uninstall = |executable: &str, arguments: &str| {
            Some(Action::Uninstall { executable: executable.into(), arguments: arguments.into() })
        };
        assert_eq!(
            parse(r#""D:\Moved Apps\Example\uninstall.exe" /uninstall "two words""#),
            uninstall(r"D:\Moved Apps\Example\uninstall.exe", r#"/uninstall "two words""#)
        );
        // Unquoted paths with spaces resolve to the executable that exists, as CreateProcess does.
        assert_eq!(parse(r"D:\Moved Apps\uninstall.exe /S"), uninstall(r"D:\Moved Apps\uninstall.exe", "/S"));
        assert_eq!(parse(r"D:\Moved Apps\uninstall.exe"), uninstall(r"D:\Moved Apps\uninstall.exe", ""));
        for command in [r"D:\Other Apps\uninstall.exe /S", "uninstall.exe", r#""D:\app.exe"extra"#, "cmd /c del", ""] {
            assert!(parse(command).is_none(), "{command}");
        }
        assert!(product_code("{12345678-1234-5678-90AB-123456789ABC}"));
    }

    #[test]
    fn installer_folders_agree_inside_an_application() {
        let own = |folder: &Path| folder.components().count() > 3;
        let folders = |paths: &[&str]| paths.iter().map(PathBuf::from).collect::<Vec<_>>();
        // A large subfolder does not narrow the application to it.
        let mut panels = vec![r"C:\Program Files\Vendor\Panels\Setup", r"C:\Program Files\Vendor\Panels\Driver"];
        panels.extend([r"C:\Program Files\Vendor\Panels\Utility\qml"; 20]);
        assert_eq!(dominant(&folders(&panels), &own), Some(r"C:\Program Files\Vendor\Panels".into()));
        // A stray shortcut outside the application does not widen it.
        let mut app = vec![r"C:\Users\Someone\AppData\Roaming\Start Menu\Vendor"];
        app.extend([r"C:\Program Files\Vendor\App"; 9]);
        assert_eq!(dominant(&folders(&app), &own), Some(r"C:\Program Files\Vendor\App".into()));
        assert_eq!(dominant(&folders(&[r"C:\a", r"D:\b"]), &own), None);
    }

    #[test]
    fn application_folders_step_out_of_binary_folders() {
        let folder = |file: &str| folder_of(Path::new(file)).unwrap();
        assert_eq!(
            folder(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe"),
            Path::new(r"C:\Program Files\obs-studio")
        );
        assert_eq!(folder(r"C:\Program Files\App\Uninstall\un.exe"), Path::new(r"C:\Program Files\App"));
        assert_eq!(folder(r"C:\Program Files\App\app.exe"), Path::new(r"C:\Program Files\App"));
        assert_eq!(icon_file(r#""C:\Program Files\App\app.exe",0"#), Some(r"C:\Program Files\App\app.exe".into()));
        assert!(!product_code("{12345678-1234-5678-90AB-123456789ABZ}"));
    }
}
