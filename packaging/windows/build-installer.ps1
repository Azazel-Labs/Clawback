param(
    [Parameter(Mandatory)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string] $Target,
    [Parameter(Mandatory)]
    [ValidatePattern('^(nightly|v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?)$')]
    [string] $Tag,
    [Parameter(Mandatory)]
    [string] $Compiler
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$name = "clawback-$Tag-$Target"
$payload = Join-Path $repo "dist/$name"
foreach ($file in @('clawback.exe', 'README.md', 'LICENSE', 'NotoSans-OFL.txt', 'NotoSansThai-OFL.txt', 'Phosphor-LICENSE.txt')) {
    if (!(Test-Path -LiteralPath (Join-Path $payload $file) -PathType Leaf)) {
        throw "Missing installer payload: $file. Run cargo xtask package first."
    }
}
$arch = if ($Target -eq 'aarch64-pc-windows-msvc') { 'arm64' } else { 'x64' }
# Keep pre-release/nightly labels visible in Windows' installed-app list.
$version = if ($Tag -eq 'nightly') { 'nightly' } else { $Tag.Substring(1) }
& $Compiler "/DPayloadDir=$payload" "/DAppVersion=$version" "/DTargetArch=$arch" "/O$repo/dist" "/F$name-setup" (Join-Path $PSScriptRoot 'clawback.iss')
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }
if (!(Test-Path -LiteralPath (Join-Path $repo "dist/$name-setup.exe"))) {
    throw 'Inno Setup did not produce the expected installer'
}
