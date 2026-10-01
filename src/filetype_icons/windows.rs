use crate::platform::Apartment;
use eframe::egui::ColorImage;
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GdiFlush, SelectObject,
    },
    Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
    UI::{
        Shell::{
            SHDefExtractIconW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGSI_ICONLOCATION,
            SHGetFileInfoW, SHGetStockIconInfo, SHSTOCKICONID, SHSTOCKICONINFO,
        },
        WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
    },
};

thread_local! {
    /// COM for the calling thread, initialized on first use and released when the thread exits.
    static COM: Option<Apartment> = Apartment::enter().ok();
}

#[allow(clippy::multiple_unsafe_ops_per_block)]
pub fn load(extension: Option<&str>, size: u32) -> Option<ColorImage> {
    if !COM.with(Option::is_some) {
        return None;
    }
    let name: Vec<u16> = extension.unwrap_or("file").encode_utf16().chain(Some(0)).collect();
    // SAFETY: All pointers reference initialized, correctly sized native structures
    // or owned GDI buffers. Every successful allocation is released on each exit.
    // USEFILEATTRIBUTES queries associations without touching a scanned file.
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let result = SHGetFileInfoW(
            name.as_ptr(),
            FILE_ATTRIBUTE_NORMAL,
            &raw mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        );
        if result == 0 || info.hIcon.is_null() {
            return None;
        }
        let image = rasterize(info.hIcon, size);
        DestroyIcon(info.hIcon);
        image
    }
}

/// Render an icon at `size` pixels with straight alpha. The caller keeps ownership of `icon`.
#[allow(clippy::multiple_unsafe_ops_per_block)]
pub(crate) fn rasterize(icon: HICON, size: u32) -> Option<ColorImage> {
    // SAFETY: All pointers reference initialized, correctly sized native structures
    // or owned GDI buffers. Every successful allocation is released on each exit.
    unsafe {
        let dc = CreateCompatibleDC(ptr::null_mut());
        let mut bitmap: BITMAPINFO = std::mem::zeroed();
        bitmap.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };
        let mut bits = ptr::null_mut();
        let dib = CreateDIBSection(dc, &raw const bitmap, DIB_RGB_COLORS, &raw mut bits, ptr::null_mut(), 0);
        if dc.is_null() || dib.is_null() || bits.is_null() {
            if !dib.is_null() {
                DeleteObject(dib);
            }
            if !dc.is_null() {
                DeleteDC(dc);
            }
            return None;
        }
        let previous = SelectObject(dc, dib);
        let len = (size * size * 4) as usize;
        std::slice::from_raw_parts_mut(bits.cast::<u8>(), len).fill(0);
        let black_ok = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, ptr::null_mut(), DI_NORMAL) != 0;
        GdiFlush();
        let black = std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec();
        std::slice::from_raw_parts_mut(bits.cast::<u8>(), len).fill(255);
        let white_ok = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, ptr::null_mut(), DI_NORMAL) != 0;
        GdiFlush();
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), len);
        let mut rgba = Vec::with_capacity(len);
        // Recover coverage from two backgrounds, including legacy mask-only icons.
        for (black, white) in black.as_chunks::<4>().0.iter().zip(pixels.as_chunks::<4>().0) {
            let alpha = 255 - white[0].saturating_sub(black[0]);
            let straight = |c: u8| if alpha == 0 { 0 } else { (u32::from(c) * 255 / u32::from(alpha)).min(255) as u8 };
            rgba.extend_from_slice(&[straight(black[2]), straight(black[1]), straight(black[0]), alpha]);
        }
        SelectObject(dc, previous);
        DeleteObject(dib);
        DeleteDC(dc);
        (black_ok && white_ok).then(|| ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba))
    }
}

/// A shell stock icon (drives, Recycle Bin, …) at `size` pixels.
pub(crate) fn stock(id: SHSTOCKICONID, size: u32) -> Option<ColorImage> {
    // SAFETY: the info structure is sized for this call and the shell fills its path.
    let mut info: SHSTOCKICONINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHSTOCKICONINFO>() as u32;
    // SAFETY: valid stock icon ID and writable, correctly sized structure.
    if unsafe { SHGetStockIconInfo(id, SHGSI_ICONLOCATION, &raw mut info) } < 0 {
        return None;
    }
    let mut large: HICON = ptr::null_mut();
    // SAFETY: szPath is terminated by the shell; only the large icon is requested.
    let extracted = unsafe {
        SHDefExtractIconW(info.szPath.as_ptr(), info.iIcon, 0, &raw mut large, ptr::null_mut(), size & 0xffff)
    };
    if extracted < 0 || large.is_null() {
        return None;
    }
    let image = rasterize(large, size);
    // SAFETY: the extracted icon is owned by this function.
    unsafe { DestroyIcon(large) };
    image
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_returns_type_icons_without_existing_files() {
        for extension in [Some(".txt"), Some(".zip"), Some(".clawback-unknown-extension"), None] {
            let icon = super::load(extension, 32).expect("Windows shell type icon");
            assert_eq!(icon.size, [32, 32]);
            assert!(icon.pixels.iter().any(|pixel| pixel.a() > 0));
        }
    }
}
