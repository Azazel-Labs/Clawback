//! Opt-in startup timings, written only when `CLAWBACK_STARTUP_TRACE` is set.
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
    first_frame: AtomicU64,
    next_frame: AtomicBool,
    ui_finished: AtomicBool,
}

static TRACE: LazyLock<Option<Trace>> = LazyLock::new(|| {
    let started = Instant::now();
    let path = std::env::var_os("CLAWBACK_STARTUP_TRACE")?;
    let file = File::create(path).ok()?;
    Some(Trace {
        started,
        file: Mutex::new(file),
        first_frame: AtomicU64::new(u64::MAX),
        next_frame: AtomicBool::new(false),
        ui_finished: AtomicBool::new(false),
    })
});

pub fn mark(stage: &str) {
    crate::perf::instant(stage);
    if let Some(trace) = &*TRACE {
        let mut file = clawback_core::scan::lock(&trace.file);
        let _ = writeln!(file, "{:.3} ms\t{stage}", trace.started.elapsed().as_secs_f64() * 1000.0);
    }
}

pub fn frame_started(ctx: &eframe::egui::Context) {
    // Full perf traces also need these markers, independently of the legacy TSV.
    if crate::perf::enabled() {
        match ctx.cumulative_frame_nr() {
            0 => {
                crate::perf::instant("first_ui_started");
                ctx.request_repaint();
            }
            1 => crate::perf::instant("second_frame_started"),
            _ => {}
        }
    }
    let Some(trace) = &*TRACE else { return };
    let frame = ctx.cumulative_frame_nr();
    let first = trace.first_frame.load(Ordering::Relaxed);
    if first == u64::MAX {
        trace.first_frame.store(frame, Ordering::Relaxed);
        mark("first_ui_started");
        ctx.request_repaint();
    } else if frame > first && !trace.next_frame.swap(true, Ordering::Relaxed) {
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
