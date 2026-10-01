//! Native profiling orchestration. Never invokes a shell or elevates itself.
use crate::Result;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

/// Command-line words, passed through unchanged to the core profiler.
macro_rules! keywords {
    ($name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
        enum $name {
            $(#[value(name = $text)] $variant),+
        }
        impl $name {
            fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
    };
}
keywords!(Storage { Unknown = "unknown", Ssd = "ssd", Hdd = "hdd" });
// `All` selects the standard comparison set rather than one scanner.
keywords!(Mode { All = "all", Auto = "auto", Directory = "directory", Mft = "mft" });

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true, arg_required_else_help = true)]
pub struct Args {
    #[command(subcommand)]
    probe: Option<Probe>,
    #[command(flatten)]
    options: Options,
}

/// Dedicated Windows probes, run as ignored clawback-core tests.
#[derive(Debug, clap::Subcommand)]
enum Probe {
    /// Time MFT parsing of a synthetic volume (Windows)
    Synthetic {
        /// Number of synthetic files
        #[arg(default_value_t = 200_000, value_parser = clap::value_parser!(u64).range(1..))]
        files: u64,
    },
    /// Time Windows metadata calls under ROOT (Windows)
    Metadata {
        /// Existing directory to probe
        root: PathBuf,
    },
}

#[derive(Debug, clap::Args)]
struct Options {
    /// Existing directory to scan
    #[arg(required = true)]
    root: Option<PathBuf>,
    /// Runs per case
    #[arg(long, value_name = "N", default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..))]
    repeats: u64,
    /// Per-run timeout
    #[arg(long, value_name = "SECONDS", default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    /// Storage kind reported to the scanner
    #[arg(long, value_enum, default_value_t = Storage::Unknown)]
    storage: Storage,
    /// One scanner, or all for the standard comparison set
    #[arg(long, value_enum, default_value_t = Mode::All)]
    mode: Mode,
    /// Worker threads for a single --mode (0 = automatic)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(..=256))]
    threads: Option<u64>,
}

impl Options {
    /// Checks clap cannot express; returns the scan root.
    fn validate(&self) -> Result<&Path> {
        if self.mode == Mode::All && self.threads.is_some() {
            return Err("--threads requires a single --mode".into());
        }
        let root = self.root.as_deref().ok_or("missing profile root")?;
        if !root.is_dir() {
            return Err("profile root must be an existing directory".into());
        }
        Ok(root)
    }

    fn cases(&self) -> Vec<(Mode, u64)> {
        if self.mode != Mode::All {
            return vec![(self.mode, self.threads.unwrap_or(0))];
        }
        let mut cases = Vec::new();
        if cfg!(windows) {
            cases.push((Mode::Mft, 1));
        }
        cases.extend([(Mode::Directory, 0), (Mode::Directory, 4), (Mode::Directory, 16), (Mode::Auto, 0)]);
        cases
    }
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

pub fn run(workspace: &Path, args: &Args) -> Result<()> {
    if let Some(probe) = &args.probe {
        return run_probe(workspace, probe);
    }
    let options = &args.options;
    let root = options.validate()?;
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
    fs::write(
        output.join("environment.txt"),
        format!(
            "root={}\nstorage={}\nrepeats={}\ntimeout_seconds={}\nmode={}\nthreads={:?}\n",
            root.display(),
            options.storage.as_str(),
            options.repeats,
            options.timeout,
            options.mode.as_str(),
            options.threads
        ),
    )?;
    println!("Results: {}", output.display());
    for (mode, workers) in options.cases() {
        let mut command = Command::new(&executable);
        command
            .current_dir(workspace)
            .arg(root)
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

fn run_probe(workspace: &Path, probe: &Probe) -> Result<()> {
    if !cfg!(windows) {
        return Err("The MFT and Windows metadata probes require Windows".into());
    }
    let mut command = crate::cargo(workspace);
    command.args(["test", "--release", "--locked", "-p", "clawback-core", "--features", "profiling"]);
    let name = match probe {
        Probe::Synthetic { files } => {
            command.arg("profile_synthetic_mft").env("CLAWBACK_PROFILE_FILES", files.to_string());
            "synthetic"
        }
        Probe::Metadata { root } => {
            if !root.is_dir() {
                return Err("metadata root must be an existing directory".into());
            }
            command.arg("profile_metadata_calls").env("CLAWBACK_PROFILE_ROOT", fs::canonicalize(root)?);
            "metadata"
        }
    };
    command.args(["--", "--ignored", "--nocapture"]);
    let output = output_directory(workspace)?;
    fs::write(output.join("environment.txt"), format!("probe={probe:?}\n"))?;
    println!("Results: {}", output.display());
    logged(&mut command, &output, name, "txt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    fn parse(tail: &[&str]) -> Result<Args> {
        let root = env::temp_dir().display().to_string();
        let words = ["profile-scans", root.as_str()].into_iter().chain(tail.iter().copied());
        let args = Cli::try_parse_from(words)?.args;
        args.options.validate()?;
        Ok(args)
    }

    #[test]
    fn rejects_ambiguous_or_invalid_options_before_building() {
        for tail in [
            &["--repeats", "0"][..],
            &["--timeout", "oops"],
            &["--storage", "bad"],
            &["--mode", "bad"],
            &["--mode"],
            &["--threads", "4"],
            &["--mode", "all", "--threads", "4"],
            &["--mode", "auto", "--threads", "257"],
        ] {
            assert!(parse(tail).is_err(), "{tail:?}");
        }
        assert!(Cli::try_parse_from(["profile-scans", "--mode", "auto"]).is_err());
    }

    #[test]
    fn single_mode_uses_requested_concurrency() -> Result<()> {
        let args = parse(&["--mode", "directory", "--threads", "8", "--storage", "ssd"])?;
        assert!(args.probe.is_none());
        assert_eq!(args.options.cases(), [(Mode::Directory, 8)]);
        assert_eq!(args.options.storage.as_str(), "ssd");
        Ok(())
    }

    #[test]
    fn probe_keywords_select_probes() -> Result<()> {
        let args = Cli::try_parse_from(["profile-scans", "synthetic"])?.args;
        assert!(matches!(args.probe, Some(Probe::Synthetic { files: 200_000 })));
        let args = Cli::try_parse_from(["profile-scans", "metadata", "."])?.args;
        assert!(matches!(args.probe, Some(Probe::Metadata { .. })));
        Ok(())
    }
}
