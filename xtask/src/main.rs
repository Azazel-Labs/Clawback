use clap::{Parser, Subcommand};
use std::{env, error::Error, path::Path, process::Command};
mod distribution;
mod icons;
mod package;
mod profiling;
mod release;
mod screenshots;
mod translations;
mod turbo;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Clawback developer tasks.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Run the real Turbo helper transport tests (Windows only)
    TestTurbo(turbo::Args),
    /// Check the translation catalogs or format them
    #[command(subcommand)]
    Translations(translations::Action),
    /// Compare scanner backends and phase timings with optimized builds
    ProfileScans(profiling::Args),
    /// Regenerate the checked-in PNG, ICO, ICNS and SVG icons
    Icons,
    /// Prepare a native release layout in dist/
    Package(package::Args),
    /// Generate package-manager metadata for a published stable release
    Distribution(distribution::Args),
    /// Capture the fictional demo dataset and render the screenshots
    Screenshots(screenshots::Args),
    /// Bump, validate or publish a release version
    Release(release::Args),
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli.task) {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run(task: Task) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("Cannot locate workspace")?;
    // Relative path arguments (Turbo and profile roots, distribution files) resolve against the workspace.
    env::set_current_dir(root)?;
    match task {
        Task::TestTurbo(args) => turbo::run(root, &args),
        Task::Translations(action) => translations::run(root, action),
        Task::ProfileScans(args) => profiling::run(root, &args),
        Task::Icons => icons::run(root),
        Task::Package(args) => package::run(root, &args.target, &args.tag),
        Task::Distribution(args) => distribution::run(&args),
        Task::Screenshots(args) => screenshots::run(root, &args),
        Task::Release(args) => release::run(root, &args),
    }
}

/// The Cargo running this xtask (honouring toolchain overrides), in the workspace.
fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
}

/// Run with inherited output; `failure` describes a nonzero exit.
fn run_checked(command: &mut Command, failure: &str) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{failure} ({status})").into());
    }
    Ok(())
}

/// Run quietly and return trimmed stdout; a failure reports the command and its stderr.
fn output(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        let line = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(format!("{line} failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// `#rrggbb` for SVG fills.
fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(line: &str) -> std::result::Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("cargo-xtask").chain(line.split_whitespace()))
    }

    #[test]
    fn command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// Every invocation in the workflows, VS Code tasks and docs must keep parsing.
    #[test]
    fn documented_invocations_parse() {
        for line in [
            "translations check",
            "translations fmt",
            "translations fmt --check",
            "test-turbo smoke .",
            "test-turbo fixture .",
            r"test-turbo smoke C:\",
            r"test-turbo decline C:\",
            r"test-turbo elevated C:\ --release",
            "release prepare 0.1.0",
            "release validate v0.1.0",
            "release publish 0.1.0",
            "distribution v1.2.3 target/distribution/SHA256SUMS.txt target/distribution/packages",
            "package x86_64-unknown-linux-gnu nightly",
            "package aarch64-apple-darwin v1.2.3",
            "icons",
            "screenshots",
            "screenshots --render-only",
            "profile-scans .",
            "profile-scans . --repeats 2 --timeout 30 --storage ssd --mode directory --threads 8",
            "profile-scans . --mode all",
            "profile-scans synthetic",
            "profile-scans synthetic 1000",
            "profile-scans metadata .",
        ] {
            if let Err(error) = parse(line) {
                panic!("{line}: {error}");
            }
        }
        for line in ["--help", "profile-scans --help", "release --help"] {
            assert_eq!(parse(line).expect_err(line).kind(), clap::error::ErrorKind::DisplayHelp, "{line}");
        }
    }

    #[test]
    fn rejects_malformed_invocations() {
        for line in [
            "",
            "bogus",
            "icons extra",
            "translations",
            "translations check --check",
            "test-turbo smoke",
            "test-turbo bogus .",
            "release ship 1.0.0",
            "release prepare",
            "package bogus-target v1.2.3",
            "distribution v1.2.3 sums.txt",
            "screenshots --bogus",
            "profile-scans . --repeats 0",
            "profile-scans . --timeout oops",
            "profile-scans . --storage bad",
            "profile-scans . --mode bad",
            "profile-scans . --mode",
            "profile-scans . --threads 257",
            "profile-scans synthetic 0",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
    }
}
