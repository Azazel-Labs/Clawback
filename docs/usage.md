# Using Clawback

Click **Open folder** and pick a drive (or **Other Folder...**). The map fills in as the background scan discovers files. Large live previews group smaller entries together; the full tree appears when scanning finishes. You can move or resize the window and cancel while it scans. File actions become available when the scan stops.

Initial scans adapt their concurrency. HDDs and unknown storage start with one worker; SSDs start with two. Once per second, the scanner samples entry throughput and average processing latency using batch counters. It tests a small increase or one fewer worker, allows a settling window, and compares two measurement windows. Increases need at least a 10% throughput gain without a large latency increase; decreases can be kept when they deliver nearly the same throughput with fewer workers, or materially reduce latency with little throughput loss. Failed trials roll back and retries are spaced out. Empty queues and tiny samples never justify an increase. CPU availability sets a safety ceiling of at most 32 workers; additional threads are created only when needed and park when concurrency drops. The GUI and terminal show the current worker limit. Live-update and recovery scans retain their fixed one-worker limit.

Rectangles containing other rectangles are folders; colours show how deeply things are nested; the gray block is free space.

| Mouse | What it does |
|---|---|
| Click | Select the item under the cursor. Click a folder's **frame or title bar** to select the folder itself; clicking inside it selects what's inside. Clicking free space clears the selection. |
| Double-click | Zoom into a folder bucket; file tiles zoom into their containing folder. Available during scanning too. |
| Right-click | Select the item and open the menu: Zoom In / Zoom Out / Zoom Full, Run / Open, Delete, Open Drive, Rescan Drive, Show Free Space, Properties. The default action is in bold. |
| Rest the mouse | After a moment a **name tip** shows names that don't fit in their box, and an **info tip** near the cursor shows size, date and more (configurable in Setup). Any mouse or key input hides them. |

Two compact rows at the top show scan progress, the current location, totals, and navigation. Rescan and additional actions live in More. **Show free space** includes unused capacity when viewing a whole drive. **More** contains Settings, About, Open selected item, and Move selected item to trash. Buttons grey out when they don't apply.

The resizable **Directories** pane above the map shows an expandable folder tree with sizes and percentages. Click an arrow to expand a folder, or its name to navigate the map. Folder navigation has no zoom animation. Redraws keep the previous map and its labels visible until the complete replacement is ready.

Both desktop and terminal views stay **live** after a successful scan. Watching begins before scanning, so changes during the initial scan are reconciled too. On Windows, one asynchronous subtree watch waits for filesystem notifications; there is no periodic drive scan. Changes are coalesced into one-second batches, metadata and directory updates run in the background, and unchanged tree storage is shared between snapshots. The desktop status shows **Live**, **Catching up**, or any watcher failure.

Event queues and pending paths are capped at 4,096 each. Overflow triggers a background reconciliation with one scan worker and at least 30 seconds between recovery scans. Recovery replaces the complete snapshot and returns navigation to the scan root. Failed watches are reported without falling back to polling; use **Rescan** to reconnect. Cancelled scans stay as static partial results.

macOS and Linux use native notifications too, but whole-drive watch limits depend on the OS. Unix hard-link deduplication stays incremental: a device/inode index updates each affected group, counts its bytes once, and transfers those bytes to a surviving link when the counted name is removed. The index uses identities captured during scanning, with no additional filesystem walk. Windows uses incremental updates with its existing size-accounting rules.

**Delete** moves the item to the Recycle Bin / Trash immediately, with no confirmation, just like SpaceMonger (you can restore it from the trash). If that makes you nervous, turn on **Disable "Delete" Command** in Setup. And the original's advice still stands: if you don't know what something is, don't delete it.

Selection clears when you zoom, resize the window, change settings or switch to another app, as it did in SpaceMonger.

The toolbar **Back** button and mouse Back button also return through visited views. **More → Up a level** navigates to the parent directory.

**Keyboard shortcuts** (additions, since SpaceMonger had none): Ctrl/⌘+O Open, F5 Rescan, Home Zoom Full, Escape / Backspace / Alt+Left Back to the previous view, Enter Zoom In (folder) or Run / Open (file), Delete Delete.

Use the toolbar **Palette** menu for preset previews and one-click switching. It applies the chosen palette to files and folders together; Settings also lets you choose them independently.

### Setup

Settings adapted from SpaceMonger's Setup dialog:

- **File Layout:** Density (from "Too Few Files" to "Too Many Files") and a Horizontal ↔ Vertical Bias slider.
- **Display Colors:** Material, Candy, Sunset, Lagoon, Muted, Windows Colors, White, Gray shades, Red … Violet, separately for files and folders.
- **ToolTips:** name tips and info tips, their delays, and which details the info tip shows (path, name, icon, date, size, attributes).
- **Miscellaneous:** Auto Rescan on Delete, Disable Delete, Remember Window Position, Show Rollover Boxes.

Plus a **Scanning** group that SpaceMonger didn't need: stay on one filesystem, use file lengths instead of size on disk, and count hard links once.

Settings are saved to `%APPDATA%\Clawback\settings.ini` on Windows, `~/Library/Application Support/Clawback/settings.ini` on macOS, and `~/.config/clawback/settings.ini` (or `$XDG_CONFIG_HOME/clawback/`) elsewhere.

## Permissions on each platform

Clawback never needs special rights, but it can only measure what your account can read. Folders it couldn't open are counted, and **More → Unreadable folders** lists them with the reason.

- **macOS:** privacy-protected folders (Mail, Messages, Safari data, other users' homes) are skipped unless you grant Clawback **Full Disk Access** in System Settings › Privacy & Security. Scanning `/` includes the Data volume (where `/Users` and `/Applications` actually live) without counting it twice.
- **Linux / BSD:** other users' files and root-only system folders are skipped unless you run with elevated privileges (e.g. `sudo -E clawback /`). Pseudo filesystems such as `/proc`, `/sys` and `/dev` are never scanned.
- **Windows:** a few system folders are administrator-only; run Clawback as administrator to include them.

By default Clawback stays on one filesystem, so scanning `/` won't wander into network mounts, USB drives or container overlays. Symlinks and Windows junctions are never followed. On Unix, sizes are space actually allocated on disk and hard links are counted once; on Windows, sizes are rounded up to whole clusters, as SpaceMonger did.

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

Use **↑/↓** or **j/k** to select, **Enter/→** to explore a folder, **Backspace/←** to go up, **Home** to return to the scan root, **r/F5** to rescan, **Tab** to expand the map, and **q/Ctrl+C** to quit. **Esc** cancels a running scan; its partial results remain browsable. Navigation becomes available after scanning stops. Narrow terminals show the map with the selected item's details underneath.

On Windows, both debug and release GUI builds launch without a console window. Terminal, report, and help modes attach to the existing launching console when available.

Live previews, scan counts, unreadable-entry counts, and a progress spinner update while scanning. This initial terminal interface supports browsing; file operations and Setup remain in the desktop interface. Use `--report` for scripts and redirected output. `--tui` requires an interactive terminal and cannot be combined with `--gui` or `--report`.
