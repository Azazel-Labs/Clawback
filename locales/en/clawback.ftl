# Clawback GUI messages. Keep message IDs and variable names unchanged.

ok = { "   OK   " }
of-scan = % OF SCAN
asset-3d = 3D asset
a-fast-cross-platform-disk-space-map = A fast, cross-platform disk space map.
about-clawback = About Clawback
all-files = All files
application-library = Application / library
arch = Arch
archive = Archive
attributes = Attributes
attributes-2 = Attributes:
audio = Audio
auto-rescan-on-delete = Auto Rescan on Delete
back = Back
bias = Bias:
binary-data = Binary data
cancel = Cancel
chinese-simplified = Simplified Chinese
chinese-traditional = Traditional Chinese
claw-back-your-disk-space = Claw back your disk space.
clawback-settings = Clawback Settings
scan-folder-error = { "Clawback could not scan the folder.\u000A\u000A" }{ $error }
scan-path-error = Clawback could not scan { $path }{ ".\u000A\u000A" }{ $error }
compress = Compress
contains = Contains:
copyright-2026-azazel-labs = Copyright © 2026 Azazel Labs.
count-hard-linked-files-once = Count hard-linked files once
workers-help = Current concurrency limit; automatically adapts to measured throughput and latency.
data-configuration = Data / configuration
date-time = Date / Time
delay = Delay:
delete = Delete
deleting = { "Deleting...\u000A" }{ $path }
delete-preparing = Preparing to recycle…
delete-recycling = Moving to Recycle Bin…
delete-permanently = Deleting permanently…
emptying-recycle-bin = Emptying Recycle Bin…
delete-permanently-title = Delete permanently?
delete-too-big-for-recycle-bin = “{ $name }” is too big for the Recycle Bin.
empty-recycle-bin-title = Empty the Recycle Bin?
empty-recycle-bin-body = Everything in your Recycle Bin on { $drive } will be deleted permanently.
delete-cannot-be-undone = This can’t be undone.
delete-size-files =
    { $size } · { $files ->
        [one] { $files } file
       *[other] { $files } files
    }
delete-permanently-button = Delete permanently
empty-recycle-bin-button = Empty Recycle Bin
delete-progress-files = { $done } of { $total } files
purge-failed =
    { $count ->
        [one] { $count } item couldn’t be deleted.
       *[other] { $count } items couldn’t be deleted.
    } { $error }
delete-updating = Updating disk map…
delete-details = { $size } · { $seconds }s elapsed
delete-worker-stopped = The recycling worker stopped unexpectedly. Check the selected item before trying again.
delete-queued =
    { $count ->
        [one] { $count } more item queued
       *[other] { $count } more items queued
    }
density = Density:
design-document = Design document
directories = Directories
disable-delete-command = Disable "Delete" Command
discovering-folders = Discovering folders…
disk-image = Disk image
display-colors = Display Colors
document = Document
don-t-descend-into-other-drives-or-network = Don't descend into other drives or network mounts inside the scanned folder
double-click-to-explore-right-click-for-actions = Double-click to explore / Right-click for actions
encrypt = Encrypt
english = English
equal = Equal
exit = Exit
expand-to-browse-click-a-folder-to-view = Expand to browse · Click a folder to view
file-type = FILE TYPE
files = FILES
folder = FOLDER
delete-error = Failed to delete { $path }{ ".\u000A\u000A" }{ $error }
click-to-empty-recycle-bin = Click to empty this drive’s Recycle Bin
file = File
file-layout = File Layout
file-size = File Size
file-types = File types
filename = Filename
files-2 = Files:
contents-count =
    { $files ->
        [one] { $files } file
       *[other] { $files } files
    }, { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }
finding-drives = Finding drives...
folder-2 = Folder
folders-not-scanned = Folders Not Scanned
folders = Folders:
french = French
full-path = Full Path
game-archive = Game archive
german = German
help = Help
hidden = Hidden
horz = Horz
icon = Icon
image = Image
language = Language
korean = Korean
japanese = Japanese
italian = Italian
polish = Polish
russian = Russian
portuguese-brazil = Brazilian Portuguese
licensed-under-mit-0-mit-no-attribution-no = Licensed under MIT-0 (MIT No Attribution). No warranty of any kind.
location = Location:
lots-of-files = Lots of Files
make-room-for-what-matters = Make room for what matters.
mapped-bytes-relative-to-used-drive-space-an = Mapped bytes relative to used drive space; an estimate, not a file-count percentage.
miscellaneous-options = Miscellaneous Options
modified = Modified:
move-selected-item-to-trash = Move selected item to trash
mute-colors = Mute colors
name = Name:
no-drives-found-use-other-folder-to-pick = No drives found. Use "Other Folder..." to pick a folder.
no-extension = No extension
no-files-in-this-folder = No files in this folder.
normal = Normal
offline = Offline
open-drive = Open Drive...
open-a-drive-or-folder-to-see-where = Open a drive or folder to see where your space goes.
open-a-folder-or-drive-to-browse-its = Open a folder or drive to browse its directory tree.
open-a-folder-to-begin = Open a folder to begin
open-folder = Open folder…
open-selected-item = Open selected item
other-folder = Other Folder...
palette = Palette
pause-scan = Pause scan
properties = Properties
properties-2 = Properties...
read-only = Read-Only
reading-file-details = Reading file details…
ready = Ready
recent = Recent
remember-window-position = Remember Window Position
reparse-pt = Reparse-Pt
rescan = Rescan
rescan-drive = Rescan Drive
resume-scan = Resume scan
run-open = Run / Open
size = SIZE
scan-paused = Scan paused
scanning = Scanning
scanning-options-apply-to-the-next-scan = Scanning options apply to the next scan.
select-drive-to-view = Select Drive to View
select-a-folder-to-view = Select a Folder to View
settings = Settings…
show-free-space = Show Free Space
show-rollover-boxes = Show Rollover Boxes
show-file-info-tips = Show file details tooltips
show-file-name-tips = Show file name tooltips
show-free-space-2 = Show free space
show-in-file-manager = Show in File Manager
size-on-disk = Size on disk:
size-2 = Size:
permission-hint-unix = Some folders are only readable by other users or root. Run Clawback with elevated privileges (for example `sudo -E clawback /`) to include them.
permission-hint-windows = Some system folders are only readable by administrators. Run Clawback as administrator to include them.
source-code = Source code
spanish-latin-america = Latin American Spanish
spanish-spain = Spanish from Spain
sparse = Sparse
special-file = Special file
stay-on-one-filesystem = Stay on one filesystem
symbolic-link = Symbolic link
symlink = Symlink
system = System
system-default = System default
type = TYPE
takes-effect-the-next-time-clawback-starts = Takes effect the next time Clawback starts
temp = Temp
the-scan-worker-stopped-unexpectedly = The scan worker stopped unexpectedly.
too-few-files = Too Few Files
too-many-files = Too Many Files
tooltips = Tooltips
try-a-faster-ntfs-scan-with-administrator-permission = Try a faster NTFS scan with administrator permission. The current scan keeps running.
turbo = Turbo
turbo-cancelled-normal-scan-continues = Turbo cancelled — normal scan continues
turbo-unavailable-normal-scan-continues = Turbo unavailable — normal scan continues
turbo-reading-mft = Turbo: reading MFT…
type-2 = Type:
unreadable-folders = Unreadable folders
up-a-level = Up a level
updating = Updating…
use-file-lengths-not-size-on-disk = Use file lengths, not size on disk
vert = Vert
very-few-files = Very Few Files
very-many-files = Very Many Files
video = Video
view = View
waiting-for-windows-permission = Waiting for Windows permission…
workers =
    { $count ->
        [one] { $count } worker
       *[other] { $count } workers
    }
zoom-full = Zoom Full
zoom-in = Zoom In
zoom-out = Zoom Out
zoom-in-2 = Zoom in
permission-hint-macos = macOS protects some folders (Mail, Messages, other users, …). Grant Clawback "Full Disk Access" in System Settings › Privacy & Security to include them.
msec = msec
drive-free-of-total = { $free } free of { $total }
drive-summary = { $icon }  { $drive }{ "\u000A        " }{ $free } free of { $total }  ·  { $used } used  ·  { $filesystem }
properties-title = { $name } Properties
window-title = { $path }  -  { $total } Total  -  { $free } Free  -  Clawback
scan-summary = { $size } | { contents-count }
turkish = Turkish
ukrainian = Ukrainian
czech = Czech
portuguese-portugal = European Portuguese
dutch = Dutch
indonesian = Indonesian
vietnamese = Vietnamese
thai = Thai
swedish = Swedish
romanian = Romanian
hungarian = Hungarian
afrikaans = Afrikaans
catalan = Catalan
serbian-cyrillic = Serbian, Cyrillic
danish = Danish
finnish = Finnish
greek = Greek
norwegian-bokmal = Norwegian Bokmål
turbo-read-progress = Turbo: reading MFT { $percent }% · { $records } records
turbo-resolving = Turbo: resolving file names…
turbo-assembling = Turbo: assembling folders · { $files } files
turbo-sorting = Turbo: sorting results…
turbo-transferring = Turbo: loading results…
turbo-main-progress = Showing Turbo progress · validating results…
turbo-progress-help = Counters switch to Turbo when it has assembled more entries. The map keeps the regular scan preview until the complete Turbo tree is validated. If Turbo fails, the regular scan continues.
settings-appearance = Appearance
settings-behavior = Behavior
settings-save = Save changes
settings-bias-help = Left favors horizontal shapes; center balances them; right favors vertical shapes.
settings-name-tips = File names
settings-info-tips = File details
settings-window = Window & interaction
settings-deletion = Deleting files
settings-scan-scope = Scan boundaries
settings-file-sizes = Size accounting
