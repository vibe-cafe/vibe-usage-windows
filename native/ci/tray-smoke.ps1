# Clicks the real tray icon on the Actions runner and judges each step from
# the app's own log: open and close, the click race, the 「^」 overflow,
# clicks on the taskbar, Esc (decision 4: no global hotkey), hover, tooltip
# and wheel on the non-activated panel, 展开详情 growing away from the
# taskbar, the right-click menu and its actions, second starts, quitting,
# end of session and an Explorer restart. A Windows Server VM is not a real
# desktop: this is a smoke test, the real-machine checklist still decides.
# Runner only (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Tag = '100'
)

. "$PSScriptRoot/common.ps1"
$script:Exe = Join-Path $AppDir 'vibe-usage-app.exe'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$logDir = Join-Path $OutDir "logs-tray-$Tag"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$log = Join-Path $logDir 'shell.log'
$iconFile = Join-Path $OutDir "icon-rect-$Tag.txt"
$rectFile = Join-Path $OutDir "panel-rect-$Tag.txt"
Use-Log $log

# 1080 lines: the expanded panel is taller than the work area, so the middle scrolls.
Set-Resolution 1920 1080
Set-DarkTheme
Stop-App
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$work = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
Write-Host "work area: $($work.Left),$($work.Top) - $($work.Right),$($work.Bottom)"
$base = @("--test-log-dir=$logDir", "--test-icon-file=$iconFile", "--test-rect-file=$rectFile")

function Get-PanelRect {
    $n = Read-Numbers $rectFile
    if (-not $n) { return $null }
    return @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4])
}
function Get-IconCenter {
    $r = Read-Numbers $iconFile
    if (-not $r) { return $null }
    return @([int]($r[0] + $r[2] / 2), [int]($r[1] + $r[3] / 2))
}
function Wait-IconCenter([int]$TimeoutMs = 10000) {
    $end = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $end) {
        $c = Get-IconCenter
        if ($c) { return $c }
        Pump 250
    }
    return $null
}
function Get-Dpi([string[]]$Lines) {
    $m = $Lines | Select-String -Pattern 'show source=.* dpi=(\d+)' | Select-Object -Last 1
    if ($m) { return [int]$m.Matches[0].Groups[1].Value }
    return 96
}
function Any([string[]]$Lines, [string]$Pattern) { return [bool]($Lines | Where-Object { $_ -match $Pattern }) }
function Count([string[]]$Lines, [string]$Pattern) { return @($Lines | Where-Object { $_ -match $Pattern }).Count }
function Set-Promoted([int]$Value) {
    $done = $false
    Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | ForEach-Object {
        $p = Get-ItemProperty $_.PSPath
        if ($p.ExecutablePath -and $p.ExecutablePath -like '*vibe-usage-app.exe') {
            Set-ItemProperty -Path $_.PSPath -Name IsPromoted -Value $Value -Type DWord
            $done = $true
        }
    }
    return $done
}
function Close-Panel { Click 200 150; Pump 1000; New-LogLines | Out-Null }

# --- Tray icon add, with two simulated failures -------------------------------------------
Set-Promoted 0 | Out-Null
$proc = Start-App ($base + @('--autostart', '--test-tray-add-failures=2'))
$ok = Wait-LogLine 'tray: added after 2 failed attempt' 10000
Add-Result 'T1 托盘图标添加失败两次后自动重试成功' $ok (New-LogLines)

# --- 「^」 overflow (Windows 11 puts new icons there) ----------------------------------------
Pump 1500
$uia = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot/find-chevron.ps1" 2>&1 | ForEach-Object { "$_" })
$chev = $uia | Where-Object { $_ -match '^(\d+ \d+|none)$' } | Select-Object -Last 1
$onTaskbar = Get-IconCenter
Write-Host "chevron: $chev; icon rect while the overflow is closed: $(if ($onTaskbar) { $onTaskbar -join ',' } else { 'none' })"
if ($chev -match '^(\d+) (\d+)$' -and -not $onTaskbar) {
    $cx = [int]$Matches[1]; $cy = [int]$Matches[2]
    Click $cx $cy
    Pump 1500
    $ic = Get-IconCenter
    if ($ic) {
        New-LogLines | Out-Null
        Click $ic[0] $ic[1]
        Pump 2000
        $l = New-LogLines
        $pr = Get-PanelRect
        $m = [Math]::Round(12 * (Get-Dpi $l) / 96)
        $gap = if ($pr) { $work.Bottom - ($pr[1] + $pr[3]) } else { -1 }
        $pass = (Any $l 'show source=TrayMouse.*overflow=True') -and -not (Any $l 'hide reason=') -and [Math]::Abs($gap - $m) -le 1
        Add-Result "O1 「^」里点图标：面板贴任务栏弹出（离任务栏 $gap / 应为 $m 像素），不一闪就没" $pass $l
        Save-Screen (Join-Path $OutDir "$Tag-overflow-open.png")
        Click $cx $cy
        Pump 1500
        Check 'O2 面板开着时再点「^」：面板收起' (New-LogLines) 'hide reason='
        Key 0x1B
        Pump 800
    } else {
        Add-Result 'O1 「^」里点图标（打开「^」后没拿到图标位置）' $false $uia
        Add-Result 'O2 面板开着时再点「^」：面板收起（未测）' $false
    }
} else {
    Add-Result "O1 「^」里点图标（「^」按钮：$chev，图标已在任务栏：$([bool]$onTaskbar)）" $false $uia
    Add-Result 'O2 面板开着时再点「^」：面板收起（未测）' $false
}

# --- Icon on the taskbar for the rest (the user can drag it out; Explorer reads this at add) --
Stop-App
Set-Promoted 1 | Out-Null
$proc = Start-App ($base + @('--autostart'))
$ic = Wait-IconCenter 15000
if (-not $ic) {
    Add-Result '托盘图标位置拿不到，后续步骤跳过' $false (New-LogLines)
    Save-Screen (Join-Path $OutDir "$Tag-no-tray-icon.png")
    Write-Summary (Join-Path $OutDir "tray-results-$Tag.json")
    exit 1
}
$ix = $ic[0]; $iy = $ic[1]
Write-Host "tray icon at ($ix,$iy)"
Pump 1000
New-LogLines | Out-Null

# A, A', H: a click opens it next to the icon without taking the foreground.
Click $ix $iy
Pump 1500
$l = New-LogLines
Check 'A 点托盘图标：面板弹出' $l 'show source=TrayMouse'
$dpi = Get-Dpi $l
$s = $dpi / 96
$m = [Math]::Round(12 * $s)
$pr = Get-PanelRect
if ($pr) {
    $bottomGap = $work.Bottom - ($pr[1] + $pr[3]); $rightGap = $work.Right - ($pr[0] + $pr[2])
    Add-Result "A' 面板离任务栏 $bottomGap、离屏幕右边 $rightGap 像素（应各为 12 DIP = $m）" ([Math]::Abs($bottomGap - $m) -le 1 -and [Math]::Abs($rightGap - $m) -le 1)
}
Add-Result 'H 鼠标打开不抢前台（输入法中/英不受影响）' (Any $l 'show source=TrayMouse.*isForeground=False')
Save-Screen (Join-Path $OutDir "$Tag-5-click-open.png")

# E: decision 4, no global Esc hotkey.
Key 0x1B
Pump 1000
Add-Result 'E 鼠标打开时按 Esc 不收起（决定 4：不注册全局热键）' (-not (Any (New-LogLines) 'hide reason='))

# V: hover and tooltip on the non-activated panel (the power button, bottom right).
$px = $pr[0] + $pr[2] - [int](28 * $s) - [int](6 * $s)
$py = $pr[1] + $pr[3] - [int](19 * $s)
Move-Cursor ($px - 40) ($py - 60)
Pump 300
Move-Cursor $px $py
Pump 3000
$l = New-LogLines
Add-Result 'V 不激活时悬停有反应并出提示' ((Any $l 'panel hover quit') -and (Any $l 'panel tooltip quit')) $l
Save-Screen (Join-Path $OutDir "$Tag-6-hover-tooltip.png")

# B: a click on the desktop closes it.
Click 200 150
Pump 1200
Check 'B 点桌面：面板收起' (New-LogLines) 'hide reason=(deactivated|foreground-changed)'

# TB: the taskbar is already the foreground after a tray open; a click on its empty part closes.
$tbx = [int]($bounds.Left + $bounds.Width * 0.70)
$tby = [int](($work.Bottom + $bounds.Bottom) / 2)
Click $ix $iy
Pump 1200
New-LogLines | Out-Null
Move-Cursor $tbx $tby
Pump 1200
Add-Result "TB' 鼠标划过任务栏：面板不收起" (-not (Any (New-LogLines) 'hide reason='))
Click $tbx $tby
Pump 1500
Check 'TB 点任务栏空白处：面板收起' (New-LogLines) 'hide reason='

# C: a second click on the icon closes it and it does not pop back.
Click $ix $iy
Pump 1200
Click $ix $iy
Pump 1500
$l = New-LogLines
Check 'C 再点图标：收起' $l 'hide reason='
$shows = Count $l 'show source=TrayMouse'; $hides = Count $l 'hide reason='
Add-Result "C' 开 $shows 次 / 关 $hides 次（应各 1 次）" ($shows -eq 1 -and $hides -eq 1)

# D: slow press-and-hold on the icon while open.
Click $ix $iy
Pump 1200
New-LogLines | Out-Null
Click $ix $iy -HoldMs 900
Pump 1500
$l = New-LogLines
Add-Result 'D 按住图标 0.9 秒再松开：收起不重开' ((-not (Any $l 'show source=')) -and (Any $l 'hide reason=')) $l

# F: six clicks one second apart end closed.
for ($i = 0; $i -lt 6; $i++) { Click $ix $iy; Pump 1000 }
Pump 800
$l = New-LogLines
$shows = Count $l 'show source=TrayMouse'; $hides = Count $l 'hide reason='
Add-Result "F 隔 1 秒连点 6 次：开 $shows 次 / 关 $hides 次（应各 3 次）" ($shows -eq 3 -and $hides -eq 3)

# X: 展开详情 inside the non-activated panel: it stays open and grows away from the taskbar.
Click $ix $iy
Pump 1500
New-LogLines | Out-Null
$before = Get-PanelRect
Click ($before[0] + [int](48 * $s)) ($before[1] + $before[3] - [int](19 * $s))
Pump 1500
$l = New-LogLines
$after = Get-PanelRect
$pass = (Any $l 'details expanded') -and -not (Any $l 'hide reason=') -and
    [Math]::Abs(($after[1] + $after[3]) - ($before[1] + $before[3])) -le 1 -and $after[1] -lt $before[1]
Add-Result "X 点面板里的「展开详情」：不收起，底边不动往上长（$($before -join ',') → $($after -join ',')）" $pass $l
Save-Screen (Join-Path $OutDir "$Tag-7-expanded-in-place.png")

# W: the wheel scrolls the middle of the non-activated panel.
Move-Cursor ($after[0] + [int]($after[2] / 2)) ($after[1] + [int]($after[3] / 2))
Pump 300
Wheel -5
Pump 1200
Check 'W 不激活时滚轮能滚动中间区' (New-LogLines) 'panel scroll offset='
Close-Panel

# P: 「设置」 in the panel: the panel closes, settings comes to the front.
Click $ix $iy
Pump 1500
New-LogLines | Out-Null
$pr = Get-PanelRect
Click ($pr[0] + $pr[2] - [int](40 * $s)) ($pr[1] + [int](22 * $s))
Pump 2000
$l = New-LogLines
Add-Result 'P 点面板「设置」：面板收起，设置窗口到最前面' ((Any $l 'hide reason=open-settings') -and (Any $l 'settings shown foreground=True')) $l
Save-Screen (Join-Path $OutDir "$Tag-8-settings.png")
Key 0x73 @(0x12)
Pump 1000
Check "P' Alt+F4 关闭设置窗口只是隐藏" (New-LogLines) 'settings hidden'

# G: right-click: native menu against the taskbar's inner edge, at the click.
Click $ix $iy -Right
Pump 1200
Save-Screen (Join-Path $OutDir "$Tag-9-right-click-menu.png")
$menu = [VuNative]::FindWindowW('#32768', $null)
if ($menu -ne [IntPtr]::Zero) {
    $mr = [VuNative]::Rect($menu)
    Add-Result "G' 右键菜单底边贴着任务栏上沿、在点击处（菜单底 $($mr[3]) / 任务栏上沿 $($work.Bottom)）" ([Math]::Abs($mr[3] - $work.Bottom) -le 2 -and $mr[0] -le $ix + 2 -and $mr[2] -ge $ix - 2)
} else {
    Add-Result "G' 右键菜单窗口没找到" $false
}
Key 0x1B
Pump 800
Check 'G 右键：系统原生菜单' (New-LogLines) 'menu: owner foreground=True'

# M: the menu's actions.
Click $ix $iy -Right; Pump 1000; Key 0x28; Key 0x28; Key 0x0D; Pump 2500
$l = New-LogLines
Add-Result 'M1 菜单「立即同步」：只同步用量，不开面板' ((Any $l 'command=2') -and (Any $l 'sync requested') -and (Any $l 'sync label Syncing') -and -not (Any $l 'show source=')) $l
Click $ix $iy -Right; Pump 1000; Key 0x28; Key 0x0D; Pump 1500
Check 'M2 菜单「打开面板」：面板弹出' (New-LogLines) 'show source=Menu'
Close-Panel
Click $ix $iy -Right; Pump 1000; Key 0x28; Key 0x28; Key 0x28; Key 0x0D; Pump 2000
Check 'M3 菜单「设置」：设置窗口到最前面' (New-LogLines) 'settings shown foreground=True'
Key 0x73 @(0x12)
Pump 800
New-LogLines | Out-Null

# R: a second start of the exe (Start menu, shortcut, restart after an update).
$form = New-Object System.Windows.Forms.Form
$form.Text = 'previous window'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point(60, 520)
$form.Size = New-Object System.Drawing.Size(360, 240)
$form.Show()
Pump 800
$fx = 240; $fy = 640
Click $fx $fy
Pump 600
Start-App @("--test-log-dir=$logDir") | Out-Null
Pump 3500
$l = New-LogLines
Add-Result 'R1 再次启动：交给已运行的实例，面板打开并拿到键盘' ((Any $l 'second start: asked') -and (Any $l 'show source=Relaunch.*activate=True.*isForeground=True')) $l
Key 0x1B
Pump 1000
Check 'R2 再次启动打开后按 Esc：面板收起' (New-LogLines) 'hide reason=esc'
Click $fx $fy
Pump 600
Start-App @("--test-log-dir=$logDir") | Out-Null
Pump 3500
New-LogLines | Out-Null
Click $fx $fy
Pump 1500
Check 'R3 再次启动打开后点回原窗口：面板收起' (New-LogLines) 'hide reason=(deactivated|foreground-changed)'
$late = Start-App @("--test-log-dir=$logDir", '--autostart')
$exited = $late.WaitForExit(10000)
Pump 1000
$l = New-LogLines
Add-Result 'R4 带 --autostart 的晚到启动：不弹面板，自己退出' ($exited -and -not (Any $l 'show source=')) $l

# Q: 退出 from the menu closes the backend, then the process ends.
Click $ix $iy -Right; Pump 1000; Key 0x28; Key 0x28; Key 0x28; Key 0x28; Key 0x0D
$exited = $proc.WaitForExit(10000)
Add-Result 'Q 菜单「退出」：关闭后端后进程结束' ($exited -and (Wait-LogLine 'quit: backend closed in' 2000)) (New-LogLines)

# S: a start by the user opens the panel with the keyboard; clicking back to the previous window closes it.
Click $fx $fy
Pump 600
$proc = Start-App $base
Pump 5000
$l = New-LogLines
Add-Result 'S1 手动启动：面板打开并拿到键盘' (Any $l 'show source=Startup.*activate=True.*isForeground=True') $l
Click $fx $fy
Pump 1500
Check 'S2 手动启动后点回原窗口：面板收起' (New-LogLines) 'hide reason=(deactivated|foreground-changed)'

# P2: the footer's power button quits.
$ic = Wait-IconCenter 5000
if ($ic) { $ix = $ic[0]; $iy = $ic[1] }
Click $ix $iy
Pump 1500
$pr = Get-PanelRect
Click ($pr[0] + $pr[2] - [int](34 * $s)) ($pr[1] + $pr[3] - [int](19 * $s))
$exited = $proc.WaitForExit(10000)
Add-Result 'P2 底栏电源键：退出' ($exited -and (Wait-LogLine 'click quit' 1000)) (New-LogLines)

# ES: shutdown or sign-out: the app closes the backend and leaves in time.
$proc = Start-App ($base + @('--autostart'))
Pump 5000
New-LogLines | Out-Null
$trayHost = [VuNative]::FindWindowW('VibeUsage.Native.TrayHost', $null)
$r = [IntPtr]::Zero
[VuNative]::SendMessageTimeoutW($trayHost, 0x11, [IntPtr]::Zero, [IntPtr]::Zero, 2, 5000, [ref]$r) | Out-Null
[VuNative]::SendMessageTimeoutW($trayHost, 0x16, [IntPtr]1, [IntPtr]::Zero, 2, 5000, [ref]$r) | Out-Null
$exited = $proc.WaitForExit(8000)
Add-Result 'ES 关机或注销：关闭后端后及时退出' ($exited -and (Wait-LogLine 'session ending: backend closed=True' 2000)) (New-LogLines)

# EX: Explorer restarts: the icon comes back.
$proc = Start-App ($base + @('--autostart'))
Pump 5000
New-LogLines | Out-Null
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Pump 8000
if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) { Start-Process explorer.exe; Pump 8000 }
$ok = Wait-LogLine 'TaskbarCreated' 15000
Pump 1500
$l = New-LogLines
Add-Result 'EX Explorer 重启后托盘图标自动回来' ($ok -and (Any $l 'tray: added')) $l

Stop-App
$form.Close()
Write-Summary (Join-Path $OutDir "tray-results-$Tag.json")
Write-Host '----- shell.log -----'
if (Test-Path $log) { Get-Content $log -Encoding utf8 | ForEach-Object { Write-Host $_ } }
if ($script:Results | Where-Object { -not $_.Pass }) { exit 1 }
