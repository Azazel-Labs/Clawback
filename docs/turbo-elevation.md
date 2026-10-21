# Windows Turbo service

The Windows installer offers an optional **Enable Turbo without repeated
administrator prompts** task. Installation requires administrator approval;
subsequent Turbo scans by the authorized Windows user use the `ClawbackTurbo`
service, including after application restarts and Windows restarts. The app
itself stays unelevated. No passwords or UAC tokens are saved.

The task is unchecked by default because it grants access to NTFS file names,
sizes and other scan metadata across local volumes, including metadata ordinary
file permissions may hide. It authorizes one account SID, not all local users.
By default the installer uses the Windows desktop user's shell identity, even
when someone else's administrator credentials approve setup. Upgrades preserve
the existing authorized SID. Managed/headless installations can specify the
account explicitly with `/TASKS=turbo /TurboUser=S-1-5-21-...`.

## Runtime and security boundary

The service runs the protected `Program Files\Clawback\clawback.exe` in a
separate broker mode as LocalSystem. It accepts a fixed-size, versioned request
containing only a random pipe nonce. It derives the requesting process ID from
Windows and launches only its own executable's read-only MFT worker. Requests
cannot specify executables, commands, writes or deletion operations.

The broker pipe has an explicit ACL for LocalSystem and the authorized account,
rejects remote pipe connections and does not give clients permission to create
pipe instances. Clients verify the broker process ID against the Windows service
manager before sending a request. The broker transfers only query/wait rights
for the worker process; the existing scan pipe also checks both peer process IDs.
Mounted-folder volumes are resolved to volume GUIDs by the unelevated client.
Service workers reject arbitrary paths, UNC paths and device paths before any
filesystem lookup, then retain the core scanner's local-NTFS-volume validation.

At most four workers can run concurrently. Partial broker requests time out,
and workers abandon incomplete handshakes after 30 seconds. Closing the scan
pipe cancels its worker, including when ordinary scanning wins the race. A
Windows job object terminates remaining workers if the service stops or crashes.
Metadata access is intentionally granted to the account: other software running
as that account can also request scans, just as that user can read the results.

## Installation, updates and removal

Service installation accepts only the fixed Program Files installation path and
rejects reparse points. The installer stops the service before replacing files,
then configures and starts it. Reinstalling with the task deselected removes the
service; uninstalling removes it before deleting the executable. These changes
require administrator rights. Normal user settings are retained by uninstall.

If the service is absent, stopped or the current account is not authorized,
Turbo uses its existing per-scan UAC helper. Cancelling UAC leaves normal scanning
available. A busy or malformed broker response fails the Turbo attempt rather
than silently switching to another privileged mechanism. Portable builds remain
usable without installing anything; they can also use an installed service when
the current account is authorized and the protocol is compatible.

## Validation and diagnostics

`clawback.exe --turbo-service-check` checks broker authentication, worker startup,
process-handle transfer and the scan protocol using a deliberately non-volume
path. It performs no volume scan, never requests UAC, and returns nonzero if the
service is unavailable or rejects the caller. `Get-Service ClawbackTurbo` reports
service status. Rerun the installer to repair a failed service configuration.

Windows unit tests cover the pipe ACL, rejected malformed/partial requests and
SID validation. Release jobs exercise service installation, reinstall, broker
checks, restart and removal on both Windows architectures. The existing Turbo
protocol and scan tests continue to cover cancellation, progress and snapshots.

A service is not the only Windows persistence mechanism, but it provides an
explicit lifecycle and access boundary for this use case. Keeping a UAC helper
alive only for the app session would not survive application restarts. A saved
settings flag cannot authorize future elevation; see [Microsoft's UAC
explanation](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/user-account-control/how-it-works).
