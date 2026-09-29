//! Large snapshots can contain millions of allocations. Release them away
//! from the event loop, including results made obsolete by a newer request.
use std::sync::{OnceLock, mpsc};

pub fn retire(value: impl Send + 'static) {
    static RETIRED: OnceLock<mpsc::Sender<Box<dyn Send>>> = OnceLock::new();
    let tx = RETIRED.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Box<dyn Send>>();
        std::thread::Builder::new()
            .name("clawback-cleanup".into())
            .spawn(move || {
                for value in rx {
                    drop(value);
                }
            })
            .expect("start snapshot cleanup worker");
        tx
    });
    let _ = tx.send(Box::new(value));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
}
