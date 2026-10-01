//! Native profiling orchestration. Never invokes a shell or elevates itself.
use crate::Result;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub const USAGE: &str = "cargo xtask profile-scans <root> [--repeats N] [--timeout SECONDS] [--storage unknown|ssd|hdd] [--mode all|auto|directory|mft] [--threads N]\n       cargo xtask profile-scans synthetic [FILES]\n       cargo xtask profile-scans metadata <root>";

/// Command-line words, passed through unchanged to the core profiler.
macro_rules! keywords {
    ($name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }
        impl $name {
            fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
        impl std::str::FromStr for $name {
            type Err = Box<dyn std::error::Error>;
            fn from_str(value: &str) -> Result<Self> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(crate::usage(USAGE)),
                }
            }
        }
    };
}
keywords!(Storage { Unknown = "unknown", Ssd = "ssd", Hdd = "hdd" });
keywords!(Mode { Auto = "auto", Directory = "directory", Mft = "mft" });

struct Options {
    root: PathBuf,
    repeats: u64,
    timeout: u64,
    storage: Storage,
    /// One scanner, or None for the standard comparison set ("all").
    mode: Option<Mode>,
    threads: Option<u64>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let (root, rest) = args.split_first().ok_or_else(|| crate::usage(USAGE))?;
        let mut result =
            Self { root: root.into(), repeats: 3, timeout: 120, storage: Storage::Unknown, mode: None, threads: None };
        for pair in rest.chunks(2) {
            let [flag, value] = pair else { return Err(crate::usage(USAGE)) };
            match flag.as_str() {
                "--repeats" => result.repeats = positive(value)?,
                "--timeout" => result.timeout = positive(value)?,
                "--storage" => result.storage = value.parse()?,
                "--mode" => result.mode = if value == "all" { None } else { Some(value.parse()?) },
                "--threads" => {
                    let n = value.parse()?;
                    if n > 256 {
                        return Err("threads must be between 0 and 256".into());
                    }
                    result.threads = Some(n);
                }
                _ => return Err(crate::usage(USAGE)),
            }
        }
        if result.mode.is_none() && result.threads.is_some() {
            return Err("--threads requires a single --mode".into());
        }
        if !result.root.is_dir() {
            return Err("profile root must be an existing directory".into());
        }
        Ok(result)
    }

    fn cases(&self) -> Vec<(Mode, u64)> {
        if let Some(mode) = self.mode {
            return vec![(mode, self.threads.unwrap_or(0))];
        }
        let mut cases = Vec::new();
        if cfg!(windows) {
            cases.push((Mode::Mft, 1));
        }
        cases.extend([(Mode::Directory, 0), (Mode::Directory, 4), (Mode::Directory, 16), (Mode::Auto, 0)]);
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
    command
        .stdout(Stdio::from(fs::File::create(output.join(format!("{name}.{extension}")))?))
        .stderr(Stdio::from(fs::File::create(output.join(format!("{name}-errors.txt")))?));
    crate::run_checked(command, &format!("{name} failed; logs: {}", output.display()))
}

pub fn run(workspace: &Path, args: &[String]) -> Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!("Usage: {USAGE}");
        return Ok(());
    }
    if matches!(args[0].as_str(), "synthetic" | "metadata") {
        return probe(workspace, args);
    }
    let options = Options::parse(args)?;
    let target = workspace.join("target");
    crate::run_checked(
        crate::cargo(workspace)
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
            .arg(&target),
        "Failed to build the optimized core profiler",
    )?;
    let executable = target.join("release/examples").join(format!("scan-profile{}", env::consts::EXE_SUFFIX));
    let output = output_directory(workspace)?;
    let mode = options.mode.map_or("all", Mode::as_str);
    fs::write(
        output.join("environment.txt"),
        format!(
            "root={}\nstorage={}\nrepeats={}\ntimeout_seconds={}\nmode={mode}\nthreads={:?}\n",
            options.root.display(),
            options.storage.as_str(),
            options.repeats,
            options.timeout,
            options.threads
        ),
    )?;
    println!("Results: {}", output.display());
    for (mode, workers) in options.cases() {
        let mut command = Command::new(&executable);
        command
            .current_dir(workspace)
            .arg(&options.root)
            .arg(mode.as_str())
            .arg(workers.to_string())
            .arg(options.repeats.to_string())
            .arg("allocated")
            .arg(options.timeout.to_string())
            .arg(options.storage.as_str());
        logged(&mut command, &output, &format!("{}-{workers}", mode.as_str()), "csv")?;
    }
    println!("Saved {}. Check failed/cancelled/fallback columns before comparing timings.", output.display());
    Ok(())
}

fn probe(workspace: &Path, args: &[String]) -> Result<()> {
    if !cfg!(windows) {
        return Err("The MFT and Windows metadata probes require Windows".into());
    }
    let mut command = crate::cargo(workspace);
    command.args(["test", "--release", "--locked", "-p", "clawback-core", "--features", "profiling"]);
    match args {
        [kind, files @ ..] if kind == "synthetic" && files.len() <= 1 => {
            let files = files.first().map_or(Ok(200_000), |s| positive(s))?;
            command.arg("profile_synthetic_mft").env("CLAWBACK_PROFILE_FILES", files.to_string());
        }
        [kind, root] if kind == "metadata" && Path::new(root).is_dir() => {
            command.arg("profile_metadata_calls").env("CLAWBACK_PROFILE_ROOT", fs::canonicalize(root)?);
        }
        _ => return Err(crate::usage(USAGE)),
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
            vec!["--mode", "bad"],
            vec!["--mode"],
            vec!["--threads", "4"],
            vec!["--mode", "all", "--threads", "4"],
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
            "--storage".into(),
            "ssd".into(),
        ];
        let options = Options::parse(&args)?;
        assert_eq!(options.cases(), [(Mode::Directory, 8)]);
        assert_eq!(options.storage.as_str(), "ssd");
        Ok(())
    }
}
