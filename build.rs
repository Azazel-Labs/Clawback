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
