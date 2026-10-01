//! Prepare native release layouts; archive compression stays in the workflow.
use crate::Result;
use std::{fs, path::Path};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Rust target triple of the release build
    #[arg(value_parser = clap::builder::PossibleValuesParser::new(TARGETS))]
    pub target: String,
    /// Release tag (vMAJOR.MINOR.PATCH[-channel.N]) or nightly
    pub tag: String,
}

pub const TARGETS: [&str; 6] = [
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

/// Font and icon licenses, from the repository to their packaged names.
const NOTICES: [(&str, &str); 3] = [
    ("assets/fonts/OFL.txt", "NotoSans-OFL.txt"),
    ("assets/fonts/NotoSansThai-OFL.txt", "NotoSansThai-OFL.txt"),
    ("assets/fonts/Phosphor-LICENSE.txt", "Phosphor-LICENSE.txt"),
];

pub fn exe_name(target: &str) -> &'static str {
    if target.contains("windows") { "clawback.exe" } else { "clawback" }
}

pub fn run(root: &Path, target: &str, tag: &str) -> Result<()> {
    if !TARGETS.contains(&target) {
        return Err("Unsupported release target".into());
    }
    if tag != "nightly" {
        crate::release::parse_version(tag)?;
    }
    let out = root.join("dist").join(format!("clawback-{tag}-{target}"));
    if out.exists() {
        return Err(format!("Package directory already exists: {}", out.display()).into());
    }
    fs::create_dir_all(&out)?;
    for name in ["README.md", "LICENSE"] {
        fs::copy(root.join(name), out.join(name))?;
    }
    copy_dir(&root.join("docs"), &out.join("docs"))?;
    copy_notices(root, &out)?;
    let exe = exe_name(target);
    let binary = root.join("target").join(target).join("release").join(exe);
    fs::copy(&binary, out.join(exe))?;
    if target.contains("apple") {
        let contents = out.join("Clawback.app/Contents");
        fs::create_dir_all(contents.join("MacOS"))?;
        fs::create_dir_all(contents.join("Resources"))?;
        fs::copy(&binary, contents.join("MacOS/clawback"))?;
        fs::copy(root.join("assets/icons/clawback.icns"), contents.join("Resources/clawback.icns"))?;
        copy_notices(root, &contents.join("Resources"))?;
        fs::write(contents.join("Info.plist"), plist(&crate::release::workspace_version(root)?))?;
    } else if target.contains("linux") {
        let share = out.join("share");
        fs::create_dir_all(share.join("applications"))?;
        fs::copy(root.join("assets/clawback.desktop"), share.join("applications/clawback.desktop"))?;
        for size in [16, 24, 32, 48, 64, 96, 128, 256, 512, 1024] {
            let dir = share.join(format!("icons/hicolor/{size}x{size}/apps"));
            fs::create_dir_all(&dir)?;
            fs::copy(root.join(format!("assets/icons/clawback-{size}.png")), dir.join("clawback.png"))?;
        }
        let scalable = share.join("icons/hicolor/scalable/apps");
        fs::create_dir_all(&scalable)?;
        fs::copy(root.join("assets/icons/clawback.svg"), scalable.join("clawback.svg"))?;
    }
    Ok(())
}

fn copy_notices(root: &Path, to: &Path) -> Result<()> {
    for (from, name) in NOTICES {
        fs::copy(root.join(from), to.join(name))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn plist(version: &str) -> String {
    let version = version.split('-').next().unwrap_or(version);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Clawback</string>
<key>CFBundleDisplayName</key><string>Clawback</string>
<key>CFBundleIdentifier</key><string>com.azazellabs.clawback</string>
<key>CFBundleExecutable</key><string>clawback</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>clawback.icns</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>NSHumanReadableCopyright</key><string>Azazel Labs</string>
</dict></plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packages_native_metadata_for_every_release_target() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace");
        let temp = workspace.join("target").join(format!("package-test-{}", std::process::id()));
        fs::create_dir_all(temp.join("docs")).expect("fixture");
        for file in ["README.md", "LICENSE", "Cargo.toml"] {
            fs::copy(workspace.join(file), temp.join(file)).expect("fixture file");
        }
        copy_dir(&workspace.join("assets"), &temp.join("assets")).expect("assets");
        for target in TARGETS {
            let exe = exe_name(target);
            let build = temp.join("target").join(target).join("release");
            fs::create_dir_all(&build).expect("build");
            fs::write(build.join(exe), b"fixture executable").expect("exe");
            run(&temp, target, "v1.2.3").expect("package");
            let out = temp.join("dist").join(format!("clawback-v1.2.3-{target}"));
            assert_eq!(fs::read(out.join(exe)).expect("packaged exe"), b"fixture executable");
            assert_eq!(
                fs::read(out.join("NotoSans-OFL.txt")).expect("font license"),
                fs::read(workspace.join("assets/fonts/OFL.txt")).expect("source font license")
            );
            assert_eq!(
                fs::read(out.join("NotoSansThai-OFL.txt")).expect("Thai font license"),
                fs::read(workspace.join("assets/fonts/NotoSansThai-OFL.txt")).expect("source Thai font license")
            );
            if target.contains("apple") {
                assert!(out.join("Clawback.app/Contents/MacOS/clawback").is_file());
                assert!(out.join("Clawback.app/Contents/Resources/clawback.icns").is_file());
                assert!(out.join("Clawback.app/Contents/Resources/NotoSans-OFL.txt").is_file());
                assert!(out.join("Clawback.app/Contents/Resources/NotoSansThai-OFL.txt").is_file());
                assert!(
                    fs::read_to_string(out.join("Clawback.app/Contents/Info.plist"))
                        .expect("plist")
                        .contains("CFBundleIconFile")
                );
            } else if target.contains("linux") {
                assert!(out.join("share/applications/clawback.desktop").is_file());
                assert!(out.join("share/icons/hicolor/scalable/apps/clawback.svg").is_file());
                assert!(out.join("share/icons/hicolor/256x256/apps/clawback.png").is_file());
            }
        }
        fs::remove_dir_all(temp).expect("remove fixture");
    }
}
