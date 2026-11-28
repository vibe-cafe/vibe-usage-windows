# The first full chain on the Actions runner (#94): the published app with the
# real Rust backend reads a local fake VibeCafé service, draws the panel, and
# quits. On the way it checks, from the outside:
#   1. a sign-in start (--autostart) sends no read and no sync while the
#      system proxy is not up yet, and starts once it is;
#   2. "test two" (#85): with a logging proxy as the system proxy, which
#      process went where and whether through the proxy; with no proxy and
#      「暂停」, no vendor request; with no proxy and the egress check getting
#      no answer, only the anonymous check and no vendor request;
#   3. quitting with a managed Node alive, then deleting the install folder
#      at once, meets no file in use.
# Nothing here reaches a real server: the app's and Node's outbound traffic is
# blocked by firewall rules (loopback is exempt), the proxy forwards only to
# the local fake service and refuses or holds the rest, and the hosts under
# test point at 127.0.0.1 where a sink logs and drops direct attempts. The
# account is made up (a fake API key and a fake Codex token).
# Runner only: it changes the runner's proxy settings, hosts file, firewall
# and audit policy, and injects input (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$Package,
    [Parameter(Mandatory = $true)][string]$OutDir
)

. "$PSScriptRoot/common.ps1"
$Install = 'C:\vu\install'
$script:Exe = Join-Path $Install 'vibe-usage-app.exe'
$Data = 'C:\vu\data'
$Codex = 'C:\vu\codex'
New-Item -ItemType Directory -Force -Path $OutDir, $Data, $Codex | Out-Null
$fakeLog = Join-Path $OutDir 'fake-vibecafe.jsonl'
$proxyLog = Join-Path $OutDir 'proxy.jsonl'
$sinkLog = Join-Path $OutDir 'direct-tls.jsonl'
$FakeHost = 'vibecafe-fake.test'
$ProxyPort = 18888

# ConvertFrom-Json may already have turned the ISO stamp into a DateTime.
function Get-Stamp($Value) {
    if ($Value -is [datetime]) { return $Value.ToUniversalTime() }
    return [datetime]::Parse("$Value", [cultureinfo]::InvariantCulture, [System.Globalization.DateTimeStyles]'AssumeUniversal, AdjustToUniversal')
}
function Read-Jsonl([string]$Path, [datetime]$Since) {
    if (-not (Test-Path $Path)) { return @() }
    return @(Get-Content $Path -Encoding utf8 | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } |
        Where-Object { (Get-Stamp $_.t) -ge $Since.ToUniversalTime() })
}
function Show-Rows([string]$Title, $Rows) {
    Write-Host "----- $Title ($(@($Rows).Count)) -----"
    $Rows | ForEach-Object { Write-Host ("      " + ($_ | ConvertTo-Json -Compress)) }
}
function Install-App {
    if (Test-Path $Install) { Remove-Item $Install -Recurse -Force }
    Copy-Item $Package $Install -Recurse
}
function New-DataDir([string]$Name, [string]$EgressMode) {
    $dir = Join-Path $Data $Name
    New-Item -ItemType Directory -Force -Path (Join-Path $dir 'roaming') | Out-Null
    # Codex only, chosen already: the fake token below makes it a vendor read to watch.
    @{
        showCostInTray = $true; showTokensInTray = $false; codexRateLimitEnabled = $true; claudeRateLimitEnabled = $false
        selectedQuotaProductIds = @('codex'); quotaProductOrder = @('codex'); quotaSelectedTabId = $null
        quotaSelectionInitialized = $true; zCodeQuotaRegion = 'bigModel'; egressMode = $EgressMode
    } | ConvertTo-Json | Set-Content (Join-Path $dir 'roaming\settings.json') -Encoding utf8
    return $dir
}
function Set-SystemProxy([bool]$On) {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    Set-ItemProperty $key -Name ProxyEnable -Value ([int]$On) -Type DWord
    Set-ItemProperty $key -Name ProxyServer -Value "127.0.0.1:$ProxyPort"
    Remove-ItemProperty $key -Name ProxyOverride -ErrorAction SilentlyContinue
    Remove-ItemProperty $key -Name AutoConfigURL -ErrorAction SilentlyContinue
    Write-Host "system proxy enabled=$On (127.0.0.1:$ProxyPort, no bypass list, no PAC)"
}
function Set-TestHosts([string[]]$Names) {
    $file = "$env:SystemRoot\System32\drivers\etc\hosts"
    $kept = @(Get-Content $file | Where-Object { $_ -notmatch '# vibe-usage-chain$' })
    $added = $Names | ForEach-Object { "127.0.0.1 $_ # vibe-usage-chain" }
    Set-Content $file -Value ($kept + $added) -Encoding ascii
    ipconfig /flushdns | Out-Null
    Write-Host "hosts -> 127.0.0.1: $($Names -join ', ')"
}
function Get-ManagedNode {
    return @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" | Where-Object { $_.ExecutablePath -like "$Install\*" })
}
# Firewall audit: connections of the app or its Node that left (or tried to leave) this machine.
function Get-DirectConnections([datetime]$Since) {
    $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 5156, 5157; StartTime = $Since } -ErrorAction SilentlyContinue)
    return @($events | Where-Object { "$($_.Properties[1].Value)" -match 'vu\\install' } | ForEach-Object {
        [pscustomobject]@{ event = $_.Id; app = Split-Path "$($_.Properties[1].Value)" -Leaf; dest = "$($_.Properties[5].Value)"; port = "$($_.Properties[6].Value)" }
    } | Where-Object { $_.dest -notin @('127.0.0.1', '::1') })
}

$form = New-Object System.Windows.Forms.Form
$form.Text = 'previous window'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point(60, 520)
$form.Size = New-Object System.Drawing.Size(360, 240)
$form.TopMost = $true
$form.Show()
Pump 800
$fx = [int]($form.Bounds.Left + $form.Bounds.Width / 2); $fy = [int]($form.Bounds.Top + $form.Bounds.Height / 2)
function Focus-Form { $form.WindowState = 'Normal'; Click $fx $fy; Pump 300; $form.Activate(); Pump 400 }

# The tray menu from the keyboard path (see tray-smoke.ps1 M2k): 1 = 打开面板, 2 = 立即同步, 4 = 退出.
function Invoke-TrayMenu([int]$Item) {
    Focus-Form
    $trayHost = Find-Window 'VibeUsage.Native.TrayHost'
    $app = Get-Process -Name 'vibe-usage-app' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $app -or $trayHost -eq [IntPtr]::Zero) { Write-Host 'tray menu: app not running'; return }
    [VuNative]::AllowSetForegroundWindow([uint32]$app.Id) | Out-Null
    [VuNative]::PostMessageW($trayHost, 0x8001, [IntPtr]((1050 -shl 16) -bor 1800), [IntPtr]((1 -shl 16) -bor 0x7B)) | Out-Null
    Pump 1200
    for ($i = 0; $i -lt $Item; $i++) { Key 0x28 }
    Key 0x0D
}

# Quit with 退出, then delete the install folder at once (what an uninstall right after quitting does).
function Test-QuitThenDelete([System.Diagnostics.Process]$Proc, [string]$Label) {
    $nodeBefore = Get-ManagedNode
    Invoke-TrayMenu 4
    $exited = $Proc.WaitForExit(15000)
    $err = $null
    try { Remove-Item $Install -Recurse -Force -ErrorAction Stop } catch { $err = $_.Exception.Message }
    $gone = -not (Test-Path $Install)
    $nodeAfter = Get-ManagedNode
    Add-Result "$Label 退出（当时有 $($nodeBefore.Count) 个托管 node）后立刻删除安装目录：$(if ($err) { '失败：' + $err } else { '成功，没有文件占用' })" ($exited -and $gone -and -not $err -and $nodeAfter.Count -eq 0)
    return $nodeBefore.Count
}

# --- set-up -------------------------------------------------------------------------------------
Write-Host "python: $(python --version 2>&1)"
Set-Resolution @('1920x1080')
Set-DarkTheme
Stop-App
Install-App
foreach ($f in 'vibe_ffi.dll', 'cli-bootstrap.mjs', 'node\node.exe') {
    if (-not (Test-Path (Join-Path $Install $f))) { throw "install is missing $f" }
}
Get-NetFirewallProfile | ForEach-Object { Write-Host "firewall $($_.Name) enabled=$($_.Enabled)" }
Set-NetFirewallProfile -Profile Domain, Private, Public -Enabled True
foreach ($program in "$Install\vibe-usage-app.exe", "$Install\node\node.exe") {
    New-NetFirewallRule -DisplayName "vibe-usage-chain block $(Split-Path $program -Leaf)" -Direction Outbound -Action Block -Program $program | Out-Null
}
auditpol /set /subcategory:"Filtering Platform Connection" /success:enable /failure:enable | Out-Null
foreach ($name in 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy') {
    Remove-Item "Env:$name" -ErrorAction SilentlyContinue
}
# The made-up account: the backend reads ~/.vibe-usage/config.json (shared with the CLI).
$cfgDir = Join-Path $env:USERPROFILE '.vibe-usage'
New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
@{ apiKey = 'vu_chain_test_not_a_real_key'; apiUrl = "http://${FakeHost}:18080"; hostname = 'ci-runner' } |
    ConvertTo-Json | Set-Content (Join-Path $cfgDir 'config.json') -Encoding utf8
@{ tokens = @{ access_token = 'fake-chain-test-token'; account_id = 'acct_chain_test' } } |
    ConvertTo-Json | Set-Content (Join-Path $Codex 'auth.json') -Encoding utf8
$env:CODEX_HOME = $Codex
Set-TestHosts @()
$fake = Start-Process python -ArgumentList @("$PSScriptRoot\fake_vibecafe.py", '--port', '18080', '--log', $fakeLog) -PassThru -WindowStyle Hidden
$sink = Start-Process python -ArgumentList @("$PSScriptRoot\tls_sink.py", '--port', '443', '--log', $sinkLog) -PassThru -WindowStyle Hidden
Pump 1500

# --- 1. sign-in start before the proxy is up, then 2. test two through the proxy, then 3. quit --
$logA = Join-Path $OutDir 'logs-boot'
$rectA = Join-Path $OutDir 'panel-rect.txt'
$dataA = New-DataDir 'boot' 'auto'
Set-SystemProxy $true
$t0 = Get-Date
$app = Start-App @('--autostart', "--test-data-dir=$dataA", "--test-log-dir=$logA", "--test-rect-file=$rectA")
Use-Log (Join-Path $logA 'shell.log')
Pump 20000
$early = Read-Jsonl $fakeLog $t0
$nodeEarly = Get-ManagedNode
$l = New-LogLines
Add-Result 'A0 外壳加载真后端 DLL 并以开机自启方式初始化' ((Any $l 'backend: native .*autostart=True') -and (Any $l 'status: linked=True')) $l
Add-Result "A1 开机自启、代理没就绪的 20 秒里：假服务收到 $($early.Count) 次请求、托管 node $($nodeEarly.Count) 个（都应为 0）" ($early.Count -eq 0 -and $nodeEarly.Count -eq 0)

$tProxy = Get-Date
$proxy = Start-Process python -ArgumentList @("$PSScriptRoot\logging_proxy.py", '--port', "$ProxyPort", '--log', $proxyLog, '--hold', 'registry.npmjs.org') -PassThru -WindowStyle Hidden
$deadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $deadline -and (Read-Jsonl $fakeLog $t0).Count -eq 0) { Pump 500 }
Pump 15000
$fakeA = Read-Jsonl $fakeLog $t0
$proxyA = Read-Jsonl $proxyLog $t0
$first = if ($fakeA.Count) { Get-Stamp $fakeA[0].t } else { $null }
Add-Result "A2 代理就绪后才开始读取（代理 $($tProxy.ToUniversalTime().ToString('HH:mm:ss.fff')) 起，第一次读取 $(if ($first) { $first.ToString('HH:mm:ss.fff') } else { '无' })）" ($first -and $first -ge $tProxy.ToUniversalTime())
Show-Rows '代理日志（开机自启这一轮）' $proxyA
Show-Rows '假服务日志（开机自启这一轮）' $fakeA
$usageViaProxy = @($proxyA | Where-Object { $_.process -eq 'vibe-usage-app.exe' -and $_.host -eq $FakeHost -and $_.verdict -eq 'forwarded-to-fake' })
$fakeFromProxy = @($fakeA | Where-Object { $_.path -eq '/api/usage' -and $_.process -match '^python' })
Add-Result "T1 主进程读用量走代理：代理转发 $($usageViaProxy.Count) 次，假服务收到的都来自代理" ($usageViaProxy.Count -gt 0 -and $fakeFromProxy.Count -eq @($fakeA | Where-Object { $_.path -eq '/api/usage' }).Count)
$codexViaProxy = @($proxyA | Where-Object { $_.process -eq 'vibe-usage-app.exe' -and $_.host -eq 'chatgpt.com' })
Add-Result "T2 Codex 额度请求（带厂商账号）走代理：$($codexViaProxy.Count) 次 CONNECT chatgpt.com，由测试代理拒绝" ($codexViaProxy.Count -gt 0)
$cliViaProxy = @($proxyA | Where-Object { $_.process -eq 'node.exe' -and $_.host -eq 'registry.npmjs.org' })
Add-Result "T3 子进程 CLI（node.exe）走代理：$($cliViaProxy.Count) 次 CONNECT registry.npmjs.org" ($cliViaProxy.Count -gt 0)
$directA = Get-DirectConnections $t0
Show-Rows '防火墙审计：主程序和 node 的外连（应为空）' $directA
$sinkA = Read-Jsonl $sinkLog $t0
Add-Result "T4 没有任何绕过代理的直连：防火墙审计 $($directA.Count) 条、本机直连捕获 $($sinkA.Count) 条（都应为 0）" ($directA.Count -eq 0 -and $sinkA.Count -eq 0)
$l = New-LogLines
Show-Rows '外壳日志（开机自启这一轮）' $l

# The panel with what the fake service returned (a start of the exe opens it).
Focus-Form
Start-App @("--test-data-dir=$dataA", "--test-log-dir=$logA") | Out-Null
Pump 4000
$l = New-LogLines
$n = Read-Numbers $rectA
if ($n) { Save-Window $n[0] @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4]) (Join-Path $OutDir 'chain-panel.png') | Out-Null }
Add-Result 'P1 面板打开并画出假服务的用量' ((Any $l 'show source=Relaunch') -and [bool](Get-Content (Join-Path $logA 'shell.log') | Where-Object { $_ -match 'usage: [1-9]\d* buckets' }) -and [bool]$n) $l
Key 0x1B
Pump 800

# Quit while a managed Node waits on the held registry connection.
if ((Get-ManagedNode).Count -eq 0) { Invoke-TrayMenu 2; Pump 4000 }
$nodes = Test-QuitThenDelete $app 'U1'
Add-Result "U1' 退出时确有托管 node 在跑（$nodes 个）" ($nodes -gt 0)
Show-Rows '外壳日志（退出）' (New-LogLines)
Stop-Process -Id $proxy.Id -Force -ErrorAction SilentlyContinue

# --- 2b. no proxy, 「没检测到代理时」= 暂停 -------------------------------------------------------
Set-SystemProxy $false
Set-TestHosts @($FakeHost, 'chatgpt.com', 'registry.npmjs.org', 'github.com', 'api.anthropic.com')
Install-App
$logB = Join-Path $OutDir 'logs-pause'
$dataB = New-DataDir 'pause' 'pause'
$t1 = Get-Date
$app = Start-App @("--test-data-dir=$dataB", "--test-log-dir=$logB")
Use-Log (Join-Path $logB 'shell.log')
Pump 25000
$fakeB = Read-Jsonl $fakeLog $t1
$sinkB = Read-Jsonl $sinkLog $t1
$l = New-LogLines
Show-Rows '假服务日志（暂停）' $fakeB
Show-Rows '本机直连捕获（暂停）' $sinkB
Show-Rows '外壳日志（暂停）' $l
$vendorB = @($sinkB | Where-Object { $_.sni -eq 'chatgpt.com' })
Add-Result "T5 没有代理、设为暂停：带账号的 Codex 请求 0 次（实际 $($vendorB.Count) 次），连出口检测也不发，额度标「没检测到代理」" ($vendorB.Count -eq 0 -and (Any $l 'Codex=\w+/proxyNotDetected'))
$usageDirect = @($fakeB | Where-Object { $_.path -eq '/api/usage' -and $_.process -eq 'vibe-usage-app.exe' })
Add-Result "T6 没有代理时，不带厂商账号的用量读取照常直连自家服务：$($usageDirect.Count) 次" ($usageDirect.Count -gt 0)
Test-QuitThenDelete $app 'U2' | Out-Null

# --- 2c. no proxy, 自动: the egress check gets no answer -------------------------------------------
Install-App
$logC = Join-Path $OutDir 'logs-auto'
$dataC = New-DataDir 'auto' 'auto'
$t2 = Get-Date
$app = Start-App @("--test-data-dir=$dataC", "--test-log-dir=$logC")
Use-Log (Join-Path $logC 'shell.log')
Pump 25000
$sinkC = Read-Jsonl $sinkLog $t2
$l = New-LogLines
Show-Rows '本机直连捕获（自动）' $sinkC
Show-Rows '外壳日志（自动）' $l
$vendorC = @($sinkC | Where-Object { $_.sni -eq 'chatgpt.com' })
Add-Result "T7 没有代理、出口检测得不到答复：只发了 1 次不带账号的检测（实际 $($vendorC.Count) 次连 chatgpt.com），Codex 请求暂停" ($vendorC.Count -eq 1 -and (Any $l 'Codex=\w+/egressUnconfirmed'))
Test-QuitThenDelete $app 'U3' | Out-Null

# --- report ---------------------------------------------------------------------------------------
$directAll = Get-DirectConnections $t0
Show-Rows '防火墙审计：整个测试里主程序和 node 的外连（全被拦下）' $directAll
Stop-Process -Id $fake.Id, $sink.Id -Force -ErrorAction SilentlyContinue
Set-TestHosts @()
$form.Close()
Write-Summary (Join-Path $OutDir 'chain-results.json')
if ($script:Results | Where-Object { -not $_.Pass }) { exit 1 }
