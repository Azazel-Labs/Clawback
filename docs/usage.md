# Using Clawback

Choose **File → Open folder** and pick a drive (or **Other Folder...**). The map fills in as the background scan discovers files. Large live previews group smaller entries together; the full tree appears when scanning finishes. You can move or resize the window and use **Pause scan / Resume scan** while it scans. Pausing preserves the current map and queued work; resuming continues the same scan. File actions become available when the scan finishes.

Initial scans adapt their concurrency. HDDs and unknown storage start with one worker; SSDs start with two. Once per second, the scanner samples entry throughput and average processing latency using batch counters. It tests a small increase or one fewer worker, allows a settling window, and compares two measurement windows. Increases need at least a 10% throughput gain without a large latency increase; decreases can be kept when they deliver nearly the same throughput with fewer workers, or materially reduce latency with little throughput loss. Failed trials roll back and retries are spaced out. Empty queues and tiny samples never justify an increase. CPU availability sets a safety ceiling of at most 32 workers; additional threads are created only when needed and park when concurrency drops. The GUI and terminal show the current worker limit. Live-update and recovery scans retain their fixed one-worker limit.

Rectangles containing other rectangles are folders; colours show how deeply things are nested; the gray block is free space.

| Mouse | What it does |
|---|---|
| Click | Select the item under the cursor. Click a folder's **frame or title bar** to select the folder itself; clicking inside it selects what's inside. Clicking free space clears the selection. |
| Double-click | Zoom into a folder bucket; file tiles zoom into their containing folder. Available during scanning too. |
| Right-click | Select the item and open the menu: Zoom In / Zoom Out / Zoom Full, Run / Open, Delete, Open Drive, Rescan Drive, Show Free Space, Properties. The default action is in bold. |
| Rest the mouse | After a moment a **name tip** shows names that don't fit in their box, and an **info tip** near the cursor shows size, date and more (configurable in Setup). Any mouse or key input hides them. |

The menu bar and status row show the current location, totals, and scan progress. **File** contains Open folder, Rescan, file actions, Settings, and Exit. **View** contains navigation and **Show free space**, which includes unused capacity when viewing a whole drive. **Help** contains About Clawback. Commands grey out when they don't apply.

Desktop file and folder counts use the selected application language's number
grouping: for example, English `2,225,814`, German `2.225.814`, and French
`2 225 814`. This also applies when an untranslated message falls back to English.

The resizable **Directories** pane above the map shows an expandable folder tree with sizes and percentages. Click an arrow to expand a folder, or its name to navigate the map. Folder navigation has no zoom animation. Redraws keep the previous map and its labels visible until the complete replacement is ready.

**File types**, beside the tree, summarizes extensions within the selected map folder, or the current folder when nothing is selected. Selecting a file shows its containing folder's breakdown. Rows are sorted by space used and show the type description, percentage, size, and file count; hover for the full description in a narrow pane. Percentages are relative to that folder's files, using the scan's size-accounting settings. Totals refresh in the background during scans and live updates. Drag the divider to resize; small windows use **Directories / File types** tabs.

Map tiles use a soft directional gradient with restrained shading on tiny tiles and free space. Geometry and colors are cached by the layout worker, keeping redraw work bounded.

Both desktop and terminal views stay **live** after a successful scan. Watching begins before scanning, so changes during the initial scan are reconciled too. On Windows, one asynchronous subtree watch waits for filesystem notifications; there is no periodic drive scan. Changes are coalesced into one-second batches, metadata and directory updates run in the background, and unchanged tree storage is shared between snapshots. The desktop status shows **Live**, **Catching up**, or any watcher failure.

Event queues and pending paths are capped at 4,096 each. Overflow triggers a background reconciliation with one scan worker and at least 30 seconds between recovery scans. Recovery replaces the complete snapshot and returns navigation to the scan root. Failed watches are reported without falling back to polling; use **Rescan** to reconnect. Paused scans retain their work and start live updates after resuming and completing.

macOS and Linux use native notifications too, but whole-drive watch limits depend on the OS. Hard-link deduplication on Unix and Windows stays incremental: a filesystem identity index updates each affected group, counts its bytes once, and transfers those bytes to a surviving link when the counted name is removed. The index uses identities captured during scanning, with no additional filesystem walk.

**Delete** moves the item to the Recycle Bin / Trash immediately, with no confirmation (you can restore it from there). Deletes queue up and run in the background, with progress in the lower-right corner, so you can keep working and delete more. If something is too large for the Recycle Bin, or the drive has none, Clawback asks before deleting it permanently, then deletes it quickly with its own progress and a Cancel button. If that makes you nervous, turn on **Disable "Delete" Command** in Setup. If you don't know what something is, don't delete it.

On Windows, the Recycle Bin appears on a drive's map as a single cell. Click it to empty your Recycle Bin on that drive, after a confirmation.

On Windows, **Uninstall…** appears only after a background lookup confirms an
owning application or Steam game with an uninstall action. Program Files roots
come from Windows' configured folders, including 32-bit and 64-bit locations.
Registered install paths and Steam app manifests identify owners, including when
you select a nested folder or file. Local manifests also support Steam libraries
that are not in the current Steam configuration.

The chooser shows the application name and one uninstall action; installation
paths, versions and Steam IDs are under **Installation details**. Uninstalling
removes the whole application, not just the selected subfolder. Unverified
managed folders do not offer Uninstall or raw deletion. Live changes update the
map; use **Rescan** after uninstalling if live updates are unavailable.

Your `%LOCALAPPDATA%\Temp` folder also appears as a single cell with a broom icon.
Click it or choose **Clean Temp folder** from its right-click menu to request a
cleanup. After confirmation, Clawback permanently removes its contents while
keeping the Temp folder. Items Windows refuses to delete stay in place and are
silently skipped. Cleanup explicitly refreshes the directory. **Disable "Delete" Command** also disables Temp cleanup.

Selecting a folder in the treemap highlights it in the directory navigator,
expands its ancestors, and scrolls it into view. Selecting a file highlights its
containing folder. Selection leaves the treemap's zoom unchanged.

On Windows, clicking a recognized WSL disk opens an action chooser. It identifies
Docker's data and system disks, or the Linux distribution using the current user's
WSL registration. It shows the disk path, Windows file size, space used on disk,
last-modified time, and Store package when available. Standard installation paths
are recognized even when the owner cannot be confirmed; registered distributions
in custom locations are also recognized. Registrations used for recognition are
cached for the session; restart Clawback after installing or moving a distribution.

Choose **Open Docker Desktop**, **Browse Linux files**, **Show cleanup guidance**,
**Open in Explorer**, or **Compact disk**. Browsing Linux files may start that
distribution. Compacting opens a separate preparation screen: clean unwanted data
inside Docker or Linux first, save Linux work, and quit Docker Desktop. Confirm
**Stop WSL and compact** to have Clawback stop all WSL sessions automatically.
Compaction requests administrator permission and runs in the background; Clawback
updates the disk file and totals without replacing the current scan or view. It preserves contents and only reclaims zero-filled blocks, so
it may reclaim little or no space. Recognized disks cannot be deleted directly
with Clawback's Delete command.

Selection clears when you zoom, resize the window, change settings or switch to another app.

**View → Back** and the mouse Back button also return through visited views. **View → Up a level** navigates to the parent directory.

**Keyboard shortcuts:** Ctrl/⌘+O Open, F5 Rescan, Home Zoom Full, Escape / Backspace / Alt+Left Back to the previous view, Enter Zoom In (folder) or Run / Open (file), Delete Delete.

Use the **Palette** menu for preset previews and one-click switching. Click anywhere in a preset row, including its color swatches, to select it. It applies the chosen palette to files and folders together; Settings also lets you choose them independently. **Mute colors** softens the selected palette without replacing it, and stays enabled when you switch palettes. The setting is saved and also applies to terminal maps. Older saved Muted presets migrate to Electric with muting enabled.

### Setup

Settings:

- **File Layout:** Density (from "Too Few Files" to "Too Many Files") and a Horizontal ↔ Vertical Bias slider.
- **Display Colors:** Arcade, Aurora, Blackbody Radiation, Candy, Citrus, Electric (default), Gemstone, Lagoon, Material, Orchid and Sunset, separately for files and folders.
- **ToolTips:** name tips and info tips, their delays, and which details the info tip adds to the name (full path, icon, date, size, attributes).
- **Miscellaneous:** Auto Rescan on Delete, Disable Delete, Remember Window Position, Show Rollover Boxes.

- **Scanning:** stay on one filesystem, use file lengths instead of size on disk, and count hard links once.

Settings are saved to `%APPDATA%\Clawback\settings.ini` on Windows, `~/Library/Application Support/Clawback/settings.ini` on macOS, and `~/.config/clawback/settings.ini` (or `$XDG_CONFIG_HOME/clawback/`) elsewhere.

## Permissions on each platform

Clawback never needs special rights, but it can only measure what your account can read. Folders it couldn't open are counted, and **View → Unreadable folders** lists them with the reason.

- **macOS:** privacy-protected folders (Mail, Messages, Safari data, other users' homes) are skipped unless you grant Clawback **Full Disk Access** in System Settings › Privacy & Security. Scanning `/` includes the Data volume (where `/Users` and `/Applications` actually live) without counting it twice.
- **Linux / BSD:** other users' files and root-only system folders are skipped unless you run with elevated privileges (e.g. `sudo -E clawback /`). Pseudo filesystems such as `/proc`, `/sys` and `/dev` are never scanned.
- **Windows:** whole local NTFS volumes automatically use a read-only Master File Table (MFT) scan when volume access is available, normally when running as administrator. Folder scans, other filesystems, mapped network drives, UNC shares, and scans without volume access use directory traversal. Drive type and filesystem are checked before opening a raw volume; a remote server reporting NTFS does not enable the MFT backend. Unsupported or inconsistent NTFS metadata also falls back to traversal. Elevation is opt-in: select an eligible drive and click the highlighted **Turbo** button in the drive picker, or use the dedicated **Turbo** row during a whole-volume scan, to request administrator permission for a separate read-only MFT helper. The GUI stays unelevated and the normal scan keeps running. Cancelling UAC, helper errors, or unsupported metadata leave the normal scan in place; the first completed result wins. Pause/Resume controls both scans. Closing the app or opening another root cancels the helper. If the normal scan finishes while UAC is still open, dismiss the prompt; approving it late cannot replace the finished scan. The MFT reader is compiled only on Windows; macOS and Linux use the directory scanner.

By default Clawback stays on one filesystem, so scanning `/` won't wander into network mounts, USB drives or container overlays. Symlinks and Windows junctions are never followed. Windows directory scans use fast size estimates: file lengths rounded to whole clusters, without opening every file. These estimates can overcount resident, sparse, or compressed files, and count each hard-link name separately. Windows MFT scans and Unix scans still use allocated storage and count hard links once by default. Live updates preserve the accounting mode of the original scan. Use **file lengths** or `--apparent-size` to see logical lengths instead.

The MFT scanner reads metadata in bulk and publishes its map after validation, so it does not provide a partial map during ingestion. Pause/Resume also works during MFT ingestion; the map appears once validation finishes. Directory traversal still provides progressive previews. MFT scans can include protected files that directory traversal cannot list; reserved NTFS metadata files and their internal subtrees are excluded.

Windows file sizes cover the unnamed data stream; alternate data streams and directory index allocation are not included. These totals are file accounting, not a complete reconciliation of the volume's used-space counter. Live scans are not atomic filesystem snapshots.

## Command line

```
clawback [PATH]                  desktop window; terminal map when no graphical session exists
clawback --tui [PATH]            force the interactive terminal map
clawback --gui [PATH]            force the desktop window
clawback --report [PATH]         print a text summary instead of opening a window
     --top N                 entries per report section (default 20)
     --apparent-size         file lengths instead of size on disk
     --cross-filesystems     descend into other mounted filesystems
     --count-hardlinks       count every hard link
```

### Terminal interface

The Ratatui terminal interface draws the same nested, colorful disk map alongside a size-sorted file list. The desktop window is the default, including when launched from a terminal. Use `--tui` to choose the terminal interface. On systems without a graphical session, Clawback falls back to the terminal interface when both input and output are interactive and `TERM` is not `dumb`. Without either interface, it exits with guidance to use `--report`; it does not start an unattended scan. Without a path, terminal mode scans the current directory. `--gui` forces a desktop launch even if session detection says none is available.

Use **↑/↓** or **j/k** to select, **Enter/→** to explore a folder, **Backspace/←** to go up, **Home** to return to the scan root, **r/F5** to rescan, **Tab** to expand the map, and **q/Ctrl+C** to quit. **p/Esc** pauses or resumes a running scan without discarding progress. Navigation becomes available after scanning finishes. Narrow terminals show the map with the selected item's details underneath.

On Windows, both debug and release GUI builds launch without a console window. Terminal, report, and help modes attach to the existing launching console when available.

Live previews, scan counts, unreadable-entry counts, and a progress spinner update while scanning. This initial terminal interface supports browsing; file operations and Setup remain in the desktop interface. Use `--report` for scripts and redirected output. `--tui` requires an interactive terminal and cannot be combined with `--gui` or `--report`.

File-type icons in the right-hand list come from Windows Shell, Finder, or the Linux desktop icon theme. Their saturation and brightness are reduced to match the dark UI. Only visible rows request icons; native lookups run on one background worker and are cached by extension and display size. If the OS cannot supply an icon, the row keeps its text label.

## Native application icons

Release downloads include native desktop integration:

- Windows: the executable embeds a multi-resolution ICO, including 16–256px sizes for common display scales. Explorer and shortcuts use this resource; the running app also supplies its window icon.
- macOS: drag `Clawback.app` into Applications and launch it there for the Finder and Dock icon. The bundle includes standard and Retina representations through 1024px. The standalone `clawback` executable remains available for terminal use. Homebrew installs the bundle under its formula prefix; copy it from `$(brew --prefix clawback)/Clawback.app` into Applications if desired.
- Linux: release archives include `share/applications/clawback.desktop` and `share/icons/hicolor` with PNG and scalable SVG icons. Install `clawback` on your PATH (for example `~/.local/bin`), and copy the archive's `share` contents into `~/.local/share` for application-menu integration. Log out and back in if your desktop caches launchers. An unpacked executable alone does not register a desktop launcher.

Artwork lives in `crates/clawback-core/src/icon.rs`. Run `cargo xtask icons` to regenerate the checked-in PNG, ICO, ICNS, and SVG assets. `cargo xtask package <target> <tag>` prepares the native release layout; the release workflow archives it. PNG edges are aligned to the pixel grid at each generated size. Arbitrary desktop scaling may still involve OS resampling.

The file-manager action uses **Open in Explorer**, **Open in Finder**, or **Open in
File Manager**, depending on the platform. It reveals the selected item rather
than running it. The compact context menu groups scan-wide controls under **View**.
Properties shows one main size, with a separate on-disk-size box only when its
formatted value differs. Attribute names use plain language; **Marked for backup**
is Windows' archive attribute, not an indication that the file is compressed.
