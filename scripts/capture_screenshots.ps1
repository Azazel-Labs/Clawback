# Rebuild and capture only the fictional, in-memory demo. No drive is scanned.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
$captureVariables = @('CLAWBACK_DEMO_CAPTURE', 'CLAWBACK_DEMO_VIEW', 'CLAWBACK_TERMINAL_CAPTURE')
$previous = @{}
foreach ($name in $captureVariables) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    cargo build --features screenshots --locked
    if ($LASTEXITCODE -ne 0) { throw 'Screenshot build failed' }
    $captureDir = Join-Path $projectRoot 'target/demo-capture'
    New-Item -ItemType Directory -Force $captureDir | Out-Null
    $exe = Join-Path $captureDir 'clawback.exe'
    Copy-Item -LiteralPath 'target/debug/clawback.exe' -Destination $exe
    Remove-Item Env:\CLAWBACK_TERMINAL_CAPTURE -ErrorAction SilentlyContinue
    foreach ($shot in @(@('desktop', ''), @('explore', 'Projects'))) {
        $env:CLAWBACK_DEMO_CAPTURE = Join-Path $projectRoot "target/demo-$($shot[0]).ppm"
        $env:CLAWBACK_DEMO_VIEW = $shot[1]
        $process = Start-Process -FilePath $exe -ArgumentList '--gui' -WindowStyle Normal -PassThru -Wait
        if ($process.ExitCode -ne 0) { throw "Demo capture failed: code=$($process.ExitCode), capture=$env:CLAWBACK_DEMO_CAPTURE, view=$env:CLAWBACK_DEMO_VIEW" }
    }
    Remove-Item Env:\CLAWBACK_DEMO_CAPTURE -ErrorAction SilentlyContinue
    Remove-Item Env:\CLAWBACK_DEMO_VIEW -ErrorAction SilentlyContinue
    $env:CLAWBACK_TERMINAL_CAPTURE = Join-Path $projectRoot 'target/demo-terminal.tsv'
    $process = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw 'Terminal capture failed' }
    python scripts/render_screenshots.py
    if ($LASTEXITCODE -ne 0) { throw 'Screenshot conversion failed' }
    & ./scripts/render_terminal.ps1
} finally {
    foreach ($name in $captureVariables) { if ($null -eq $previous[$name]) { Remove-Item "Env:\$name" -ErrorAction SilentlyContinue } else { Set-Item "Env:\$name" $previous[$name] } }
    Pop-Location
}
