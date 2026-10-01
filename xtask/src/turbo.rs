//! Real helper transport tests. Elevation requires an explicit mode; no shell scripts.
use crate::Result;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which transport test the probe runs
    mode: Mode,
    /// Directory to scan through the helper
    root: PathBuf,
    /// Build and run the optimized probe
    #[arg(long)]
    release: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Mode {
    /// Launch the real helper and scan without elevation
    Smoke,
    /// Verify transfer of a synthetic fixture
    Fixture,
    /// Request elevation; approve the UAC prompt
    Elevated,
    /// Request elevation; cancel the UAC prompt
    Decline,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Fixture => "fixture",
            Self::Elevated => "elevated",
            Self::Decline => "decline",
        }
    }
}

pub fn run(root: &Path, args: &Args) -> Result<()> {
    if !cfg!(windows) {
        return Err("Turbo subprocess tests are Windows-only".into());
    }
    let (mode, release) = (args.mode, args.release);
    let path = std::path::absolute(&args.root)?;
    if !path.is_dir() {
        return Err("Turbo test root must be a directory".into());
    }
    let mut build = crate::cargo(root);
    build
        .args(["build", "--locked", "--bin", "clawback", "--features", "turbo-probe", "--target-dir"])
        .arg(root.join("target"));
    if release {
        build.arg("--release");
    }
    crate::run_checked(&mut build, "Could not build the Turbo probe")?;
    if matches!(mode, Mode::Elevated | Mode::Decline) {
        println!(
            "Windows will request permission. {} the UAC prompt.",
            if mode == Mode::Decline { "Cancel" } else { "Approve" }
        );
    }
    let executable = root.join("target").join(if release { "release" } else { "debug" }).join("clawback.exe");
    // Captured and replayed: the probe is a GUI-subsystem binary.
    let output = Command::new(executable).args(["--turbo-probe", mode.as_str()]).arg(path).output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("Turbo test failed: {}", output.status).into());
    }
    Ok(())
}
