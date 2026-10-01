//! Use the installed MIME database and desktop icon theme without linking GTK.
use eframe::egui::ColorImage;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    fs,
    path::PathBuf,
    sync::OnceLock,
};

pub fn load(extension: Option<&str>, size: u32) -> Option<ColorImage> {
    static THEME: OnceLock<String> = OnceLock::new();
    let theme = THEME.get_or_init(desktop_theme);
    let mut names = mime_icons(extension).unwrap_or_default();
    names.extend(["application-octet-stream".into(), "text-x-generic".into()]);
    for name in names {
        let Some(path) = freedesktop_icons::lookup(&name).with_size(size as u16).with_theme(theme).find() else {
            continue;
        };
        let Ok(bytes) = fs::read(&path) else { continue };
        let icon = if path.extension().is_some_and(|ext| ext == "svg") {
            svg(&bytes, size)
        } else {
            super::decode(&bytes, size)
        };
        if icon.is_some() {
            return icon;
        }
    }
    None
}
fn desktop_theme() -> String {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    if std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase().contains("kde")
        && let Some(config) = config
        && let Ok(text) = fs::read_to_string(config.join("kdeglobals"))
    {
        let mut icons = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                icons = line == "[Icons]";
            } else if icons && let Some(theme) = line.strip_prefix("Theme=") {
                return theme.to_owned();
            }
        }
    }
    // Keep the theme directory ID (the theme's translated display name may differ).
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().trim_matches('\'').to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "hicolor".into())
}
fn svg(bytes: &[u8], size: u32) -> Option<ColorImage> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let scale = (size as f32 / tree.size().width()).min(size as f32 / tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(ColorImage::from_rgba_premultiplied([size as usize, size as usize], pixmap.data()))
}
/// GIO/GLib entry points, resolved once. The library stays loaded with them.
struct Gio {
    guess: unsafe extern "C" fn(*const c_char, *const u8, usize, *mut i32) -> *mut c_char,
    get_icon: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    names: unsafe extern "C" fn(*mut c_void) -> *const *const c_char,
    themed_type: unsafe extern "C" fn() -> usize,
    is_type: unsafe extern "C" fn(*mut c_void, usize) -> i32,
    free: unsafe extern "C" fn(*mut c_void),
    unref: unsafe extern "C" fn(*mut c_void),
    _library: libloading::Library,
}

impl Gio {
    fn get() -> Option<&'static Self> {
        static GIO: OnceLock<Option<Gio>> = OnceLock::new();
        GIO.get_or_init(Self::open).as_ref()
    }

    #[allow(clippy::multiple_unsafe_ops_per_block)]
    fn open() -> Option<Self> {
        // SAFETY: This is the platform GIO library, retained for the process lifetime
        // alongside the copied symbols. Signatures match GIO/GLib.
        unsafe {
            let library = libloading::Library::new("libgio-2.0.so.0").ok()?;
            let guess = *library.get(b"g_content_type_guess\0").ok()?;
            let get_icon = *library.get(b"g_content_type_get_icon\0").ok()?;
            let names = *library.get(b"g_themed_icon_get_names\0").ok()?;
            let themed_type = *library.get(b"g_themed_icon_get_type\0").ok()?;
            let is_type = *library.get(b"g_type_check_instance_is_a\0").ok()?;
            let free = *library.get(b"g_free\0").ok()?;
            let unref = *library.get(b"g_object_unref\0").ok()?;
            Some(Self { guess, get_icon, names, themed_type, is_type, free, unref, _library: library })
        }
    }
}

#[allow(clippy::multiple_unsafe_ops_per_block)]
fn mime_icons(extension: Option<&str>) -> Option<Vec<String>> {
    let gio = Gio::get()?;
    let name =
        CString::new(extension.map_or_else(|| "file".to_owned(), |extension| format!("file{extension}"))).ok()?;
    // SAFETY: All returned strings are copied before their owning GIcon/content-type
    // allocations are released. No file is opened.
    unsafe {
        let content = (gio.guess)(name.as_ptr(), std::ptr::null(), 0, std::ptr::null_mut());
        if content.is_null() {
            return None;
        }
        let icon = (gio.get_icon)(content);
        (gio.free)(content.cast());
        if icon.is_null() {
            return None;
        }
        let mut result = Vec::new();
        if (gio.is_type)(icon, (gio.themed_type)()) != 0 {
            let mut ptr = (gio.names)(icon);
            if !ptr.is_null() {
                while !(*ptr).is_null() {
                    result.push(CStr::from_ptr(*ptr).to_string_lossy().into_owned());
                    ptr = ptr.add(1);
                }
            }
        }
        (gio.unref)(icon);
        Some(result)
    }
}
