# Building and development

You need Rust 1.98 or newer; `rust-toolchain.toml` pins the exact version, and `rustup` will fetch it automatically.

```
cargo run --release -- --gui
```

No system libraries or `-dev` packages are needed at build time on any platform. On Linux, Clawback loads X11 or Wayland and the GPU driver at runtime like any desktop app.

## Release builds

Every night at 06:17 UTC, if `main` has new commits since the last nightly, GitHub Actions builds Clawback for Linux, macOS and Windows (x86-64 and ARM64) and replaces the **nightly** pre-release. Pushing a `v*` tag publishes a regular release the same way. Builds are unsigned:

- **macOS:** right-click Clawback › Open the first time, or run `xattr -d com.apple.quarantine clawback`.
- **Windows:** SmartScreen may warn; choose More info › Run anyway.

## Project layout

```
crates/clawback-core/     the engine, pure std with no dependencies
  scan.rs             parallel scanner (thread pool over a shared work queue, live progress, cancel)
  adaptive.rs         disk hints and measured concurrency trials
  tree.rs             arena-backed size tree (delete, graft, largest-files queries)
  layout.rs           SpaceMonger's box layout and hit-testing, reproduced exactly
  live.rs             incremental metadata refresh and ancestor size updates
  palette.rs          SpaceMonger's colour tables
  format.rs           size, percent and date formatting (local time without a date crate)
  settings.rs         Setup dialog settings, stored as key = value text
  report.rs           the --report output
src/
  main.rs             command line, window setup
  tui.rs              Ratatui terminal map, file list and keyboard navigation
  app.rs              toolbar, commands and dialogs
  directoryview.rs    virtualized directory tree with background row preparation
  mapview.rs          drawing and mouse interaction for the map
  maprender.rs        cached tile geometry and budgeted label preparation
  background.rs       off-thread disposal of large snapshots
  watching.rs         bounded event batches and live snapshot coordination
  watch_windows.rs    asynchronous native subtree notifications and overflow detection
  platform.rs         drives, open / reveal, trash, folder picker, attributes
  icon.rs             the app icon, drawn in code
```

Dependencies include `eframe`/`egui` (GUI), `ratatui`/Crossterm (terminal UI), `rfd` (folder picker), `sysinfo` (drive list and free space) and `trash` (Recycle Bin / Trash).

## Development

CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy -D warnings` with pedantic lints (configured in the workspace `Cargo.toml`), the tests, and a release build on Linux, macOS and Windows. Dependabot keeps crates and actions up to date weekly; the actions are pinned to commit SHAs. It doesn't update `rust-toolchain.toml`, so bump that by hand when a new Rust release lands.

Use `cargo run -- --gui` to launch the desktop interface explicitly. To measure dense-map CPU frame times (including egui tessellation), run `cargo test frame_cost_dense_map -- --ignored --nocapture`. This benchmark covers stationary rendering and moving-pointer highlights; it does not measure GPU or display latency.

Static map geometry and label strings are prepared on the layout worker. Each UI frame prepares at most eight new labels, with a 1 ms preparation budget; subsequent frames reuse the cached geometry and text. At high density, the largest readable labels take priority while every box remains interactive.


## Marketing screenshots

On Windows with Python 3 installed, run:

```powershell
pwsh -File scripts/capture_screenshots.ps1
```

The opt-in `screenshots` feature seeds both frontends from `src/demo.rs`: a fictional in-memory drive with fixed names, sizes, and timestamps. It starts no scanner or filesystem watcher and does not persist demo settings. The desktop captures use the actual GPU renderer. The terminal capture exports the actual Ratatui cell buffer, with SVG and PNG versions. Generated source captures stay in ignored `target/`; publishable images go in `docs/images/`.

Normal builds do not include the demo capture code. Screenshots are labeled as fictional data in the app and README.
