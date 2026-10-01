// Neither debug nor release GUI launches should create a console window.
// Terminal/report modes explicitly attach to the launcher's existing console.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Clawback: a fast, cross-platform disk space map in the spirit of SpaceMonger.

mod app;
mod background;
mod deletion;
#[cfg(feature = "screenshots")]
mod demo;
mod directoryview;
mod filetype_icons;
mod filetypes;
mod i18n;
mod icon;
mod maprender;
mod mapview;
mod perf;
#[cfg(feature = "perf-probe")]
mod perf_probe;
mod picker;
mod platform;
mod properties;
#[cfg(windows)]
mod recycle_bin;
mod scanning;
mod session;
mod settings_ui;
mod startup;
mod theme;
mod tui;
#[cfg(windows)]
mod turbo;
#[cfg(windows)]
mod watch_windows;
mod watching;

use clap::Parser;
use clawback_core::{Settings, report, scan};
use eframe::egui;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "clawback",
    bin_name = "clawback",
    version,
    about = "Clawback - see where your disk space went.",
    args_override_self = true
)]
struct Cli {
    /// Folder to open: desktop window by default (terminal map on headless systems)
    path: Option<PathBuf>,
    /// Print a text report instead of opening a window
    #[arg(long, conflicts_with_all = ["tui", "gui"])]
    report: bool,
    /// Force the interactive terminal map
    #[arg(long, conflicts_with = "gui")]
    tui: bool,
    /// Force the desktop window
    #[arg(long)]
    gui: bool,
    /// Entries per report section
    #[arg(long, value_name = "N", default_value_t = 20)]
    top: usize,
    /// Use file lengths instead of space used on disk
    #[arg(long = "apparent-size")]
    apparent: bool,
    /// Descend into other mounted filesystems
    #[arg(long = "cross-filesystems")]
    cross_fs: bool,
    /// Count every hard link (default: count once)
    #[arg(long = "count-hardlinks")]
    all_links: bool,
}

impl Cli {
    fn mode(&self) -> Mode {
        match (self.tui, self.gui) {
            (true, _) => Mode::Tui,
            (_, true) => Mode::Gui,
            _ => Mode::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Auto,
    Tui,
    Gui,
}

fn launch_mode(mode: Mode, graphical: bool, terminal: bool, dumb: bool) -> Option<Mode> {
    match mode {
        Mode::Gui => Some(Mode::Gui),
        Mode::Tui => terminal.then_some(Mode::Tui),
        Mode::Auto if graphical => Some(Mode::Gui),
        Mode::Auto if terminal && !dumb => Some(Mode::Tui),
        Mode::Auto => None,
    }
}

fn main() -> ExitCode {
    #[cfg(all(windows, feature = "turbo-probe"))]
    if let Some(result) = turbo::probe_entry() {
        return if result.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    #[cfg(windows)]
    if let Some(result) = turbo::worker_entry() {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => std::process::exit(error.raw_os_error().unwrap_or(1)),
        };
    }
    let _perf_session = perf::start();
    startup::mark("main");
    #[cfg(feature = "screenshots")]
    if let Some(path) = std::env::var_os("CLAWBACK_TERMINAL_CAPTURE") {
        return match tui::capture_demo(&PathBuf::from(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    let args = match Cli::try_parse() {
        Ok(args) => args,
        // Help and version are "errors" with a success exit code; all print to the console.
        Err(error) => {
            platform::attach_console();
            let _ = error.print();
            return ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2));
        }
    };
    if args.report {
        return run_report(&args);
    }
    let mode = args.mode();
    let graphical = mode == Mode::Gui || (mode == Mode::Auto && session::graphical());
    // Attach only for a terminal launch, keeping default GUI launches console-free.
    if !graphical {
        platform::attach_console();
    }
    let input = std::io::stdin().is_terminal();
    let output = std::io::stdout().is_terminal();
    match launch_mode(mode, graphical, input && output, std::env::var_os("TERM").is_some_and(|term| term == "dumb")) {
        None => {
            eprintln!(
                "clawback: no usable interactive interface; use --report for headless or redirected output, or --gui to force a window"
            );
            ExitCode::from(2)
        }
        Some(Mode::Tui) => {
            match tui::run(args.path.clone().unwrap_or_else(|| PathBuf::from(".")), scan_settings(&args)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("clawback: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(_) => run_gui(args.path.as_deref()),
    }
}

fn scan_settings(args: &Cli) -> Settings {
    let mut settings = Settings::load();
    settings.apparent_size |= args.apparent;
    settings.one_filesystem &= !args.cross_fs;
    settings.dedupe_hardlinks &= !args.all_links;
    settings
}

fn run_report(args: &Cli) -> ExitCode {
    platform::attach_console();
    let settings = scan_settings(args);
    let root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut options = settings.scan_options();
    if let Some(disk) = platform::disk_for(&root, &platform::all_disks()) {
        options.storage = disk.kind;
    }
    match scan::scan(&root, options) {
        Ok(r) => {
            print!("{}", report::render(&r, args.top));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("clawback: {}: {e}", root.display());
            ExitCode::FAILURE
        }
    }
}

fn run_gui(path: Option<&std::path::Path>) -> ExitCode {
    let settings = Settings::load();
    startup::mark("settings_loaded");
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Clawback")
            .with_app_id("clawback")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([420.0, 300.0])
            .with_icon(icon::icon()),
        persist_window: settings.save_pos,
        ..Default::default()
    };
    #[cfg(feature = "screenshots")]
    if let Some(capture) = std::env::var_os("CLAWBACK_DEMO_CAPTURE") {
        options.persist_window = false;
        options.viewport = options.viewport.with_app_id("clawback-demo-capture");
        options.persistence_path = Some(PathBuf::from(capture).with_extension(format!("{}.egui", std::process::id())));
    }
    #[cfg(feature = "perf-probe")]
    if std::env::var_os("CLAWBACK_PERF_SCENARIO").is_some() {
        options.persist_window = false;
        options.viewport = options.viewport.with_app_id("clawback-perf-probe");
    }
    startup::mark("native_options_ready");
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let descriptor = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            startup::mark("graphics_adapter_selected");
            if perf::enabled() {
                perf::instant(&format!("adapter.{:?}", adapter.get_info()));
            }
            descriptor(adapter)
        });
    }
    // Initialize one backend at a time on Windows, with DX12 preferred.
    // Other platforms and explicit environment overrides retain their defaults.
    let prefer_dx12 = cfg!(windows) && std::env::var_os("WGPU_BACKEND").is_none();
    let mut fallback_options = options.clone();
    if prefer_dx12 && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(fallback) = &mut fallback_options.wgpu_options.wgpu_setup {
            fallback.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN;
        }
        #[cfg(feature = "startup-probe")]
        if std::env::var_os("CLAWBACK_STARTUP_FORCE_FALLBACK").is_some() {
            setup.instance_descriptor.backends = eframe::wgpu::Backends::empty();
        }
    }
    let app_created = std::cell::Cell::new(false);
    let mut result = launch_gui(options, &settings, path, &app_created);
    // Only retry an initialization failure, never restart a running application.
    // eframe's run-and-return mode reuses its event loop for this second attempt.
    if prefer_dx12 && !app_created.get() && matches!(result, Err(eframe::Error::Wgpu(_))) {
        startup::mark("graphics_fallback");
        result = launch_gui(fallback_options, &settings, path, &app_created);
    }
    startup::mark("graphics_run_returned");
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            platform::attach_console();
            eprintln!("clawback: {e}");
            ExitCode::FAILURE
        }
    }
}

fn launch_gui(
    options: eframe::NativeOptions,
    settings: &Settings,
    path: Option<&std::path::Path>,
    app_created: &std::cell::Cell<bool>,
) -> eframe::Result {
    eframe::run_native(
        "Clawback",
        options,
        Box::new(move |cc| {
            app_created.set(true);
            startup::mark("graphics_ready");
            if let Some(state) = &cc.wgpu_render_state {
                startup::mark(&format!("graphics_backend_{:?}", state.adapter.get_info().backend));
            }
            Ok(Box::new(app::ClawbackApp::new(cc, settings.clone(), path.map(std::path::Path::to_path_buf))))
        }),
    )
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn gui_is_default_and_headless_fallback_requires_a_usable_terminal() {
        for input in [false, true] {
            for output in [false, true] {
                assert_eq!(launch_mode(Mode::Auto, true, input && output, false), Some(Mode::Gui));
                assert_eq!(
                    launch_mode(Mode::Auto, false, input && output, false),
                    (input && output).then_some(Mode::Tui)
                );
            }
        }
        assert_eq!(launch_mode(Mode::Auto, false, true, true), None);
        assert_eq!(launch_mode(Mode::Auto, true, true, true), Some(Mode::Gui));
        assert_eq!(launch_mode(Mode::Gui, false, false, true), Some(Mode::Gui));
        assert_eq!(launch_mode(Mode::Tui, true, true, true), Some(Mode::Tui));
        assert_eq!(launch_mode(Mode::Tui, true, false, false), None);
    }

    #[test]
    fn command_line_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("clawback").chain(args.iter().copied()))
    }

    #[test]
    fn modes_conflict_and_paths_can_follow_double_dash() {
        for args in [["--tui", "--gui"], ["--report", "--tui"], ["--gui", "--report"]] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        let args = parse(&["--tui", "--apparent-size", "--", "-folder"]).unwrap();
        assert_eq!(args.mode(), Mode::Tui);
        assert_eq!(args.path, Some(PathBuf::from("-folder")));
        assert!(args.apparent);
    }

    #[test]
    fn report_options_defaults_and_errors() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.mode(), Mode::Auto);
        assert!(!args.report && !args.apparent && !args.cross_fs && !args.all_links);
        assert_eq!((args.top, args.path), (20, None));
        let args = parse(&["--report", "--top", "3", "--cross-filesystems", "--count-hardlinks", "crates"]).unwrap();
        assert!(args.report && args.cross_fs && args.all_links);
        assert_eq!((args.top, args.path), (3, Some(PathBuf::from("crates"))));
        assert_eq!(parse(&["--gui", "--gui"]).unwrap().mode(), Mode::Gui);
        for args in [&["--top"][..], &["--top", "x"], &["a", "b"], &["--bogus"]] {
            assert_eq!(parse(args).unwrap_err().exit_code(), 2, "{args:?}");
        }
        for args in ["--help", "-h", "--version", "-V"] {
            assert_eq!(parse(&[args]).unwrap_err().exit_code(), 0, "{args}");
        }
    }
}
