//! Detect whether this process has a graphical desktop session.

#[cfg(windows)]
pub fn graphical() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
    };
    // SAFETY: This returns a borrowed handle owned by the process; do not close it.
    let station = unsafe { GetProcessWindowStation() };
    if station.is_null() {
        return false;
    }
    let mut flags = USEROBJECTFLAGS::default();
    // SAFETY: The handle is valid and the output buffer has the declared size.
    let ok = unsafe {
        GetUserObjectInformationW(
            station,
            UOI_FLAGS,
            std::ptr::from_mut(&mut flags).cast(),
            size_of::<USEROBJECTFLAGS>() as u32,
            std::ptr::null_mut(),
        )
    };
    ok != 0 && flags.dwFlags & 1 != 0 // WSF_VISIBLE: interactive window station.
}

#[cfg(target_os = "macos")]
pub fn graphical() -> bool {
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
    }
    // SAFETY: No arguments; Quartz returns an owned dictionary or null without a GUI session.
    let session = unsafe { CGSessionCopyCurrentDictionary() };
    if session.is_null() {
        return false;
    }
    // SAFETY: Release the non-null dictionary returned by the Copy function exactly once.
    unsafe { CFRelease(session) };
    true
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn graphical() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"].iter().any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}
