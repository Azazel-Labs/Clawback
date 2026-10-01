//! The drive's Recycle Bin, shown on the map as one cell that empties it.
use crate::platform::wide;
use clawback_core::{NodeId, ROOT, Tree};
use eframe::egui::ColorImage;
use std::{
    ffi::OsStr,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
    sync::OnceLock,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::Shell::{SHCNE_UPDATEDIR, SHCNF_PATHW, SHChangeNotify, SIID_RECYCLER, SIID_RECYCLERFULL},
};

/// The `$Recycle.Bin` folder at the root of a scanned drive.
pub fn find(tree: &Tree, is_mount: bool) -> Option<NodeId> {
    if !is_mount {
        return None;
    }
    tree.node(ROOT)
        .children
        .iter()
        .copied()
        .find(|&n| tree.node(n).is_dir() && tree.node(n).name_lossy().eq_ignore_ascii_case("$Recycle.Bin"))
}

/// The shell's own full or empty Recycle Bin icon at `size` pixels.
pub fn icon(full: bool, size: u32) -> Option<ColorImage> {
    crate::filetype_icons::windows::stock(if full { SIID_RECYCLERFULL } else { SIID_RECYCLER }, size)
}

/// The current user's folder inside a drive's Recycle Bin, named by their SID. Emptying the
/// Recycle Bin means emptying this folder; other users' folders are theirs.
pub fn user_folder(tree: &Tree, bin: NodeId) -> Option<NodeId> {
    tree.child_named(bin, OsStr::new(user_sid()?))
}

pub(crate) fn user_sid() -> Option<&'static str> {
    static SID: OnceLock<Option<String>> = OnceLock::new();
    SID.get_or_init(|| {
        let mut token = ptr::null_mut();
        // SAFETY: no arguments; the pseudo-handle needs no closing.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: writable handle storage.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
            return None;
        }
        // SAFETY: OpenProcessToken succeeded and transferred ownership of the token.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut buffer = vec![0u64; 64];
        let mut length = 0;
        // SAFETY: an aligned, writable buffer of the declared size for TOKEN_USER.
        let ok = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size_of_val(buffer.as_slice()) as u32,
                &raw mut length,
            )
        };
        if ok == 0 {
            return None;
        }
        // SAFETY: GetTokenInformation filled a TOKEN_USER at the start of the buffer.
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text = ptr::null_mut();
        // SAFETY: a valid SID from the token; the string is LocalAlloc'd and freed below.
        if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
            return None;
        }
        // SAFETY: a terminated UTF-16 string returned by ConvertSidToStringSidW.
        let value = unsafe { windows_core::PCWSTR(text).to_string() }.ok();
        // SAFETY: frees exactly the allocation returned above.
        unsafe { LocalFree(text.cast()) };
        value
    })
    .as_deref()
}

/// Let Explorer refresh its Recycle Bin after Clawback emptied it directly.
pub fn notify_changed(folder: &Path) {
    let path = wide(folder.as_os_str());
    // SAFETY: a terminated path, read during this synchronous call only.
    unsafe { SHChangeNotify(SHCNE_UPDATEDIR as i32, SHCNF_PATHW, path.as_ptr().cast(), ptr::null()) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_provides_both_recycle_bin_icons() {
        for full in [false, true] {
            let icon = super::icon(full, 96).expect("stock Recycle Bin icon");
            assert_eq!(icon.size, [96, 96]);
            assert!(icon.pixels.iter().any(|pixel| pixel.a() > 0));
        }
    }

    #[test]
    fn current_user_has_a_sid() {
        assert!(super::user_sid().is_some_and(|sid| sid.starts_with("S-1-")));
    }
}
