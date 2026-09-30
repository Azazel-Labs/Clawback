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
}
