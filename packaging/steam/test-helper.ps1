# Run only on a disposable elevated Windows CI runner/VM: installs a real service.
param([Parameter(Mandatory)][string] $Depot)
$ErrorActionPreference = 'Stop'
$depotPath = (Resolve-Path -LiteralPath $Depot).Path
$installed = Join-Path $env:ProgramW6432 'Clawback Steam Turbo'
if (Test-Path -LiteralPath $installed) { throw 'Refusing to change an existing Steam Turbo installation' }
if (Get-Service ClawbackSteamTurbo -ErrorAction SilentlyContinue) { throw 'Steam Turbo service already exists' }
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
function Invoke-Checked([string] $File, [string[]] $Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "$File failed: $($process.ExitCode)" }
}
$setup = Join-Path $depotPath 'content/redist/clawback-turbo-setup.exe'
$client = Join-Path $depotPath 'content/clawback.exe'
try {
    foreach ($attempt in 1..2) {
        $setupArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-')
        # Headless CI has no Explorer desktop. Upgrades must preserve the SID.
        if ($attempt -eq 1) { $setupArgs += "/TurboUser=$sid" }
        Invoke-Checked $setup $setupArgs
        if ((Get-Service ClawbackSteamTurbo).Status -ne 'Running') { throw 'Steam Turbo did not start' }
        if ((Get-FileHash -LiteralPath $client).Hash -ne (Get-FileHash -LiteralPath (Join-Path $installed 'clawback.exe')).Hash) {
            throw 'Protected helper does not match the depot'
        }
        Invoke-Checked $client @('--turbo-service-check')
    }
    Restart-Service ClawbackSteamTurbo
    (Get-Service ClawbackSteamTurbo).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
    Invoke-Checked $client @('--turbo-service-check')
    Invoke-Checked (Join-Path $installed 'clawback.exe') @('--turbo-service-install', 'S-1-5-21-111-222-333-1001')
    $denied = Start-Process -FilePath $client -ArgumentList '--turbo-service-check' -Wait -PassThru -WindowStyle Hidden
    if ($denied.ExitCode -eq 0) { throw 'Unauthorized user was accepted' }
} finally {
    $uninstaller = Join-Path $installed 'unins000.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        Invoke-Checked $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    }
}
if (Get-Service ClawbackSteamTurbo -ErrorAction SilentlyContinue) { throw 'Steam Turbo service was not removed' }
if (Test-Path -LiteralPath (Join-Path $installed 'clawback.exe')) { throw 'Protected helper was not removed' }
