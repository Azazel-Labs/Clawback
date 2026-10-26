# Steam Windows distribution

The Steam build installs a read-only Turbo helper during first-launch setup.
Windows may request administrator approval for setup; subsequent Turbo scans
need no UAC prompt for the authorized Windows account. The desktop app remains
unelevated. This is Windows NTFS support, not a Proton/Steam Deck feature.

The helper lives in `Program Files\Clawback Steam Turbo`, independently of the
Steam library location. Its `ClawbackSteamTurbo` service and named pipe are
separate from the standalone installer's `ClawbackTurbo` service. Never configure
a privileged service to execute from a user-writable Steam library.

## Build a depot

On Windows, install Rust and Inno Setup 6, then run from the repository root,
substituting the actual Steamworks app and Windows depot IDs:

```powershell
./packaging/steam/build-depot.ps1 -AppId 123456 -DepotId 123457 -Tag v0.3.0 -Compiler 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
```

The command builds the x64 app with the `steam` Cargo feature in `target/steam`,
compiles the helper installer, and prepares `dist/steam-v0.3.0-windows`.
It refuses to overwrite an existing output directory. The helper-payload folder
is only a compiler input; only `content` is included in the depot.

Alternatively, run **Actions → Build Steam Windows depot** with those IDs and
the package label. It builds the selected workflow ref, tests helper installation,
upgrade, authorization and removal on the disposable runner, and saves an
artifact for manual upload. It never logs into Steam or publishes a build.

In Steamworks, configure this depot for Windows and set the normal launch option
to `clawback.exe`, arguments `--gui`, with no administrator requirement. Upload
the generated `app_build.vdf` using SteamPipe's `run_app_build` command. This
script does not upload or publish anything, or select a live branch. Publish to
a private testing branch first. SteamPipe must sign and flag `installscript.vdf`
using the generated depot's `InstallScript` directive; copying the VDF beside an
executable alone does not activate first-launch setup.

See Valve's [install script documentation](https://partner.steamgames.com/doc/sdk/installscripts).

## Setup, updates, and removal

Setup authorizes the interactive desktop user's Windows SID, even when a
different administrator supplies the UAC credentials. It fails closed if there
is no desktop user to resolve. Upgrades retain the existing authorized SID;
this does not grant protected metadata access to all users of a shared PC.

Every changed helper installer receives a new Steam prerequisite marker based
on its SHA-256, so the protected copy updates along with the depot, including
nightlies. An update can require administrator approval again. The installer
stops the service and its workers before replacing the executable and restarts
it afterward. Failed setup returns a nonzero exit code, allowing Steam to retry.
Normal scanning still works without the service; Turbo retains its per-scan UAC
fallback when the service is missing or the current user is not authorized.

Steam's uninstall hook runs the dedicated helper uninstaller, which stops and
removes only `ClawbackSteamTurbo`. If cleanup is declined or fails, remove
**Clawback Steam Turbo** from Windows Installed Apps. The standalone Clawback
installation is independent. Reinstalling through Steam runs setup again.

Describe the privileged helper and its ability to read protected file names and
sizes on the store page. The app does not need to run as administrator.

## Achievements

Builds with the `steam` Cargo feature, on any OS, report achievements through
the Steamworks API (`src/achievements.rs`). The portable GitHub builds never
include it. Achievements connect only when Steam launched the app, and only
from the desktop app; on Windows `steam_api64.dll` is delay-loaded and must sit
beside `clawback.exe`. The depot script copies it there, but not into the Turbo
helper payload. Linux and macOS Steam builds must ship `libsteam_api.so` or
`libsteam_api.dylib` beside the executable; the build adds an rpath for it.

Create these in Steamworks under **Stats & Achievements** with exactly these
API names, then publish the changes:

| API name | Name | Description |
| --- | --- | --- |
| `FIRST_SCAN` | Is This Thing On? | Finish your first scan. |
| `WHOLE_DRIVE` | The Whole Enchilada | Scan an entire drive. |
| `TERABYTE` | Digital Hoarder | Scan a terabyte of stuff. |
| `MILLION_FILES` | A Million Little Files | Scan a million files at once. |
| `ABSOLUTE_UNIT` | Absolute Unit | Find a single file of 50 GB or more. |
| `NODE_MODULES` | Event Horizon | Find a `node_modules` folder over 1 GB. |
| `FIRST_DELETE` | Snip Snip | Delete something. |
| `SPARK_JOY` | Does It Spark Joy? | Delete 10 GB or more at once. |
| `TRASH_DAY` | Trash Day | Empty the Recycle Bin. |
| `TEMP_CLEAN` | Temporary Insanity | Clean out your Temp folder. |
| `WSL_DIET` | Penguin Diet | Compact a WSL disk. |
| `DONT_TOUCH` | Please Don't Touch the OS | Poke the Windows OS tile. |
| `SPELUNKER` | Spelunker | Zoom ten folders deep. |
| `GOTTA_GO_FAST` | Gotta Go Fast | Finish a Turbo scan. |
| `GAME_OVER` | Game Over | Uninstall a Steam game from the map. |
| `DRIP_CHECK` | Drip Check | Change the map's color palette. |
| `DELETE_WINDOWS` | Ctrl+Alt+Delete | Delete something on Windows. |
| `DELETE_LINUX` | rm -rf Enthusiast | Delete something on Linux. |
| `DELETE_MACOS` | Command+Delete | Delete something on macOS. |
| `WORLD_TOUR` | Platform Agnostic | Delete something on Windows, Linux and macOS. |

To test locally, run a Steam build with Steam open and a `steam_appid.txt`
holding the app ID in the working directory. On Windows, first copy
`steam_api64.dll` from `target/<profile>/build/steamworks-sys-*/out` beside the
built executable; without it, achievements stay off.

## Release validation

Before publishing, test the signed depot on a Windows VM through Steam:

1. Install in a non-system Steam library. Approve first-launch setup and verify
   the service executable is under `Program Files\Clawback Steam Turbo`.
2. As the original, unelevated user, run `clawback.exe --turbo-service-check`
   from the Steam depot, then scan an entire local NTFS drive using Turbo.
   Neither operation should prompt. Restart Windows and repeat.
3. Test a standard account with a different administrator approving setup;
   verify only the original account has prompt-free Turbo access.
4. Decline setup and confirm ordinary scans still work. Retry setup through
   Steam; check that a successful setup does not repeat on every launch.
5. Upload a changed build and confirm first-launch setup updates the protected
   executable. Test uninstall/reinstall, and uninstall with approval declined.
6. Install standalone Clawback alongside Steam. Remove either distribution and
   verify that the other distribution's service still works.

Local Rust tests do not validate Steam's signing, elevation, or uninstall flow;
the signed-depot checks above are required before a public release.
