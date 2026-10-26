#[path = "localization/catalog.rs"]
mod catalog;

fn main() {
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=i18n.toml");
    println!("cargo:rerun-if-changed=localization/catalog.rs");
    let catalogs = catalog::load(std::path::Path::new("locales")).expect("Invalid translation catalogs");
    let generated = format!("static LANGUAGES: &[&str] = &{:?};\n", catalogs.keys().collect::<Vec<_>>());
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(output.join("translations.rs"), generated).expect("Write embedded translations");
    // crates.io limits package size, so its package leaves out the CJK fonts; those
    // builds use the operating system's instead (see src/theme/system_fonts.rs).
    println!("cargo:rerun-if-changed=assets/fonts");
    println!("cargo:rustc-check-cfg=cfg(bundled_cjk_fonts)");
    let cjk = ["SC", "TC", "JP", "KR"].map(|region| format!("assets/fonts/NotoSans{region}-Regular.otf"));
    if cjk.iter().all(|path| std::path::Path::new(path).is_file()) {
        println!("cargo:rustc-cfg=bundled_cjk_fonts");
    }
    // Steam builds load the Steam API library shipped beside the executable. On Windows the
    // Turbo service runs the same executable and must start without it, so it is delay-loaded
    // until achievements connect.
    if std::env::var_os("CARGO_FEATURE_STEAM").is_some() {
        match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
            Ok("windows") if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") => {
                println!("cargo:rustc-link-arg=/DELAYLOAD:steam_api64.dll");
                println!("cargo:rustc-link-arg=delayimp.lib");
            }
            Ok("macos") => println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path"),
            Ok("linux") => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
            _ => {}
        }
    }
    println!("cargo:rerun-if-changed=assets/icons/clawback.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icons/clawback.ico")
            .set("ProductName", "Clawback")
            .set("CompanyName", "Azazel Labs")
            .set("LegalCopyright", "Copyright © 2026 Azazel Labs")
            .set("FileDescription", "Clawback disk space visualizer")
            .compile()
            .expect("Could not embed the Windows application icon");
    }
}
