//! Opt-in startup timings, written only when `CLAWBACK_STARTUP_TRACE` is set.
//! Every mark is also a perf-trace instant.
use std::{
    fs::File,
    io::Write,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

struct Trace {
    started: Instant,
    file: Mutex<File>,
    ui_finished: AtomicBool,
}

static TRACE: LazyLock<Option<Trace>> = LazyLock::new(|| {
    let started = Instant::now();
    let path = std::env::var_os("CLAWBACK_STARTUP_TRACE")?;
    let file = File::create(path).ok()?;
    Some(Trace { started, file: Mutex::new(file), ui_finished: AtomicBool::new(false) })
});

/// The first UI frame's number, and whether a later frame has started.
static FIRST_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static NEXT_FRAME: AtomicBool = AtomicBool::new(false);

pub fn mark(stage: &str) {
    crate::perf::instant(stage);
    if let Some(trace) = &*TRACE {
        let mut file = clawback_core::scan::lock(&trace.file);
        let _ = writeln!(file, "{:.3} ms\t{stage}", trace.started.elapsed().as_secs_f64() * 1000.0);
    }
}

pub fn frame_started(ctx: &eframe::egui::Context) {
    if !crate::perf::enabled() && TRACE.is_none() {
        return;
    }
    let frame = ctx.cumulative_frame_nr();
    let first = FIRST_FRAME.load(Ordering::Relaxed);
    if first == u64::MAX {
        FIRST_FRAME.store(frame, Ordering::Relaxed);
        mark("first_ui_started");
        ctx.request_repaint();
    } else if frame > first && !NEXT_FRAME.swap(true, Ordering::Relaxed) {
        // The first frame has returned through the renderer before this point.
        mark("second_frame_started");
    }
}

pub fn frame_finished() {
    if let Some(trace) = &*TRACE
        && !trace.ui_finished.swap(true, Ordering::Relaxed)
    {
        mark("first_ui_finished");
    }
}
