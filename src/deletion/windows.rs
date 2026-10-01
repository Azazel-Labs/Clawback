//! Windows shell recycling with native progress and no permanent-delete fallback.
// The windows::implement macro emits pointer casts and always-inline COM glue.
#![allow(clippy::ref_as_ptr, clippy::inline_always)]
use super::{Phase, Progress};
use clawback_core::scan::lock;
use std::{os::windows::ffi::OsStrExt, path::Path, sync::Arc, time::Instant};
use windows::{
    Win32::{
        Foundation::E_ABORT,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::Shell::{
            FOF_ALLOWUNDO, FOF_NO_UI, FOF_WANTNUKEWARNING, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
            IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
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
        if hrresult.is_err() {
            lock(&self.progress.state).error = Some(hrresult.message());
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
            lock(&self.progress.state).error = Some("This item cannot be moved to the Recycle Bin".into());
            return Err(Error::new(E_ABORT, "This item cannot be moved to the Recycle Bin"));
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
        if hrdelete.is_err() {
            lock(&self.progress.state).error = Some(hrdelete.message());
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
        state.total = iworktotal;
        state.done = iworksofar.min(iworktotal);
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
pub fn recycle(path: &Path, progress: &Arc<Progress>) -> windows::core::Result<()> {
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
    unsafe {
        operation.SetOperationFlags(
            FOF_NO_UI | FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE,
        )
    }?;
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
    if let Some(error) = progress.snapshot().error {
        return Err(Error::new(E_ABORT, error));
    }
    result?;
    if aborted?.as_bool() {
        return Err(Error::new(E_ABORT, "Recycling was cancelled or could not complete"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // SAFETY: absent item is permitted and the guard rejects before dereferencing it.
        assert!(unsafe { sink.PreDeleteItem(0, None) }.is_err());
        assert!(progress.snapshot().error.is_some());
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
