# Clawback

**See where your disk space went.** Clawback draws your drive as nested boxes: the bigger the box, the more space it takes. It's a from-scratch Rust homage to [SpaceMonger 1.4](https://github.com/seanofw/spacemonger1), and it runs on Windows, macOS and Linux.

> Claw back your disk space.

Clawback pairs SpaceMonger's nested layout and familiar mouse behavior with a modern dark interface: subtle map gradients, crisp borders, mint selection highlights, and live scan progress below the map.

## Using it

Click **Open** and pick a drive (or **Other Folder...**). The map fills in as the background scan discovers files. Large live previews group smaller entries together; the full tree appears when scanning finishes. You can move or resize the window and cancel while it scans. File actions become available when the scan stops.

Initial scans adapt their concurrency. HDDs and unknown storage start with one worker; SSDs start with two. Once per second, the scanner samples entry throughput and average processing latency using batch counters. It tests a small increase or one fewer worker, allows a settling window, and compares two measurement windows. Increases need at least a 10% throughput gain without a large latency increase; decreases can be kept when they deliver nearly the same throughput with fewer workers, or materially reduce latency with little throughput loss. Failed trials roll back and retries are spaced out. Empty queues and tiny samples never justify an increase. CPU availability sets a safety ceiling of at most 32 workers; additional threads are created only when needed and park when concurrency drops. The GUI and terminal show the current worker limit. Live-update and recovery scans retain their fixed one-worker limit.

Rectangles containing other rectangles are folders; colours show how deeply things are nested; the gray block is free space.

| Mouse | What it does |
|---|---|
| Click | Select the item under the cursor. Click a folder's **frame or title bar** to select the folder itself; clicking inside it selects what's inside. Clicking free space clears the selection. |
| Double-click | Zoom into a folder, or run/open a file with its default app. |
| Right-click | Select the item and open the menu: Zoom In / Zoom Out / Zoom Full, Run / Open, Delete, Open Drive, Rescan Drive, Show Free Space, Properties. The default action is in bold. |
| Rest the mouse | After a moment a **name tip** shows names that don't fit in their box, and an **info tip** near the cursor shows size, date and more (configurable in Setup). Any mouse or key input hides them. |

The header shows the current location and totals, with Open folder, Rescan, and map navigation. **Show free space** includes unused capacity when viewing a whole drive. **More** contains Settings, About, Open selected item, and Move selected item to trash. Buttons grey out when they don't apply.

The resizable **Directories** pane above the map shows an expandable folder tree with sizes and percentages. Click an arrow to expand a folder, or its name to navigate the map. Folder navigation has no zoom animation. Redraws keep the previous map and its labels visible until the complete replacement is ready.

Both desktop and terminal views stay **live** after a successful scan. Watching begins before scanning, so changes during the initial scan are reconciled too. On Windows, one asynchronous subtree watch waits for filesystem notifications; there is no periodic drive scan. Changes are coalesced into one-second batches, metadata and directory updates run in the background, and unchanged tree storage is shared between snapshots. The desktop status shows **Live**, **Catching up**, or any watcher failure.

Event queues and pending paths are capped at 4,096 each. Overflow triggers a background reconciliation with one scan worker and at least 30 seconds between recovery scans. Recovery replaces the complete snapshot and returns navigation to the scan root. Failed watches are reported without falling back to polling; use **Rescan** to reconnect. Cancelled scans stay as static partial results.

macOS and Linux use native notifications too, but whole-drive watch limits depend on the OS. Unix hard-link deduplication stays incremental: a device/inode index updates each affected group, counts its bytes once, and transfers those bytes to a surviving link when the counted name is removed. The index uses identities captured during scanning, with no additional filesystem walk. Windows uses incremental updates with its existing size-accounting rules.

**Delete** moves the item to the Recycle Bin / Trash immediately, with no confirmation, just like SpaceMonger (you can restore it from the trash). If that makes you nervous, turn on **Disable "Delete" Command** in Setup. And the original's advice still stands: if you don't know what something is, don't delete it.

Selection clears when you zoom, resize the window, change settings or switch to another app, as it did in SpaceMonger.

**Keyboard shortcuts** (additions, since SpaceMonger had none): Ctrl/⌘+O Open, F5 Rescan, Home Zoom Full, Backspace Zoom Out, Enter Zoom In (folder) or Run / Open (file), Delete Delete.

### Setup

Settings adapted from SpaceMonger's Setup dialog:

- **File Layout:** Density (from "Too Few Files" to "Too Many Files") and a Horizontal ↔ Vertical Bias slider.
- **Display Colors:** Rainbow, Windows Colors, White, Gray shades, Red … Violet, separately for files and folders.
- **ToolTips:** name tips and info tips, their delays, and which details the info tip shows (path, name, icon, date, size, attributes).
- **Miscellaneous:** Auto Rescan on Delete, Disable Delete, Remember Window Position, Show Rollover Boxes.

Plus a **Scanning** group that SpaceMonger didn't need: stay on one filesystem, use file lengths instead of size on disk, and count hard links once.

Settings are saved to `%APPDATA%\Clawback\settings.ini` on Windows, `~/Library/Application Support/Clawback/settings.ini` on macOS, and `~/.config/clawback/settings.ini` (or `$XDG_CONFIG_HOME/clawback/`) elsewhere.

## Permissions on each platform

Clawback never needs special rights, but it can only measure what your account can read. Folders it couldn't open are counted, and a **⚠ N not readable** button on the toolbar lists them with the reason.

- **macOS:** privacy-protected folders (Mail, Messages, Safari data, other users' homes) are skipped unless you grant Clawback **Full Disk Access** in System Settings › Privacy & Security. Scanning `/` includes the Data volume (where `/Users` and `/Applications` actually live) without counting it twice.
- **Linux / BSD:** other users' files and root-only system folders are skipped unless you run with elevated privileges (e.g. `sudo -E clawback /`). Pseudo filesystems such as `/proc`, `/sys` and `/dev` are never scanned.
- **Windows:** a few system folders are administrator-only; run Clawback as administrator to include them.

By default Clawback stays on one filesystem, so scanning `/` won't wander into network mounts, USB drives or container overlays. Symlinks and Windows junctions are never followed. On Unix, sizes are space actually allocated on disk and hard links are counted once; on Windows, sizes are rounded up to whole clusters, as SpaceMonger did.

## Command line

```
clawback [PATH]                  terminal map in a terminal; desktop window otherwise
clawback --tui [PATH]            force the interactive terminal map
clawback --gui [PATH]            force the desktop window
clawback --report [PATH]         print a text summary instead of opening a window
     --top N                 entries per report section (default 20)
     --apparent-size         file lengths instead of size on disk
     --cross-filesystems     descend into other mounted filesystems
     --count-hardlinks       count every hard link
```

### Terminal interface

The Ratatui terminal interface draws the same nested, colorful disk map alongside a size-sorted file list. It starts automatically when both input and output are interactive terminals (`TERM=dumb` disables automatic selection). Without a path, it scans the current directory. Desktop launches still open the window; `--gui` also opens it from a terminal.

Use **↑/↓** or **j/k** to select, **Enter/→** to explore a folder, **Backspace/←** to go up, **Home** to return to the scan root, **r/F5** to rescan, **Tab** to expand the map, and **q/Ctrl+C** to quit. **Esc** cancels a running scan; its partial results remain browsable. Navigation becomes available after scanning stops. Narrow terminals show the map with the selected item's details underneath.

On Windows, both debug and release GUI builds launch without a console window. Terminal, report, and help modes attach to the existing launching console when available.

Live previews, scan counts, unreadable-entry counts, and a progress spinner update while scanning. This initial terminal interface supports browsing; file operations and Setup remain in the desktop interface. Use `--report` for scripts and redirected output. `--tui` requires an interactive terminal and cannot be combined with `--gui` or `--report`.

## Building

You need Rust 1.98 or newer; `rust-toolchain.toml` pins the exact version, and `rustup` will fetch it automatically.

```
cargo run --release
```

No system libraries or `-dev` packages are needed at build time on any platform. On Linux, Clawback loads X11 or Wayland and the GPU driver at runtime like any desktop app.

### Downloads

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

## Credits

SpaceMonger 1.4 is © 1997–2000 Sean Werkema, who released its source code under the MIT license. Clawback contains no SpaceMonger code; it re-implements the behaviour in Rust.

## License

MIT-0 (MIT No Attribution), see [LICENSE](LICENSE). Copyright © 2026 Azazel Labs.
