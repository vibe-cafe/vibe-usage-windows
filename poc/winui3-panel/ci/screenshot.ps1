# Runs the published sample on the Actions Windows runner and screenshots the
# open panel in dark and light themes. Runner-only: it flips the runner
# account's theme in the registry. Never run this on a real machine.
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$logDir = Join-Path $OutDir 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$exe = Join-Path $AppDir 'VibeUsagePanelPoc.exe'

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class PocNative {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeoutW(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam,
        uint flags, uint timeout, out UIntPtr result);
}
"@
[PocNative]::SetProcessDPIAware() | Out-Null

# Tall enough for the expanded panel (1068 DIP) when the runner allows it.
foreach ($mode in @(@(1920, 1200), @(1920, 1080))) {
    try {
        Set-DisplayResolution -Width $mode[0] -Height $mode[1] -Force -ErrorAction Stop
        Start-Sleep -Seconds 3
    } catch {
        Write-Host "display resolution $($mode[0])x$($mode[1]) refused: $($_.Exception.Message)"
        continue
    }
    $vc = Get-CimInstance Win32_VideoController | Select-Object -First 1
    Write-Host "asked $($mode[0])x$($mode[1]), got $($vc.CurrentHorizontalResolution)x$($vc.CurrentVerticalResolution)"
    if ($vc.CurrentHorizontalResolution -eq $mode[0] -and $vc.CurrentVerticalResolution -eq $mode[1]) { break }
}
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
Write-Host "screen: $($screen.Width)x$($screen.Height) at $($screen.Left),$($screen.Top)"
Write-Host "os: $([Environment]::OSVersion.VersionString)"
Write-Host "explorer running: $([bool](Get-Process explorer -ErrorAction SilentlyContinue))"

function Set-Theme([bool]$light) {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
    if (-not (Test-Path $key)) { New-Item -Path $key -Force | Out-Null }
    $v = [int]$light
    Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value $v -Type DWord
    Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value $v -Type DWord
    Set-ItemProperty -Path $key -Name EnableTransparency -Value 1 -Type DWord
    $r = [UIntPtr]::Zero
    [PocNative]::SendMessageTimeoutW([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'ImmersiveColorSet', 2, 5000, [ref]$r) | Out-Null
}

function Save-Screen([string]$name, [string]$rectFile) {
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    } catch {
        Write-Host "::warning::screen capture failed for ${name}: $($_.Exception.Message)"
        $g.Dispose(); $bmp.Dispose(); return
    }
    $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name-full.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    if ($rectFile -and (Test-Path $rectFile)) {
        $p = (Get-Content $rectFile -Raw).Trim().Split(' ') | ForEach-Object { [int]$_ }
        Write-Host "${name}: panel rect $($p -join ' ')"
        $pad = 0
        $x = [Math]::Max($b.Left, $p[0] - $pad)
        $y = [Math]::Max($b.Top, $p[1] - $pad)
        $right = [Math]::Min($b.Right, $p[0] + $p[2] + $pad)
        $bottom = [Math]::Min($b.Bottom, $p[1] + $p[3] + $pad)
        if ($right -gt $x -and $bottom -gt $y) {
            $crop = $bmp.Clone([System.Drawing.Rectangle]::new($x - $b.Left, $y - $b.Top, $right - $x, $bottom - $y), $bmp.PixelFormat)
            $crop.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
            $crop.Dispose()
        }
    } else {
        Write-Host "::warning::${name}: no panel rect (the panel did not open?)"
    }
    $bmp.Dispose()
}

function Invoke-Case([string]$name, [bool]$light, [string[]]$extra, [scriptblock]$after) {
    Set-Theme $light
    Start-Sleep -Seconds 2
    $rect = Join-Path $OutDir "$name.rect.txt"
    $appArgs = @('--open', '--pin', "--rect-file=$rect", "--log-dir=$logDir") + $extra
    $proc = Start-Process -FilePath $exe -ArgumentList $appArgs -PassThru
    Start-Sleep -Seconds 10
    if ($proc.HasExited) { Write-Host "::warning::${name}: app exited early, code $($proc.ExitCode)" }
    Save-Screen $name $rect
    if ($after) {
        & $after
        Save-Screen "$name-then" $rect
    }
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
    Start-Sleep -Seconds 2
}

# The Mac new panel's states, for side-by-side comparison (the panel is dark only).
Invoke-Case '1-collapsed' $false @()
Invoke-Case '2-expanded' $false @('--expand')
Invoke-Case '2b-expanded-end' $false @('--expand', '--scroll-end')
Invoke-Case '3-single' $false @('--only=claude')
Invoke-Case '4-detail' $false @('--tab=claude')

$log = Join-Path $logDir 'panel-poc.log'
if (Test-Path $log) {
    Write-Host '----- panel-poc.log -----'
    Get-Content $log | ForEach-Object { Write-Host $_ }
} else {
    Write-Host '::warning::no log written'
}
