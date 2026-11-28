# Clicks the real tray icon on the Actions runner and checks, from the app's
# own log, that the panel opens, closes on a click elsewhere, toggles on the
# icon without popping back, closes on Esc, and that the right-click menu
# shows. Synthetic mouse/keyboard input is injected on the runner VM only.
# A Windows Server VM is not a real desktop; treat this as a smoke test, the
# real-machine checklist still decides.
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = 'Stop'
$logDir = Join-Path $OutDir 'interaction-logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$log = Join-Path $logDir 'panel-poc.log'
$exe = Join-Path $AppDir 'VibeUsagePanelPoc.exe'

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class PocInput {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string name);
    [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT rect);
    public static int[] IconRect(IntPtr host) {
        var id = new NOTIFYICONIDENTIFIER { cbSize = (uint)Marshal.SizeOf(typeof(NOTIFYICONIDENTIFIER)), hWnd = host, uID = 1 };
        RECT r;
        int hr = Shell_NotifyIconGetRect(ref id, out r);
        return hr == 0 ? new[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top } : null;
    }
}
"@
[PocInput]::SetProcessDPIAware() | Out-Null

function Click([int]$x, [int]$y, [switch]$Right, [int]$HoldMs = 60) {
    [PocInput]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 120
    $down = if ($Right) { 0x08 } else { 0x02 }
    $up = if ($Right) { 0x10 } else { 0x04 }
    [PocInput]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $HoldMs
    [PocInput]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}

function Key([byte]$vk) {
    [PocInput]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 40
    [PocInput]::keybd_event($vk, 0, 2, [UIntPtr]::Zero)
}

function Save-Shot([string]$name) {
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

$script:seen = 0
function New-LogLines {
    if (-not (Test-Path $log)) { return @() }
    $all = @(Get-Content $log)
    $new = if ($all.Count -gt $script:seen) { $all[$script:seen..($all.Count - 1)] } else { @() }
    $script:seen = $all.Count
    return $new
}

$results = New-Object System.Collections.Generic.List[object]
function Check([string]$step, [string[]]$lines, [string]$mustMatch, [string]$mustNotMatch = $null) {
    $ok = [bool]($lines | Where-Object { $_ -match $mustMatch })
    if ($ok -and $mustNotMatch) { $ok = -not [bool]($lines | Where-Object { $_ -match $mustNotMatch }) }
    $results.Add([pscustomobject]@{ Step = $step; Pass = $ok })
    Write-Host ("[{0}] {1}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' })), $step)
    $lines | ForEach-Object { Write-Host "      $_" }
}

# Dark taskbar and apps for this run.
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value 0 -Type DWord
Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value 0 -Type DWord

$proc = Start-Process -FilePath $exe -ArgumentList @('--autostart', "--log-dir=$logDir") -PassThru
Start-Sleep -Seconds 6
New-LogLines | Out-Null

$host_ = [PocInput]::FindWindowW('VibeUsagePanelPoc.TrayHost', $null)
$rect = [PocInput]::IconRect($host_)
if (-not $rect) {
    # Windows 11 puts new icons in the overflow; promote ours like the Settings toggle does.
    Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | ForEach-Object {
        $p = Get-ItemProperty $_.PSPath
        if ($p.ExecutablePath -and $p.ExecutablePath -like '*VibeUsagePanelPoc.exe') {
            Set-ItemProperty -Path $_.PSPath -Name IsPromoted -Value 1 -Type DWord
            Write-Host "promoted tray icon ($($_.PSChildName))"
        }
    }
    Start-Sleep -Seconds 3
    $rect = [PocInput]::IconRect($host_)
}
if (-not $rect) {
    Write-Host '::warning::tray icon rect unavailable; skipping interaction test'
    Save-Shot '9-no-tray-icon'
    Stop-Process -Id $proc.Id -Force
    exit 0
}
$ix = $rect[0] + [int]($rect[2] / 2)
$iy = $rect[1] + [int]($rect[3] / 2)
Write-Host "tray icon at $($rect -join ' ') -> click ($ix,$iy)"

# A. click the icon: opens
Click $ix $iy
Start-Sleep -Milliseconds 1500
Save-Shot '5-click-icon-open'
Check 'A 点托盘图标：面板弹出' (New-LogLines) 'show source=Mouse'

# B. click the desktop: closes
Click 200 150
Start-Sleep -Milliseconds 1200
Check 'B 点桌面：面板收起' (New-LogLines) 'hide reason=(deactivated|foreground-changed)'

# C. open, then click the icon again: closes and does not pop back
Click $ix $iy
Start-Sleep -Milliseconds 1200
Click $ix $iy
Start-Sleep -Milliseconds 1500
$lines = New-LogLines
Save-Shot '6-click-icon-again-closed'
Check 'C 再点图标：收起' $lines 'hide reason='
$shows = @($lines | Where-Object { $_ -match 'show source=Mouse' }).Count
$hides = @($lines | Where-Object { $_ -match 'hide reason=' }).Count
$results.Add([pscustomobject]@{ Step = "C' 开 $shows 次 / 关 $hides 次（应各 1 次）"; Pass = ($shows -eq 1 -and $hides -eq 1) })

# D. slow press-and-hold on the icon while open: still closes
Click $ix $iy
Start-Sleep -Milliseconds 1200
New-LogLines | Out-Null
Click $ix $iy -HoldMs 900
Start-Sleep -Milliseconds 1500
$lines = New-LogLines
$reopened = [bool]($lines | Where-Object { $_ -match 'show source=Mouse' })
Write-Host ($lines -join "`n")
$results.Add([pscustomobject]@{ Step = 'D 按住图标 0.9 秒再松开：收起不重开'; Pass = (-not $reopened -and [bool]($lines | Where-Object { $_ -match 'hide reason=' })) })

# E. open, press Esc: closes
Click $ix $iy
Start-Sleep -Milliseconds 1200
Key 0x1B
Start-Sleep -Milliseconds 1000
Check 'E 按 Esc：面板收起' (New-LogLines) 'hide reason=esc'

# F. six clicks one second apart: ends closed
for ($i = 0; $i -lt 6; $i++) { Click $ix $iy; Start-Sleep -Milliseconds 1000 }
Start-Sleep -Milliseconds 800
$lines = New-LogLines
$shows = @($lines | Where-Object { $_ -match 'show source=Mouse' }).Count
$hides = @($lines | Where-Object { $_ -match 'hide reason=' }).Count
$results.Add([pscustomobject]@{ Step = "F 隔 1 秒连点 6 次：开 $shows 次 / 关 $hides 次（应各 3 次）"; Pass = ($shows -eq 3 -and $hides -eq 3) })

# G. right-click: native menu
Click $ix $iy -Right
Start-Sleep -Milliseconds 1200
Save-Shot '7-right-click-menu'
Key 0x1B
Start-Sleep -Milliseconds 600
Check 'G 右键：原生菜单' (New-LogLines) 'menu: owner foreground=True'

Stop-Process -Id $proc.Id -Force
Write-Host ''
Write-Host '----- summary -----'
$results | ForEach-Object { Write-Host ("{0}  {1}" -f ($(if ($_.Pass) { 'PASS' } else { 'FAIL' })), $_.Step) }
$results | ConvertTo-Json | Set-Content (Join-Path $OutDir 'interaction-results.json') -Encoding utf8
Write-Host '----- interaction log -----'
Get-Content $log | ForEach-Object { Write-Host $_ }
