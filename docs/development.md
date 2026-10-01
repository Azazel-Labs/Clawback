# Building and development

## Windows Turbo helper checks

`cargo xtask test-turbo smoke .` builds the developer probe and launches the real
subprocess without elevation. It verifies the authenticated named-pipe round
trip and the strict MFT rejection of a folder (no silent directory fallback).
`cargo xtask test-turbo smoke C:\` checks the same transport on a volume; a
permission-denied response from an unelevated helper is expected and accepted.
`cargo xtask test-turbo fixture .` verifies successful transfer of a synthetic
10,240-file snapshot through the real helper process without elevation or disk
scanning. The synthetic path is supported only by the developer probe feature.

For interactive validation, use `cargo xtask test-turbo decline C:\` and cancel
the Windows UAC dialog. The check requires `ERROR_CANCELLED`. Use
`cargo xtask test-turbo elevated C:\ --release` and approve the prompt to measure
a real optimized MFT scan plus snapshot transfer. These modes request elevation
only when explicitly selected; they invoke native Win32 APIs, never a shell.
The `turbo-probe` feature is not enabled in normal release packages.

In the GUI, test cancelling UAC while a normal whole-volume scan is progressing,
switching roots while consent is pending, pausing/resuming, and closing the app.
The GUI keeps its ordinary scan and watcher until a complete helper result wins.
The helper runs once per attempt and exits on completion or parent disconnect;
it is not a persistent privileged service. Windows owns the pending UAC dialog,
so it may still need dismissal if the ordinary scan finishes first.

The wire protocol preserves UTF-16 names, file identities, allocation sizes and
hard-link flags, batches data through bounded buffers, rejects malformed records,
and validates peer process IDs on both sides. The helper opens its pipe at
identification security level so the server cannot impersonate its elevated token.

## Startup timings

Set `CLAWBACK_PERF_TRACE` to a writable file path to capture full-session timing
spans. Recycling has separate `delete.recycle` and `delete.reconcile` spans for
the native operation and the subsequent tree/disk update.

Windows initializes DirectX 12 first. If graphics initialization fails before the
app is created, it retries with Vulkan. Only the selected backend is initialized
on each attempt. Other platforms retain their defaults. An explicit
`WGPU_BACKEND` setting overrides this choice and disables the automatic retry.

Pipeline caching is driver-managed. wgpu 30 only exposes application-managed
persistent pipeline caches for Vulkan, and egui-wgpu 0.36 does not expose a cache
parameter for its UI pipeline. Clawback does not maintain a renderer fork for
this feature or write its own pipeline cache files.

Set **CLAWBACK_STARTUP_TRACE** to a writable file path before launching to record
cumulative timings for settings, graphics initialization, translations, theme,
and the first two UI frames. The `graphics_backend_*` entry identifies the
backend actually selected. For example, in PowerShell:

```powershell
$env:CLAWBACK_STARTUP_TRACE = "$PWD\target\startup.tsv"
cargo run --release -- --gui
Remove-Item Env:CLAWBACK_STARTUP_TRACE
```

`second_frame_started` measures when the first frame has returned through the
renderer. Timings begin inside `main`; OS process loading before `main` is not
included. No trace file is created unless this variable is set.

Graphics initialization dominated initial local startup measurements; settings,
translations, and theme setup took only a few milliseconds. Timings varied
between launches and backends. There is no established speedup for the current
DX12-first configuration; measure cold and warm launches separately on the
target machine before attributing a delay to shader compilation.

For a deliberate fallback check, build with **startup-probe**, set
**CLAWBACK_STARTUP_FORCE_FALLBACK=1** and the trace path, and launch without
`WGPU_BACKEND`. The first attempt has no enabled backend; the trace should contain
`graphics_fallback`, then `graphics_ready`, `graphics_backend_Vulkan`, and
`second_frame_started` on a Vulkan-capable Windows machine. The fault
injection is absent from ordinary builds. Clear these environment variables
after testing.

You need Rust 1.98 or newer; `rust-toolchain.toml` pins the exact version, and `rustup` will fetch it automatically.

```
cargo run --release -- --gui
```

No system libraries or `-dev` packages are needed at build time on any platform. On Linux, Clawback loads X11 or Wayland and the GPU driver at runtime like any desktop app.

## Release builds

Every night at 06:17 UTC, if `main` has new commits since the last nightly, GitHub Actions builds Clawback for Linux, macOS and Windows (x86-64 and ARM64) and replaces the **nightly** pre-release. Stable version tags become **Latest**; alpha, beta and RC tags remain prereleases. Use the Rust `cargo xtask` commands or VS Code release tasks described in [Releasing Clawback](releases.md). Builds are unsigned:

- **macOS:** right-click Clawback › Open the first time, or run `xattr -d com.apple.quarantine clawback`.
- **Windows:** SmartScreen may warn; choose More info › Run anyway.

## Project layout

```
crates/clawback-core/     the engine, pure std with no dependencies
  scan.rs             parallel scanner (thread pool over a shared work queue, live progress, cancel)
  ntfs.rs / ntfs/      bounded NTFS parser, bulk MFT reader and synthetic image tests
  macos.rs / macos/    bounded getattrlistbulk parser, macOS reader and fallback tests
  windows.rs          read-only Windows metadata, allocation and volume APIs
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

With a graphical desktop session and working GPU driver, run:

```sh
cargo xtask screenshots
```

The opt-in `screenshots` feature seeds both frontends from `src/demo.rs`: a fictional in-memory drive with fixed names, sizes, and timestamps. It starts no scanner or filesystem watcher and does not persist demo settings. The desktop captures use the actual GPU renderer. The terminal capture exports the actual Ratatui cell buffer, with SVG and PNG versions. Generated source captures stay in ignored `target/`; publishable images go in `docs/images/`.

Capture, PNG encoding and terminal rendering are all Rust. The terminal PNG uses an embedded Hack monospace font, with no system-font dependency. To rerender existing captures without opening the app, use `cargo xtask screenshots --render-only`. VS Code also provides **Clawback: Capture screenshots**.

Normal builds do not include the demo capture code. Screenshots are labeled as fictional data in the app and README.

## Windows storage validation

Use `cargo xtask profile-scans <root>` for optimized backend comparisons and phase timings. Run `cargo xtask profile-scans --help` for single-mode and dedicated probe commands.

`cargo test -p clawback-core` covers listing-only Windows estimates, sparse/compressed allocation helpers, resident NTFS data, hard links and incremental updates. Ordinary Windows directory scans intentionally use cluster-rounded estimates; exact per-file queries are reserved for incremental changes to MFT documents. Synthetic fragmented MFT images exercise ingestion, extension records, integrity checks, cancellation and fallback boundaries without raw-volume privileges. The parser tests also compile on Unix.

For a read-only test of the actual raw-volume path, run from an administrator PowerShell on an NTFS volume:

```powershell
$env:CLAWBACK_MFT_TEST_ROOT = "C:\"
cargo test -p clawback-core elevated_volume_scan -- --ignored --nocapture
```

This test fails on an MFT error instead of silently falling back. Ordinary application scans fall back automatically. `ScanResult.backend` identifies the backend that completed the scan. Bootstrap uses `FSCTL_GET_NTFS_VOLUME_DATA` and `FSCTL_GET_NTFS_FILE_RECORD` for the MFT extent list, followed by sector-aligned bulk reads. It never writes the volume, enables privileges, or installs a driver. Very large or unsupported attribute lists use traversal. No persistent index or USN-journal replay is implemented.

## macOS bulk directory reads

On macOS only, the directory workers use `getattrlistbulk` with 64 KiB buffers. Regular files get allocation, data-fork length, modification time, device/inode identity, and link count from the batch. Directories, symlinks, special files, and incomplete metadata use `symlink_metadata`; resolving directories separately preserves mount boundaries and System/Data firmlinks. Allocated size includes all forks, while apparent size uses the data-fork length, matching the existing stat-based accounting. APFS clones still have distinct identities; these totals are per-file allocation, not a prediction of uniquely reclaimable space.

A failed bulk call or malformed batch switches that directory to a fresh `read_dir`, filtering names already published so a late fallback cannot double-count them. Cancellation is checked between entries, including while skipping previously returned names. Memory is bounded per metadata batch, plus a set of delivered names for the current directory. This remains the `Directory` scan backend: it uses the same adaptive workers, live tree, hard-link deduplication, and progress counters.

`cargo test -p clawback-core` runs portable parser tests on every platform. On macOS it also checks native bulk results against stat (including sparse files, resource forks, hard links, raw-byte names, and symlinks), forces both initial and mid-stream fallback, and tests cancellation. These native tests run in the existing macOS CI job. API definitions follow Apple's [getattrlistbulk documentation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/getattrlistbulk.2) and [attribute header](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/attr.h).
