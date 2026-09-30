use std::{env, error::Error, fs, path::Path, process::Command};
use toml_edit::{DocumentMut, value};
mod distribution;
mod screenshots;
mod translations;

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
    env::set_current_dir(root)?;
    if args.first().is_some_and(|arg| arg == "translations") {
        return translations::run(root, &args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "distribution") {
        return distribution::run(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "screenshots") {
        return screenshots::run(root, &args[1..]);
    }
    if args.len() != 3 || args[0] != "release" {
        return Err("Usage: cargo xtask translations <check|fmt [--check]>\n       cargo xtask screenshots [--render-only]\n       cargo xtask release <prepare|validate|publish> <version>\n       cargo xtask distribution <version> <SHA256SUMS.txt> <output-directory>".into());
    }
    let version = parse_version(&args[2])?;
    match args[1].as_str() {
        "prepare" => {
            prepare(root, version)?;
            println!("Prepared v{version}. Review Cargo.toml and Cargo.lock, commit and push main.");
            println!("Then run: cargo xtask release publish {version}");
        }
        "validate" => {
            validate(root, version)?;
            println!("Validated v{version}.");
        }
        "publish" => publish(root, version)?,
        _ => return Err("Expected prepare, validate, or publish".into()),
    }
    Ok(())
}

fn parse_version(input: &str) -> Result<&str> {
    let version = input.strip_prefix('v').unwrap_or(input);
    let (base, prerelease) = version.split_once('-').map_or((version, None), |(base, suffix)| (base, Some(suffix)));
    let number = |s: &str| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'))
            && s.parse::<u64>().is_ok()
    };
    let parts: Vec<_> = base.split('.').collect();
    let valid_suffix = prerelease.is_none_or(|suffix| {
        suffix.split_once('.').is_some_and(|(channel, n)| matches!(channel, "alpha" | "beta" | "rc") && number(n))
    });
    if parts.len() != 3 || !parts.iter().all(|s| number(s)) || !valid_suffix {
        return Err("Use MAJOR.MINOR.PATCH, optionally followed by -alpha.N, -beta.N, or -rc.N".into());
    }
    Ok(version)
}

fn command(program: &str, args: &[&str]) -> Result<String> {
    command_at(Path::new("."), program, args)
}

fn command_at(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(format!("{program} {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn updated_manifest(original: &str, version: &str) -> Result<String> {
    let mut manifest: DocumentMut = original.parse()?;
    manifest["workspace"]["package"]["version"] = value(version);
    manifest["workspace"]["dependencies"]["clawback-core"]["version"] = value(version);
    Ok(manifest.to_string())
}

fn prepare(root: &Path, version: &str) -> Result<()> {
    let manifest_path = root.join("Cargo.toml");
    let lock_path = root.join("Cargo.lock");
    let original = fs::read_to_string(&manifest_path)?;
    let lock = fs::read(&lock_path)?;
    fs::write(&manifest_path, updated_manifest(&original, version)?)?;
    // --workspace retains all third-party locked versions. Offline prevents an
    // accidental dependency refresh and needs no credentials or registry access.
    if let Err(error) =
        command_at(root, "cargo", &["update", "--workspace", "--offline"]).and_then(|_| validate(root, version))
    {
        fs::write(manifest_path, original)?;
        fs::write(lock_path, lock)?;
        return Err(error);
    }
    Ok(())
}

fn validate(root: &Path, version: &str) -> Result<()> {
    let manifest: DocumentMut = fs::read_to_string(root.join("Cargo.toml"))?.parse()?;
    if manifest["workspace"]["package"]["version"].as_str() != Some(version)
        || manifest["workspace"]["dependencies"]["clawback-core"]["version"].as_str() != Some(version)
    {
        return Err(format!("v{version} does not match Cargo.toml; run release prepare first").into());
    }
    let lock: DocumentMut = fs::read_to_string(root.join("Cargo.lock"))?.parse()?;
    let packages = lock["package"].as_array_of_tables().ok_or("Invalid lockfile")?;
    for name in ["clawback", "clawback-core"] {
        if !packages.iter().any(|p| p["name"].as_str() == Some(name) && p["version"].as_str() == Some(version)) {
            return Err(format!("Cargo.lock does not match {name} v{version}").into());
        }
    }
    Ok(())
}

fn publish(root: &Path, version: &str) -> Result<()> {
    validate(root, version)?;
    if !command("git", &["status", "--porcelain"])?.is_empty() {
        return Err("Commit your changes before publishing".into());
    }
    if command("git", &["branch", "--show-current"])? != "main" {
        return Err("Publish from main".into());
    }
    // Nightly tags move; fetching all tags can fail when a local nightly is old.
    command("git", &["fetch", "origin", "main", "--no-tags"])?;
    if command("git", &["rev-parse", "HEAD"])? != command("git", &["rev-parse", "refs/remotes/origin/main"])? {
        return Err("Push main (or pull remote changes) first; HEAD must equal origin/main".into());
    }
    let tag = format!("v{version}");
    if !command("git", &["tag", "--list", &tag])?.is_empty()
        || !command("git", &["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")])?.is_empty()
    {
        return Err(format!("{tag} exists. Use a new version or rerun the failed workflow in Actions").into());
    }
    command("git", &["tag", "-a", &tag, "-m", &format!("Clawback {version}")])?;
    if let Err(error) = command("git", &["push", "origin", &format!("refs/tags/{tag}")]) {
        return Err(format!("{error}\nLocal tag remains. Retry: git push origin refs/tags/{tag}").into());
    }
    println!("Pushed {tag}. Builds: https://github.com/Azazel-Labs/Clawback/actions/workflows/release.yml");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_reject_malformed_tags_and_accept_release_channels() {
        for valid in ["0.1.0", "v1.2.3", "1.0.0-alpha.0", "1.0.0-beta.1", "1.0.0-rc.2"] {
            assert!(parse_version(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "nightly",
            "v1.2",
            "01.2.3",
            "1.0.0-beta.01",
            "1.0.0-preview.1",
            "1.0.0+meta",
            "1.0.0-rc.1\n",
            "1.0.0;echo hi",
        ] {
            assert!(parse_version(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn manifest_updates_workspace_and_prerelease_dependency_together() -> Result<()> {
        let original = include_str!("../../Cargo.toml");
        let updated = updated_manifest(original, "2.0.0-rc.1")?;
        let manifest: DocumentMut = updated.parse()?;
        assert_eq!(manifest["workspace"]["package"]["version"].as_str(), Some("2.0.0-rc.1"));
        assert_eq!(manifest["workspace"]["dependencies"]["clawback-core"]["version"].as_str(), Some("2.0.0-rc.1"));
        assert!(updated.contains("# Lints:"));
        Ok(())
    }

    #[test]
    fn prepare_refreshes_lock_and_validation_catches_mismatched_tags() -> Result<()> {
        let root = env::temp_dir().join(format!("clawback-release-test-{}", std::process::id()));
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("crates/clawback-core/src"))?;
        let result = (|| -> Result<()> {
            fs::write(
                root.join("Cargo.toml"),
                r#"
[workspace]
members = ["crates/clawback-core"]
[workspace.package]
version = "0.1.0"
[workspace.dependencies]
clawback-core = { path = "crates/clawback-core", version = "0.1.0" }
[package]
name = "clawback"
version.workspace = true
[dependencies]
clawback-core.workspace = true
"#,
            )?;
            fs::write(root.join("src/lib.rs"), "")?;
            fs::write(
                root.join("crates/clawback-core/Cargo.toml"),
                "[package]\nname = \"clawback-core\"\nversion.workspace = true\n",
            )?;
            fs::write(root.join("crates/clawback-core/src/lib.rs"), "")?;
            command_at(&root, "cargo", &["generate-lockfile", "--offline"])?;
            for version in ["0.2.0-beta.1", "0.2.0-rc.1", "0.2.0"] {
                prepare(&root, version)?;
                validate(&root, version)?;
                assert!(validate(&root, "9.0.0").is_err());
            }
            let lock = fs::read_to_string(root.join("Cargo.lock"))?;
            fs::write(root.join("Cargo.lock"), lock.replace("0.2.0", "0.1.0"))?;
            assert!(validate(&root, "0.2.0").is_err());
            // Failed resolution restores both files exactly.
            fs::remove_file(root.join("crates/clawback-core/Cargo.toml"))?;
            let before = fs::read(root.join("Cargo.toml"))?;
            let lock_before = fs::read(root.join("Cargo.lock"))?;
            assert!(prepare(&root, "0.3.0").is_err());
            assert_eq!(fs::read(root.join("Cargo.toml"))?, before);
            assert_eq!(fs::read(root.join("Cargo.lock"))?, lock_before);
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }
}
