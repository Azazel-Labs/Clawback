//! One asynchronous subtree watch. Zero-byte completions mean lost events.
use crate::watching::{Change, Inbox};
use std::{
    fs::OpenOptions,
    io,
    os::windows::{
        ffi::OsStringExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NOTIFY_ENUM_DIR, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, ReadDirectoryChangesW,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects},
    },
};

pub struct Watcher {
    stop: Arc<OwnedHandle>,
}

impl Watcher {
    pub fn start(root: &Path, inbox: Inbox) -> io::Result<Self> {
        let stop = Arc::new(event()?);
        let signal = stop.clone();
        let root = root.to_path_buf();
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("clawback-notifications".into()).spawn(move || {
            let result = run(&root, &signal, &inbox, &tx);
            if let Err(error) = result {
                let message = error.to_string();
                let _ = tx.try_send(Err(message.clone()));
                inbox.send(Change::Failed(message));
            }
        })?;
        rx.recv().map_err(|_| io::Error::other("Notification worker stopped"))?.map_err(io::Error::other)?;
        Ok(Self { stop })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: stop remains a valid event handle throughout this call.
        unsafe {
            SetEvent(self.stop.as_raw_handle());
        }
    }
}

fn event() -> io::Result<OwnedHandle> {
    // SAFETY: no security descriptor or event name; manual-reset event.
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateEventW transferred ownership of a valid handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn run(root: &Path, stop: &OwnedHandle, inbox: &Inbox, ready: &mpsc::SyncSender<Result<(), String>>) -> io::Result<()> {
    // Overlapped and only ever used through its raw handle, never std I/O.
    let directory = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .open(root)?;
    let completed = event()?;
    let mut overlapped = OVERLAPPED { hEvent: completed.as_raw_handle(), ..OVERLAPPED::default() };
    // DWORD-aligned buffer, at most 64 KiB (also works with network shares).
    let mut buffer = vec![0u32; 16 * 1024];
    let mut first = true;
    loop {
        // SAFETY: valid event, exclusively used by this worker.
        unsafe {
            ResetEvent(completed.as_raw_handle());
        }
        // SAFETY: buffer and OVERLAPPED stay alive and unmoved until completion
        // or cancellation has been acknowledged below. The buffer is aligned.
        let ok = unsafe {
            ReadDirectoryChangesW(
                directory.as_raw_handle(),
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 4) as u32,
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
                std::ptr::null_mut(),
                &raw mut overlapped,
                None,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if first {
            let _ = ready.send(Ok(()));
            first = false;
        }
        let handles = [stop.as_raw_handle(), completed.as_raw_handle()];
        // SAFETY: both event handles remain open while the wait is pending.
        let wait = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if wait != WAIT_OBJECT_0 + 1 {
            // SAFETY: cancel the one operation owned by this worker, then wait
            // for completion before dropping its buffer/OVERLAPPED/handle.
            unsafe {
                CancelIoEx(directory.as_raw_handle(), &raw const overlapped);
            }
            let mut bytes = 0;
            // SAFETY: operation storage is still valid; bWait drains cancellation.
            unsafe {
                GetOverlappedResult(directory.as_raw_handle(), &raw const overlapped, &raw mut bytes, 1);
            }
            return if wait == WAIT_OBJECT_0 { Ok(()) } else { Err(io::Error::last_os_error()) };
        }
        let mut bytes = 0;
        // SAFETY: signaled completion, valid operation storage and output pointer.
        let ok = unsafe { GetOverlappedResult(directory.as_raw_handle(), &raw const overlapped, &raw mut bytes, 0) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) {
                inbox.send(Change::Rescan);
                continue;
            }
            return Err(error);
        }
        if bytes == 0 {
            inbox.send(Change::Rescan);
            continue;
        }
        // SAFETY: completion wrote bytes <= capacity; view only the completed bytes.
        let raw = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), bytes as usize) };
        parse(root, raw, inbox);
    }
}

fn parse(root: &Path, bytes: &[u8], inbox: &Inbox) {
    let mut offset = 0;
    loop {
        let Some(header) = bytes.get(offset..offset + 12) else {
            inbox.send(Change::Rescan);
            return;
        };
        let next = u32::from_le_bytes(header[0..4].try_into().expect("four bytes")) as usize;
        let action = u32::from_le_bytes(header[4..8].try_into().expect("four bytes"));
        let len = u32::from_le_bytes(header[8..12].try_into().expect("four bytes")) as usize;
        let Some(name) = bytes.get(offset + 12..offset + 12 + len) else {
            inbox.send(Change::Rescan);
            return;
        };
        if !len.is_multiple_of(2) {
            inbox.send(Change::Rescan);
            return;
        }
        let wide: Vec<_> = name.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
        let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));
        let path = root.join(path);
        if matches!(action, FILE_ACTION_RENAMED_OLD_NAME | FILE_ACTION_RENAMED_NEW_NAME) {
            // Enumerate each affected parent once, including case-only renames
            // where both old and new spellings still resolve on Windows.
            if let Some(parent) = path.parent() {
                inbox.send(Change::Path(parent.to_path_buf()));
            }
        } else {
            inbox.send(Change::Path(path));
        }
        if next == 0 {
            break;
        }
        if next < 12 + len {
            inbox.send(Change::Rescan);
            return;
        }
        offset += next;
    }
}
