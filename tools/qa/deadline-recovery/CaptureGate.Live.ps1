param([string]$Root,[string]$Nonce,[string]$EnvelopeBase64,[string]$CoreSha256)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:COMPUTERNAME-ne'RDPGRID-COM122'-or$Root-notmatch'^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$'-or$Nonce-cnotmatch'^[a-f0-9]{32}$'){throw 'LIVE_GATE_SCOPE'}
$core=Join-Path $Root 'CaptureGate.Core.ps1';if($CoreSha256-notmatch'^[a-f0-9]{64}$'-or(Get-FileHash $core).Hash.ToLowerInvariant()-cne$CoreSha256){throw 'CORE_HASH'}
. $core
$raw=[Convert]::FromBase64String($EnvelopeBase64);if($raw.Length-gt8192){throw 'ENVELOPE_SIZE'};$e=[Text.Encoding]::UTF8.GetString($raw)|ConvertFrom-Json
$dir=Join-Path "$Root\traces" $Nonce
$peer=Read-CgGateJson "$dir\peer.json";$ready=Read-CgGateJson "$dir\ready.json"
$process=Get-CimInstance Win32_Process -Filter ('ProcessId='+[int]$peer.process)
$prefix='C:\ProgramData\Cloudgime\Host\moonlight\staged-runtime\'
if(!$process-or$process.Name-notmatch'^streamer-.*\.exe$'-or!$process.ExecutablePath.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'PROCESS_NOT_OWNED'}
$hash=(Get-FileHash $process.ExecutablePath).Hash.ToLowerInvariant();$handle=Get-Process -Id $peer.process
$processStart=([DateTimeOffset]$handle.StartTime).ToUnixTimeMilliseconds();if($processStart-ne([DateTimeOffset]$process.CreationDate).ToUnixTimeMilliseconds()){throw 'PROCESS_REUSED'}
# Exact path binding before reading local rental/fixture JSON.
$ctx=$peer.context;if($ctx.connection_nonce-cne$Nonce-or$ctx.rental_session_id-notmatch'^[A-Za-z0-9_-]{1,256}$'){throw 'CONTEXT_PATH'}
$fixturePath='C:\Users\CloudGimeRenter\AppData\Local\CgAvSync\'+$ctx.rental_session_id+'-scoped\state.json'
if($e.fixtureState-cne$fixturePath){throw 'FIXTURE_PATH'}
$rental=Read-CgGateJson 'C:\Users\CloudGimeRenter\AppData\Roaming\SentinelLauncher\state\session.json' 8192
$fixture=Read-CgGateJson $fixturePath 8192
$progress=$null;for($i=0;$i-lt3;$i++){try{$progress=Read-CgGateJson "$dir\progress.json";break}catch{Start-Sleep -Milliseconds 20}};if(!$progress){throw 'PROGRESS_UNREADABLE'}
$s=[pscustomobject]@{nonce=$Nonce;peer=$peer;ready=$ready;progress=$progress;process=[pscustomobject]@{pid=$process.ProcessId;owned=$true;alive=(!$handle.HasExited);sha256=$hash};fixture=$fixture;fixtureAtMs=([DateTimeOffset]::Parse($fixture.at).ToUnixTimeMilliseconds());rental=$rental;requestExists=(Test-Path "$dir\request.json");armedExists=(Test-Path "$dir\armed.json");stopExists=(Test-Path "$dir\stop.json");stoppedExists=(Test-Path "$dir\gate-stopped.json")}
$now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();$result=Test-CgCaptureGate $e $s $now
# Recheck mutable scope and freshness after expensive process/hash work, before creation.
$s.rental=Read-CgGateJson 'C:\Users\CloudGimeRenter\AppData\Roaming\SentinelLauncher\state\session.json' 8192
$s.process.alive=(!$handle.HasExited);$s.stopExists=Test-Path "$dir\stop.json";$s.stoppedExists=Test-Path "$dir\gate-stopped.json"
$now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();$result=Test-CgCaptureGate $e $s $now
# Seal the collection conditions before request creation. This receipt is not a review approval.
$receipt=@{schema=1;requestNonce=$e.requestNonce;protocol=(Get-CgField $e 'protocol');analysisDisposition=$result.analysisDisposition;context=$peer.context;image=(Get-CgField $e 'image');hostMs=$now;imageAgeUpperMs=$result.imageAgeUpperMs;backendAgeUpperMs=$result.backendAgeUpperMs;progressAgeMs=$result.progressAgeMs;durationMs=$result.durationMs;fixture=$fixture;processId=$peer.process;coreSha256=$CoreSha256}
$receiptPath=Join-Path $dir ('collection-conditions-'+$e.requestNonce+'.json');$receiptBytes=[Text.Encoding]::UTF8.GetBytes(($receipt|ConvertTo-Json -Depth 9 -Compress));$rf=[IO.File]::Open($receiptPath,'CreateNew','Write','None');try{$rf.Write($receiptBytes,0,$receiptBytes.Length)}finally{$rf.Dispose()}
# Recheck original evidence ages after receipt I/O; never restamp captured data.
$now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();$result=Test-CgCaptureGate $e $s $now
$json=$result.request|ConvertTo-Json -Depth 7 -Compress;$bytes=[Text.Encoding]::UTF8.GetBytes($json)
$f=[IO.File]::Open("$dir\request.json",'CreateNew','Write','None');try{$f.Write($bytes,0,$bytes.Length)}finally{$f.Dispose()}
# Receipt is best effort after request creation; caller must reconcile request/armed if reply lost.
@{requested=$true;dryRun=$false;hostMs=$now;durationMs=$result.durationMs;projectedEvents=$result.projectedEvents;backendAgeUpperMs=$result.backendAgeUpperMs;reviewAgeUpperMs=$result.reviewAgeUpperMs;imageAgeUpperMs=$result.imageAgeUpperMs;analysisDisposition=$result.analysisDisposition;progressAgeMs=$result.progressAgeMs;coreSha256=$CoreSha256}|ConvertTo-Json -Depth 5 -Compress
