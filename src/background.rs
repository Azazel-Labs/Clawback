//! Work that runs off the event loop: one-shot jobs whose results the UI polls,
//! and `retire`, which releases large snapshots away from the event loop,
//! including results made obsolete by a newer request.
use eframe::egui;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub fn retire(value: impl Send + 'static) {
    static RETIRED: OnceLock<mpsc::Sender<Box<dyn Send>>> = OnceLock::new();
    let tx = RETIRED.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Box<dyn Send>>();
        std::thread::Builder::new()
            .name("clawback-cleanup".into())
            .spawn(move || {
                for value in rx {
                    let _span = crate::perf::span("worker.cleanup");
                    drop(value);
                }
            })
            .expect("start snapshot cleanup worker");
        tx
    });
    let _ = tx.send(Box::new(value));
}

/// A one-shot worker computing a `T` for `key`, which repaints the UI when done.
///
/// Dropping an unfinished job raises its cancel flag and retires its channel,
/// so a result that lands late is never freed on the event loop.
pub struct Job<K, T: Send + 'static> {
    key: K,
    /// `None` once the result was taken or the worker stopped.
    rx: Option<mpsc::Receiver<T>>,
    cancel: Arc<AtomicBool>,
}

impl<K, T: Send + 'static> Job<K, T> {
    /// Run `work` on a new thread.
    pub fn spawn(key: K, ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::spawn_cancellable(key, ctx, move |_| Some(work()))
    }

    /// Run `work` on a new thread; it should give up (returning `None`) once
    /// the flag it is passed is raised by [`Job::cancel`] or by dropping the job.
    pub fn spawn_cancellable(
        key: K,
        ctx: &egui::Context,
        work: impl FnOnce(&AtomicBool) -> Option<T> + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (flag, repaint) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(result) = work(&flag) {
                let _ = tx.send(result);
            }
            repaint.request_repaint();
        });
        Job { key, rx: Some(rx), cancel }
    }

    /// A job fed by the returned sender instead of a worker thread.
    #[cfg(test)]
    pub fn manual(key: K) -> (mpsc::Sender<T>, Self) {
        let (tx, rx) = mpsc::channel();
        (tx, Job { key, rx: Some(rx), cancel: Arc::default() })
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    /// Ask the worker to stop; its result, if any, is still delivered.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The result, once ready. `Disconnected` means the worker stopped without one.
    pub fn try_recv(&mut self) -> Result<T, mpsc::TryRecvError> {
        let rx = self.rx.as_ref().ok_or(mpsc::TryRecvError::Disconnected)?;
        let result = rx.try_recv();
        if !matches!(result, Err(mpsc::TryRecvError::Empty)) {
            self.rx = None;
        }
        result
    }

    /// Take a finished job's key and result out of `slot`. The slot is
    /// emptied when the job finishes or its worker stopped without a result.
    pub fn poll(slot: &mut Option<Self>) -> Option<(K, T)>
    where
        K: Copy,
    {
        let job = slot.as_mut()?;
        match job.try_recv() {
            Ok(result) => {
                let key = job.key;
                *slot = None;
                Some((key, result))
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                *slot = None;
                None
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

impl<K, T: Send + 'static> Drop for Job<K, T> {
    fn drop(&mut self) {
        if let Some(rx) = self.rx.take() {
            self.cancel();
            retire(rx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn expensive_destruction_does_not_run_on_the_caller() {
        struct SlowDrop(mpsc::Sender<std::thread::ThreadId>, mpsc::Receiver<()>);
        impl Drop for SlowDrop {
            fn drop(&mut self) {
                self.0.send(std::thread::current().id()).unwrap();
                let _ = self.1.recv_timeout(Duration::from_secs(5));
            }
        }
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        retire(SlowDrop(started_tx, release_rx));
        let worker = started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        release_tx.send(()).unwrap();
        assert_ne!(worker, std::thread::current().id());
    }

    /// Poll until the slot yields a result or empties.
    fn wait<K: Copy, T: Send>(slot: &mut Option<Job<K, T>>) -> Option<(K, T)> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while slot.is_some() {
            if let Some(done) = Job::poll(slot) {
                return Some(done);
            }
            assert!(Instant::now() < deadline, "job did not finish");
            std::thread::yield_now();
        }
        None
    }

    #[test]
    fn poll_delivers_the_key_and_result_then_empties_the_slot() {
        let ctx = egui::Context::default();
        let mut slot = Some(Job::spawn(7u32, &ctx, || "done".to_owned()));
        assert_eq!(slot.as_ref().map(|job| *job.key()), Some(7));
        assert_eq!(wait(&mut slot), Some((7, "done".to_owned())));
        assert!(slot.is_none());
        assert_eq!(Job::<u32, String>::poll(&mut slot), None);
    }

    #[test]
    fn pending_jobs_stay_and_stopped_workers_clear_the_slot() {
        let (tx, job) = Job::<(), u8>::manual(());
        let mut slot = Some(job);
        assert_eq!(Job::poll(&mut slot), None);
        assert!(slot.is_some(), "still running");
        drop(tx);
        assert_eq!(Job::poll(&mut slot), None);
        assert!(slot.is_none(), "worker stopped without a result");
    }

    #[test]
    fn try_recv_reports_each_result_once() {
        let (tx, mut job) = Job::<(), u8>::manual(());
        assert_eq!(job.try_recv(), Err(mpsc::TryRecvError::Empty));
        tx.send(3).unwrap();
        assert_eq!(job.try_recv(), Ok(3));
        assert_eq!(job.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn cancelled_workers_deliver_nothing() {
        let ctx = egui::Context::default();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let mut slot = Some(Job::spawn_cancellable((), &ctx, move |cancel| {
            go_rx.recv().unwrap();
            (!cancel.load(Ordering::Relaxed)).then_some(1u8)
        }));
        slot.as_ref().unwrap().cancel();
        go_tx.send(()).unwrap();
        assert_eq!(wait(&mut slot), None);
    }

    #[test]
    fn dropping_a_running_job_cancels_it() {
        let ctx = egui::Context::default();
        let (seen_tx, seen_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let job = Job::spawn_cancellable((), &ctx, move |cancel| {
            go_rx.recv().unwrap();
            seen_tx.send(cancel.load(Ordering::Relaxed)).unwrap();
            None::<u8>
        });
        drop(job);
        go_tx.send(()).unwrap();
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(5)), Ok(true));
    }
}
