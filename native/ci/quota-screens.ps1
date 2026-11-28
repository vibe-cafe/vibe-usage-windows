# Saves the panel at original size (PrintWindow) for every scene of the
# 订阅配额 card (src/VibeUsage.App/State/DemoScenes.cs): the Mac panel
# renderer's scenes 131–133 and its special-state table under the same ids,
# plus the Windows-only states (win-*). One app start per scene, on the
# in-memory fake backend: no network, no credentials, no user data.
# Runner only (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Tag = '100',
    [string]$Only = ''
)

. "$PSScriptRoot/common.ps1"
$script:Exe = Join-Path $AppDir 'vibe-usage-app.exe'
$shots = Join-Path $OutDir "quota-$Tag"
$logDir = Join-Path $OutDir "logs-quota-$Tag"
New-Item -ItemType Directory -Force -Path $shots, $logDir | Out-Null

Set-Resolution @('1920x1200', '1920x1080')
Set-DarkTheme
Write-Host "os: $([Environment]::OSVersion.VersionString)"

# The scene list is the app's own: ids and titles from DemoScenes.cs.
$source = Join-Path $PSScriptRoot '../src/VibeUsage.App/State/DemoScenes.cs'
$scenes = @(Select-String -Path $source -Encoding utf8 -Pattern '^\s*(?:new|State)\("([^"]+)", "([^"]+)"' | ForEach-Object {
    [pscustomobject]@{ Id = $_.Matches[0].Groups[1].Value; Title = $_.Matches[0].Groups[2].Value }
})
if ($Only) { $wanted = $Only.Split(','); $scenes = @($scenes | Where-Object { $wanted -contains $_.Id }) }
Write-Host "$($scenes.Count) scenes"

$failed = 0
$manifest = New-Object System.Collections.Generic.List[object]
foreach ($s in $scenes) {
    Stop-App
    $rect = Join-Path $shots "$($s.Id).rect.txt"
    Remove-Item $rect -ErrorAction SilentlyContinue
    $proc = Start-App @('--fake-backend', '--test-pin', "--test-demo=$($s.Id)", "--test-rect-file=$rect", "--test-log-dir=$logDir")
    $end = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path $rect) -and [DateTime]::UtcNow -lt $end) { Start-Sleep -Milliseconds 200 }
    # The entrance animation, a relayout for the content's height, the tooltip.
    Start-Sleep -Milliseconds 1800
    $n = Read-Numbers $rect
    if (-not $n) {
        Write-Host "::error::$($s.Id): the panel did not open (no rect file)"
        $failed++
        continue
    }
    $visible = @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4])
    $file = "$($s.Id).png"
    if (Save-Window $n[0] $visible (Join-Path $shots $file)) {
        $manifest.Add([pscustomobject]@{ id = $s.Id; title = $s.Title; file = $file; width = $visible[2]; height = $visible[3] })
    } else {
        $failed++
    }
    Remove-Item $rect -ErrorAction SilentlyContinue
}
Stop-App
$manifest | ConvertTo-Json | Set-Content (Join-Path $shots 'scenes.json') -Encoding utf8
Write-Host "saved $($manifest.Count) of $($scenes.Count)"
$logs = @('shell.old.log', 'shell.log') | ForEach-Object { Join-Path $logDir $_ } | Where-Object { Test-Path $_ }
if ($logs) {
    $lines = @($logs | ForEach-Object { Get-Content $_ -Encoding utf8 })
    $bad = @($lines | Where-Object { $_ -match 'unhandled|launch failed|backend start failed|backend: native' })
    if ($bad) { $bad | ForEach-Object { Write-Host "::error::$_" }; $failed++ }
    # Every scene ran on the in-memory fake backend (no DLL, no network).
    $fake = @($lines | Where-Object { $_ -match 'backend: fake \(' }).Count
    Write-Host "fake backend starts: $fake"
    # (The log rolls over at 1 MB on a start; then only the newest lines are left to count.)
    $rolled = $logs.Count -gt 1
    if ($fake -eq 0 -or (-not $rolled -and $fake -lt $manifest.Count)) { Write-Host "::error::only $fake of $($manifest.Count) scenes logged the fake backend"; $failed++ }
}
if ($failed -gt 0) { exit 1 }
