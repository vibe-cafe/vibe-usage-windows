# Switches the runner from 100 % to the given scale while the app runs and
# checks from its log that the tray icon was reloaded at the new small-icon
# size (24 px at 150 %). Leaves the runner at the new scale for the next
# steps. Exit 3: the runner refused the scale change (not an app failure).
# Runner only (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [int]$Percent = 150
)

. "$PSScriptRoot/common.ps1"
$script:Exe = Join-Path $AppDir 'vibe-usage-app.exe'
$logDir = Join-Path $OutDir 'logs-dpi'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
Use-Log (Join-Path $logDir 'shell.log')
Stop-App
$proc = Start-App @('--autostart', "--test-log-dir=$logDir")
$added = Wait-LogLine 'tray: added after' 10000
Write-Host ((New-LogLines) -join "`n")
& "$PSScriptRoot/set-dpi.ps1" -Percent $Percent
$changed = $LASTEXITCODE -eq 0
if (-not $changed) {
    Stop-App
    Write-Host "::warning::the runner refused $Percent %; the scaled checks are skipped"
    exit 3
}
$size = [int][Math]::Round(16 * $Percent / 100)
$reloaded = Wait-LogLine "tray: icon .*taskbar, ${size}px" 10000
$l = New-LogLines
Add-Result "I 系统缩放改成 $Percent %：托盘图标按 ${size} 像素重载" ($added -and $reloaded -and [bool]($l | Where-Object { $_ -match 'tray: icon reloaded .*ok=True' })) $l
Stop-App
Write-Summary (Join-Path $OutDir "dpi-results.json")
if ($script:Results | Where-Object { -not $_.Pass }) { exit 1 }
