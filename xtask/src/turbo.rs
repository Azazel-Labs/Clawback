//! Real helper transport tests. Elevation requires an explicit mode; no shell scripts.
use crate::Result;
use std::{path::Path, process::Command};

pub const USAGE: &str = "cargo xtask test-turbo <smoke|fixture|elevated|decline> <root> [--release]";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    if !cfg!(windows) {
        return Err("Turbo subprocess tests are Windows-only".into());
    }
    let (mode, path, release) = match args {
        [mode, path] => (mode, path, false),
        [mode, path, flag] if flag == "--release" => (mode, path, true),
        _ => return Err(crate::usage(USAGE)),
    };
    if !matches!(mode.as_str(), "smoke" | "fixture" | "elevated" | "decline") {
        return Err(crate::usage(USAGE));
    }
    let path = std::path::absolute(Path::new(path))?;
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
    if matches!(mode.as_str(), "elevated" | "decline") {
        println!(
            "Windows will request permission. {} the UAC prompt.",
            if mode == "decline" { "Cancel" } else { "Approve" }
        );
    }
    let executable = root.join("target").join(if release { "release" } else { "debug" }).join("clawback.exe");
    // Captured and replayed: the probe is a GUI-subsystem binary.
    let output = Command::new(executable).args(["--turbo-probe", mode]).arg(path).output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("Turbo test failed: {}", output.status).into());
    }
    Ok(())
}
