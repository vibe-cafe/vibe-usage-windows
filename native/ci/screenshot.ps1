# Opens the published app on the Actions runner with the fake backend's
# sample data and saves the panel at original size with PrintWindow, one
# image per state, for side-by-side comparison with the Mac new panel.
# Runner only (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Tag = '100'
)

. "$PSScriptRoot/common.ps1"
$script:Exe = Join-Path $AppDir 'vibe-usage-app.exe'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$logDir = Join-Path $OutDir "logs-screenshot-$Tag"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Tall enough for more of the expanded panel when the runner allows it.
Set-Resolution 1920 1200
Set-DarkTheme
Write-Host "os: $([Environment]::OSVersion.VersionString)"

$failed = 0
function Invoke-Case([string]$Name, [string[]]$Extra) {
    Stop-App
    $rect = Join-Path $OutDir "$Tag-$Name.rect.txt"
    Remove-Item $rect -ErrorAction SilentlyContinue
    $appArgs = @('--test-pin', "--test-rect-file=$rect", "--test-log-dir=$logDir") + $Extra
    $proc = Start-App $appArgs
    Start-Sleep -Seconds 8
    if ($proc.HasExited) { Write-Host "::warning::${Name}: app exited early, code $($proc.ExitCode)" }
    $n = Read-Numbers $rect
    if (-not $n) {
        Write-Host "::error::${Name}: the panel did not open (no rect file)"
        $script:failed++
    } else {
        $visible = @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4])
        Write-Host "${Name}: panel visible rect $($visible -join ' ')"
        if (-not (Save-Window $n[0] $visible (Join-Path $OutDir "$Tag-$Name.png"))) { $script:failed++ }
        # The same area from the screen, in case PrintWindow and the composed screen ever differ.
        $full = Join-Path $OutDir "$Tag-$Name-screen-full.png"
        Save-Screen $full
    }
    Stop-App
}

# The Mac new panel's states (dark only, like the Mac panel).
Invoke-Case '1-collapsed' @()
Invoke-Case '2-expanded' @('--test-expand')
Invoke-Case '3-expanded-end' @('--test-expand', '--test-scroll-end')
Invoke-Case '4-single' @('--test-demo=single')

$log = Join-Path $logDir 'shell.log'
if (Test-Path $log) {
    Write-Host '----- shell.log -----'
    Get-Content $log -Encoding utf8 | ForEach-Object { Write-Host $_ }
}
if ($failed -gt 0) { exit 1 }
