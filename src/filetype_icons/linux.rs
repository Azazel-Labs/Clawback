//! Use the installed MIME database and desktop icon theme without linking GTK.
use eframe::egui::ColorImage;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    fs,
    path::PathBuf,
    sync::OnceLock,
};

pub fn load(extension: &str, size: u32) -> Option<ColorImage> {
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
#[allow(clippy::multiple_unsafe_ops_per_block)]
fn mime_icons(extension: &str) -> Option<Vec<String>> {
    static GIO: OnceLock<Option<libloading::Library>> = OnceLock::new();
    // SAFETY: This is the platform GIO library, retained for the process lifetime.
    let lib = GIO.get_or_init(|| unsafe { libloading::Library::new("libgio-2.0.so.0").ok() }).as_ref()?;
    let name = CString::new(if extension == "(none)" { "file".to_owned() } else { format!("file{extension}") }).ok()?;
    // SAFETY: Signatures match GIO/GLib. All returned strings are copied before
    // their owning GIcon/content-type allocations are released. No file is opened.
    unsafe {
        let guess = lib
            .get::<unsafe extern "C" fn(*const c_char, *const u8, usize, *mut i32) -> *mut c_char>(
                b"g_content_type_guess\0",
            )
            .ok()?;
        let get_icon =
            lib.get::<unsafe extern "C" fn(*const c_char) -> *mut c_void>(b"g_content_type_get_icon\0").ok()?;
        let names =
            lib.get::<unsafe extern "C" fn(*mut c_void) -> *const *const c_char>(b"g_themed_icon_get_names\0").ok()?;
        let themed_type = lib.get::<unsafe extern "C" fn() -> usize>(b"g_themed_icon_get_type\0").ok()?;
        let is_type =
            lib.get::<unsafe extern "C" fn(*mut c_void, usize) -> i32>(b"g_type_check_instance_is_a\0").ok()?;
        let free = lib.get::<unsafe extern "C" fn(*mut c_void)>(b"g_free\0").ok()?;
        let unref = lib.get::<unsafe extern "C" fn(*mut c_void)>(b"g_object_unref\0").ok()?;
        let content = guess(name.as_ptr(), std::ptr::null(), 0, std::ptr::null_mut());
        if content.is_null() {
            return None;
        }
        let icon = get_icon(content);
        free(content.cast());
        if icon.is_null() {
            return None;
        }
        let mut result = Vec::new();
        if is_type(icon, themed_type()) != 0 {
            let mut ptr = names(icon);
            if !ptr.is_null() {
                while !(*ptr).is_null() {
                    result.push(CStr::from_ptr(*ptr).to_string_lossy().into_owned());
                    ptr = ptr.add(1);
                }
            }
        }
        unref(icon);
        Some(result)
    }
}
