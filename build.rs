#[path = "localization/catalog.rs"]
mod catalog;

fn main() {
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=i18n.toml");
    println!("cargo:rerun-if-changed=localization/catalog.rs");
    let catalogs = catalog::load(std::path::Path::new("locales")).expect("Invalid translation catalogs");
    let mut generated = String::from("static LANGUAGES: &[&str] = &[\n");
    for locale in catalogs.keys() {
        use std::fmt::Write as _;
        writeln!(generated, "{locale:?},").expect("Write to string");
    }
    generated.push_str("];\n");
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
