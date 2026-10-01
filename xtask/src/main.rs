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

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("Cannot locate workspace")?;
    // Relative path arguments (Turbo and profile roots, distribution files) resolve against the workspace.
    env::set_current_dir(root)?;
    let Some((command, rest)) = args.split_first() else { return Err(usage_all()) };
    match command.as_str() {
        "test-turbo" => turbo::run(root, rest),
        "translations" => translations::run(root, rest),
        "profile-scans" => profiling::run(root, rest),
        "icons" if rest.is_empty() => icons::run(root),
        "package" => package::run(root, rest),
        "distribution" => distribution::run(rest),
        "screenshots" => screenshots::run(root, rest),
        "release" => release::run(root, rest),
        _ => Err(usage_all()),
    }
}

fn usage(lines: &str) -> Box<dyn Error> {
    format!("Usage: {lines}").into()
}

fn usage_all() -> Box<dyn Error> {
    usage(
        &[
            turbo::USAGE,
            translations::USAGE,
            icons::USAGE,
            package::USAGE,
            screenshots::USAGE,
            profiling::USAGE,
            release::USAGE,
            distribution::USAGE,
        ]
        .join("\n       "),
    )
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
