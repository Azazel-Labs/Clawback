//! Optional, per-user-authorized `LocalSystem` broker installed in Program Files.
//! It accepts only a nonce and starts this protected executable's MFT worker.
mod setup;
use super::{available, wide, worker_args};
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
        INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES},
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        },
        Services::{
            CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, RegisterServiceCtrlHandlerW,
            SC_HANDLE, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
            SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
            SERVICE_STATUS_PROCESS, SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW,
            SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW,
        },
        Threading::{
            CREATE_NO_WINDOW, GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
    },
};

const NAME: &str = if cfg!(feature = "steam") { "ClawbackSteamTurbo" } else { "ClawbackTurbo" };
const PIPE: &str =
    if cfg!(feature = "steam") { r"\\.\pipe\ClawbackSteamTurbo-v1" } else { r"\\.\pipe\ClawbackTurbo-v1" };
const INSTALL_FOLDER: &str = if cfg!(feature = "steam") { "Clawback Steam Turbo" } else { "Clawback" };
const MAGIC: &[u8; 8] = b"CLAWBRK1";
// Excludes FILE_CREATE_PIPE_INSTANCE: clients must never create broker instances.
const CLIENT_ACCESS: u32 = 0x0012_019b;
static STOP: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub(super) fn volume_root(root: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
    let mut root = wide(root.as_os_str());
    if root.get(root.len().checked_sub(2)?) != Some(&u16::from(b'\\')) {
        root.insert(root.len() - 1, u16::from(b'\\'));
    }
    let mut name = [0u16; 64];
    // SAFETY: terminated caller-owned path and correctly sized output buffer.
    if unsafe { GetVolumeNameForVolumeMountPointW(root.as_ptr(), name.as_mut_ptr(), name.len() as u32) } == 0 {
        return None;
    }
    let end = name.iter().position(|&c| c == 0)?;
    Some(std::ffi::OsString::from_wide(&name[..end]).into())
}

fn volume_path_syntax(root: &std::path::Path) -> bool {
    let Some(root) = root.to_str() else {
        return false;
    };
    let bytes = root.as_bytes();
    if bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..] == b":\\" {
        return true;
    }
    let Some(guid) = root.strip_prefix(r"\\?\Volume{").and_then(|s| s.strip_suffix("}\\")) else {
        return false;
    };
    guid.len() == 36
        && guid
            .bytes()
            .enumerate()
            .all(|(i, b)| if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() })
}

pub(super) fn local_volume_root(root: &std::path::Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    if !volume_path_syntax(root) {
        return false;
    }
    let root = wide(root.as_os_str());
    // SAFETY: only a syntactically restricted local volume name reaches Windows.
    // 2=removable, 3=fixed, 6=RAM disk; mapped/network drives are excluded.
    matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 6)
}

struct ServiceHandle(SC_HANDLE);
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns an SCM handle.
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
fn service(access: u32) -> io::Result<ServiceHandle> {
    // SAFETY: null selects the local SCM, with only connect access.
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = ServiceHandle(manager);
    let name = wide(OsStr::new(NAME));
    // SAFETY: live SCM and terminated name.
    let handle = unsafe { OpenServiceW(manager.0, name.as_ptr(), access) };
    if handle.is_null() { Err(io::Error::last_os_error()) } else { Ok(ServiceHandle(handle)) }
}
fn query(handle: &ServiceHandle) -> io::Result<SERVICE_STATUS_PROCESS> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0;
    // SAFETY: correctly sized and aligned output structure and live handle.
    if unsafe {
        QueryServiceStatusEx(
            handle.0,
            SC_STATUS_PROCESS_INFO,
            (&raw mut status).cast(),
            size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &raw mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(status)
}

/// The service is installed and running. Whether it serves this user is known only when
/// it is asked.
pub(super) fn running() -> bool {
    service(SERVICE_QUERY_STATUS).and_then(|service| query(&service)).is_ok_and(|s| s.dwCurrentState == SERVICE_RUNNING)
}

/// None means unavailable/unauthorized; retain the portable UAC path in that case.
pub(super) fn request(name: &str) -> io::Result<Option<OwnedHandle>> {
    let Ok(service) = service(SERVICE_QUERY_STATUS) else {
        return Ok(None);
    };
    let status = query(&service)?;
    if status.dwCurrentState != SERVICE_RUNNING {
        return Ok(None);
    }
    let start = Instant::now();
    let mut pipe = loop {
        match OpenOptions::new()
            .access_mode(CLIENT_ACCESS)
            .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
            .open(PIPE)
        {
            Ok(pipe) => break pipe,
            Err(error) if error.raw_os_error() == Some(231) && start.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) if matches!(error.raw_os_error(), Some(2 | 5)) => return Ok(None),
            Err(error) => return Err(error),
        }
    };
    let mut pid = 0;
    // SAFETY: live connected pipe and writable PID.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut pid) } == 0
        || pid != status.dwProcessId
        || query(&service)?.dwProcessId != pid
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Untrusted Turbo broker"));
    }
    let (_, nonce) = name.rsplit_once('-').ok_or_else(super::wire::invalid)?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(super::wire::invalid());
    }
    pipe.write_all(MAGIC)?;
    pipe.write_all(nonce.as_bytes())?;
    wait_bytes(&pipe, 8, Duration::from_secs(5))?;
    let mut bytes = [0; 8];
    pipe.read_exact(&mut bytes)?;
    let handle = usize::try_from(u64::from_le_bytes(bytes)).map_err(|_| super::wire::invalid())?;
    if handle == 0 || handle == usize::MAX {
        return Err(io::Error::other("Turbo broker could not start a worker"));
    }
    // SAFETY: authenticated service duplicated an owned, limited process handle into us.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(handle as *mut _) }))
}

fn wait_bytes(pipe: &File, count: u32, timeout: Duration) -> io::Result<()> {
    let start = Instant::now();
    while available(pipe)? < count {
        if STOP.load(Ordering::Relaxed) || start.elapsed() > timeout {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo broker request timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn broker_pipe(pipe_name: &str, sid: &str) -> io::Result<File> {
    setup::validate_sid(sid)?;
    // Explicit medium integrity permits the unelevated desktop app to write,
    // while keeping low-integrity processes out even when they share its SID.
    let sddl = wide(OsStr::new(&format!("D:P(A;;GA;;;SY)(A;;0x12019b;;;{sid})S:(ML;;NW;;;ME)")));
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: validated SID embedded in a fixed SDDL template, output is LocalAlloc-owned.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &raw mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let name = wide(OsStr::new(pipe_name));
    // SAFETY: live security descriptor; first-instance protection and no remote clients.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            1024,
            1024,
            0,
            &raw const attributes,
        )
    };
    let error = io::Error::last_os_error();
    // SAFETY: descriptor was allocated by the SDDL conversion above and has been copied.
    unsafe {
        LocalFree(descriptor);
    }
    if handle == INVALID_HANDLE_VALUE {
        Err(error)
    } else {
        // SAFETY: transfers unique pipe ownership into File.
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

fn worker_job() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed job, default security, no inherited handle.
    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: new uniquely owned job.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: correctly sized initialized limits and a live job handle.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn start_worker(pipe: &mut File, job: &OwnedHandle) -> io::Result<Child> {
    wait_bytes(pipe, 40, Duration::from_secs(2))?;
    let mut request = [0; 40];
    pipe.read_exact(&mut request)?;
    if &request[..8] != MAGIC {
        return Err(super::wire::invalid());
    }
    let nonce = std::str::from_utf8(&request[8..]).map_err(|_| super::wire::invalid())?;
    let mut parent = 0;
    // SAFETY: OS-authenticated client PID; never accept a PID from the request.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &raw mut parent) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let name = format!(r"\\.\pipe\clawback-mft-{parent}-{nonce}");
    worker_args([parent.to_string().into(), name.clone().into()].into_iter())?;
    // SAFETY: PID obtained from the pipe; keep the process alive through handle transfer.
    let target = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, parent) };
    if target.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: new owned process handle.
    let target = unsafe { OwnedHandle::from_raw_handle(target) };
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--mft-service-worker")
        .arg(parent.to_string())
        .arg(name)
        .current_dir(std::env::current_exe()?.parent().ok_or_else(super::wire::invalid)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    // SAFETY: owned job and child; child inherits no job handles.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let mut duplicate = std::ptr::null_mut();
    // SAFETY: pseudo handle of current process requires no closing.
    let this_process = unsafe { GetCurrentProcess() };
    // SAFETY: transfer only query/wait rights for this child into the authenticated caller.
    if unsafe {
        DuplicateHandle(
            this_process,
            child.as_raw_handle(),
            target.as_raw_handle(),
            &raw mut duplicate,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            0,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    if let Err(error) = pipe.write_all(&(duplicate as usize as u64).to_le_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child)
}

fn connected(pipe: &File) -> io::Result<bool> {
    // SAFETY: PIPE_NOWAIT makes connection polling nonblocking.
    if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) } != 0 {
        return Ok(false); // In nonblocking mode, success can mean "now listening".
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_PIPE_CONNECTED) => Ok(true),
        Some(ERROR_PIPE_LISTENING) => Ok(false),
        Some(ERROR_NO_DATA | ERROR_BROKEN_PIPE) => {
            // A client can exit between opening the pipe and this poll. Reset
            // the instance rather than letting one disconnect stop the service.
            // SAFETY: this service owns the single server instance.
            unsafe {
                DisconnectNamedPipe(pipe.as_raw_handle());
            }
            Ok(false)
        }
        _ => Err(error),
    }
}

fn serve(sid: &str) -> io::Result<()> {
    let job = worker_job()?;
    let mut pipe = broker_pipe(PIPE, sid)?;
    let mut children: Vec<Child> = Vec::new();
    report(SERVICE_RUNNING, 0);
    while !STOP.load(Ordering::Relaxed) {
        children.retain_mut(|child| !matches!(child.try_wait(), Ok(Some(_))));
        if connected(&pipe)? {
            if children.len() < 4
                && let Ok(child) = start_worker(&mut pipe, &job)
            {
                children.push(child);
            }
            // Wait for the client to consume its reply before disconnecting (bounded).
            let start = Instant::now();
            while available(&pipe).is_ok() && start.elapsed() < Duration::from_secs(2) && !STOP.load(Ordering::Relaxed)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            // SAFETY: disconnect this single instance before accepting another caller.
            unsafe {
                DisconnectNamedPipe(pipe.as_raw_handle());
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(job); // Also kills blocked/paused workers on service shutdown or failure.
    for mut child in children {
        let _ = child.wait();
    }
    Ok(())
}

fn report(state: u32, error: u32) {
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 },
        dwWin32ExitCode: error,
        dwWaitHint: if state == SERVICE_STOP_PENDING { 10000 } else { 0 },
        ..SERVICE_STATUS::default()
    };
    // SAFETY: handle published by service_main and retained until STOPPED is reported.
    unsafe {
        SetServiceStatus(STATUS.load(Ordering::Acquire), &raw const status);
    }
}
unsafe extern "system" fn control(command: u32) {
    if command == SERVICE_CONTROL_STOP || command == SERVICE_CONTROL_SHUTDOWN {
        report(SERVICE_STOP_PENDING, 0);
        STOP.store(true, Ordering::Release);
    }
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let name = wide(OsStr::new(NAME));
    // SAFETY: terminated name and callback remain valid for the service lifetime.
    let status = unsafe { RegisterServiceCtrlHandlerW(name.as_ptr(), Some(control)) };
    if status.is_null() {
        return;
    }
    STATUS.store(status, Ordering::Release);
    let result = std::env::args().nth(2).ok_or_else(super::wire::invalid).and_then(|sid| serve(&sid));
    report(SERVICE_STOPPED, result.err().map_or(0, |e| e.raw_os_error().unwrap_or(1) as u32));
}

pub fn entry() -> Option<io::Result<()>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first()?.to_str()? {
        "--turbo-service" if args.len() == 2 => Some(dispatch()),
        "--turbo-service-install" if args.len() <= 2 => Some(
            args.get(1).map(|sid| sid.to_str().ok_or_else(super::wire::invalid)).transpose().and_then(setup::install),
        ),
        "--turbo-service-remove" if args.len() == 1 => Some(setup::remove()),
        "--turbo-service-stop" if args.len() == 1 => Some(setup::stop()),
        "--turbo-service-check" if args.len() == 1 => Some(check()),
        _ => None,
    }
}

/// Exercise broker authentication, child handle transfer and the real worker wire
/// protocol without scanning a volume. Never opens a UAC prompt on failure.
fn check() -> io::Result<()> {
    use std::sync::Arc;
    let root = std::env::current_exe()?;
    let control = super::Control::default();
    let result = super::launch(
        &root,
        &clawback_core::ScanOptions::default(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        &control,
        super::Launch::Service,
    );
    match result {
        Err(error) if error.kind() == io::ErrorKind::Unsupported && control.status() == super::Status::Reading => {
            Ok(())
        }
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::other("Turbo worker accepted a non-volume path")),
    }
}
fn dispatch() -> io::Result<()> {
    let mut name = wide(OsStr::new(NAME));
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: name.as_mut_ptr(), lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: table, terminated name and static callback live until dispatcher returns.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_never_resolves_network_device_or_arbitrary_file_paths() {
        use std::path::Path;
        for allowed in [r"C:\", r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\"] {
            assert!(volume_path_syntax(Path::new(allowed)), "{allowed}");
        }
        for denied in [
            r"\\server\share\",
            r"\\?\UNC\server\share\",
            r"\\.\pipe\name",
            r"C:\folder\",
            "C:",
            r"C:\..\",
            r"\\?\Volume{not-a-guid}\",
        ] {
            assert!(!volume_path_syntax(Path::new(denied)), "{denied}");
        }
    }

    #[test]
    fn client_resolves_real_volume_roots_but_not_executable_paths() {
        use std::path::{Component, PathBuf, Prefix};
        let executable = std::env::current_exe().expect("test executable");
        let Some(Component::Prefix(prefix)) = executable.components().next() else {
            panic!("Windows prefix");
        };
        let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else {
            panic!("tests must run from a local Windows volume");
        };
        let root = PathBuf::from(format!("{}:\\", char::from(drive)));
        let guid = volume_root(&root).expect("volume GUID");
        assert!(local_volume_root(&guid));
        assert!(volume_root(&executable).is_none(), "diagnostic path must not become a volume scan");
    }

    fn client(name: &str) -> io::Result<File> {
        OpenOptions::new()
            .access_mode(CLIENT_ACCESS)
            .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
            .open(name)
    }

    #[test]
    fn pipe_acl_authorizes_only_selected_account_and_rejects_invalid_requests() {
        // SAFETY: non-owning pseudo handle for the current process.
        let sid = setup::account_sid(unsafe { GetCurrentProcess() }).expect("current user");
        let name = super::super::pipe_name().expect("unique pipe name");
        let mut pipe = broker_pipe(&name, &sid).expect("broker pipe");
        assert!(broker_pipe(&name, &sid).is_err(), "another server must not claim the same pipe");
        let mut connection = client(&name).expect("authorized account");
        connection.write_all(&[0; 40]).expect("malformed request");
        let job = worker_job().expect("worker job");
        assert_eq!(start_worker(&mut pipe, &job).expect_err("reject bad magic").kind(), io::ErrorKind::InvalidData);
        connection.write_all(MAGIC).expect("valid header");
        connection.write_all(&[b'z'; 32]).expect("invalid nonce");
        assert_eq!(
            start_worker(&mut pipe, &job).expect_err("reject malformed nonce").kind(),
            io::ErrorKind::InvalidData
        );
        drop(connection);
        assert!(available(&pipe).is_err());

        let name = super::super::pipe_name().expect("unique denied pipe");
        let _pipe = broker_pipe(&name, "S-1-5-21-111-222-333-1001").expect("different account ACL");
        assert_eq!(client(&name).expect_err("other account must be denied").kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(CLIENT_ACCESS & 4, 0, "client must not get FILE_CREATE_PIPE_INSTANCE");
    }

    #[test]
    fn partial_requests_time_out_without_starting_workers() {
        // SAFETY: non-owning current-process pseudo handle.
        let sid = setup::account_sid(unsafe { GetCurrentProcess() }).expect("current user");
        let name = super::super::pipe_name().expect("unique pipe name");
        let pipe = broker_pipe(&name, &sid).expect("broker pipe");
        let mut client = client(&name).expect("client");
        client.write_all(MAGIC).expect("partial request");
        assert_eq!(
            wait_bytes(&pipe, 40, Duration::from_millis(20)).expect_err("bounded wait").kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn abandoned_connections_do_not_stop_the_broker() {
        // SAFETY: non-owning current-process pseudo handle.
        let sid = setup::account_sid(unsafe { GetCurrentProcess() }).expect("current user");
        let name = super::super::pipe_name().expect("unique pipe name");
        let pipe = broker_pipe(&name, &sid).expect("broker pipe");
        drop(client(&name).expect("abandoned client"));
        if connected(&pipe).expect("an early disconnect is recoverable") {
            assert!(available(&pipe).is_err());
            // SAFETY: mirrors the normal connected-client cleanup in serve().
            unsafe {
                DisconnectNamedPipe(pipe.as_raw_handle());
            }
        }
        assert!(!connected(&pipe).expect("resume listening after disconnect"));
        let _client = client(&name).expect("next client can connect");
        assert!(connected(&pipe).expect("next connection"));
    }

    #[test]
    #[ignore = "subprocess fixture for closing_the_job_terminates_workers"]
    fn job_child() {
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn closing_the_job_terminates_workers() {
        let job = worker_job().expect("kill-on-close job");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--ignored", "--exact", "turbo::service::tests::job_child"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("worker fixture");
        // SAFETY: owned job and live child process handle.
        let assigned = unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) };
        if assigned == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            panic!("assign fixture: {error}");
        }
        drop(job);
        let started = Instant::now();
        loop {
            if child.try_wait().expect("child status").is_some() {
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("job closure left its worker running");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
