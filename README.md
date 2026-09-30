# Clawback

**Your disk is full. See why. Take it back.**

Clawback turns files and folders into a map you can explore. Big boxes mean big files. Follow the space, find what you no longer need, and watch the map update as you clean up.

A modern, open-source homage to SpaceMonger, with a dark desktop interface and a full terminal mode. Built in Rust for Windows, macOS, and Linux.

[**Downloads**](https://github.com/Azazel-Labs/Clawback/releases) · [Getting started](#get-started) · [User guide](docs/usage.md) · [MIT-0 license](LICENSE)

![Clawback's dark desktop interface with compact stats, a directory tree, and a disk-space map](docs/images/desktop.png)

*Actual app rendering with a fictional demo drive. All screenshot names, sizes, and dates are synthetic.*

## Find the space hogs at a glance

No digging through folder properties one directory at a time. Clawback puts the big picture and the details together: a size-sorted directory tree above a colorful, nested space map.

- **Follow the big boxes.** Double-click a folder bucket to zoom in. File tiles take you into their containing folder.
- **Go back to where you were.** Back, Escape, and your mouse's Back button retrace your visited views.
- **Keep the data front and center.** Compact progress and stats leave the window for your folders and map. Resize the directory pane to suit your workflow.
- **Make it yours.** Adjust map density, colors, tooltips, and size-accounting options.

![Exploring fictional creative projects in Clawback, with nested folders and file sizes](docs/images/explore.png)

*Drill into a folder without losing your way.*

## Quiet chrome. Colorful space.

Choose **Palette** in the toolbar to try **Material**, **Candy**, **Sunset**, or **Lagoon**, with live color swatches. Material is the default, using [Google’s Material Design 400 swatches](https://m1.material.io/style/color.html). The panels stay charcoal; the map gets the color. Prefer subdued tiles? **Muted** is still available. Labels automatically use light or dark text for contrast.

![Four colorful map palette options: Material, Candy, Sunset, and Lagoon](docs/images/palettes.svg)

## Start exploring before the scan finishes

The map fills in as files are discovered. Scanning runs in the background, so you can navigate the preview, resize the window, or stop the scan and explore the partial results.

The scanner starts conservatively, using disk type as a hint. It measures throughput and latency, adds workers when they help, and backs off when they hurt. Small scans can finish before tuning is needed.

## Clean up. Watch it change.

After a successful scan, Clawback listens for filesystem changes and updates the map in the background. Delete or move files elsewhere and see the space change without manually rescanning.

Open files, inspect properties, or move unwanted items to the Recycle Bin / Trash from the desktop context menu. **Trash actions happen immediately**; restore items from your system's trash if needed, or disable deletion in Settings.

## Desktop when you want it. Terminal when you need it.

Prefer a keyboard? The terminal interface pairs a colored space map with a size-sorted file list, folder navigation, scan progress, and live updates. Text reports work in scripts and redirected output too.

```sh
clawback --gui             # Desktop app
clawback --tui .           # Interactive terminal map
clawback --report --top 10 .
```

![Clawback terminal interface showing the same fictional drive, a size-sorted directory list, and a colorful space map](docs/images/terminal.png)

*Captured from the actual terminal renderer using the same fictional dataset as the desktop views.*

The terminal interface is for browsing; file actions are available in the desktop app.

## Get started

1. Choose a build from [Releases](https://github.com/Azazel-Labs/Clawback/releases). Nightly builds are available through the project's release workflow.
2. Launch Clawback, or run `clawback --gui` from a terminal.
3. Click **Open folder**, choose a drive or folder, and follow the biggest boxes.

Clawback measures what your account can read and reports skipped folders. It does not follow symlinks or Windows junctions. Platform permissions and size-accounting details are in the [user guide](docs/usage.md).

Want to build it yourself? See [Building and development](docs/development.md).

## Inspired by a classic. Built for today.

Clawback is a from-scratch Rust homage to [SpaceMonger 1.4](https://github.com/seanofw/spacemonger1), created by Sean Werkema. It reimplements the nested-map experience without including SpaceMonger source code.

**MIT-0 licensed.** Use it, modify it, and make it your own. Copyright © 2026 Azazel Labs. See [LICENSE](LICENSE).
