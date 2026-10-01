//! Windows-only read-only MFT helper. The GUI never elevates itself.
mod wire;
use crate::platform::{Apartment, wide};
use clawback_core::{
    ScanOptions,
    scan::{MftPhase, MftProgress, MftScan, ScanResult, lock},
};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_CANCELLED, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    },
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_NOWAIT,
            PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
            SetNamedPipeHandleState,
        },
        Threading::{GetExitCodeProcess, GetProcessId, WaitForSingleObject},
    },
    UI::{
        Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

/// Buffering on each end of the snapshot stream.
const STREAM_BUFFER: usize = 256 * 1024;

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

#[derive(Default)]
struct State {
    status: Status,
    /// Provisional telemetry, shown only while `Reading`.
    progress: MftProgress,
}

/// Consent and telemetry shared by the UI, the scan coordinator and the launcher.
#[derive(Clone, Default)]
pub struct Control(Arc<Mutex<State>>);
impl Control {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.0)
    }
    pub fn status(&self) -> Status {
        self.state().status.clone()
    }
    pub fn progress(&self) -> Option<MftProgress> {
        let state = self.state();
        (state.status == Status::Reading).then_some(state.progress)
    }
    pub fn request(&self) {
        let mut state = self.state();
        if matches!(state.status, Status::Idle | Status::Declined | Status::Failed(_)) {
            *state = State { status: Status::Requested, progress: MftProgress::default() };
        }
    }
    pub fn take_request(&self) -> bool {
        let mut state = self.state();
        if state.status != Status::Requested {
            return false;
        }
        state.status = Status::AwaitingConsent;
        true
    }
    fn set(&self, status: Status) {
        self.state().status = status;
    }
    fn report(&self, progress: MftProgress) {
        self.state().progress = progress;
    }
    fn transferring(&self) {
        self.state().progress.phase = MftPhase::Transferring;
    }
    pub fn failed(&self, error: &io::Error) {
        self.set(if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            Status::Declined
        } else {
            Status::Failed(error.to_string())
        });
    }
}

/// One elevated helper run on its launcher thread; dropping it cancels the helper.
pub struct Attempt {
    thread: Option<JoinHandle<io::Result<ScanResult>>>,
    stop: Arc<AtomicBool>,
    control: Control,
}
impl Attempt {
    pub fn start(root: PathBuf, options: ScanOptions, paused: Arc<AtomicBool>, control: Control) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let reporter = control.clone();
        let thread = std::thread::Builder::new().name("clawback-turbo".into()).spawn(move || {
            let result = launch(&root, &options, cancelled, paused, &reporter, true);
            if let Err(error) = &result {
                reporter.failed(error);
            }
            result
        })?;
        Ok(Self { thread: Some(thread), stop, control })
    }

    /// The helper's result once the launcher is done. Failures are already in `Control`.
    pub fn poll(&mut self) -> Option<io::Result<ScanResult>> {
        if !self.thread.as_ref()?.is_finished() {
            return None;
        }
        Some(self.thread.take()?.join().unwrap_or_else(|_| {
            let error = io::Error::other("Turbo launcher stopped");
            self.control.failed(&error);
            Err(error)
        }))
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Only offer Turbo after disk discovery identifies a whole drive the MFT
/// reader accepts. The elevated core checks the actual volume again.
pub fn eligible(disk: Option<&crate::platform::DiskInfo>, is_mount: bool) -> bool {
    is_mount && disk.is_some_and(|d| clawback_core::raw_volume_eligible(&d.mount))
}

fn stopped() -> io::Error {
    // Read::read_exact retries Interrupted forever; cancellation must escape it.
    io::Error::new(io::ErrorKind::ConnectionAborted, "Turbo cancelled")
}
fn helper_exited() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "Turbo helper exited without a complete result")
}

/// Polls without blocking.
fn has_exited(process: &OwnedHandle) -> bool {
    // SAFETY: live owned process handle; a zero timeout never blocks.
    let state = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) };
    state == WAIT_OBJECT_0
}

/// Bytes waiting in `pipe`, without consuming them.
fn available(pipe: &File) -> io::Result<u32> {
    let mut available = 0;
    // SAFETY: live pipe and valid count storage; no data is read.
    let ok = unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &raw mut available,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(available) }
}

fn pipe_name() -> io::Result<String> {
    let guid = windows_core::GUID::new().map_err(|_| io::Error::other("Cannot create Turbo pipe ID"))?;
    Ok(format!(r"\\.\pipe\clawback-mft-{}-{:032x}", std::process::id(), guid.to_u128()))
}

fn server(name: &str) -> io::Result<File> {
    let name = wide(OsStr::new(name));
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
    let arguments = wide(OsStr::new(&format!("--mft-worker {} {name}", std::process::id())));
    let verb = wide(OsStr::new(if elevated { "runas" } else { "open" }));
    // COM is scoped to this dedicated launcher thread; the shell manages without it.
    let _apartment = Apartment::enter().ok();
    // SAFETY: Win32 structure permits zero initialization before setting cbSize.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = arguments.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: all strings and structure storage live through this synchronous call.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        return Err(io::Error::last_os_error());
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
                let command = if paused { wire::Command::Pause } else { wire::Command::Resume };
                wire::write_command(&mut self.pipe, command)?;
                self.sent_pause = paused;
            }
            // Check for exit before peeking: the helper writes its last bytes and exits
            // immediately, so checking after an empty peek can discard the end of the tree.
            let exited = has_exited(&self.process);
            let available = available(&self.pipe).map_err(|error| if exited { helper_exited() } else { error })?;
            if available != 0 {
                self.last_data = Instant::now();
                let length = data.len().min(available as usize);
                return self.pipe.read(&mut data[..length]);
            }
            if exited {
                return Err(helper_exited());
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
        if has_exited(&process) {
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
    wire::write_options(&mut pipe, options)?;
    control.set(Status::Reading);
    let mut input = BufReader::with_capacity(
        STREAM_BUFFER,
        Incoming { pipe, process, stop, paused, sent_pause: false, last_data: Instant::now() },
    );
    loop {
        match wire::read_tag(&mut input)? {
            wire::Tag::Progress => {
                let progress = wire::read_progress(&mut input)?;
                crate::perf::counter("turbo.phase", f64::from(progress.phase as u8));
                crate::perf::counter("turbo.mft_bytes_read", progress.read as f64);
                crate::perf::counter("turbo.records", progress.records as f64);
                crate::perf::counter("turbo.files_assembled", progress.files as f64);
                control.report(progress);
            }
            wire::Tag::Tree => {
                control.transferring();
                let _span = crate::perf::span("worker.turbo_receive_tree");
                return wire::read_tree(&mut input, &root);
            }
            wire::Tag::Failed => return Err(wire::read_error(&mut input)?),
        }
    }
}

pub fn worker_entry() -> Option<io::Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new("--mft-worker")) {
        return None;
    }
    Some(worker_args(args).and_then(|(parent, name)| worker(parent, &name)))
}

/// `<parent PID> <pipe name>`, where the name must be one that parent generates.
fn worker_args(mut args: impl Iterator<Item = OsString>) -> io::Result<(u32, String)> {
    let parent: u32 = args.next().and_then(|v| v.to_str()?.parse().ok()).ok_or_else(wire::invalid)?;
    let name = args.next().and_then(|s| s.into_string().ok()).ok_or_else(wire::invalid)?;
    let prefix = format!(r"\\.\pipe\clawback-mft-{parent}-");
    let suffix = name.strip_prefix(&prefix).ok_or_else(wire::invalid)?;
    if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) || args.next().is_some() {
        return Err(wire::invalid());
    }
    Ok((parent, name))
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
    let scan = MftScan::new(&root, wire::read_options(&mut pipe)?)?;
    let telemetry = scan.shared().clone();
    let shared = telemetry.clone();
    let mut commands = pipe.try_clone()?;
    std::thread::Builder::new().name("clawback-turbo-control".into()).spawn(move || {
        // Closing the parent's pipe (cancel, normal scan wins, parent exits)
        // terminates this disposable read-only worker even during blocked I/O.
        // Do not block in ReadFile on a duplicated synchronous pipe handle:
        // that can serialize against the writer and prevent progress/results.
        loop {
            match available(&commands) {
                Err(_) => std::process::exit(0),
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(_) => match wire::read_command(&mut commands) {
                    Ok(command) => shared.progress.set_paused(command == wire::Command::Pause),
                    Err(_) => std::process::exit(0),
                },
            }
        }
    })?;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("clawback-mft".into()).spawn(move || {
        #[cfg(feature = "turbo-probe")]
        if root.file_name() == Some(OsStr::new("<clawback-turbo-fixture>")) {
            let _ = tx.send(Ok(fixture(&root)));
            return;
        }
        let _ = tx.send(scan.run());
    })?;
    let mut output = BufWriter::with_capacity(STREAM_BUFFER, pipe);
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(result)) => {
                let progress = MftProgress {
                    phase: MftPhase::Transferring,
                    files: result.files,
                    dirs: result.dirs,
                    bytes: result.bytes,
                    ..*lock(&telemetry.mft_progress)
                };
                wire::write_tag(&mut output, wire::Tag::Progress)?;
                wire::write_progress(&mut output, progress)?;
                wire::write_tag(&mut output, wire::Tag::Tree)?;
                wire::write_tree(&mut output, &result)?;
                output.flush()?;
                return Ok(());
            }
            Ok(Err(error)) => {
                wire::write_tag(&mut output, wire::Tag::Failed)?;
                wire::write_error(&mut output, &error)?;
                output.flush()?;
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                wire::write_tag(&mut output, wire::Tag::Progress)?;
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
    let result = probe(&args);
    if let Err(error) = &result {
        eprintln!("Turbo probe failed: {error}");
    }
    Some(result)
}

/// `--turbo-probe <smoke|fixture|elevated|decline> <root>`.
#[cfg(feature = "turbo-probe")]
fn probe(args: &[OsString]) -> io::Result<()> {
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
            if progress.phase != MftPhase::Transferring
                || (progress.files, progress.dirs, progress.bytes) != (result.files, result.dirs, result.bytes)
            {
                return Err(io::Error::other("Turbo telemetry did not round-trip"));
            }
            if mode == "fixture"
                && (result.files != 10240 || result.bytes != 10240 * 4096 || result.tree.root().file_id != Some((1, 5)))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_hides_provisional_progress_and_retry_resets_it() {
        let control = Control::default();
        control.set(Status::Reading);
        control.report(MftProgress { files: 123, ..MftProgress::default() });
        assert_eq!(control.progress().unwrap().files, 123);
        control.failed(&io::Error::other("invalid MFT"));
        assert!(control.progress().is_none());
        control.request();
        assert_eq!(control.state().progress.files, 0);
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
    fn worker_accepts_only_the_parents_pipe_name() {
        let args = |parent: &str, name: &str| [parent, name].map(OsString::from).into_iter();
        let name = pipe_name().expect("unique pipe");
        let pid = std::process::id().to_string();
        assert_eq!(worker_args(args(&pid, &name)).expect("valid").1, name);
        assert!(worker_args(args("1", &name)).is_err());
        assert!(worker_args(args(&pid, &name[..name.len() - 1])).is_err());
    }
    #[test]
    fn strict_helper_never_falls_back_to_walking_a_folder() {
        let scan = MftScan::new(Path::new(env!("CARGO_MANIFEST_DIR")), ScanOptions::default()).expect("start");
        assert_eq!(scan.run().expect_err("not a volume").kind(), io::ErrorKind::Unsupported);
    }
    #[test]
    fn data_written_before_the_helper_exits_is_still_read() {
        use std::os::windows::io::IntoRawHandle;
        let name = pipe_name().expect("unique pipe");
        let pipe = server(&name).expect("server");
        let mut client = OpenOptions::new().read(true).write(true).open(&name).expect("client");
        client.write_all(b"end").expect("write");
        drop(client);
        let mut helper = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().expect("spawn");
        helper.wait().expect("exit");
        // SAFETY: the child's process handle, now owned by the reader.
        let process = unsafe { OwnedHandle::from_raw_handle(helper.into_raw_handle()) };
        let mut input = Incoming {
            pipe,
            process,
            stop: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            sent_pause: false,
            last_data: Instant::now(),
        };
        let mut data = [0; 3];
        input.read_exact(&mut data).expect("buffered data");
        assert_eq!(&data, b"end");
        assert_eq!(input.read_exact(&mut [0]).expect_err("drained").kind(), io::ErrorKind::UnexpectedEof);
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
