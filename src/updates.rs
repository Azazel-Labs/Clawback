//! A quiet check for a newer release: one request to the public GitHub API at startup, sending
//! nothing about the user or their files.
//!
//! Only builds made by the Release workflow check, and it names their channel at compile
//! time. Steam updates the app itself, and source and crates.io builds never go online.
//! The request goes through the system's `curl`, so the app carries no TLS stack; without
//! `curl` there is simply no notice.
use crate::background::Job;
use eframe::egui;
use std::process::{Command, Stdio};

/// "release" or "nightly", from the Release workflow.
const CHANNEL: Option<&str> = option_env!("CLAWBACK_UPDATE_CHANNEL");
/// The commit a nightly was built from.
const COMMIT: Option<&str> = option_env!("CLAWBACK_BUILD_COMMIT");
const API: &str = "https://api.github.com/repos/Azazel-Labs/Clawback/releases";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    /// The newer version, as shown to the user.
    pub version: String,
    /// Its release page.
    pub url: String,
}

/// How the Docker image is updated, with the Compose setup the documentation recommends.
pub const PULL_COMMAND: &str = "docker compose pull && docker compose up -d";

/// Whether this build checks for updates at all, so whether to offer the setting.
pub fn supported() -> bool {
    !cfg!(feature = "steam") && matches!(CHANNEL, Some("release" | "nightly"))
}

/// Look for a newer release on a worker; the job yields `None` when there is none or the
/// check failed.
pub fn check(ctx: &egui::Context) -> Option<Job<(), Option<Update>>> {
    supported().then(|| {
        Job::spawn((), ctx, || {
            let _span = crate::perf::span("worker.update_check");
            if CHANNEL == Some("nightly") { newer_nightly() } else { newer_release() }
        })
    })
}

fn newer_release() -> Option<Update> {
    let body = fetch(&format!("{API}/latest"))?;
    let tag = json_string(&body, "tag_name")?;
    if !newer(&tag, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    Some(Update { version: tag.trim_start_matches('v').to_owned(), url: json_string(&body, "html_url")? })
}

/// Nightlies share a version number; the release name records the commit instead.
fn newer_nightly() -> Option<Update> {
    let body = fetch(&format!("{API}/tags/nightly"))?;
    let name = json_string(&body, "name")?;
    let latest = name.rsplit(' ').next()?;
    let commit = COMMIT?;
    if latest.is_empty() || latest == commit {
        return None;
    }
    Some(Update {
        version: format!("nightly {}", latest.get(..7).unwrap_or(latest)),
        url: json_string(&body, "html_url")?,
    })
}

fn fetch(url: &str) -> Option<String> {
    let mut command = Command::new("curl");
    command
        .args(["--fail", "--silent", "--location", "--proto", "=https", "--max-time", "15"])
        .args(["--user-agent", "Clawback", "--header", "Accept: application/vnd.github+json", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The first string value of `key` in a JSON object. Tag names, release names and URLs from
/// GitHub need no unescaping beyond `\/` and `\"`.
fn json_string(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => value.push(chars.next()?),
            c => value.push(c),
        }
    }
    None
}

/// Whether version `latest` (optionally `v`-prefixed) is newer than `current`. A release
/// is newer than its own previews.
fn newer(latest: &str, current: &str) -> bool {
    fn parse(version: &str) -> Option<([u64; 3], bool)> {
        let version = version.trim().trim_start_matches('v');
        let (number, preview) = version.split_once('-').map_or((version, false), |(number, _)| (number, true));
        let mut parts = number.split('.').map(|part| part.parse().ok());
        let number = [parts.next()??, parts.next()??, parts.next()??];
        parts.next().is_none().then_some((number, preview))
    }
    match (parse(latest), parse(current)) {
        (Some((latest, latest_preview)), Some((current, current_preview))) => {
            latest > current || (latest == current && current_preview && !latest_preview)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_releases_follow_previews() {
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("v1.0.0", "0.99.0"));
        assert!(newer("v0.4.0", "0.4.0-rc.1"));
        assert!(!newer("v0.4.0", "0.4.0"));
        assert!(!newer("v0.3.9", "0.4.0"));
        assert!(!newer("v0.4.0-rc.2", "0.4.0"));
        for bad in ["", "nightly", "v1.2", "v1.2.3.4", "v1.x.3"] {
            assert!(!newer(bad, "0.1.0"), "{bad}");
        }
    }

    #[test]
    fn reads_string_fields_from_github_responses() {
        let body = r#"{"url": "https://api.github.com/x", "html_url" : "https:\/\/github.com\/Azazel-Labs\/Clawback\/releases\/tag\/v0.4.0",
            "tag_name":"v0.4.0", "name": "Nightly \"abc\"", "draft": false}"#;
        assert_eq!(json_string(body, "tag_name").as_deref(), Some("v0.4.0"));
        assert_eq!(
            json_string(body, "html_url").as_deref(),
            Some("https://github.com/Azazel-Labs/Clawback/releases/tag/v0.4.0")
        );
        assert_eq!(json_string(body, "name").as_deref(), Some("Nightly \"abc\""));
        assert_eq!(json_string(body, "draft"), None);
        assert_eq!(json_string(body, "missing"), None);
        assert_eq!(json_string(r#"{"tag_name": "unterminated"#, "tag_name"), None);
    }
}
