# The first full chain on the Actions runner (#94): the published app with the
# real Rust backend reads a local fake VibeCafé service, draws the panel, and
# quits. On the way it checks, from the outside:
#   1. a sign-in start (--autostart) attempts no read and starts no sync while
#      the system proxy is not up yet (the whole wait, from the app's log and
#      the sync child's start time), and starts once it is;
#   2. "test two" (#85): with a logging proxy as the system proxy, which
#      process went where and whether through the proxy; with no proxy and
#      「暂停」, no vendor request; with no proxy and an egress check whose
#      answer has no country, only the anonymous check and no vendor request.
#      The HTTPS hosts under test are served locally with a throwaway test CA,
#      so each request's path and whether it carried account headers is seen.
#      In every round any destination outside the expected list, and any
#      outbound connection of the app or its Node in the firewall audit, fails;
#   3. quitting with a managed Node alive, then deleting the install folder
#      at once, meets no file in use.
# Nothing here reaches a real server: the app's and Node's outbound traffic is
# blocked by firewall rules (loopback is exempt), the proxy forwards only to
# local services and refuses or holds the rest, and the hosts under test
# point at 127.0.0.1. The account is made up (a fake API key and Codex token).
# Runner only: it changes the runner's proxy settings, hosts file, firewall,
# audit policy and trusted roots, and injects input (see common.ps1).
param(
    [Parameter(Mandatory = $true)][string]$Package,
    [Parameter(Mandatory = $true)][string]$OutDir
)

. "$PSScriptRoot/common.ps1"
$Install = 'C:\vu\install'
$script:Exe = Join-Path $Install 'vibe-usage-app.exe'
$Data = 'C:\vu\data'
$Codex = 'C:\vu\codex'
$Certs = 'C:\vu\certs'
New-Item -ItemType Directory -Force -Path $OutDir, $Data, $Codex, $Certs | Out-Null
$fakeLog = Join-Path $OutDir 'fake-vibecafe.jsonl'
$proxyLog = Join-Path $OutDir 'proxy.jsonl'
$httpsLog = Join-Path $OutDir 'local-https.jsonl'
$FakeHost = 'vibecafe-fake.test'
$ProxyPort = 18888
$HttpsHosts = @('chatgpt.com', 'github.com', 'registry.npmjs.org')
$Ours = @('vibe-usage-app.exe', 'node.exe')
# What the fake service returns for today (fake_vibecafe.py), as the panel shows it.
$ExpectedCost = '12.34'
$ExpectedTokens = '4.2M'

# ConvertFrom-Json may already have turned the ISO stamp into a DateTime.
function Get-Stamp($Value) {
    if ($Value -is [datetime]) { return $Value.ToUniversalTime() }
    return [datetime]::Parse("$Value", [cultureinfo]::InvariantCulture, [System.Globalization.DateTimeStyles]'AssumeUniversal, AdjustToUniversal')
}
function Read-Jsonl([string]$Path, [datetime]$Since, [datetime]$Until = [datetime]::MaxValue) {
    if (-not (Test-Path $Path)) { return @() }
    return @(Get-Content $Path -Encoding utf8 | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } |
        Where-Object { $s = Get-Stamp $_.t; $s -ge $Since.ToUniversalTime() -and $s -le $Until.ToUniversalTime() })
}
# The shell log's local time stamps (the runner's clock).
function Get-LogStamp([string]$Line) {
    return [datetime]::ParseExact($Line.Substring(0, 23), 'yyyy-MM-dd HH:mm:ss.fff', [cultureinfo]::InvariantCulture)
}
function Show-Rows([string]$Title, $Rows) {
    Write-Host "----- $Title ($(@($Rows).Count)) -----"
    $Rows | ForEach-Object { Write-Host ("      " + ($(if ($_ -is [string]) { $_ } else { $_ | ConvertTo-Json -Compress }))) }
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
# Every managed node seen while waiting, with its start time and whether it is the CLI sync.
$script:NodeSeen = @{}
function Watch-Wait([int]$Ms) {
    $end = [DateTime]::UtcNow.AddMilliseconds($Ms)
    while ([DateTime]::UtcNow -lt $end) {
        foreach ($p in Get-ManagedNode) {
            if (-not $script:NodeSeen.ContainsKey($p.ProcessId)) {
                $script:NodeSeen[$p.ProcessId] = [pscustomobject]@{ pid = $p.ProcessId; started = $p.CreationDate; sync = "$($p.CommandLine)" -match 'cli-bootstrap' }
            }
        }
        Pump 250
    }
}
# Firewall audit: connections of the app or its Node that left (or tried to leave) this machine.
function Get-DirectConnections([datetime]$Since) {
    $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 5156, 5157; StartTime = $Since } -ErrorAction SilentlyContinue)
    return @($events | Where-Object { "$($_.Properties[1].Value)" -match 'vu\\install' } | ForEach-Object {
        [pscustomobject]@{ event = $_.Id; app = Split-Path "$($_.Properties[1].Value)" -Leaf; dest = "$($_.Properties[5].Value)"; port = "$($_.Properties[6].Value)" }
    } | Where-Object { $_.dest -notin @('127.0.0.1', '::1') })
}
function Read-PanelTexts([int]$ProcessId) {
    return @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot/read-texts.ps1" -ProcessId $ProcessId 2>&1 | ForEach-Object { "$_" })
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

# Every destination of the app and its Node in one round, checked against what is expected.
# $Proxy, $Https: log rows of the round; $Expected: script blocks deciding whether a row is expected.
function Test-Destinations([string]$Label, [datetime]$Since, $Proxy, $Https, [scriptblock]$ProxyOk, [scriptblock]$HttpsOk) {
    $unexpectedProxy = @($Proxy | Where-Object { $_.process -in $Ours -and -not (& $ProxyOk $_) })
    $unexpectedHttps = @($Https | Where-Object { $_.process -in $Ours -and -not (& $HttpsOk $_) })
    $direct = Get-DirectConnections $Since
    Show-Rows "$Label 防火墙审计：主程序和 node 的外连（应为空）" $direct
    Show-Rows "$Label 意外的经代理目的地（应为空）" $unexpectedProxy
    Show-Rows "$Label 意外的直连目的地（应为空）" $unexpectedHttps
    Add-Result "$Label 整轮没有意外目的地、没有外连（经代理意外 $($unexpectedProxy.Count)、直连意外 $($unexpectedHttps.Count)、防火墙审计 $($direct.Count)，都应为 0）" ($unexpectedProxy.Count -eq 0 -and $unexpectedHttps.Count -eq 0 -and $direct.Count -eq 0)
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
# Control for the audit checks: the install's own node tries 192.0.2.1:443 (TEST-NET-1, never
# routed anywhere); the block rule must stop it and the audit must log the block (5157) on 443.
$tF = Get-Date
$probe = & "$Install\node\node.exe" -e "const s=require('net').connect({host:'192.0.2.1',port:443,timeout:4000});s.on('connect',()=>{console.log('connected');process.exit(0)});s.on('error',e=>{console.log('error '+e.code);process.exit(0)});s.on('timeout',()=>{console.log('timeout');process.exit(0)})" 2>&1
Start-Sleep -Seconds 2
$control = @(Get-DirectConnections $tF | Where-Object { $_.event -eq 5157 -and $_.dest -eq '192.0.2.1' -and $_.port -eq '443' })
Add-Result "F0 对照：安装目录里的 node 直连 192.0.2.1:443 被防火墙拦下，审计有对应的阻断事件 5157（node 输出：$probe；5157 共 $($control.Count) 条）" ($control.Count -gt 0 -and "$probe" -notmatch 'connected')
foreach ($name in 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy') {
    Remove-Item "Env:$name" -ErrorAction SilentlyContinue
}

# A throwaway CA for the local HTTPS service, trusted machine-wide for this run only
# (the backend's TLS uses the Windows store; Node keeps its own list and won't trust it).
$openssl = (Get-Command openssl -ErrorAction SilentlyContinue).Source
if (-not $openssl) { $openssl = 'C:\Program Files\Git\usr\bin\openssl.exe' }
$env:MSYS_NO_PATHCONV = '1'
& $openssl req -x509 -new -nodes -newkey rsa:2048 -keyout "$Certs\ca.key" -out "$Certs\ca.crt" -days 2 -subj '/CN=Vibe Usage chain test CA' `
    -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' 2>&1 | Out-Null
if (-not (Test-Path "$Certs\ca.crt")) { throw 'no test CA certificate' }
foreach ($h in $HttpsHosts) {
    & $openssl req -new -nodes -newkey rsa:2048 -keyout "$Certs\$h.key" -out "$Certs\$h.csr" -subj "/CN=$h" 2>&1 | Out-Null
    "basicConstraints=CA:FALSE`nkeyUsage=critical,digitalSignature,keyEncipherment`nextendedKeyUsage=serverAuth`nsubjectAltName=DNS:$h`n" |
        Set-Content "$Certs\$h.ext" -Encoding ascii
    & $openssl x509 -req -in "$Certs\$h.csr" -CA "$Certs\ca.crt" -CAkey "$Certs\ca.key" -CAcreateserial -out "$Certs\$h.crt" -days 2 -sha256 -extfile "$Certs\$h.ext" 2>&1 | Out-Null
    if (-not (Test-Path "$Certs\$h.crt")) { throw "no test certificate for $h" }
}
$testCa = Import-Certificate -FilePath "$Certs\ca.crt" -CertStoreLocation Cert:\LocalMachine\Root
Write-Host "test CA trusted: $($testCa.Thumbprint)"

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
$httpsArgs = @("$PSScriptRoot\tls_service.py", '--port', '443', '--log', $httpsLog, '--certs', $Certs) + ($HttpsHosts | ForEach-Object { '--host', $_ })
$https = Start-Process python -ArgumentList $httpsArgs -PassThru -WindowStyle Hidden
Pump 1500

# --- 1. sign-in start before the proxy is up, then 2. test two through the proxy, then 3. quit --
$logA = Join-Path $OutDir 'logs-boot'
$shellA = Join-Path $logA 'shell.log'
$rectA = Join-Path $OutDir 'panel-rect.txt'
$dataA = New-DataDir 'boot' 'auto'
Set-SystemProxy $true
$t0 = Get-Date
$app = Start-App @('--autostart', "--test-data-dir=$dataA", "--test-log-dir=$logA", "--test-rect-file=$rectA")
Use-Log $shellA
Watch-Wait 20000
$l = New-LogLines
Add-Result 'A0 外壳加载真后端 DLL 并以开机自启方式初始化' ((Any $l 'backend: native .*autostart=True') -and (Any $l 'status: linked=True')) $l

$tProxy = Get-Date
$proxy = Start-Process python -ArgumentList @("$PSScriptRoot\logging_proxy.py", '--port', "$ProxyPort", '--log', $proxyLog, '--hold', 'registry.npmjs.org', '--tunnel', 'chatgpt.com=443') -PassThru -WindowStyle Hidden
$deadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $deadline -and (Read-Jsonl $fakeLog $t0).Count -eq 0) { Watch-Wait 500 }
Watch-Wait 15000
# 1. Nothing the backend reports as a read or a sync, and no CLI sync child, before the proxy was up.
#    (A quota line without a result code is the cached state the backend reports while it waits.)
$events = @(Get-Content $shellA -Encoding utf8 | Where-Object {
    $_ -match 'usage: (read failed|\d+ buckets)|sync: (syncing|error|success)|quotas: refreshing=False .*Codex=(ok|\w+/\w+)' })
$beforeProxy = @($events | Where-Object { (Get-LogStamp $_) -lt $tProxy })
$syncChildren = @($script:NodeSeen.Values | Where-Object { $_.sync })
$earlyChildren = @($syncChildren | Where-Object { $_.started -lt $tProxy })
Show-Rows '后端报告的读取和同步（开机自启这一轮）' $events
Show-Rows '这一轮出现过的托管 node' @($script:NodeSeen.Values)
$fakeA = Read-Jsonl $fakeLog $t0
$first = if ($fakeA.Count) { (Get-Stamp $fakeA[0].t).ToLocalTime() } else { $null }
Add-Result "A1 开机自启、代理没就绪的整段等待里：后端没有报告任何读取或同步（$($beforeProxy.Count) 条），没有启动同步子进程（$($earlyChildren.Count) 个）" ($beforeProxy.Count -eq 0 -and $earlyChildren.Count -eq 0)
Add-Result "A2 代理就绪后才开始：代理 $($tProxy.ToString('HH:mm:ss.fff')) 起，第一次读到用量 $(if ($first) { $first.ToString('HH:mm:ss.fff') } else { '无' })，同步子进程 $($syncChildren.Count) 个都在之后启动" ($first -and $first -ge $tProxy -and $syncChildren.Count -gt 0 -and $events.Count -gt 0)

$proxyA = Read-Jsonl $proxyLog $t0
$httpsA = Read-Jsonl $httpsLog $t0
Show-Rows '代理日志（开机自启这一轮）' $proxyA
Show-Rows '本机 HTTPS 服务日志（开机自启这一轮）' $httpsA
Show-Rows '假服务日志（开机自启这一轮）' $fakeA
$usageViaProxy = @($proxyA | Where-Object { $_.process -eq 'vibe-usage-app.exe' -and $_.host -eq $FakeHost -and $_.verdict -eq 'forwarded-to-fake' })
$usageAtFake = @($fakeA | Where-Object { $_.path -eq '/api/usage' })
Add-Result "T1 主进程读用量走代理：代理转发 $($usageViaProxy.Count) 次，假服务收到的 $($usageAtFake.Count) 次都来自代理进程" ($usageViaProxy.Count -gt 0 -and @($usageAtFake | Where-Object { $_.process -notmatch '^python' }).Count -eq 0)
$codexTunnel = @($proxyA | Where-Object { $_.process -eq 'vibe-usage-app.exe' -and $_.host -eq 'chatgpt.com' -and $_.verdict -eq 'tunnel-to-local-https' })
$codexReads = @($httpsA | Where-Object { $_.sni -eq 'chatgpt.com' -and $_.path -like '/backend-api/*' })
Add-Result "T2 带厂商账号的 Codex 额度请求走代理：主程序 CONNECT chatgpt.com $($codexTunnel.Count) 次，额度端点收到 $($codexReads.Count) 次，都经代理进程、带认证头和账号头" ($codexTunnel.Count -gt 0 -and $codexReads.Count -gt 0 -and @($codexReads | Where-Object { $_.process -notmatch '^python' -or -not $_.authorization -or -not $_.account }).Count -eq 0)
$cliViaProxy = @($proxyA | Where-Object { $_.process -eq 'node.exe' -and $_.host -eq 'registry.npmjs.org' })
Add-Result "T3 子进程 CLI（node.exe）走代理：$($cliViaProxy.Count) 次 CONNECT registry.npmjs.org" ($cliViaProxy.Count -gt 0)

# The panel with what the fake service returned (a start of the exe opens it).
Focus-Form
Start-App @("--test-data-dir=$dataA", "--test-log-dir=$logA") | Out-Null
Pump 4000
$l = New-LogLines
$n = Read-Numbers $rectA
if ($n) { Save-Window $n[0] @([int]$n[1], [int]$n[2], [int]$n[3], [int]$n[4]) (Join-Path $OutDir 'chain-panel.png') | Out-Null }
$texts = Read-PanelTexts $app.Id
Show-Rows '面板上的文字（UI Automation）' $texts
$costShown = [bool]($texts | Where-Object { $_ -match [regex]::Escape($ExpectedCost) })
$tokensShown = [bool]($texts | Where-Object { $_.Trim() -eq $ExpectedTokens })
Add-Result "P1 面板打开并画出假服务的当日用量：金额 $ExpectedCost $(if ($costShown) { '在' } else { '不在' })，Token $ExpectedTokens $(if ($tokensShown) { '在' } else { '不在' })" ((Any $l 'show source=Relaunch') -and $costShown -and $tokensShown -and [bool]$n) $l
Key 0x1B
Pump 800

# Quit while a managed Node waits on the held registry connection.
if ((Get-ManagedNode).Count -eq 0) { Invoke-TrayMenu 2; Pump 4000 }
$nodes = Test-QuitThenDelete $app 'U1'
Add-Result "U1' 退出时确有托管 node 在跑（$nodes 个）" ($nodes -gt 0)
Show-Rows '外壳日志（退出）' (New-LogLines)
Stop-Process -Id $proxy.Id -Force -ErrorAction SilentlyContinue
$proxyA = Read-Jsonl $proxyLog $t0
$httpsA = Read-Jsonl $httpsLog $t0
Test-Destinations 'T4 经代理这一轮' $t0 $proxyA $httpsA `
    { param($r) ($r.host -eq $FakeHost -and $r.verdict -eq 'forwarded-to-fake') -or ($r.host -eq 'chatgpt.com' -and $r.process -eq 'vibe-usage-app.exe') -or
        ($r.host -eq 'registry.npmjs.org' -and $r.process -eq 'node.exe') -or ($r.host -eq 'github.com' -and $r.process -eq 'vibe-usage-app.exe') } `
    { param($r) $false }

# --- 2b. no proxy, 「没检测到代理时」= 暂停 -------------------------------------------------------
Set-SystemProxy $false
Set-TestHosts (@($FakeHost) + $HttpsHosts)
Install-App
$logB = Join-Path $OutDir 'logs-pause'
$dataB = New-DataDir 'pause' 'pause'
$t1 = Get-Date
$app = Start-App @("--test-data-dir=$dataB", "--test-log-dir=$logB")
Use-Log (Join-Path $logB 'shell.log')
Pump 25000
$l = New-LogLines
Show-Rows '外壳日志（暂停）' $l
Test-QuitThenDelete $app 'U2' | Out-Null
$fakeB = Read-Jsonl $fakeLog $t1
$httpsB = Read-Jsonl $httpsLog $t1
Show-Rows '假服务日志（暂停）' $fakeB
Show-Rows '本机 HTTPS 服务日志（暂停）' $httpsB
$vendorB = @($httpsB | Where-Object { $_.sni -eq 'chatgpt.com' })
Add-Result "T5 没有代理、设为暂停：chatgpt.com 收到 $($vendorB.Count) 次连接（应为 0：不发带账号的请求，也不发出口检测），额度标「没检测到代理」" ($vendorB.Count -eq 0 -and (Any $l 'Codex=\w+/proxyNotDetected'))
$usageDirect = @($fakeB | Where-Object { $_.path -eq '/api/usage' -and $_.process -eq 'vibe-usage-app.exe' })
Add-Result "T6 没有代理时，不带厂商账号的用量读取照常直连自家服务：$($usageDirect.Count) 次" ($usageDirect.Count -gt 0)
Test-Destinations 'T6′ 暂停这一轮' $t1 @() $httpsB { param($r) $false } `
    { param($r) ($r.sni -eq 'github.com' -and $r.process -eq 'vibe-usage-app.exe' -and -not $r.authorization) -or ($r.sni -eq 'registry.npmjs.org' -and $r.process -eq 'node.exe') }

# --- 2c. no proxy, 自动: the egress check's answer has no country --------------------------------
Install-App
$logC = Join-Path $OutDir 'logs-auto'
$dataC = New-DataDir 'auto' 'auto'
$t2 = Get-Date
$app = Start-App @("--test-data-dir=$dataC", "--test-log-dir=$logC")
Use-Log (Join-Path $logC 'shell.log')
Pump 25000
$l = New-LogLines
Show-Rows '外壳日志（自动）' $l
Test-QuitThenDelete $app 'U3' | Out-Null
$httpsC = Read-Jsonl $httpsLog $t2
Show-Rows '本机 HTTPS 服务日志（自动）' $httpsC
$traces = @($httpsC | Where-Object { $_.sni -eq 'chatgpt.com' -and $_.path -eq '/cdn-cgi/trace' })
$anonymous = @($traces | Where-Object { -not $_.authorization -and -not $_.account -and -not $_.cookie })
$vendorC = @($httpsC | Where-Object { $_.sni -eq 'chatgpt.com' -and $_.path -ne '/cdn-cgi/trace' })
Add-Result "T7 没有代理、出口检测拿到没有国家码的答复：出口检测 $($traces.Count) 次（不带认证、账号头和 Cookie 的 $($anonymous.Count) 次），额度端点 $($vendorC.Count) 次（应为 0），额度标「无法确认网络出口」" ($traces.Count -ge 1 -and $anonymous.Count -eq $traces.Count -and $vendorC.Count -eq 0 -and (Any $l 'Codex=\w+/egressUnconfirmed'))
Test-Destinations 'T7′ 自动这一轮' $t2 @() $httpsC { param($r) $false } `
    { param($r) ($r.sni -eq 'chatgpt.com' -and $r.path -eq '/cdn-cgi/trace' -and $r.process -eq 'vibe-usage-app.exe') -or
        ($r.sni -eq 'github.com' -and $r.process -eq 'vibe-usage-app.exe' -and -not $r.authorization) -or ($r.sni -eq 'registry.npmjs.org' -and $r.process -eq 'node.exe') }

# --- clean up -------------------------------------------------------------------------------------
Stop-Process -Id $fake.Id, $https.Id -Force -ErrorAction SilentlyContinue
Remove-Item "Cert:\LocalMachine\Root\$($testCa.Thumbprint)" -ErrorAction SilentlyContinue
Set-TestHosts @()
$form.Close()
Write-Summary (Join-Path $OutDir 'chain-results.json')
if ($script:Results | Where-Object { -not $_.Pass }) { exit 1 }
