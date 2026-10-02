# Clawback

Clawback shows where your disk space went. It scans a drive or folder and draws it as a treemap, where each file is a box sized by how much space it uses, nested inside boxes for its folders. It runs on Windows, macOS, and Linux, with a desktop app and a terminal mode.

[Releases](https://github.com/Azazel-Labs/Clawback/releases) · [User guide](docs/usage.md) · [Building](docs/development.md) · [License (MIT-0)](LICENSE)

**Get it:** download the [latest release](https://github.com/Azazel-Labs/Clawback/releases/latest) for Windows, macOS, or Linux, or install it from [crates.io](https://crates.io/crates/clawback) with Rust 1.98 or newer:

```sh
cargo install clawback
```

The crates.io build uses your system's Chinese, Japanese, and Korean fonts, while the release downloads include their own.

![Clawback desktop window showing a directory tree, a file-type breakdown, and a treemap of a demo drive](docs/images/desktop.png)

*Screenshots use a built-in demo drive; the file names and sizes are made up.*

## Features

- Treemap of a drive or folder, with a size-sorted directory tree and a breakdown by file extension for the selected folder.
- Double-click a folder to zoom into it. Back, Escape, and the mouse Back button return to previous views.
- The map is usable while the scan is running, and scans can be paused and resumed.
- After a scan finishes, Clawback watches for filesystem changes and updates the map, so deleted or moved files disappear without a rescan.
- Open files, view properties, or move items to the Recycle Bin / Trash from the right-click menu. Deletion can be turned off in Settings.
- Ten color palettes, an option to mute them, and settings for map density, tooltips, and how sizes are counted.
- Interface translated into 31 languages.

![Zoomed into the Projects folder of the demo drive](docs/images/explore.png)

## Install

Download a build from [Releases](https://github.com/Azazel-Labs/Clawback/releases).

- **Windows:** run `clawback.exe`.
- **macOS:** drag `Clawback.app` into Applications.
- **Linux:** run `clawback`. See the user guide for [adding a desktop launcher](docs/usage.md#native-application-icons).

Then choose **File → Open folder** and pick a drive or folder.

To build from source, see [docs/development.md](docs/development.md).

## Terminal mode

```sh
clawback                       # desktop app
clawback --tui .               # interactive treemap in the terminal
clawback --report --top 10 .   # print a text summary, 10 entries per section
```

The terminal interface supports browsing, zooming, and live updates, but not file actions. `--report` works in scripts and with redirected output. See the [command-line reference](docs/usage.md#command-line) for all options.

![Terminal mode showing the demo drive](docs/images/terminal.png)

## How scanning works

- **Windows:** scanning a whole NTFS volume as administrator reads the Master File Table directly, which is much faster than walking directories. Other scans walk the directory tree and estimate disk usage from file sizes rounded up to the cluster size.
- **macOS:** directory metadata is read in batches with `getattrlistbulk`, falling back to ordinary directory reads on filesystems that don't support it. No administrator access is needed.
- **All platforms:** the directory scanner adjusts how many threads it uses based on measured throughput and latency.

Clawback only counts what your account can read and lists the folders it couldn't. It doesn't follow symlinks or Windows junctions. Permissions and size accounting are covered in the [user guide](docs/usage.md).

## Contributing

Build instructions and project layout are in [docs/development.md](docs/development.md). Translations are managed through Crowdin. Maintainers can find the release process in [docs/releases.md](docs/releases.md).

## License

Copyright © 2026 Azazel Labs. Released under [MIT-0](LICENSE).
