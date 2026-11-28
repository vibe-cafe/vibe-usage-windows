# Runner only. Issue #86, size and antivirus: for one published variant of the
# sample, measure the folder, a copy with only en/zh language resources, and
# zip / 7z (LZMA2) / NSIS (solid LZMA) packages of both; estimate a full
# installer by adding the Tauri build's node.exe and main exe (stand-in for the
# Rust backend); start each copy (panel + settings) and screenshot it; scan
# every product with Microsoft Defender.
param(
    [Parameter(Mandatory = $true)][string]$Variant,
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [Parameter(Mandatory = $true)][string]$PublishLog,
    [string]$TauriDir = ''
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$work = Join-Path $env:RUNNER_TEMP "size-$Variant"
New-Item -ItemType Directory -Force -Path $work | Out-Null

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

function Size-Of([string]$path) {
    if (Test-Path $path -PathType Container) { return (Get-ChildItem $path -Recurse -File | Measure-Object Length -Sum).Sum }
    if (Test-Path $path) { return (Get-Item $path).Length }
    return $null
}
function MB($bytes) { if ($null -eq $bytes) { return $null } [Math]::Round($bytes / 1MB, 1) }

$result = [ordered]@{ variant = $Variant }
$log = if (Test-Path $PublishLog) { Get-Content $PublishLog -Raw } else { '' }
$result.publishSucceeded = (Test-Path (Join-Path $AppDir 'VibeUsagePanelPoc.exe'))
$result.trimWarnings = ([regex]::Matches($log, 'warning IL\d{4}')).Count
$result.aotWarnings = ([regex]::Matches($log, 'warning (IL3\d{3}|CsWinRT\d+)')).Count
$result.publishErrors = @([regex]::Matches($log, 'error [A-Z]+\d+[^\r\n]*') | ForEach-Object { $_.Value } | Select-Object -Unique -First 5)
if (-not $result.publishSucceeded) {
    $result | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $OutDir "size-$Variant.json") -Encoding utf8
    Write-Host ($result | ConvertTo-Json -Depth 5)
    exit 0
}

# -- copies ------------------------------------------------------------------------------
$full = Join-Path $work 'full'
$lean = Join-Path $work 'lean'
Copy-Item $AppDir $full -Recurse
Copy-Item $AppDir $lean -Recurse
$keep = @('en', 'en-us', 'zh', 'zh-cn', 'zh-hans', 'zh-tw', 'zh-hant')
$removed = 0
Get-ChildItem $lean -Directory | Where-Object { $_.Name -match '^[a-z]{2,3}(-[A-Za-z0-9]+)*$' -and $_.Name.ToLower() -notin $keep -and $_.Name -ne 'Assets' } | ForEach-Object {
    Remove-Item $_.FullName -Recurse -Force; $removed++
}
$result.languageFoldersRemoved = $removed

# -- packaging ---------------------------------------------------------------------------
$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
if (-not $makensis) { $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (-not (Test-Path $makensis)) {
    choco install nsis -y --no-progress | Out-Null
    $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
}
$nsi = Join-Path $work 'installer.nsi'
@'
Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 64
Name "Vibe Usage panel sample"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\VibeUsagePanelSample"
RequestExecutionLevel user
Section
  SetOutPath "$INSTDIR"
  File /r "${SRC}\*.*"
SectionEnd
'@ | Set-Content $nsi -Encoding utf8

function Package([string]$name, [string]$dir) {
    $zip = Join-Path $work "$name.zip"
    $sz = Join-Path $work "$name.7z"
    $setup = Join-Path $work "$name-setup.exe"
    & 7z a -tzip -mx=9 $zip "$dir\*" | Out-Null
    & 7z a -t7z -m0=lzma2 -mx=9 -ms=on $sz "$dir\*" | Out-Null
    & $makensis /V1 "/DSRC=$dir" "/DOUT=$setup" $nsi | Out-Null
    [ordered]@{ folderMB = MB (Size-Of $dir); files = @(Get-ChildItem $dir -Recurse -File).Count; zipMB = MB (Size-Of $zip); sevenZipLzma2MB = MB (Size-Of $sz); nsisLzmaMB = MB (Size-Of $setup); zip = $zip; sevenZip = $sz; setup = $setup }
}
$result.full = Package 'full' $full
$result.lean = Package 'lean' $lean

# Full-installer estimate: lean shell + node.exe + the Tauri main exe as the Rust backend's stand-in.
if ($TauriDir -and (Test-Path $TauriDir)) {
    $node = Get-ChildItem $TauriDir -Recurse -File -Filter 'node.exe' | Select-Object -First 1
    $main = Get-ChildItem $TauriDir -Recurse -File -Filter '*.exe' | Where-Object { $_.Name -notmatch 'node|uninstall|unins' } | Sort-Object Length -Descending | Select-Object -First 1
    if ($node -and $main) {
        $combo = Join-Path $work 'estimate'
        Copy-Item $lean $combo -Recurse
        New-Item -ItemType Directory -Force -Path (Join-Path $combo 'resources\node') | Out-Null
        Copy-Item $node.FullName (Join-Path $combo 'resources\node\node.exe')
        Copy-Item $main.FullName (Join-Path $combo 'backend-standin.exe')
        $estSetup = Join-Path $work 'estimate-setup.exe'
        & $makensis /V1 "/DSRC=$combo" "/DOUT=$estSetup" $nsi | Out-Null
        $result.estimate = [ordered]@{ nodeMB = MB $node.Length; backendStandInMB = MB $main.Length; backendStandIn = $main.Name; folderMB = MB (Size-Of $combo); nsisLzmaMB = MB (Size-Of $estSetup) }
    }
}

# -- smoke: start, panel + settings, screenshot ------------------------------------------
function Smoke([string]$name, [string]$dir) {
    $logDir = Join-Path $work "smoke-$name"
    $rect = Join-Path $work "smoke-$name-rect.txt"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process (Join-Path $dir 'VibeUsagePanelPoc.exe') -ArgumentList @('--open', '--pin', '--open-settings', "--log-dir=$logDir", "--rect-file=$rect") -PassThru
    $shownMs = $null
    for ($i = 0; $i -lt 60 -and -not $p.HasExited; $i++) {
        Start-Sleep -Milliseconds 250
        $l = Get-Content (Join-Path $logDir 'panel-poc.log') -ErrorAction SilentlyContinue
        if ($l | Where-Object { $_ -match 'show source=Startup' }) { $shownMs = $sw.ElapsedMilliseconds; break }
    }
    Start-Sleep -Seconds 4
    $alive = -not $p.HasExited
    $shot = Join-Path $OutDir "smoke-$Variant-$name.png"
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $g.Dispose()
    $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    if ($alive) { Stop-Process -Id $p.Id -Force }
    Start-Sleep -Seconds 2
    $l = Get-Content (Join-Path $logDir 'panel-poc.log') -ErrorAction SilentlyContinue
    $errors = @($l | Where-Object { $_ -match 'failed|unhandled|ERROR' } | Select-Object -First 3)
    [ordered]@{ started = ($null -ne $shownMs); alive = $alive; msToPanel = $shownMs; errors = $errors }
}
$result.smokeFull = Smoke 'full' $full
$result.smokeLean = Smoke 'lean' $lean

# -- Defender ----------------------------------------------------------------------------
$mp = "$env:ProgramFiles\Windows Defender\MpCmdRun.exe"
$status = Get-MpComputerStatus -ErrorAction SilentlyContinue
$result.defender = [ordered]@{
    available = (Test-Path $mp)
    amServiceEnabled = $status.AMServiceEnabled
    antivirusEnabled = $status.AntivirusEnabled
    realTimeProtection = $status.RealTimeProtectionEnabled
    engine = $status.AMEngineVersion
    signatures = $status.AntivirusSignatureVersion
    signaturesUpdated = "$($status.AntivirusSignatureLastUpdated)"
    scans = @()
}
if (Test-Path $mp) {
    foreach ($target in @($full, $result.full.zip, $result.full.sevenZip, $result.full.setup, $result.lean.setup)) {
        $out = & $mp -Scan -ScanType 3 -File $target -DisableRemediation 2>&1 | Out-String
        $code = $LASTEXITCODE
        $result.defender.scans += [ordered]@{ target = (Split-Path $target -Leaf); exitCode = $code; verdict = $(if ($code -eq 0) { 'no threats' } elseif ($code -eq 2) { 'THREAT FOUND' } else { 'scan error' }); output = ($out.Trim() -split "`r?`n" | Select-Object -Last 3) -join ' | ' }
    }
}

foreach ($k in 'full', 'lean') { foreach ($f in 'zip', 'sevenZip', 'setup') { $result[$k].Remove($f) } }
$result | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $OutDir "size-$Variant.json") -Encoding utf8
Write-Host ($result | ConvertTo-Json -Depth 6)
