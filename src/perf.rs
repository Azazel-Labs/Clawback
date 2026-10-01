//! Opt-in, bounded, asynchronous CPU tracing. No file names or input text are recorded.
use std::{
    borrow::Cow,
    fs::File,
    io::{BufWriter, Write},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{JoinHandle, ThreadId},
    time::Instant,
};

struct Event {
    at: u64,
    duration: u64,
    thread: ThreadId,
    kind: &'static str,
    name: Cow<'static, str>,
    value: f64,
}
enum Message {
    Event(Event),
    Finish,
}
struct Trace {
    start: Instant,
    tx: mpsc::SyncSender<Message>,
    dropped: AtomicU64,
    writer: Mutex<Option<JoinHandle<()>>>,
}
static TRACE: OnceLock<Option<Trace>> = OnceLock::new();

pub struct Session;
pub fn start() -> Session {
    TRACE.get_or_init(|| {
        let start = Instant::now();
        let path = std::env::var_os("CLAWBACK_PERF_TRACE")?;
        let file = File::create(path).ok()?;
        let (tx, rx) = mpsc::sync_channel(8192);
        let writer = std::thread::Builder::new()
            .name("clawback-trace".into())
            .spawn(move || {
                let mut file = BufWriter::new(file);
                let _ = writeln!(file, "at_us\tduration_us\tthread\tkind\tname\tvalue");
                while let Ok(Message::Event(e)) = rx.recv() {
                    let _ =
                        writeln!(file, "{}\t{}\t{:?}\t{}\t{}\t{}", e.at, e.duration, e.thread, e.kind, e.name, e.value);
                }
                let _ = file.flush();
            })
            .ok()?;
        Some(Trace { start, tx, dropped: AtomicU64::new(0), writer: Mutex::new(Some(writer)) })
    });
    instant("process.main");
    Session
}

fn trace() -> Option<&'static Trace> {
    TRACE.get().and_then(Option::as_ref)
}
pub fn enabled() -> bool {
    trace().is_some()
}

fn send(trace: &Trace, event: Event) {
    if trace.tx.try_send(Message::Event(event)).is_err() {
        trace.dropped.fetch_add(1, Ordering::Relaxed);
    }
}
pub fn instant(name: &str) {
    if let Some(trace) = trace() {
        send(
            trace,
            Event {
                at: trace.start.elapsed().as_micros() as u64,
                duration: 0,
                thread: std::thread::current().id(),
                kind: "instant",
                name: name.to_owned().into(),
                value: 0.0,
            },
        );
    }
}
pub fn counter(name: &'static str, value: f64) {
    if let Some(trace) = trace() {
        send(
            trace,
            Event {
                at: trace.start.elapsed().as_micros() as u64,
                duration: 0,
                thread: std::thread::current().id(),
                kind: "counter",
                name: name.into(),
                value,
            },
        );
    }
}

pub struct Span {
    start: Option<Instant>,
    name: &'static str,
}
pub fn span(name: &'static str) -> Span {
    Span { start: enabled().then(Instant::now), name }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let (Some(start), Some(trace)) = (self.start, trace()) {
            send(
                trace,
                Event {
                    at: start.duration_since(trace.start).as_micros() as u64,
                    duration: start.elapsed().as_micros() as u64,
                    thread: std::thread::current().id(),
                    kind: "span",
                    name: self.name.into(),
                    value: 0.0,
                },
            );
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let Some(trace) = trace() else { return };
        instant("process.main_return");
        // Only shutdown may wait for the writer. Runtime producers never wait for disk I/O.
        let event = Event {
            at: trace.start.elapsed().as_micros() as u64,
            duration: 0,
            thread: std::thread::current().id(),
            kind: "counter",
            name: "trace.dropped_events".into(),
            value: trace.dropped.load(Ordering::Relaxed) as f64,
        };
        let _ = trace.tx.send(Message::Event(event));
        let _ = trace.tx.send(Message::Finish);
        if let Some(writer) = clawback_core::scan::lock(&trace.writer).take() {
            let _ = writer.join();
        }
    }
}

/// A frame interval can include intentional idle time; it is not a hitch measurement.
pub fn frame(ctx: &eframe::egui::Context, frame: &eframe::Frame) -> Span {
    if enabled() && ctx.current_pass_index() == 0 {
        counter("frame.number", ctx.cumulative_frame_nr() as f64);
        counter("frame.pass", ctx.current_pass_index() as f64);
        if let Some(seconds) = frame.info().cpu_usage {
            counter("frame.previous_cpu_ms", f64::from(seconds) * 1000.0);
        }
        counter("frame.input_events", ctx.input(|i| i.events.len()) as f64);
        if ctx.input(|i| i.viewport().close_requested()) {
            instant("shutdown.close_requested");
        }
    }
    span("ui.frame")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturated_trace_queue_drops_events_without_waiting_for_the_writer() {
        let (tx, rx) = mpsc::sync_channel(1);
        let trace = Trace { start: Instant::now(), tx, dropped: AtomicU64::new(0), writer: Mutex::new(None) };
        for _ in 0..3 {
            send(
                &trace,
                Event {
                    at: 0,
                    duration: 1,
                    thread: std::thread::current().id(),
                    kind: "span",
                    name: "test".into(),
                    value: 0.0,
                },
            );
        }
        assert_eq!(trace.dropped.load(Ordering::Relaxed), 2);
        assert!(matches!(rx.try_recv(), Ok(Message::Event(_))));
        assert!(rx.try_recv().is_err());
    }
}
