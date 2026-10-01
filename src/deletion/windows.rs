//! Windows shell recycling with native progress. The shell never deletes permanently here:
//! items the Recycle Bin can't take are stopped before deletion and reported as too large,
//! so Clawback can ask the user itself and purge them faster than the shell would.
// The windows::implement macro emits pointer casts and always-inline COM glue.
#![allow(clippy::ref_as_ptr, clippy::inline_always)]
use super::{Phase, Progress, Recycled};
use clawback_core::scan::lock;
use std::{os::windows::ffi::OsStrExt, path::Path, sync::Arc, time::Instant};
use windows::{
    Win32::{
        Foundation::{E_ABORT, ERROR_CANCELLED},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::Shell::{
            COPYENGINE_E_USER_CANCELLED, FOF_ALLOWUNDO, FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE,
            FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
            SHCreateItemFromParsingName, SIGDN_FILESYSPATH, TSF_DELETE_RECYCLE_IF_POSSIBLE,
        },
    },
    core::{Error, PCWSTR, implement},
};

#[implement(IFileOperationProgressSink)]
struct Sink {
    progress: Arc<Progress>,
}
#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> windows_core::Result<()> {
        self.progress.phase(Phase::Recycling);
        Ok(())
    }
    fn FinishOperations(&self, hrresult: windows_core::HRESULT) -> windows_core::Result<()> {
        let mut state = lock(&self.progress.state);
        if declined(hrresult) {
            state.declined = true;
        } else if hrresult.is_err() && !state.too_large {
            // Keep the first, most specific failure over the batch summary.
            state.error.get_or_insert_with(|| Error::from(hrresult).to_string());
        }
        Ok(())
    }
    fn PreRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrrename: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<'_, IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrmove: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<'_, IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrcopy: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<'_, IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, dwflags: u32, psiitem: windows_core::Ref<'_, IShellItem>) -> windows_core::Result<()> {
        if dwflags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            // Too large for the Recycle Bin, or the drive has none: the shell would delete it
            // permanently. Stop first; Clawback asks the user and purges it itself.
            lock(&self.progress.state).too_large = true;
            return Err(Error::from(E_ABORT));
        }
        let mut last = lock(&self.progress.last_item);
        if last.is_none_or(|time| time.elapsed().as_millis() >= 100) {
            *last = Some(Instant::now());
            if let Some(item) = psiitem.as_ref() {
                // SAFETY: COM supplies a live item for this callback; returned text is task-allocated.
                if let Ok(text) = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) } {
                    // SAFETY: GetDisplayName returned a terminated UTF-16 allocation.
                    let name = unsafe { text.to_string() }.unwrap_or_default();
                    // SAFETY: release exactly the allocation returned by GetDisplayName.
                    unsafe { CoTaskMemFree(Some(text.0.cast())) };
                    lock(&self.progress.state).current = name;
                }
            }
        }
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<'_, IShellItem>,
        hrdelete: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<'_, IShellItem>,
    ) -> windows_core::Result<()> {
        let mut state = lock(&self.progress.state);
        if declined(hrdelete) {
            state.declined = true;
        } else if hrdelete.is_err() && !state.too_large {
            state.error.get_or_insert_with(|| Error::from(hrdelete).to_string());
        }
        Ok(())
    }
    fn PreNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: windows_core::Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _psztemplatename: &PCWSTR,
        _dwfileattributes: u32,
        _hrnew: windows_core::HRESULT,
        _psinewitem: windows_core::Ref<'_, IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, iworktotal: u32, iworksofar: u32) -> windows_core::Result<()> {
        let mut state = lock(&self.progress.state);
        state.total = u64::from(iworktotal);
        state.done = u64::from(iworksofar.min(iworktotal));
        Ok(())
    }
    fn ResetTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balanced successful initialization on this same worker thread.
        unsafe { CoUninitialize() };
    }
}
/// Answering no to a shell prompt, such as the permanent-delete warning.
fn declined(hr: windows_core::HRESULT) -> bool {
    hr == COPYENGINE_E_USER_CANCELLED || hr == ERROR_CANCELLED.to_hresult()
}

/// Errors are display text that already carries its code.
pub fn recycle(path: &Path, progress: &Arc<Progress>) -> Result<Recycled, String> {
    let result = perform(path, progress);
    let snapshot = progress.snapshot();
    if snapshot.too_large {
        return Ok(Recycled::TooLarge);
    }
    if let Some(error) = snapshot.error {
        return Err(error);
    }
    match result {
        Err(error) if declined(error.code()) => Ok(Recycled::Declined),
        Err(error) => Err(error.to_string()),
        Ok(()) if snapshot.declined => Ok(Recycled::Declined),
        Ok(()) => Ok(Recycled::Done),
    }
}

fn perform(path: &Path, progress: &Arc<Progress>) -> windows::core::Result<()> {
    // SAFETY: the caller uses a dedicated background thread; no existing COM apartment.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
    let _apartment = Apartment;
    let absolute = std::path::absolute(path).map_err(|error| Error::new(E_ABORT, error.to_string()))?;
    let mut wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    if wide.starts_with(&unc) {
        wide.drain(..6);
        wide[0] = u16::from(b'\\');
    } else if wide.starts_with(&prefix) && wide.get(5) == Some(&u16::from(b':')) {
        wide.drain(..4);
    }
    wide.push(0);
    let sink: IFileOperationProgressSink = Sink { progress: progress.clone() }.into();
    // SAFETY: COM is initialized on this thread.
    let operation: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: the operation is live and all supplied flags are documented shell flags.
    unsafe { operation.SetOperationFlags(FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE) }?;
    // SAFETY: the UTF-16 path is terminated and lives through this call.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }?;
    // SAFETY: the locally owned sink remains alive through Unadvise.
    let cookie = unsafe { operation.Advise(&sink) }?;
    // SAFETY: the item and operation are live interfaces in this apartment.
    let queued = unsafe { operation.DeleteItem(&item, None) };
    let result = queued.and_then(|()| {
        // SAFETY: the operation, item and advised sink are live until this synchronous call returns.
        unsafe { operation.PerformOperations() }
    });
    // SAFETY: query completion while the operation is still alive.
    let aborted = unsafe { operation.GetAnyOperationsAborted() };
    // SAFETY: cookie belongs to this operation and was successfully registered above.
    let _ = unsafe { operation.Unadvise(cookie) };
    let snapshot = progress.snapshot();
    if snapshot.too_large {
        return Ok(()); // Stopped on purpose; the shell's resulting failure is expected.
    }
    result?;
    if aborted?.as_bool() && !snapshot.declined {
        return Err(Error::new(E_ABORT, "Recycling was cancelled or could not complete"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::E_ACCESSDENIED;

    #[test]
    fn progress_and_permanent_delete_guard() {
        let progress = Arc::new(Progress::default());
        let sink: IFileOperationProgressSink = Sink { progress: progress.clone() }.into();
        // SAFETY: this locally owned sink has no filesystem operation attached.
        unsafe { sink.UpdateProgress(100, 37) }.unwrap();
        assert_eq!(progress.snapshot().done, 37);
        // SAFETY: same locally owned sink; no filesystem operation.
        unsafe { sink.UpdateProgress(0, 10) }.unwrap();
        assert_eq!(progress.snapshot().done, 0);
        // The first, specific failure is kept over the shell's batch summary.
        // SAFETY: same locally owned sink; absent items are permitted.
        unsafe { sink.PostDeleteItem(0, None, E_ACCESSDENIED, None) }.unwrap();
        // SAFETY: same locally owned sink.
        unsafe { sink.FinishOperations(E_ABORT) }.unwrap();
        assert!(progress.snapshot().error.is_some_and(|e| e.contains(&format!("{E_ACCESSDENIED}"))));

        // A permanent delete is stopped before it happens and reported as too large, not failed.
        let progress = Arc::new(Progress::default());
        let sink: IFileOperationProgressSink = Sink { progress: progress.clone() }.into();
        // SAFETY: absent item is permitted; the guard rejects before reading it.
        assert!(unsafe { sink.PreDeleteItem(0, None) }.is_err());
        // SAFETY: same locally owned sink.
        unsafe { sink.PostDeleteItem(0, None, E_ABORT, None) }.unwrap();
        // SAFETY: same locally owned sink.
        unsafe { sink.FinishOperations(E_ABORT) }.unwrap();
        let snapshot = progress.snapshot();
        assert!(snapshot.too_large && snapshot.error.is_none());
    }

    #[test]
    fn answering_no_is_not_an_error() {
        let progress = Arc::new(Progress::default());
        let sink: IFileOperationProgressSink = Sink { progress: progress.clone() }.into();
        // SAFETY: locally owned sink with no filesystem operation; absent items are permitted.
        unsafe { sink.PostDeleteItem(0, None, COPYENGINE_E_USER_CANCELLED, None) }.unwrap();
        // SAFETY: same locally owned sink.
        unsafe { sink.FinishOperations(ERROR_CANCELLED.to_hresult()) }.unwrap();
        let snapshot = progress.snapshot();
        assert!(snapshot.declined && snapshot.error.is_none());
    }

    #[test]
    #[ignore = "Uses the real Windows Recycle Bin with an isolated fixture, then restores it"]
    fn recycle_fixture_and_restore() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        let path = base.join(format!("delete-fixture-{}", std::process::id()));
        assert!(path.starts_with(&base));
        std::fs::create_dir(&path).unwrap();
        for n in 0..100 {
            std::fs::write(path.join(format!("sample-{n}.txt")), b"Clawback recycle test").unwrap();
        }
        let progress = Arc::new(Progress::default());
        let start = Instant::now();
        let result = recycle(&path, &progress);
        let elapsed = start.elapsed();
        let moved = !path.exists();
        let items: Vec<_> =
            trash::os_limited::list().unwrap().into_iter().filter(|item| item.original_path() == path).collect();
        if !items.is_empty() {
            trash::os_limited::restore_all(items).unwrap();
        }
        assert_eq!(std::fs::read(path.join("sample-99.txt")).unwrap(), b"Clawback recycle test");
        // Only remove the exact fixture created by this test, after successful recovery.
        for n in 0..100 {
            std::fs::remove_file(path.join(format!("sample-{n}.txt"))).unwrap();
        }
        std::fs::remove_dir(&path).unwrap();
        result.unwrap();
        assert!(moved, "The fixture must actually leave its original location");
        let snapshot = progress.snapshot();
        assert!(snapshot.phase == Phase::Recycling);
        assert!(snapshot.total > 0);
        eprintln!(
            "Recycled and restored 100 files in {elapsed:?}; native progress {}/{}",
            snapshot.done, snapshot.total
        );
    }
}
