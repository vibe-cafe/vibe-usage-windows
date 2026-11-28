# Drives the 订阅配额 card with real (injected) pointer and keyboard input on
# the fake backend and judges each step from the app's log and from UI
# Automation (the names Narrator reads): round buttons and their tooltips,
# opening a subscription, the allowance tooltip, 「刷新额度」 spinning and
# coming back, 「另有 N 项」, dragging round buttons into a new order, and the
# keyboard. Screens of the live states go next to the scene screenshots.
# Runner only (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir
)

. "$PSScriptRoot/common.ps1"
$script:Exe = Join-Path $AppDir 'vibe-usage-app.exe'
$shots = Join-Path $OutDir 'quota-smoke'
$logDir = Join-Path $OutDir 'logs-quota-smoke'
New-Item -ItemType Directory -Force -Path $shots, $logDir | Out-Null
$log = Join-Path $logDir 'shell.log'
$rectFile = Join-Path $shots 'panel-rect.txt'
Use-Log $log
Set-Resolution @('1920x1200', '1920x1080')
Set-DarkTheme

function Elements([int]$ProcessId) {
    $file = Join-Path $shots 'uia.txt'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot/ui-elements.ps1" -ProcessId $ProcessId -OutFile $file 2>&1 | ForEach-Object { Write-Host "uia: $_" }
    if (-not (Test-Path $file)) { return @() }
    return @(Get-Content $file -Encoding utf8 | ForEach-Object {
        $f = $_.Split("`t")
        [pscustomobject]@{ Type = $f[0]; Name = $f[1]; X = [int]$f[2]; Y = [int]$f[3]; W = [int]$f[4]; H = [int]$f[5]; Enabled = $f[6] -eq 'True'; Focusable = $f[7] -eq 'True' }
    })
}
function Any([string[]]$Lines, [string]$Pattern) { return [bool]($Lines | Where-Object { $_ -match $Pattern }) }
function Find($List, [string]$Pattern) { return $List | Where-Object { $_.Name -match $Pattern } | Select-Object -First 1 }
function Center($E) { return @([int]($E.X + $E.W / 2), [int]($E.Y + $E.H / 2)) }
function Shot([string]$Name) {
    $n = Read-Numbers $rectFile
    if ($n) { Save-Window $n[0] @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4]) (Join-Path $shots "$Name.png") | Out-Null }
}
function Drag([int[]]$From, [int[]]$To) {
    Move-Cursor $From[0] $From[1]
    Pump 200
    [VuNative]::mouse_event(0x02, 0, 0, 0, [UIntPtr]::Zero)
    Pump 150
    for ($i = 1; $i -le 14; $i++) {
        Move-Cursor ([int]($From[0] + ($To[0] - $From[0]) * $i / 14)) ([int]($From[1] + ($To[1] - $From[1]) * $i / 14))
        Pump 40
    }
    Pump 200
    [VuNative]::mouse_event(0x04, 0, 0, 0, [UIntPtr]::Zero)
    Pump 600
}
function Start-Scene([string]$Scene) {
    Stop-App
    Remove-Item $rectFile -ErrorAction SilentlyContinue
    $p = Start-App @('--fake-backend', '--test-pin', "--test-demo=$Scene", "--test-rect-file=$rectFile", "--test-log-dir=$logDir")
    $end = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path $rectFile) -and [DateTime]::UtcNow -lt $end) { Pump 200 }
    Pump 1500
    New-LogLines | Out-Null
    return $p
}

# --- 131a: four subscriptions --------------------------------------------------------------
$proc = Start-Scene '131a'
$ui = Elements $proc.Id
$ui | ForEach-Object { Write-Host ("  {0,-10} {1}  ({2},{3} {4}x{5}) enabled={6} focusable={7}" -f $_.Type, $_.Name, $_.X, $_.Y, $_.W, $_.H, $_.Enabled, $_.Focusable) }
$names = @('^Claude Max · 偏快$', '^Codex Plus · 快用完$', '^Cursor Pro · 正常$', '^Grok · 正常$', '^刷新额度$', '^打开订阅配额设置$', '^概览$',
    '^Codex 超额 17%$', '^Claude 超额 8%$', '^Cursor 进度正常$', '^Grok 余量 17%$')
$missing = @($names | Where-Object { -not (Find $ui $_) })
Add-Result "Q1 讲述人名字：圆钮（名字 · 套餐 · 状态）、概览行（名字 + 评语）、刷新额度、齿轮都在，缺 $($missing.Count) 个" ($missing.Count -eq 0) $missing
$keyboard = @($ui | Where-Object { $_.Type -eq 'Button' -and $_.Name -match '^(Claude Max|Codex Plus|Cursor Pro|Grok) · |^刷新额度$|超额|进度正常|余量' -and -not $_.Focusable })
$rowButtons = @($ui | Where-Object { $_.Type -eq 'Button' -and $_.Name -match '超额|进度正常|余量' })
if ($rowButtons.Count -ne 4) { $keyboard += "概览行按钮 $($rowButtons.Count) 个，应为 4" }
Add-Result 'Q2 圆钮、概览行、刷新额度都能用键盘聚焦' ($keyboard.Count -eq 0) ($keyboard | ForEach-Object { $_.Name })

$codex = Find $ui '^Codex Plus · '
if ($codex) {
    $c = Center $codex
    Move-Cursor $c[0] $c[1]
    Pump 2500
    $l = New-LogLines
    Add-Result 'Q3 悬停圆钮：亮起，出系统提示' ((Any $l 'panel hover sub-Codex') -and (Any $l 'panel tooltip sub-Codex')) $l
    Shot 'hover-round-button'
    Click $c[0] $c[1]
    Pump 1200
    $l = New-LogLines
    $ui = Elements $proc.Id
    $row7d = Find $ui '^7d 剩余 79%'
    Add-Result 'Q4 点圆钮：进到 Codex，额度行讲述人可读' ((Any $l 'click sub-Codex') -and [bool]$row7d -and [bool](Find $ui '^5h 剩余 12%')) $l
    Shot 'detail-codex'
    if ($row7d) {
        $r = Center $row7d
        Move-Cursor $r[0] $r[1]
        Pump 1200
        $l = New-LogLines
        Add-Result 'Q5 悬停 7d 那一行：行亮起，下面挂提示' ((Any $l 'panel hover allowance-7d') -and (Any $l 'panel tooltip allowance-7d')) $l
        Shot 'hover-allowance-7d'
    }
    $refresh = Find $ui '^刷新额度$'
    if ($refresh) {
        $f = Center $refresh
        Click $f[0] $f[1]
        Pump 400
        Shot 'refreshing'
        $busy = Find (Elements $proc.Id) '^刷新额度$'
        Pump 4000
        $l = New-LogLines
        $back = Find (Elements $proc.Id) '^刷新额度$'
        Add-Result 'Q6 刷新额度：读取中转圈且不收点击，读完恢复（假后端读全部）' ((Any $l 'quota refresh requested scope=Codex') -and $busy -and -not $busy.Enabled -and $back -and $back.Enabled) $l
    }
    $overview = Find $ui '^概览$'
    if ($overview) { $o = Center $overview; Click $o[0] $o[1]; Pump 800; New-LogLines | Out-Null }
}

# Drag Claude past the last button, then Grok onto Codex.
$ui = Elements $proc.Id
$claude = Find $ui '^Claude Max · '; $grok = Find $ui '^Grok · '
if ($claude -and $grok) {
    Drag (Center $claude) @([int]($grok.X + $grok.W + 40), [int]($grok.Y + $grok.H / 2))
    Pump 800
    $l = New-LogLines
    $ui = Elements $proc.Id
    $claude2 = Find $ui '^Claude Max · '; $grok2 = Find $ui '^Grok · '
    Add-Result 'Q7 把 Claude 拖到最后一个圆钮右边：排到最后' ((Any $l 'drag start ClaudeCode') -and (Any $l 'move ClaudeCode before end') -and $claude2 -and $grok2 -and $claude2.X -gt $grok2.X) $l
    Shot 'dragged-claude-last'
    $codex2 = Find $ui '^Codex Plus · '
    if ($grok2 -and $codex2) {
        Drag (Center $grok2) (Center $codex2)
        Pump 800
        $l = New-LogLines
        $ui = Elements $proc.Id
        $grok3 = Find $ui '^Grok · '; $codex3 = Find $ui '^Codex Plus · '
        Add-Result 'Q8 把 Grok 拖到 Codex 上：排到 Codex 前面，不算点击' ((Any $l 'move Grok before Codex') -and -not (Any $l 'click sub-Grok') -and $grok3.X -lt $codex3.X) $l
    }
}

# Keyboard: the panel took the keyboard at start; Tab from 概览 reaches the first round button.
$ui = Elements $proc.Id
$overview = Find $ui '^概览$'
if ($overview) {
    $o = Center $overview
    Click $o[0] $o[1]
    Pump 600
    New-LogLines | Out-Null
    Key 0x09
    Pump 300
    Key 0x0D
    Pump 800
    $l = New-LogLines
    Add-Result 'Q9 键盘：从概览按 Tab 到第一个圆钮，回车打开' (Any $l 'click sub-') $l
}

# --- 132k: 另有 1 项 ------------------------------------------------------------------------
$proc = Start-Scene '132k'
$ui = Elements $proc.Id
$fold = Find $ui '^另有 1 项$'
if ($fold) {
    $f = Center $fold
    Click $f[0] $f[1]
    Pump 900
    $l = New-LogLines
    $ui = Elements $proc.Id
    Add-Result 'Q10 点另有 1 项：全部列出（额外 82%），再点收起' ((Any $l 'quota rows unfolded') -and [bool](Find $ui '^额外 剩余 82%') -and [bool](Find $ui '^收起$')) $l
    Shot 'unfolded'
} else {
    Add-Result 'Q10 点另有 1 项：全部列出' $false @('no 「另有 1 项」 element')
}
Stop-App
$all = if (Test-Path $log) { @(Get-Content $log -Encoding utf8) } else { @() }
$bad = @($all | Where-Object { $_ -match 'unhandled|launch failed|backend: native' })
Add-Result 'Q11 整个过程走假后端、没有未处理的异常' ($bad.Count -eq 0 -and [bool]($all | Where-Object { $_ -match 'backend: fake \(131a\)' })) $bad
Write-Summary (Join-Path $OutDir 'quota-smoke-results.json')
if ($script:Results | Where-Object { -not $_.Pass }) { exit 1 }
