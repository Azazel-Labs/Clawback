//! Windows-only read-only MFT helper. The GUI never elevates itself.
mod wire;
use clawback_core::{
    ScanOptions,
    scan::{MftScan, ScanResult, lock},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_CANCELLED, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, GetDriveTypeW, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    },
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoCreateGuid, CoInitializeEx, CoUninitialize},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_NOWAIT,
            PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
            SetNamedPipeHandleState,
        },
        Threading::{GetExitCodeProcess, GetProcessId, WaitForSingleObject},
        WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE},
    },
    UI::{
        Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Requested,
    AwaitingConsent,
    Reading,
    Declined,
    Failed(String),
}

#[derive(Clone, Default)]
pub struct Control(Arc<Mutex<Status>>, Arc<Mutex<clawback_core::scan::MftProgress>>);
impl Control {
    pub fn status(&self) -> Status {
        lock(&self.0).clone()
    }
    pub fn progress(&self) -> Option<clawback_core::scan::MftProgress> {
        (self.status() == Status::Reading).then(|| *lock(&self.1))
    }
    pub fn request(&self) {
        let mut status = lock(&self.0);
        if matches!(*status, Status::Idle | Status::Declined | Status::Failed(_)) {
            *lock(&self.1) = clawback_core::scan::MftProgress::default();
            *status = Status::Requested;
        }
    }
    pub fn take_request(&self) -> bool {
        let mut status = lock(&self.0);
        if *status != Status::Requested {
            return false;
        }
        *status = Status::AwaitingConsent;
        true
    }
    fn set(&self, status: Status) {
        *lock(&self.0) = status;
    }
    pub fn failed(&self, error: &io::Error) {
        self.set(if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            Status::Declined
        } else {
            Status::Failed(error.to_string())
        });
    }
}

pub struct Attempt {
    pub rx: mpsc::Receiver<io::Result<ScanResult>>,
    stop: Arc<AtomicBool>,
}
impl Attempt {
    pub fn start(root: PathBuf, options: ScanOptions, paused: Arc<AtomicBool>, control: Control) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        std::thread::Builder::new().name("clawback-turbo".into()).spawn(move || {
            let result = launch(&root, &options, cancelled, paused, &control, true);
            if let Err(error) = &result {
                control.failed(error);
            }
            let _ = tx.send(result);
        })?;
        Ok(Self { rx, stop })
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Only offer Turbo after disk discovery identifies a whole NTFS drive.
/// The elevated core independently checks the actual volume (including remote drives).
pub fn eligible(disk: Option<&crate::platform::DiskInfo>, is_mount: bool) -> bool {
    is_mount
        && disk.is_some_and(|d| {
            d.fs.eq_ignore_ascii_case("NTFS") && {
                let path = wide(d.mount.as_os_str());
                // SAFETY: a terminated path, no output pointers.
                matches!(unsafe { GetDriveTypeW(path.as_ptr()) }, DRIVE_FIXED | DRIVE_REMOVABLE)
            }
        })
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn stopped() -> io::Error {
    // Read::read_exact retries Interrupted forever; cancellation must escape it.
    io::Error::new(io::ErrorKind::ConnectionAborted, "Turbo cancelled")
}

fn pipe_name() -> io::Result<String> {
    let mut guid = windows_sys::core::GUID::default();
    // SAFETY: writable GUID storage.
    if unsafe { CoCreateGuid(&raw mut guid) } < 0 {
        return Err(io::Error::other("Cannot create Turbo pipe ID"));
    }
    Ok(format!(
        r"\\.\pipe\clawback-mft-{}-{:08x}{:04x}{:04x}{:016x}",
        std::process::id(),
        guid.data1,
        guid.data2,
        guid.data3,
        u64::from_le_bytes(guid.data4)
    ))
}

fn server(name: &str) -> io::Result<File> {
    let name = wide(std::ffi::OsStr::new(name));
    // SAFETY: terminated unique name; default DACL, first-instance protection,
    // no remote clients. Peer PID is checked before sending any scan request.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65536,
            1024 * 1024,
            0,
            std::ptr::null(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: newly created owned pipe handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn spawn_helper(name: &str, elevated: bool) -> io::Result<OwnedHandle> {
    let executable = wide(std::env::current_exe()?.as_os_str());
    // Only internally generated ASCII pipe names and a numeric PID are passed.
    // No shell, user-supplied command line, or root path interpolation.
    let arguments = wide(std::ffi::OsStr::new(&format!("--mft-worker {} {name}", std::process::id())));
    let verb = wide(std::ffi::OsStr::new(if elevated { "runas" } else { "open" }));
    // SAFETY: COM is scoped to this dedicated launcher thread.
    let com = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    // SAFETY: Win32 structure permits zero initialization before setting cbSize.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = arguments.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: all strings and structure storage live through this synchronous call.
    let ok = unsafe { ShellExecuteExW(&raw mut info) };
    let error = io::Error::last_os_error();
    if com >= 0 {
        // SAFETY: balances the successful initialization on this same thread.
        unsafe {
            CoUninitialize();
        }
    }
    if ok == 0 {
        return Err(error);
    }
    if info.hProcess.is_null() {
        return Err(io::Error::other("Turbo helper returned no process handle"));
    }
    // SAFETY: SEE_MASK_NOCLOSEPROCESS transfers ownership to the caller.
    Ok(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
}

struct Incoming {
    pipe: File,
    process: OwnedHandle,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    sent_pause: bool,
    last_data: Instant,
}
impl Read for Incoming {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Err(stopped());
            }
            let paused = self.paused.load(Ordering::Relaxed);
            if paused != self.sent_pause {
                self.pipe.write_all(&[if paused { b'P' } else { b'R' }])?;
                self.sent_pause = paused;
            }
            let mut available = 0;
            // SAFETY: live pipe, valid count pointer; no data is consumed here.
            if unsafe {
                PeekNamedPipe(
                    self.pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &raw mut available,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available != 0 {
                self.last_data = Instant::now();
                let length = data.len().min(available as usize);
                return self.pipe.read(&mut data[..length]);
            }
            // SAFETY: owned process handle; zero timeout never blocks.
            if unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Turbo helper exited without a complete result",
                ));
            }
            if self.last_data.elapsed() > Duration::from_secs(120) {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo helper stopped responding"));
            }
            // Avoid a 20ms penalty for every pipe buffer during bulk transfer.
            if self.last_data.elapsed() < Duration::from_millis(2) {
                std::thread::yield_now();
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn launch(
    root: &Path,
    options: &ScanOptions,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    control: &Control,
    elevated: bool,
) -> io::Result<ScanResult> {
    // The elevated process may start in a different working directory.
    let root = std::path::absolute(root)?;
    let name = pipe_name()?;
    let mut pipe = server(&name)?;
    let process = spawn_helper(&name, elevated)?;
    let start = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        // SAFETY: synchronous pipe and no OVERLAPPED pointer; PIPE_NOWAIT makes this a poll.
        if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) } != 0 {
            // In nonblocking mode success can mean only "now listening".
            // Only ERROR_PIPE_CONNECTED below establishes an actual peer.
            continue;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) {
            break;
        }
        if error.raw_os_error() != Some(ERROR_PIPE_LISTENING as i32) {
            return Err(error);
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Turbo helper did not connect"));
        }
        // SAFETY: live owned process handle; polling without blocking.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
            let mut code = 0;
            // SAFETY: owned process handle and writable exit-code storage.
            unsafe {
                GetExitCodeProcess(process.as_raw_handle(), &raw mut code);
            }
            return Err(io::Error::other(format!(
                "Turbo helper exited before connecting (code {code}: {})",
                io::Error::from_raw_os_error(code as i32)
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut client = 0;
    // SAFETY: live handles and writable process ID storage.
    let expected_client = unsafe { GetProcessId(process.as_raw_handle()) };
    // SAFETY: live pipe and writable PID storage.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &raw mut client) } == 0 || client != expected_client {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Unexpected Turbo pipe client"));
    }
    let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
    // SAFETY: live connected pipe; mode pointer valid for the call.
    if unsafe { SetNamedPipeHandleState(pipe.as_raw_handle(), &raw const mode, std::ptr::null(), std::ptr::null()) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    pipe.write_all(wire::MAGIC)?;
    wire::string(&mut pipe, root.as_os_str())?;
    pipe.write_all(&[u8::from(options.apparent_size) | (u8::from(options.dedupe_hardlinks) << 1)])?;
    control.set(Status::Reading);
    let mut input = BufReader::with_capacity(
        256 * 1024,
        Incoming { pipe, process, stop, paused, sent_pause: false, last_data: Instant::now() },
    );
    loop {
        let mut tag = [0];
        input.read_exact(&mut tag)?;
        match tag[0] {
            0 => {} // heartbeat during raw MFT parsing, including pauses
            1 => {
                lock(&control.1).phase = 4;
                let _span = crate::perf::span("worker.turbo_receive_tree");
                return wire::read_tree(&mut input, &root);
            }
            3 => {
                let progress = wire::read_progress(&mut input)?;
                crate::perf::counter("turbo.phase", progress.phase as f64);
                crate::perf::counter("turbo.mft_bytes_read", progress.read as f64);
                crate::perf::counter("turbo.records", progress.records as f64);
                crate::perf::counter("turbo.files_assembled", progress.files as f64);
                *lock(&control.1) = progress;
            }
            2 => {
                let mut kind = [0];
                input.read_exact(&mut kind)?;
                let kind = match kind[0] {
                    1 => io::ErrorKind::Unsupported,
                    2 => io::ErrorKind::PermissionDenied,
                    _ => io::ErrorKind::Other,
                };
                return Err(io::Error::new(kind, wire::read_string(&mut input)?.to_string_lossy().into_owned()));
            }
            _ => return Err(wire::invalid()),
        }
    }
}

pub fn worker_entry() -> Option<io::Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--mft-worker")) {
        return None;
    }
    Some((|| {
        let parent: u32 = args.next().and_then(|v| v.to_str()?.parse().ok()).ok_or_else(wire::invalid)?;
        let name = args.next().and_then(|s| s.into_string().ok()).ok_or_else(wire::invalid)?;
        let prefix = format!(r"\\.\pipe\clawback-mft-{parent}-");
        let suffix = name.strip_prefix(&prefix).ok_or_else(wire::invalid)?;
        if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) || args.next().is_some() {
            return Err(wire::invalid());
        }
        worker(parent, &name)
    })())
}

fn worker(parent: u32, name: &str) -> io::Result<()> {
    // Identification only: a pipe server must never impersonate this elevated client.
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
        .open(name)?;
    let mut server_pid = 0;
    // SAFETY: live pipe and writable PID storage.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut server_pid) } == 0 || server_pid != parent {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Unexpected Turbo pipe server"));
    }
    let mut magic = [0; 8];
    pipe.read_exact(&mut magic)?;
    if &magic != wire::MAGIC {
        return Err(wire::invalid());
    }
    let root = PathBuf::from(wire::read_string(&mut pipe)?);
    let mut options = [0];
    pipe.read_exact(&mut options)?;
    if options[0] & !3 != 0 {
        return Err(wire::invalid());
    }
    let scan = MftScan::new(
        &root,
        ScanOptions {
            apparent_size: options[0] & 1 != 0,
            dedupe_hardlinks: options[0] & 2 != 0,
            ..ScanOptions::default()
        },
    )?;
    let telemetry = scan.shared().clone();
    let shared = telemetry.clone();
    let mut commands = pipe.try_clone()?;
    std::thread::Builder::new().name("clawback-turbo-control".into()).spawn(move || {
        loop {
            let mut command = [0];
            // Closing the parent's pipe (cancel, normal scan wins, parent exits)
            // terminates this disposable read-only worker even during blocked I/O.
            // Do not block in ReadFile on a duplicated synchronous pipe handle:
            // that can serialize against the writer and prevent heartbeats/results.
            let mut available = 0;
            // SAFETY: owned pipe and valid count storage; this does not consume data.
            if unsafe {
                PeekNamedPipe(
                    commands.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &raw mut available,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                std::process::exit(0);
            }
            if available == 0 {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            if commands.read_exact(&mut command).is_err() {
                std::process::exit(0);
            }
            match command[0] {
                b'P' => shared.progress.set_paused(true),
                b'R' => shared.progress.set_paused(false),
                _ => std::process::exit(0),
            }
        }
    })?;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("clawback-mft".into()).spawn(move || {
        #[cfg(feature = "turbo-probe")]
        if root.file_name() == Some(std::ffi::OsStr::new("<clawback-turbo-fixture>")) {
            let _ = tx.send(Ok(fixture(&root)));
            return;
        }
        let _ = tx.send(scan.run());
    })?;
    let mut output = BufWriter::with_capacity(256 * 1024, pipe);
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(result)) => {
                let mut progress = *lock(&telemetry.mft_progress);
                progress.phase = 4;
                progress.files = result.files;
                progress.dirs = result.dirs;
                progress.bytes = result.bytes;
                output.write_all(&[3])?;
                wire::write_progress(&mut output, progress)?;
                output.write_all(&[1])?;
                wire::write_tree(&mut output, &result)?;
                output.flush()?;
                return Ok(());
            }
            Ok(Err(error)) => {
                output.write_all(&[
                    2,
                    match error.kind() {
                        io::ErrorKind::Unsupported => 1,
                        io::ErrorKind::PermissionDenied => 2,
                        _ => 0,
                    },
                ])?;
                wire::string(&mut output, std::ffi::OsStr::new(&error.to_string()))?;
                output.flush()?;
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                output.write_all(&[3])?;
                wire::write_progress(&mut output, *lock(&telemetry.mft_progress))?;
                output.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(io::Error::other("MFT worker stopped")),
        }
    }
}

#[cfg(feature = "turbo-probe")]
fn fixture(root: &Path) -> ScanResult {
    use clawback_core::{
        ROOT, Tree,
        scan::ScanBackend,
        tree::{Kind, NewEntry},
    };
    let started = Instant::now();
    let mut tree = Tree::new(root);
    tree.node_mut(ROOT).file_id = Some((1, 5));
    for batch in 0..40 {
        tree.add_children(
            ROOT,
            (0..256)
                .map(|i| NewEntry {
                    name: format!("fixture-{}-文件.bin", batch * 256 + i).into(),
                    kind: Kind::File,
                    size: 4096,
                    len: 1000,
                    mtime: 0,
                    flags: 0,
                    file_id: Some((1, batch * 256 + i + 100)),
                })
                .collect(),
        );
    }
    tree.sort_all();
    ScanResult {
        backend: ScanBackend::NtfsMft,
        root: root.to_owned(),
        tree,
        skipped: Vec::new(),
        cancelled: false,
        elapsed: started.elapsed(),
        files: 10240,
        dirs: 1,
        bytes: 10240 * 4096,
        denied: 0,
    }
}

/// Developer-only executable probe, driven by cargo xtask test-turbo.
#[cfg(feature = "turbo-probe")]
pub fn probe_entry() -> Option<io::Result<()>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_none_or(|a| a != "--turbo-probe") {
        return None;
    }
    crate::platform::attach_console();
    let result = (|| {
        if args.len() != 3 {
            return Err(wire::invalid());
        }
        let mode = args[1].to_str().ok_or_else(wire::invalid)?;
        if !matches!(mode, "smoke" | "fixture" | "elevated" | "decline") {
            return Err(wire::invalid());
        }
        let root = std::path::absolute(Path::new(&args[2]))?;
        let root = if mode == "fixture" { root.join("<clawback-turbo-fixture>") } else { root };
        let control = Control::default();
        let start = Instant::now();
        let result = launch(
            &root,
            &ScanOptions::default(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            &control,
            matches!(mode, "elevated" | "decline"),
        );
        match result {
            Ok(result) if mode != "decline" => {
                let progress = control.progress().ok_or_else(wire::invalid)?;
                if progress.phase != 4
                    || (progress.files, progress.dirs, progress.bytes) != (result.files, result.dirs, result.bytes)
                {
                    return Err(io::Error::other("Turbo telemetry did not round-trip"));
                }
                if mode == "fixture"
                    && (result.files != 10240
                        || result.bytes != 10240 * 4096
                        || result.tree.root().file_id != Some((1, 5)))
                {
                    return Err(io::Error::other("Fixture snapshot did not round-trip"));
                }
                println!(
                    "outcome={} files={} directories={} bytes={} helper_seconds={:.3} total_seconds={:.3}",
                    if mode == "fixture" { "fixture" } else { "mft" },
                    result.files,
                    result.dirs,
                    result.bytes,
                    result.elapsed.as_secs_f64(),
                    start.elapsed().as_secs_f64()
                );
                Ok(())
            }
            Err(error) if mode == "decline" && error.raw_os_error() == Some(ERROR_CANCELLED as i32) => {
                println!("outcome=uac_cancelled normal_scan_would_continue=true");
                Ok(())
            }
            Err(error)
                if mode == "smoke"
                    && control.status() == Status::Reading
                    && matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported) =>
            {
                println!("outcome=helper_replied_unavailable reason={error}");
                Ok(())
            }
            Err(error) => Err(error),
            Ok(_) => Err(io::Error::other("Expected the user to cancel UAC")),
        }
    })();
    if let Err(error) = &result {
        eprintln!("Turbo probe failed: {error}");
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_hides_provisional_progress_and_retry_resets_it() {
        let control = Control::default();
        control.set(Status::Reading);
        lock(&control.1).files = 123;
        assert_eq!(control.progress().unwrap().files, 123);
        control.failed(&io::Error::other("invalid MFT"));
        assert!(control.progress().is_none());
        control.request();
        assert_eq!(lock(&control.1).files, 0);
    }

    #[test]
    fn consent_requests_are_single_flight_and_can_retry_after_decline() {
        let control = Control::default();
        control.request();
        assert!(control.take_request());
        control.request();
        assert!(!control.take_request());
        control.failed(&io::Error::from_raw_os_error(ERROR_CANCELLED as i32));
        assert_eq!(control.status(), Status::Declined);
        control.request();
        assert!(control.take_request());
    }
    #[test]
    fn unsupported_or_failed_helper_does_not_request_another_prompt() {
        let control = Control::default();
        control.failed(&io::Error::other("helper crashed"));
        assert!(matches!(control.status(), Status::Failed(_)));
        assert!(!control.take_request());
        assert!(!eligible(None, true));
    }
    #[test]
    fn strict_helper_never_falls_back_to_walking_a_folder() {
        let scan = MftScan::new(Path::new(env!("CARGO_MANIFEST_DIR")), ScanOptions::default()).expect("start");
        assert_eq!(scan.run().expect_err("not a volume").kind(), io::ErrorKind::Unsupported);
    }
    #[test]
    fn cancellation_escapes_read_exact_instead_of_retrying_forever() {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        let pipe = server(&pipe_name().expect("unique pipe")).expect("server");
        // SAFETY: opens only a synchronization handle to this test process.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, std::process::id()) };
        assert!(!handle.is_null());
        // SAFETY: fresh owned process handle.
        let process = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut input = Incoming {
            pipe,
            process,
            stop: Arc::new(AtomicBool::new(true)),
            paused: Arc::new(AtomicBool::new(false)),
            sent_pause: false,
            last_data: Instant::now(),
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(input.read_exact(&mut [0]).expect_err("cancelled").kind());
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).expect("must not spin"), io::ErrorKind::ConnectionAborted);
    }
}
