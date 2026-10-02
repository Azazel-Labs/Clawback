//! Read the current user's WSL registrations without starting any distributions.
use super::Registration;
use crate::platform::wide;
use std::{ffi::OsStr, path::PathBuf};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a successfully opened registry key.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn string(key: HKEY, subkey: &[u16], name: &str) -> Option<String> {
    let name = wide(OsStr::new(name));
    let mut bytes = 0;
    // SAFETY: NUL-terminated key/value names; null data requests the required size.
    if unsafe {
        RegGetValueW(
            key,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
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
    // SAFETY: buffer has at least the byte capacity reported by the first call.
    if unsafe {
        RegGetValueW(
            key,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    } != 0
    {
        return None;
    }
    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).ok()
}

pub(super) fn read() -> Vec<Registration> {
    let name = wide(OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Lxss"));
    let mut key = std::ptr::null_mut();
    // SAFETY: NUL-terminated name, predefined root, writable output key.
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, KEY_READ, &raw mut key) } != 0 {
        return Vec::new();
    }
    let key = Key(key);
    let mut result = Vec::new();
    for index in 0..4096 {
        let mut subkey = [0u16; 256];
        let mut length = 255;
        // SAFETY: name buffer and its length are valid; optional outputs are null.
        if unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                subkey.as_mut_ptr(),
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
        let Some(name) = string(key.0, &subkey, "DistributionName") else { continue };
        let Some(base) = string(key.0, &subkey, "BasePath") else { continue };
        let value = wide(OsStr::new("Version"));
        let mut version: u32 = 0;
        let mut bytes = 4;
        // SAFETY: initialized u32 and matching byte size, with valid NUL-terminated names.
        let status = unsafe {
            RegGetValueW(
                key.0,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut version).cast(),
                &raw mut bytes,
            )
        };
        if status != 0 || version != 2 {
            continue;
        }
        let filename = string(key.0, &subkey, "VhdFileName").unwrap_or_else(|| "ext4.vhdx".into());
        result.push(Registration { name, path: PathBuf::from(base).join(filename) });
    }
    result
}
