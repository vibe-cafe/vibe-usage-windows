# Runner only. Test 1 of issue #86: does the WinUI 3 / Windows App SDK runtime
# go online on its own? The sample's app layer sends nothing (fake data), so
# any connection or DNS query by its process comes from the runtime.
#
# Three phases, each: start the sample, open the panel and the settings window,
# leave it running, quit through the normal exit path.
#   offline           outbound blocked for the sample's exe (firewall rule; cutting the
#                     adapter would also cut the runner), diagnostic data "optional"
#   online-required   no block, diagnostic data "required" (AllowTelemetry=1)
#   online-optional   no block, diagnostic data "optional" (AllowTelemetry=3)
# Recorded per phase:
#   - every connection attempt the Windows Filtering Platform allowed or blocked for the
#     exe (Security log 5156/5157, needs auditing on; catches short-lived ones)
#   - TCP connections and UDP endpoints owned by the process and its children, every 3 s
#   - DNS queries issued by the process (DNS-Client/Operational, by process id) and the
#     DNS cache before/after
#   - the DiagTrack service (system diagnostic data): state and its TCP connections
# At the end: events in the local diagnostic data transcript (what Diagnostic Data
# Viewer shows) whose name or payload mentions WinUI / Windows App SDK / the sample.
param(
    [Parameter(Mandatory = $true)][string]$AppDir,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [int]$Minutes = 10
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$exe = (Resolve-Path (Join-Path $AppDir 'VibeUsagePanelPoc.exe')).Path
$exeName = 'VibeUsagePanelPoc.exe'

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
function Save-Shot([string]$path) {
    try {
        $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
        $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
        $g.Dispose(); $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    } catch { Write-Host "screenshot failed: $($_.Exception.Message)" }
}

# -- setup ------------------------------------------------------------------------------
Write-Host "os: $([Environment]::OSVersion.VersionString)"
wevtutil sl Microsoft-Windows-DNS-Client/Operational /e:true | Out-Null
auditpol /set /subcategory:"Filtering Platform Connection" /success:enable /failure:enable | Out-Null
$transcriptKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Diagnostics\DiagTrack\EventTranscriptKey'
New-Item -Path $transcriptKey -Force | Out-Null
Set-ItemProperty -Path $transcriptKey -Name EnableEventTranscript -Value 1 -Type DWord
$policyKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection'
New-Item -Path $policyKey -Force | Out-Null

function Get-DiagTrack {
    $svc = Get-CimInstance Win32_Service -Filter "Name='DiagTrack'" -ErrorAction SilentlyContinue
    if (-not $svc) { return [pscustomobject]@{ State = 'missing'; StartMode = ''; ProcessId = 0 } }
    [pscustomobject]@{ State = $svc.State; StartMode = $svc.StartMode; ProcessId = [int]$svc.ProcessId }
}

function Set-DiagnosticData([int]$level) {
    Set-ItemProperty -Path $policyKey -Name AllowTelemetry -Value $level -Type DWord
    Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection' -Name AllowTelemetry -Value $level -Type DWord -ErrorAction SilentlyContinue
    Restart-Service DiagTrack -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 3
}

function Get-Tree([int]$root) {
    $all = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
    $ids = New-Object System.Collections.Generic.List[int]
    $ids.Add($root)
    $added = $true
    while ($added) {
        $added = $false
        foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) { $ids.Add([int]$p.ProcessId); $added = $true }
        }
    }
    return $ids
}

$summary = [ordered]@{}

function Invoke-Phase([string]$name, [bool]$offline, [int]$telemetry) {
    Write-Host "===== phase $name (offline=$offline, AllowTelemetry=$telemetry, $Minutes min) ====="
    $dir = Join-Path $OutDir $name
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Set-DiagnosticData $telemetry
    $rule = $null
    if ($offline) {
        $rule = New-NetFirewallRule -DisplayName "poc-block-$name" -Direction Outbound -Program $exe -Action Block
        Write-Host "firewall: outbound blocked for $exe"
    }
    $diag = Get-DiagTrack
    $cacheBefore = @(Get-DnsClientCache -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Entry -Unique)
    $t0 = Get-Date
    $logDir = Join-Path $dir 'app-log'
    $seconds = $Minutes * 60
    $proc = Start-Process -FilePath $exe -ArgumentList @('--open', '--pin', '--open-settings', "--quit-after=$seconds", "--log-dir=$logDir") -PassThru
    $appPid = $proc.Id
    Write-Host "started pid $appPid"

    $samples = New-Object System.Collections.Generic.List[object]
    $diagConns = New-Object System.Collections.Generic.List[object]
    $deadline = (Get-Date).AddSeconds($seconds + 90)
    $shot = $false
    $treeSeen = New-Object System.Collections.Generic.HashSet[int]
    while (-not $proc.HasExited -and (Get-Date) -lt $deadline) {
        $tree = Get-Tree $appPid
        foreach ($id in $tree) { [void]$treeSeen.Add($id) }
        foreach ($id in $tree) {
            Get-NetTCPConnection -OwningProcess $id -ErrorAction SilentlyContinue | ForEach-Object {
                $samples.Add([pscustomobject]@{ t = (Get-Date).ToString('HH:mm:ss'); pid = $id; proto = 'TCP'; local = "$($_.LocalAddress):$($_.LocalPort)"; remote = "$($_.RemoteAddress):$($_.RemotePort)"; state = "$($_.State)" })
            }
            Get-NetUDPEndpoint -OwningProcess $id -ErrorAction SilentlyContinue | ForEach-Object {
                $samples.Add([pscustomobject]@{ t = (Get-Date).ToString('HH:mm:ss'); pid = $id; proto = 'UDP'; local = "$($_.LocalAddress):$($_.LocalPort)"; remote = ''; state = '' })
            }
        }
        $d = Get-DiagTrack
        if ($d.ProcessId -gt 0) {
            Get-NetTCPConnection -OwningProcess $d.ProcessId -ErrorAction SilentlyContinue |
                Where-Object { $_.RemoteAddress -notin @('0.0.0.0', '::', '127.0.0.1', '::1') } | ForEach-Object {
                    $diagConns.Add([pscustomobject]@{ t = (Get-Date).ToString('HH:mm:ss'); svchostPid = $d.ProcessId; remote = "$($_.RemoteAddress):$($_.RemotePort)"; state = "$($_.State)" })
                }
        }
        if (-not $shot -and ((Get-Date) - $t0).TotalSeconds -gt 25) { Save-Shot (Join-Path $dir 'screen.png'); $shot = $true }
        Start-Sleep -Seconds 3
    }
    if (-not $proc.HasExited) { Write-Host 'did not quit on its own; stopping'; Stop-Process -Id $appPid -Force }
    $exitCode = try { $proc.ExitCode } catch { 'n/a' }
    Start-Sleep -Seconds 5
    $t1 = Get-Date

    # Filtering Platform: every connection the exe tried (allowed 5156, blocked 5157).
    $wfp = @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 5156, 5157; StartTime = $t0; EndTime = $t1 } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match [regex]::Escape($exeName) } | ForEach-Object {
            $x = [xml]$_.ToXml()
            $f = @{}; foreach ($n in $x.Event.EventData.Data) { $f[$n.Name] = $n.'#text' }
            [pscustomobject]@{ time = $_.TimeCreated.ToString('HH:mm:ss'); id = $_.Id; pid = $f['ProcessID']; direction = $f['Direction']; protocol = $f['Protocol']; source = "$($f['SourceAddress']):$($f['SourcePort'])"; dest = "$($f['DestAddress']):$($f['DestPort'])" }
        })
    # DNS queries made by the process tree.
    $dns = @(Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-DNS-Client/Operational'; StartTime = $t0; EndTime = $t1 } -ErrorAction SilentlyContinue |
        Where-Object { $treeSeen.Contains([int]$_.ProcessId) } | ForEach-Object {
            [pscustomobject]@{ time = $_.TimeCreated.ToString('HH:mm:ss'); id = $_.Id; pid = $_.ProcessId; message = ($_.Message -split "`n")[0] }
        })
    $cacheAfter = @(Get-DnsClientCache -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Entry -Unique)
    $newNames = @($cacheAfter | Where-Object { $_ -notin $cacheBefore })

    $samples | Export-Csv (Join-Path $dir 'sockets.csv') -NoTypeInformation -Encoding utf8
    $wfp | Export-Csv (Join-Path $dir 'wfp-connections.csv') -NoTypeInformation -Encoding utf8
    $dns | Export-Csv (Join-Path $dir 'dns-events.csv') -NoTypeInformation -Encoding utf8
    $diagConns | Export-Csv (Join-Path $dir 'diagtrack-connections.csv') -NoTypeInformation -Encoding utf8
    $newNames | Set-Content (Join-Path $dir 'dns-cache-new-names.txt') -Encoding utf8

    $remoteSockets = @($samples | Where-Object { $_.proto -eq 'TCP' -and $_.remote -notmatch '^(0\.0\.0\.0|::|127\.0\.0\.1|::1):' })
    $result = [ordered]@{
        offline = $offline
        allowTelemetry = $telemetry
        minutes = $Minutes
        pid = $appPid
        processTree = @($treeSeen)
        exitCode = "$exitCode"
        tcpRemoteSamples = $remoteSockets.Count
        tcpRemoteDistinct = @($remoteSockets | Select-Object -ExpandProperty remote -Unique)
        udpEndpoints = @($samples | Where-Object proto -eq 'UDP' | Select-Object -ExpandProperty local -Unique)
        wfpAllowed = @($wfp | Where-Object id -eq 5156).Count
        wfpBlocked = @($wfp | Where-Object id -eq 5157).Count
        wfpDestinations = @($wfp | Where-Object { $_.direction -match '14593|Outbound' -or $_.direction -eq '%%14593' } | Select-Object -ExpandProperty dest -Unique)
        dnsQueriesByProcess = $dns.Count
        dnsCacheNewNamesDuringPhase = $newNames.Count
        diagTrack = $diag
        diagTrackRemoteDistinct = @($diagConns | Select-Object -ExpandProperty remote -Unique)
    }
    $result | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $dir 'summary.json') -Encoding utf8
    Write-Host ($result | ConvertTo-Json -Depth 5)
    if ($rule) { Remove-NetFirewallRule -DisplayName "poc-block-$name" }
    $summary[$name] = $result
}

Invoke-Phase 'offline' $true 3
Invoke-Phase 'online-required' $false 1
Invoke-Phase 'online-optional' $false 3

# Local diagnostic data transcript (Diagnostic Data Viewer's source).
$db = 'C:\ProgramData\Microsoft\Diagnosis\EventTranscript\EventTranscript.db'
$transcript = [ordered]@{ exists = (Test-Path $db) }
if (Test-Path $db) {
    Copy-Item $db (Join-Path $OutDir 'EventTranscript.db') -ErrorAction SilentlyContinue
    $py = @'
import sqlite3, sys, json
con = sqlite3.connect(sys.argv[1])
cur = con.cursor()
tables = [r[0] for r in cur.execute("select name from sqlite_master where type='table'")]
out = {"tables": tables}
if "events_persisted" in tables:
    out["total_events"] = cur.execute("select count(*) from events_persisted").fetchone()[0]
    rows = cur.execute("""select full_event_name, count(*) from events_persisted
        where full_event_name like '%WindowsAppSDK%' or full_event_name like '%WindowsAppRuntime%'
           or full_event_name like '%Microsoft.UI%' or full_event_name like '%Xaml%' or full_event_name like '%WinUI%'
           or payload like '%VibeUsagePanelPoc%' or payload like '%WindowsAppRuntime%' or payload like '%Microsoft.UI.Xaml%'
        group by full_event_name order by 2 desc""").fetchall()
    out["matching"] = rows
    out["top_names"] = cur.execute("select full_event_name, count(*) from events_persisted group by 1 order by 2 desc limit 25").fetchall()
print(json.dumps(out, ensure_ascii=False, indent=1))
'@
    $pyFile = Join-Path $env:RUNNER_TEMP 'transcript.py'
    Set-Content $pyFile $py -Encoding utf8
    $transcriptJson = python $pyFile (Join-Path $OutDir 'EventTranscript.db') 2>&1 | Out-String
    $transcriptJson | Set-Content (Join-Path $OutDir 'event-transcript.json') -Encoding utf8
    Write-Host $transcriptJson
}
$summary['transcript'] = $transcript
$summary | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $OutDir 'network-summary.json') -Encoding utf8
