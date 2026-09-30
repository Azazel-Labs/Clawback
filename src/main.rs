// Neither debug nor release GUI launches should create a console window.
// Terminal/report modes explicitly attach to the launcher's existing console.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Clawback: a fast, cross-platform disk space map in the spirit of SpaceMonger.

mod app;
mod background;
#[cfg(feature = "screenshots")]
mod demo;
mod directoryview;
mod icon;
mod maprender;
mod mapview;
mod platform;
mod scanning;
mod session;
mod theme;
mod tui;
#[cfg(windows)]
mod watch_windows;
mod watching;

use clawback_core::{Settings, report, scan};
use eframe::egui;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Clawback - see where your disk space went.

USAGE:
    clawback [PATH]                 Desktop window (terminal map on headless systems)
    clawback --report [OPTIONS] [PATH]
                                Print a text report instead of opening a window

OPTIONS:
    --tui                      Force the interactive terminal map
    --gui                      Force the desktop window
    --top N                     Entries per report section (default 20)
    --apparent-size             Use file lengths instead of space used on disk
    --cross-filesystems         Descend into other mounted filesystems
    --count-hardlinks           Count every hard link (default: count once)
    -h, --help                  Show this help
    -V, --version               Show the version";

struct Args {
    path: Option<PathBuf>,
    report: bool,
    mode: Mode,
    top: usize,
    apparent: bool,
    cross_fs: bool,
    all_links: bool,
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

fn parse_args() -> Result<Option<Args>, String> {
    parse_args_from(std::env::args_os().skip(1))
}

fn parse_args_from(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Option<Args>, String> {
    let mut a = Args {
        path: None,
        report: false,
        mode: Mode::Auto,
        top: 20,
        apparent: false,
        cross_fs: false,
        all_links: false,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                platform::attach_console();
                println!("{USAGE}");
                return Ok(None);
            }
            Some("-V" | "--version") => {
                platform::attach_console();
                println!("clawback {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            Some("--report") => a.report = true,
            Some("--tui" | "--gui") => {
                let mode = if arg == "--tui" { Mode::Tui } else { Mode::Gui };
                if a.mode != Mode::Auto && a.mode != mode {
                    return Err("--tui and --gui cannot be combined".into());
                }
                a.mode = mode;
            }
            Some("--") => {
                for path in it.by_ref() {
                    if a.path.replace(path.into()).is_some() {
                        return Err("only one PATH may be supplied".into());
                    }
                }
            }
            Some("--top") => {
                a.top = it.next().and_then(|v| v.to_str()?.parse().ok()).ok_or("--top needs a number")?;
            }
            Some("--apparent-size") => a.apparent = true,
            Some("--cross-filesystems") => a.cross_fs = true,
            Some("--count-hardlinks") => a.all_links = true,
            Some(s) if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}\n\n{USAGE}")),
            _ => {
                if a.path.replace(PathBuf::from(arg)).is_some() {
                    return Err("only one PATH may be supplied".into());
                }
            }
        }
    }
    if a.report && a.mode != Mode::Auto {
        return Err("--report cannot be combined with --tui or --gui".into());
    }
    Ok(Some(a))
}

fn main() -> ExitCode {
    #[cfg(feature = "screenshots")]
    if let Some(path) = std::env::var_os("CLAWBACK_TERMINAL_CAPTURE") {
        return match tui::capture_demo(&PathBuf::from(path)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            platform::attach_console();
            eprintln!("clawback: {e}");
            return ExitCode::from(2);
        }
    };
    if args.report {
        return run_report(&args);
    }
    let graphical = args.mode == Mode::Gui || (args.mode == Mode::Auto && session::graphical());
    // Attach only for a terminal launch, keeping default GUI launches console-free.
    if !graphical {
        platform::attach_console();
    }
    let input = std::io::stdin().is_terminal();
    let output = std::io::stdout().is_terminal();
    match launch_mode(
        args.mode,
        graphical,
        input && output,
        std::env::var_os("TERM").is_some_and(|term| term == "dumb"),
    ) {
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
        Some(_) => run_gui(args.path),
    }
}

fn scan_settings(args: &Args) -> Settings {
    let mut settings = Settings::load();
    settings.apparent_size |= args.apparent;
    settings.one_filesystem &= !args.cross_fs;
    settings.dedupe_hardlinks &= !args.all_links;
    settings
}

fn run_report(args: &Args) -> ExitCode {
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

fn run_gui(path: Option<PathBuf>) -> ExitCode {
    let settings = Settings::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Clawback")
            .with_app_id("clawback")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([420.0, 300.0])
            .with_icon(icon::icon()),
        persist_window: settings.save_pos,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Clawback",
        options,
        Box::new(move |cc| Ok(Box::new(app::ClawbackApp::new(cc, settings, path)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            platform::attach_console();
            eprintln!("clawback: {e}");
            ExitCode::FAILURE
        }
    }
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
    fn modes_conflict_and_paths_can_follow_double_dash() {
        for args in [["--tui", "--gui"], ["--report", "--tui"], ["--gui", "--report"]] {
            assert!(parse_args_from(args.map(Into::into)).is_err());
        }
        let args = parse_args_from(["--tui", "--apparent-size", "--", "-folder"].map(Into::into)).unwrap().unwrap();
        assert_eq!(args.mode, Mode::Tui);
        assert_eq!(args.path, Some(PathBuf::from("-folder")));
        assert!(args.apparent);
    }
}
