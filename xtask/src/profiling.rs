//! Native profiling orchestration. Never invokes a shell or elevates itself.
use crate::Result;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "Usage: cargo xtask profile-scans <root> [--repeats N] [--timeout SECONDS] [--storage unknown|ssd|hdd] [--mode all|auto|directory|mft] [--threads N]\n       cargo xtask profile-scans synthetic [FILES]\n       cargo xtask profile-scans metadata <root>";

struct Options {
    root: PathBuf,
    repeats: u64,
    timeout: u64,
    storage: String,
    mode: String,
    threads: Option<u64>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let (root, rest) = args.split_first().ok_or(USAGE)?;
        let mut result = Self {
            root: root.into(),
            repeats: 3,
            timeout: 120,
            storage: "unknown".into(),
            mode: "all".into(),
            threads: None,
        };
        for pair in rest.chunks(2) {
            let [flag, value] = pair else {
                return Err(USAGE.into());
            };
            match flag.as_str() {
                "--repeats" => result.repeats = positive(value)?,
                "--timeout" => result.timeout = positive(value)?,
                "--storage" if ["unknown", "ssd", "hdd"].contains(&value.as_str()) => result.storage.clone_from(value),
                "--mode" if ["all", "auto", "directory", "mft"].contains(&value.as_str()) => {
                    result.mode.clone_from(value);
                }
                "--threads" => {
                    let n = value.parse()?;
                    if n > 256 {
                        return Err("threads must be between 0 and 256".into());
                    }
                    result.threads = Some(n);
                }
                _ => return Err(USAGE.into()),
            }
        }
        if result.mode == "all" && result.threads.is_some() {
            return Err("--threads requires a single --mode".into());
        }
        if !result.root.is_dir() {
            return Err("profile root must be an existing directory".into());
        }
        Ok(result)
    }

    fn cases(&self) -> Vec<(&str, u64)> {
        if self.mode != "all" {
            return vec![(&self.mode, self.threads.unwrap_or(0))];
        }
        let mut cases = Vec::new();
        if cfg!(windows) {
            cases.push(("mft", 1));
        }
        cases.extend([("directory", 0), ("directory", 4), ("directory", 16), ("auto", 0)]);
        cases
    }
}

fn positive(value: &str) -> Result<u64> {
    let n = value.parse()?;
    if n == 0 {
        return Err("counts and timeouts must be greater than zero".into());
    }
    Ok(n)
}

fn cargo() -> Command {
    Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn output_directory(workspace: &Path) -> Result<PathBuf> {
    let base = workspace.join("target/scan-profile");
    fs::create_dir_all(&base)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let output = base.join(format!("run-{stamp}-{}", std::process::id()));
    fs::create_dir(&output)?;
    Ok(output)
}

fn logged(command: &mut Command, output: &Path, name: &str, extension: &str) -> Result<()> {
    println!("Profiling {name}...");
    let status = command
        .stdout(Stdio::from(fs::File::create(output.join(format!("{name}.{extension}")))?))
        .stderr(Stdio::from(fs::File::create(output.join(format!("{name}-errors.txt")))?))
        .status()?;
    if !status.success() {
        return Err(format!("{name} failed ({status}); logs: {}", output.display()).into());
    }
    Ok(())
}

pub fn run(workspace: &Path, args: &[String]) -> Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    if matches!(args[0].as_str(), "synthetic" | "metadata") {
        return probe(workspace, args);
    }
    let options = Options::parse(args)?;
    let target = workspace.join("target");
    let status = cargo()
        .current_dir(workspace)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "clawback-core",
            "--features",
            "profiling",
            "--example",
            "scan-profile",
            "--target-dir",
        ])
        .arg(&target)
        .status()?;
    if !status.success() {
        return Err("Failed to build the optimized core profiler".into());
    }
    let executable = target.join("release/examples").join(format!("scan-profile{}", env::consts::EXE_SUFFIX));
    let output = output_directory(workspace)?;
    fs::write(
        output.join("environment.txt"),
        format!(
            "root={}\nstorage={}\nrepeats={}\ntimeout_seconds={}\nmode={}\nthreads={:?}\n",
            options.root.display(),
            options.storage,
            options.repeats,
            options.timeout,
            options.mode,
            options.threads
        ),
    )?;
    println!("Results: {}", output.display());
    for (mode, workers) in options.cases() {
        let mut command = Command::new(&executable);
        command
            .current_dir(workspace)
            .arg(&options.root)
            .arg(mode)
            .arg(workers.to_string())
            .arg(options.repeats.to_string())
            .arg("allocated")
            .arg(options.timeout.to_string())
            .arg(&options.storage);
        logged(&mut command, &output, &format!("{mode}-{workers}"), "csv")?;
    }
    println!("Saved {}. Check failed/cancelled/fallback columns before comparing timings.", output.display());
    Ok(())
}

fn probe(workspace: &Path, args: &[String]) -> Result<()> {
    if !cfg!(windows) {
        return Err("The MFT and Windows metadata probes require Windows".into());
    }
    let mut command = cargo();
    command.current_dir(workspace).args([
        "test",
        "--release",
        "--locked",
        "-p",
        "clawback-core",
        "--features",
        "profiling",
    ]);
    match args[0].as_str() {
        "synthetic" if args.len() <= 2 => {
            let files = args.get(1).map_or(Ok(200_000), |s| positive(s))?;
            command.arg("profile_synthetic_mft").env("CLAWBACK_PROFILE_FILES", files.to_string());
        }
        "metadata" if args.len() == 2 && Path::new(&args[1]).is_dir() => {
            command.arg("profile_metadata_calls").env("CLAWBACK_PROFILE_ROOT", fs::canonicalize(&args[1])?);
        }
        _ => return Err(USAGE.into()),
    }
    command.args(["--", "--ignored", "--nocapture"]);
    let output = output_directory(workspace)?;
    fs::write(output.join("environment.txt"), format!("probe_arguments={args:?}\n"))?;
    println!("Results: {}", output.display());
    logged(&mut command, &output, &args[0], "txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguous_or_invalid_options_before_building() {
        let root = env::temp_dir().display().to_string();
        for tail in [
            vec!["--repeats", "0"],
            vec!["--timeout", "oops"],
            vec!["--storage", "bad"],
            vec!["--mode"],
            vec!["--threads", "4"],
        ] {
            let mut args = vec![root.clone()];
            args.extend(tail.into_iter().map(str::to_owned));
            assert!(Options::parse(&args).is_err());
        }
    }

    #[test]
    fn single_mode_uses_requested_concurrency() -> Result<()> {
        let args = [
            env::temp_dir().display().to_string(),
            "--mode".into(),
            "directory".into(),
            "--threads".into(),
            "8".into(),
        ];
        let options = Options::parse(&args)?;
        assert_eq!(options.cases(), [("directory", 8)]);
        Ok(())
    }
}
