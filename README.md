# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#install)

<!-- languages:start -->
English · [Afrikaans](docs/i18n/README.af.md) · [Català](docs/i18n/README.ca.md) · [Čeština](docs/i18n/README.cs.md) · [Dansk](docs/i18n/README.da.md) · [Deutsch](docs/i18n/README.de.md) · [Ελληνικά](docs/i18n/README.el.md) · [Español (Latinoamérica)](docs/i18n/README.es-419.md) · [Español (España)](docs/i18n/README.es-ES.md) · [Suomi](docs/i18n/README.fi.md) · [Français](docs/i18n/README.fr.md) · [Magyar](docs/i18n/README.hu.md) · [Bahasa Indonesia](docs/i18n/README.id.md) · [Italiano](docs/i18n/README.it.md) · [日本語](docs/i18n/README.ja.md) · [한국어](docs/i18n/README.ko.md) · [Norsk bokmål](docs/i18n/README.nb.md) · [Nederlands](docs/i18n/README.nl.md) · [Polski](docs/i18n/README.pl.md) · [Português (Brasil)](docs/i18n/README.pt-BR.md) · [Português (Portugal)](docs/i18n/README.pt-PT.md) · [Română](docs/i18n/README.ro.md) · [Русский](docs/i18n/README.ru.md) · [Српски](docs/i18n/README.sr-Cyrl.md) · [Svenska](docs/i18n/README.sv.md) · [ไทย](docs/i18n/README.th.md) · [Türkçe](docs/i18n/README.tr.md) · [Українська](docs/i18n/README.uk.md) · [Tiếng Việt](docs/i18n/README.vi.md) · [简体中文](docs/i18n/README.zh-Hans.md) · [繁體中文](docs/i18n/README.zh-Hant.md)
<!-- languages:end -->

Clawback shows where your disk space went. It scans a drive or folder and draws it as a treemap, where each file is a box sized by how much space it uses, nested inside boxes for its folders. It runs on Windows, macOS, and Linux, with a desktop app and a terminal mode.

[Releases](https://github.com/Azazel-Labs/Clawback/releases) · [User guide](docs/usage.md) · [Building](docs/development.md) · [License (MIT-0)](LICENSE)

**Get it:** download the [latest release](https://github.com/Azazel-Labs/Clawback/releases/latest) for Windows, macOS, or Linux, or install it from [crates.io](https://crates.io/crates/clawback) with Rust 1.95 or newer:

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
