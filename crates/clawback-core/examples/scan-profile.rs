use clawback_core::{ScanOptions, profiling};
use std::path::Path;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(7..=8).contains(&args.len()) {
        return Err(
            "usage: scan-profile ROOT auto|directory|mft THREADS REPEATS apparent|allocated TIMEOUT_SECONDS [unknown|ssd|hdd]".into()
        );
    }
    if cfg!(debug_assertions) {
        eprintln!("WARNING: unoptimized build; use --release for comparisons");
    }
    let mode: profiling::Mode = args[2].parse()?;
    let options = ScanOptions {
        storage: match args.get(7).map_or("unknown", String::as_str) {
            "unknown" => clawback_core::adaptive::StorageKind::Unknown,
            "ssd" => clawback_core::adaptive::StorageKind::SolidState,
            "hdd" => clawback_core::adaptive::StorageKind::Rotational,
            _ => return Err("storage must be unknown, ssd or hdd".into()),
        },
        threads: args[3].parse()?,
        apparent_size: match args[5].as_str() {
            "apparent" => true,
            "allocated" => false,
            _ => return Err("expected apparent or allocated".into()),
        },
        ..ScanOptions::default()
    };
    println!("{}", profiling::header());
    for _ in 0..args[4].parse::<usize>()? {
        println!("{}", profiling::run(Path::new(&args[1]), mode, &options, Duration::from_secs(args[6].parse()?))?);
    }
    Ok(())
}
