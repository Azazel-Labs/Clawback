//! Real helper transport tests. Elevation requires an explicit mode; no shell scripts.
use crate::Result;
use std::{env, path::Path, process::Command};

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    if !cfg!(windows) {
        return Err("Turbo subprocess tests are Windows-only".into());
    }
    if args.len() < 2
        || args.len() > 3
        || !matches!(args[0].as_str(), "smoke" | "fixture" | "elevated" | "decline")
        || (args.len() == 3 && args[2] != "--release")
    {
        return Err("Usage: cargo xtask test-turbo <smoke|fixture|elevated|decline> <root> [--release]".into());
    }
    let release = args.len() == 3;
    let path = std::path::absolute(Path::new(&args[1]))?;
    if !path.is_dir() {
        return Err("Turbo test root must be a directory".into());
    }
    let mut build = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .current_dir(root)
        .args(["build", "--locked", "--bin", "clawback", "--features", "turbo-probe", "--target-dir"])
        .arg(root.join("target"));
    if release {
        build.arg("--release");
    }
    if !build.status()?.success() {
        return Err("Could not build the Turbo probe".into());
    }
    if matches!(args[0].as_str(), "elevated" | "decline") {
        println!(
            "Windows will request permission. {} the UAC prompt.",
            if args[0] == "decline" { "Cancel" } else { "Approve" }
        );
    }
    let executable = root.join("target").join(if release { "release" } else { "debug" }).join("clawback.exe");
    let output = Command::new(executable).args(["--turbo-probe", &args[0]]).arg(path).output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("Turbo test failed: {}", output.status).into());
    }
    Ok(())
}
