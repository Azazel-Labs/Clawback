param(
    [Parameter(Mandatory)]
    [ValidateRange(1, 2147483647)]
    [int] $AppId,
    [Parameter(Mandatory)]
    [ValidateRange(1, 2147483647)]
    [int] $DepotId,
    [Parameter(Mandatory)]
    [ValidatePattern('^(nightly|v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?)$')]
    [string] $Tag,
    [Parameter(Mandatory)]
    [string] $Compiler
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$compilerPath = (Get-Command $Compiler -ErrorAction Stop).Source
$out = Join-Path $repo "dist/steam-$Tag-windows"
if (Test-Path -LiteralPath $out) { throw "Output already exists: $out. Choose a new tag or move the previous output." }
$target = 'x86_64-pc-windows-msvc'
Push-Location $repo
try {
    # A separate target directory prevents Steam and standalone payloads being mixed.
    # The app ID lets the About dialog link to the store page.
    $env:CLAWBACK_STEAM_APP_ID = "$AppId"
    & cargo build --release --locked --target $target --target-dir target/steam --features steam --bin clawback
    if ($LASTEXITCODE -ne 0) { throw 'Steam build failed' }
    $content = Join-Path $out 'content'
    $redist = Join-Path $content 'redist'
    $payload = Join-Path $out 'helper-payload'
    New-Item -ItemType Directory -Path $redist, $payload | Out-Null
    Copy-Item -LiteralPath (Join-Path $repo "target/steam/$target/release/clawback.exe") -Destination $content
    # The Steamworks redistributable, delay-loaded only by the desktop app for achievements.
    $steamApi = Get-ChildItem -Path (Join-Path $repo "target/steam/$target/release/build") -Filter steam_api64.dll -Recurse |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (!$steamApi) { throw 'Missing steam_api64.dll from the steamworks-sys build' }
    Copy-Item -LiteralPath $steamApi.FullName -Destination $content
    foreach ($file in @('README.md', 'LICENSE')) {
        Copy-Item -LiteralPath (Join-Path $repo $file) -Destination $content
    }
    foreach ($notice in @(@('OFL.txt', 'NotoSans-OFL.txt'), @('NotoSansThai-OFL.txt', 'NotoSansThai-OFL.txt'), @('Phosphor-LICENSE.txt', 'Phosphor-LICENSE.txt'))) {
        Copy-Item -LiteralPath (Join-Path $repo "assets/fonts/$($notice[0])") -Destination (Join-Path $content $notice[1])
    }
    Copy-Item -LiteralPath (Join-Path $repo 'docs') -Destination $content -Recurse
    # The elevated copy contains only the worker executable and required notices.
    foreach ($file in @('clawback.exe', 'LICENSE', 'NotoSans-OFL.txt', 'NotoSansThai-OFL.txt', 'Phosphor-LICENSE.txt')) {
        Copy-Item -LiteralPath (Join-Path $content $file) -Destination $payload
    }
    $version = if ($Tag -eq 'nightly') { 'nightly' } else { $Tag.Substring(1) }
    & $compilerPath '/DSteamTurbo' "/DPayloadDir=$payload" "/DAppVersion=$version" '/DTargetArch=x64' "/O$redist" '/Fclawback-turbo-setup' (Join-Path $repo 'packaging/windows/clawback.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Steam Turbo installer compilation failed' }
    $installer = Join-Path $redist 'clawback-turbo-setup.exe'
    if (!(Test-Path -LiteralPath $installer -PathType Leaf)) { throw 'Missing Steam Turbo installer' }
    # A changed installer gets a new prerequisite marker, including nightly updates.
    $revision = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    $template = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'installscript.vdf.in') -Raw
    $template.Replace('@APP_ID@', "$AppId").Replace('@REVISION@', $revision) |
        Set-Content -LiteralPath (Join-Path $content 'installscript.vdf') -Encoding utf8
    @"
"DepotBuildConfig"
{
    "DepotID" "$DepotId"
    "ContentRoot" "content"
    "FileMapping"
    {
        "LocalPath" "*"
        "DepotPath" "."
        "recursive" "1"
    }
    "InstallScript" "installscript.vdf"
}
"@ | Set-Content -LiteralPath (Join-Path $out 'depot_build.vdf') -Encoding utf8
    @"
"AppBuild"
{
    "AppID" "$AppId"
    "Desc" "Clawback $Tag Windows"
    "BuildOutput" "build-output"
    "Depots"
    {
        "$DepotId" "depot_build.vdf"
    }
}
"@ | Set-Content -LiteralPath (Join-Path $out 'app_build.vdf') -Encoding utf8
    Write-Output "Prepared Steam build: $out"
} finally {
    Pop-Location
}
