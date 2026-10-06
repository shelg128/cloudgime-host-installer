param([string]$CaseDir,[string]$EnvelopeBase64,[string]$CoreSha256)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:COMPUTERNAME-ne'RDPGRID-COM122'-or$CaseDir-notmatch'^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+\\dryrun-[a-f0-9]{32}$'){throw 'DRY_RUN_SCOPE'}
$root=Split-Path $CaseDir -Parent;$core=Join-Path $root 'CaptureGate.Core.ps1';if($CoreSha256-notmatch'^[a-f0-9]{64}$'-or(Get-FileHash $core).Hash.ToLowerInvariant()-cne$CoreSha256){throw 'CORE_HASH'}
. $core
$start=[Diagnostics.Stopwatch]::StartNew();$raw=[Convert]::FromBase64String($EnvelopeBase64);if($raw.Length-gt8192){throw 'ENVELOPE_SIZE'};$e=[Text.Encoding]::UTF8.GetString($raw)|ConvertFrom-Json
$base=Read-CgGateJson (Join-Path $CaseDir 'fixture.json') 8192
$procStart=$start.Elapsed.TotalMilliseconds
$web=Get-CimInstance Win32_Process -Filter "Name='web-server.exe'"|Where-Object{$_.ExecutablePath-eq'C:\ProgramData\Cloudgime\Host\moonlight\web-server.exe'}
if(@($web).Count-ne1){throw 'WEB_PROCESS'}
$actualHash=(Get-FileHash $web.ExecutablePath).Hash.ToLowerInvariant();$hashAt=$start.Elapsed.TotalMilliseconds
# Deliberate synthetic context; only process+file I/O+hash and transport timing are real.
$base.peer.process=$web.ProcessId;$base.ready.process=$web.ProcessId;$base.progress.process=$web.ProcessId
$base.process=[pscustomobject]@{pid=$web.ProcessId;owned=$true;alive=$true;sha256=$actualHash}
$now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
# Local producer freshness is simulated. This is not a live streaming progress claim.
$base.progress.observed_at_ms=$now-500;$base.fixtureAtMs=$now-250
$result=$null;$errorCode=$null
try{$result=Test-CgCaptureGate $e $base $now}catch{$errorCode=$_.Exception.Message}
$writeCheck=[IO.Path]::Combine($CaseDir,'request.json')
# No writer exists in this dry-run entry point; even an eligible request is discarded.
@{atHostMs=$now;dryRun=$true;requestWritten=$false;requestExists=(Test-Path $writeCheck);eligible=($null-ne$result);error=$errorCode;gate=$result;durationMs=$start.Elapsed.TotalMilliseconds;processLookupAndHashMs=$hashAt-$procStart;realProcessPid=$web.ProcessId;actualHash=$actualHash;synthetic=@('billingStatus','rental context','renderer review','SDK progress');coreSha256=$CoreSha256}|ConvertTo-Json -Depth 9 -Compress
