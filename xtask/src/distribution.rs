//! Generate package-manager metadata from an already-published stable release.
use crate::{Result, parse_version};
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

const REPOSITORY: &str = "https://github.com/Azazel-Labs/Clawback";

pub fn run(args: &[String]) -> Result<()> {
    let [tag, checksums, output] = args else {
        return Err("Usage: cargo xtask distribution <version> <SHA256SUMS.txt> <output-directory>".into());
    };
    let files = generate(tag, &fs::read_to_string(checksums)?)?;
    for (name, content) in files {
        let path = Path::new(output).join(name);
        fs::create_dir_all(path.parent().ok_or("Missing output parent")?)?;
        fs::write(path, content)?;
    }
    println!("Wrote WinGet manifests and Homebrew formula to {output}");
    Ok(())
}

fn generate(tag: &str, checksums: &str) -> Result<BTreeMap<String, String>> {
    let version = parse_version(tag)?;
    if version.contains('-') {
        return Err("Distribution is only enabled for stable versions".into());
    }
    let mut hashes = BTreeMap::new();
    for line in checksums.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2 || fields[0].len() != 64 || !fields[0].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid SHA256SUMS.txt entry".into());
        }
        if hashes.insert(fields[1].trim_start_matches('*'), fields[0].to_ascii_lowercase()).is_some() {
            return Err("Duplicate checksum filename".into());
        }
    }
    let asset = |target: &str, extension: &str| -> Result<(String, String, String)> {
        let stem = format!("clawback-v{version}-{target}");
        let name = format!("{stem}.{extension}");
        let hash = hashes.get(name.as_str()).ok_or_else(|| format!("Missing checksum for {name}"))?;
        Ok((stem, format!("{REPOSITORY}/releases/download/v{version}/{name}"), hash.clone()))
    };
    let header = format!("PackageIdentifier: AzazelLabs.Clawback\nPackageVersion: {version}\n");
    let mut installer =
        format!("{header}InstallerType: zip\nNestedInstallerType: portable\nCommands:\n- clawback\nInstallers:\n");
    for (architecture, target) in [("x64", "x86_64-pc-windows-msvc"), ("arm64", "aarch64-pc-windows-msvc")] {
        let (stem, url, hash) = asset(target, "zip")?;
        writeln!(
            installer,
            "- Architecture: {architecture}\n  InstallerUrl: {url}\n  InstallerSha256: {hash}\n  NestedInstallerFiles:\n  - RelativeFilePath: {stem}/clawback.exe\n    PortableCommandAlias: clawback"
        )?;
    }
    installer.push_str("ManifestType: installer\nManifestVersion: 1.9.0\n");
    let locale = format!(
        "{header}PackageLocale: en-US\nPublisher: Azazel Labs\nPackageName: Clawback\nLicense: MIT-0\nLicenseUrl: {REPOSITORY}/blob/v{version}/LICENSE\nShortDescription: Disk space visualizer with desktop and terminal interfaces\nPackageUrl: {REPOSITORY}\nManifestType: defaultLocale\nManifestVersion: 1.9.0\n"
    );
    let manifest = format!("{header}DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.9.0\n");
    let mut formula = format!(
        "class Clawback < Formula\n  desc \"Disk space visualizer with desktop and terminal interfaces\"\n  homepage \"{REPOSITORY}\"\n  version \"{version}\"\n  license \"MIT-0\"\n  depends_on :macos\n\n  on_macos do\n"
    );
    for (architecture, target) in [("arm", "aarch64-apple-darwin"), ("intel", "x86_64-apple-darwin")] {
        let (_, url, hash) = asset(target, "tar.gz")?;
        writeln!(formula, "    on_{architecture} do\n      url \"{url}\"\n      sha256 \"{hash}\"\n    end")?;
    }
    formula.push_str("  end\n\n  def install\n    bin.install \"clawback\"\n  end\n\n  test do\n    assert_match version.to_s, shell_output(\"#{bin}/clawback --version\")\n  end\nend\n");
    let schema = |kind: &str, content: String| {
        format!("# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.1.9.0.schema.json\n\n{content}")
    };
    Ok(BTreeMap::from([
        ("winget/AzazelLabs.Clawback.yaml".into(), schema("version", manifest)),
        ("winget/AzazelLabs.Clawback.installer.yaml".into(), schema("installer", installer)),
        ("winget/AzazelLabs.Clawback.locale.en-US.yaml".into(), schema("defaultLocale", locale)),
        ("homebrew/Formula/clawback.rb".into(), formula),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checksums() -> String {
        [
            "x86_64-pc-windows-msvc.zip",
            "aarch64-pc-windows-msvc.zip",
            "x86_64-apple-darwin.tar.gz",
            "aarch64-apple-darwin.tar.gz",
        ]
        .map(|suffix| format!("{}  clawback-v1.2.3-{suffix}\n", "a".repeat(64)))
        .concat()
    }

    #[test]
    fn metadata_uses_fixed_release_urls_and_correct_archive_paths() -> Result<()> {
        let files = generate("v1.2.3", &checksums())?;
        assert_eq!(files.len(), 4);
        let installer = &files["winget/AzazelLabs.Clawback.installer.yaml"];
        assert!(installer.contains("RelativeFilePath: clawback-v1.2.3-aarch64-pc-windows-msvc/clawback.exe"));
        assert!(installer.contains("/releases/download/v1.2.3/"));
        let formula = &files["homebrew/Formula/clawback.rb"];
        assert!(formula.contains("on_arm do"));
        assert!(formula.contains("on_intel do"));
        assert!(!formula.contains("latest"));
        Ok(())
    }

    #[test]
    fn rejects_previews_missing_assets_and_corrupt_checksums() {
        assert!(generate("1.2.3-rc.1", &checksums()).is_err());
        assert!(generate("1.2.3", "").is_err());
        assert!(generate("1.2.3", &checksums().replace(&"a".repeat(64), "nope")).is_err());
        assert!(generate("1.2.3", &(checksums() + &checksums())).is_err());
    }
}
