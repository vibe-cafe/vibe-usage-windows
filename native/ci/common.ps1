# Shared helpers for the native shell's CI jobs (Actions Windows runner only).
# They inject synthetic mouse and keyboard input into the runner VM and change
# the runner account's theme and display settings. Never run them on a real
# machine. The app itself uses none of these input APIs.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
if (-not ('VuNative' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class VuNative {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string name);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool AllowSetForegroundWindow(uint processId);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW")]
    public static extern IntPtr SendMessageTimeoutString(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
    public static int[] Rect(IntPtr h) { RECT r; return GetWindowRect(h, out r) ? new[] { r.Left, r.Top, r.Right, r.Bottom } : null; }
}
"@
}
# Per-monitor aware, so coordinates are physical pixels at any scale, like the app's.
if (-not [VuNative]::SetProcessDpiAwarenessContext([IntPtr](-4))) { [VuNative]::SetProcessDPIAware() | Out-Null }

$script:Exe = $null
$script:Results = New-Object System.Collections.Generic.List[object]

function Pump([int]$Ms) {
    # Keeps this process's test window responsive while waiting.
    $end = [DateTime]::UtcNow.AddMilliseconds($Ms)
    while ([DateTime]::UtcNow -lt $end) {
        [System.Windows.Forms.Application]::DoEvents()
        Start-Sleep -Milliseconds 15
    }
}

# A bare SetCursorPos produces no pointer input for WinUI: move with real
# (absolute) mouse input so hover reaches the window under the cursor.
function Move-Cursor([int]$X, [int]$Y) {
    $v = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $nx = [int][Math]::Round(($X - $v.Left) * 65535.0 / ($v.Width - 1))
    $ny = [int][Math]::Round(($Y - $v.Top) * 65535.0 / ($v.Height - 1))
    # MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK
    [VuNative]::mouse_event(0xC001, $nx, $ny, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 30
    [VuNative]::SetCursorPos($X, $Y) | Out-Null
}

# PowerShell turns $null into "" for string parameters; FindWindow needs a real null title.
function Find-Window([string]$Class) { return [VuNative]::FindWindowW($Class, [NullString]::Value) }

function Click([int]$X, [int]$Y, [switch]$Right, [int]$HoldMs = 60) {
    Move-Cursor $X $Y
    Pump 120
    $down = if ($Right) { 0x08 } else { 0x02 }
    $up = if ($Right) { 0x10 } else { 0x04 }
    [VuNative]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
    Pump $HoldMs
    [VuNative]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}

function Wheel([int]$Notches) {
    for ($i = 0; $i -lt [Math]::Abs($Notches); $i++) {
        [VuNative]::mouse_event(0x0800, 0, 0, ($(if ($Notches -lt 0) { -120 } else { 120 })), [UIntPtr]::Zero)
        Pump 80
    }
}

function Key([byte]$Vk, [byte[]]$With = @()) {
    foreach ($m in $With) { [VuNative]::keybd_event($m, 0, 0, [UIntPtr]::Zero) }
    [VuNative]::keybd_event($Vk, 0, 0, [UIntPtr]::Zero)
    Pump 40
    [VuNative]::keybd_event($Vk, 0, 2, [UIntPtr]::Zero)
    foreach ($m in $With) { [VuNative]::keybd_event($m, 0, 2, [UIntPtr]::Zero) }
}

function Set-DarkTheme {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
    if (-not (Test-Path $key)) { New-Item -Path $key -Force | Out-Null }
    Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value 0 -Type DWord
    Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value 0 -Type DWord
    $r = [UIntPtr]::Zero
    [VuNative]::SendMessageTimeoutString([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'ImmersiveColorSet', 2, 5000, [ref]$r) | Out-Null
}

# Tries each "WxH" in turn until the runner reports it (the default is 1024x768).
function Set-Resolution([string[]]$Modes) {
    foreach ($mode in $Modes) {
        $w, $h = $mode.Split('x') | ForEach-Object { [int]$_ }
        try {
            Set-DisplayResolution -Width $w -Height $h -Force -ErrorAction Stop
            Start-Sleep -Seconds 3
        } catch {
            Write-Host "display resolution $mode refused: $($_.Exception.Message)"
            continue
        }
        $vc = Get-CimInstance Win32_VideoController | Select-Object -First 1
        Write-Host "asked $mode, got $($vc.CurrentHorizontalResolution)x$($vc.CurrentVerticalResolution)"
        if ($vc.CurrentHorizontalResolution -eq $w -and $vc.CurrentVerticalResolution -eq $h) { break }
    }
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    Write-Host "screen $($b.Width)x$($b.Height)"
}

function Stop-App {
    Get-Process -Name 'vibe-usage-app' -ErrorAction SilentlyContinue | ForEach-Object {
        try { $_.Kill() } catch { }
        try { $_.WaitForExit(5000) | Out-Null } catch { }
    }
    Start-Sleep -Milliseconds 500
}

function Start-App([string[]]$AppArgs) {
    return Start-Process -FilePath $script:Exe -ArgumentList $AppArgs -PassThru
}

# Log reading: each check looks at the lines written since the previous call.
$script:LogFile = $null
$script:Seen = 0
function Use-Log([string]$Path) { $script:LogFile = $Path; $script:Seen = 0 }
function New-LogLines {
    if (-not $script:LogFile -or -not (Test-Path $script:LogFile)) { return @() }
    $all = @(Get-Content $script:LogFile -Encoding utf8)
    $new = if ($all.Count -gt $script:Seen) { $all[$script:Seen..($all.Count - 1)] } else { @() }
    $script:Seen = $all.Count
    return $new
}
function Wait-LogLine([string]$Pattern, [int]$TimeoutMs = 5000) {
    $end = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $end) {
        if ($script:LogFile -and (Test-Path $script:LogFile) -and (Select-String -Path $script:LogFile -Pattern $Pattern -Quiet)) { return $true }
        Pump 100
    }
    return $false
}

function Add-Result([string]$Step, [bool]$Pass, [string[]]$Lines = @()) {
    $script:Results.Add([pscustomobject]@{ Step = $Step; Pass = $Pass })
    Write-Host ("[{0}] {1}" -f ($(if ($Pass) { 'PASS' } else { 'FAIL' })), $Step)
    $Lines | ForEach-Object { Write-Host "      $_" }
}

function Check([string]$Step, [string[]]$Lines, [string]$MustMatch, [string]$MustNotMatch = $null) {
    $ok = [bool]($Lines | Where-Object { $_ -match $MustMatch })
    if ($ok -and $MustNotMatch) { $ok = -not [bool]($Lines | Where-Object { $_ -match $MustNotMatch }) }
    Add-Result $Step $ok $Lines
}

function Read-Numbers([string]$File) {
    if (-not (Test-Path $File)) { return $null }
    $txt = (Get-Content $File -Raw).Trim()
    if (-not $txt -or $txt -eq 'none') { return $null }
    return @($txt.Split(' ') | ForEach-Object { [long]$_ })
}

# Saves the window with PrintWindow (PW_RENDERFULLCONTENT, so other windows
# on top don't matter), cropped to the visible frame, at original size.
function Save-Window([long]$Hwnd, [int[]]$Visible, [string]$Path) {
    $h = [IntPtr]$Hwnd
    $wr = [VuNative]::Rect($h)
    if (-not $wr) { Write-Host "::warning::no window rect for $Path"; return $false }
    $w = $wr[2] - $wr[0]; $ht = $wr[3] - $wr[1]
    $bmp = New-Object System.Drawing.Bitmap $w, $ht, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [VuNative]::PrintWindow($h, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    if ($ok) {
        $crop = [System.Drawing.Rectangle]::new($Visible[0] - $wr[0], $Visible[1] - $wr[1], $Visible[2], $Visible[3])
        $crop.Intersect([System.Drawing.Rectangle]::new(0, 0, $w, $ht))
        $c = $bmp.Clone($crop, $bmp.PixelFormat)
        $c.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $c.Dispose()
        Write-Host "saved $Path ($($crop.Width)x$($crop.Height))"
    } else {
        Write-Host "::warning::PrintWindow failed for $Path"
    }
    $bmp.Dispose()
    return $ok
}

function Save-Screen([string]$Path) {
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try { $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size) } catch { Write-Host "::warning::screen capture failed: $($_.Exception.Message)" }
    $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function Write-Summary([string]$OutFile) {
    Write-Host ''
    Write-Host '----- summary -----'
    $script:Results | ForEach-Object { Write-Host ("{0}  {1}" -f ($(if ($_.Pass) { 'PASS' } else { 'FAIL' })), $_.Step) }
    $script:Results | ConvertTo-Json | Set-Content $OutFile -Encoding utf8
}
