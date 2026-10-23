//! Administrator-only service lifecycle; no shell commands or saved credentials.
use super::{NAME, ServiceHandle, query, service, wide};
use std::{
    ffi::OsStr,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_NOT_ACTIVE, LocalFree, WAIT_OBJECT_0},
    Security::{
        Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW},
        GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::{
        Com::CoTaskMemFree,
        Services::{
            ChangeServiceConfigW, ControlService, CreateServiceW, DeleteService, OpenSCManagerW, QUERY_SERVICE_CONFIGW,
            QueryServiceConfigW, SC_MANAGER_CREATE_SERVICE, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
            SERVICE_CONTROL_STOP, SERVICE_ERROR_NORMAL, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
            SERVICE_START, SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED, SERVICE_WIN32_OWN_PROCESS, StartServiceW,
        },
        Threading::{
            OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
    UI::{
        Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
        WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
    },
};

pub(super) fn validate_sid(sid: &str) -> io::Result<()> {
    // Only a concrete account SID, never an SDDL alias, group, or injected ACE.
    if !(sid.starts_with("S-1-5-21-") && sid.split('-').count() == 8
        || sid.starts_with("S-1-12-1-") && sid.split('-').count() == 8)
        || sid.len() > 184
        || !sid.bytes().all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        return Err(super::super::wire::invalid());
    }
    let sid = wide(OsStr::new(sid));
    let mut value = std::ptr::null_mut();
    // SAFETY: terminated SID and writable LocalAlloc pointer.
    if unsafe { ConvertStringSidToSidW(sid.as_ptr(), &raw mut value) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: allocated by ConvertStringSidToSidW.
    unsafe {
        LocalFree(value);
    }
    Ok(())
}

fn desktop_user() -> io::Result<String> {
    // Resolve the shell in this installation's desktop/session, not the account
    // whose administrator credentials were entered in an over-the-shoulder UAC prompt.
    // SAFETY: returns the shell window in this session or null, with no ownership.
    let shell = unsafe { GetShellWindow() };
    if shell.is_null() {
        return Err(io::Error::other("No desktop user; specify the authorized account SID explicitly"));
    }
    let mut pid = 0;
    // SAFETY: live shell window and writable PID storage.
    unsafe {
        GetWindowThreadProcessId(shell, &raw mut pid);
    }
    // SAFETY: ask only for identity query rights on the shell process.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: newly owned process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    account_sid(process.as_raw_handle())
}

pub(super) fn account_sid(process: windows_sys::Win32::Foundation::HANDLE) -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    // SAFETY: process is live and token output valid.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: uniquely owned token.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut size = 0;
    // SAFETY: first query obtains required size, with no output buffer.
    unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, std::ptr::null_mut(), 0, &raw mut size);
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    // SAFETY: usize storage provides TOKEN_USER alignment and requested byte capacity.
    if unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, buffer.as_mut_ptr().cast(), size, &raw mut size) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful query initialized TOKEN_USER in the aligned buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = std::ptr::null_mut();
    // SAFETY: token owns a valid SID for this call, output is LocalAlloc-owned.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful conversion returns a terminated UTF-16 string.
    let text = unsafe { windows_core::PCWSTR(sid).to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe {
        LocalFree(sid.cast());
    }
    text
}

fn protected_executable() -> io::Result<std::path::PathBuf> {
    let mut folder = std::ptr::null_mut();
    // SAFETY: known-folder GUID and valid output pointer, current machine default.
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &raw mut folder) };
    if result < 0 {
        return Err(io::Error::other("Cannot locate Program Files"));
    }
    // SAFETY: successful known-folder query returns terminated allocated UTF-16.
    let path = unsafe { windows_core::PCWSTR(folder).to_string() }.map_err(io::Error::other);
    // SAFETY: SHGetKnownFolderPath allocates through the COM task allocator.
    unsafe {
        CoTaskMemFree(folder.cast());
    }
    let expected = std::path::PathBuf::from(path?).join(super::INSTALL_FOLDER).join("clawback.exe");
    let actual = std::env::current_exe()?.canonicalize()?;
    // Reject junctions/symlinks, including the installation directory itself.
    for ancestor in expected.ancestors() {
        use std::os::windows::fs::MetadataExt;
        if std::fs::symlink_metadata(ancestor)?.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("Service installation cannot use reparse points"));
        }
    }
    if actual != expected.canonicalize()? {
        return Err(io::Error::other(format!(
            "Install the Turbo service from Program Files\\{}",
            super::INSTALL_FOLDER
        )));
    }
    Ok(expected)
}

pub(super) fn stop() -> io::Result<()> {
    let handle = match service(SERVICE_STOP | SERVICE_QUERY_STATUS) {
        Ok(handle) => handle,
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => return Ok(()),
        Err(error) => return Err(error),
    };
    let pid = query(&handle)?.dwProcessId;
    let process = if pid == 0 {
        None
    } else {
        // SAFETY: SCM-reported service process; retain only wait rights for upgrade safety.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if process.is_null() {
            let error = io::Error::last_os_error();
            // ERROR_INVALID_PARAMETER means the process already exited.
            if error.raw_os_error() != Some(87) {
                return Err(error);
            }
            None
        } else {
            // SAFETY: new unique owned process handle.
            Some(unsafe { OwnedHandle::from_raw_handle(process) })
        }
    };
    let mut status = SERVICE_STATUS::default();
    // SAFETY: live authorized service handle and initialized writable status.
    if unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &raw mut status) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_SERVICE_NOT_ACTIVE as i32) {
            return Err(error);
        }
    }
    let start = Instant::now();
    while query(&handle)?.dwCurrentState != SERVICE_STOPPED {
        if start.elapsed() > Duration::from_secs(30) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo service did not stop"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(process) = process {
        // SAFETY: live wait-only handle; ensure the image is unloaded before replacement.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) } != WAIT_OBJECT_0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo service process did not exit"));
        }
    }
    Ok(())
}

pub(super) fn remove() -> io::Result<()> {
    stop()?;
    let handle = match service(0x0001_0000) {
        // DELETE access
        Ok(handle) => handle,
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => return Ok(()),
        Err(error) => return Err(error),
    };
    // SAFETY: handle was opened with DELETE; stopped service has no active workers.
    if unsafe { DeleteService(handle.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn install(authorized_sid: Option<&str>) -> io::Result<()> {
    let executable = protected_executable()?;
    let sid = match authorized_sid {
        Some(sid) => sid.to_owned(),
        None => existing_user()?.map_or_else(desktop_user, Ok)?,
    };
    validate_sid(&sid)?;
    stop()?;
    let command = wide(OsStr::new(&format!("\"{}\" --turbo-service {sid}", executable.display())));
    let name = wide(OsStr::new(NAME));
    let display = wide(OsStr::new(if cfg!(feature = "steam") {
        "Clawback Steam Turbo (read-only NTFS scans)"
    } else {
        "Clawback Turbo (read-only NTFS scans)"
    }));
    let access = SERVICE_CHANGE_CONFIG | SERVICE_START | SERVICE_QUERY_STATUS;
    let handle = match service(access) {
        Ok(handle) => {
            // SAFETY: fixed protected path, validated SID, LocalSystem via null account.
            if unsafe {
                ChangeServiceConfigW(
                    handle.0,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    command.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    wide(OsStr::new("LocalSystem")).as_ptr(),
                    std::ptr::null(),
                    display.as_ptr(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            handle
        }
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => {
            // SAFETY: local SCM; creating a service requires administrator rights.
            let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CREATE_SERVICE) };
            if manager.is_null() {
                return Err(io::Error::last_os_error());
            }
            let manager = ServiceHandle(manager);
            // SAFETY: all strings live for call; null service account selects LocalSystem.
            let handle = unsafe {
                CreateServiceW(
                    manager.0,
                    name.as_ptr(),
                    display.as_ptr(),
                    access,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    command.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            ServiceHandle(handle)
        }
        Err(error) => return Err(error),
    };
    // SAFETY: SCM starts the configured process; no untrusted service arguments.
    if unsafe { StartServiceW(handle.0, 0, std::ptr::null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let start = Instant::now();
    loop {
        let state = query(&handle)?;
        if state.dwCurrentState == SERVICE_RUNNING {
            return Ok(());
        }
        if state.dwCurrentState == SERVICE_STOPPED {
            return Err(io::Error::other(format!("Turbo service failed to start: {}", state.dwWin32ExitCode)));
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo service did not start"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn existing_user() -> io::Result<Option<String>> {
    let handle = match service(SERVICE_QUERY_CONFIG) {
        Ok(handle) => handle,
        Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) => return Ok(None),
        Err(error) => return Err(error),
    };
    // QueryServiceConfig's documented maximum buffer is 8 KiB; align for its struct.
    let mut buffer = vec![0usize; 8192 / size_of::<usize>()];
    let mut needed = 0;
    // SAFETY: live handle and sufficiently sized, aligned writable storage.
    if unsafe { QueryServiceConfigW(handle.0, buffer.as_mut_ptr().cast(), 8192, &raw mut needed) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful query initialized the configuration and pointers within buffer.
    let config = unsafe { &*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    // SAFETY: lpBinaryPathName points to a terminated string in the live buffer.
    let command = unsafe { windows_core::PCWSTR(config.lpBinaryPathName).to_string() }.map_err(io::Error::other)?;
    let sid = command.rsplit_once(" --turbo-service ").ok_or_else(super::super::wire::invalid)?.1;
    validate_sid(sid)?;
    Ok(Some(sid.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_concrete_account_sids_can_enter_service_configuration() {
        assert!(validate_sid("S-1-5-21-111-222-333-1001").is_ok());
        for invalid in
            ["IU", "WD", "S-1-5-18", "S-1-5-32-544", "S-1-5-21-1)(A;;GA;;;WD)", "S-1-5-21-1 --command", "S-1-5-21-"]
        {
            assert!(validate_sid(invalid).is_err(), "{invalid}");
        }
    }
}
