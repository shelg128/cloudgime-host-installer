param([ValidateSet('apply','restore','watch')][string]$Action,[string]$Root)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:COMPUTERNAME-ne'RDPGRID-COM122'){throw 'HOST'}
if($Root-notmatch'^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$'){throw 'ROOT'}
$m=Get-Content "$Root\manifest.json" -Raw|ConvertFrom-Json
$live='C:\ProgramData\Cloudgime\Host';$service='CloudgimeHost-Host';$sun='CloudgimeRuntime-Host';$reg="HKLM:\SYSTEM\CurrentControlSet\Services\$service"
function Hash([string]$p){(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}
function Idle {if(@(Get-Process 'streamer*','mic_sidecar*','gamepad_sidecar*' -ErrorAction SilentlyContinue).Count){throw 'MEDIA_ACTIVE'}}
function StopServices {
 $state=Get-Content "$live\moonlight\server\host_supervisor_state.json" -Raw|ConvertFrom-Json
 $daemonId=$state.daemon_pid
 $daemon=$null
 if($daemonId){$info=Get-CimInstance Win32_Process -Filter "ProcessId=$daemonId";if($info){if($info.ExecutablePath-ne"$live\moonlight\system\cloudgime-runtime-agent.exe"-or$info.CommandLine-notmatch'run-daemon'){throw 'DAEMON_IDENTITY'};$daemon=Get-Process -Id $daemonId}}
 Stop-Service $service;(Get-Service $service).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(25))
 if($daemon-and!$daemon.HasExited){
  $agent="$live\moonlight\system\cloudgime-runtime-agent.exe"
  $cmd=Start-Process -FilePath $agent -ArgumentList ('--bundle-root "'+$live+'" shutdown-daemon') -WindowStyle Hidden -PassThru
  if(!$cmd.WaitForExit(30000)){throw 'DAEMON_SHUTDOWN_COMMAND_TIMEOUT'}
  if(!$daemon.WaitForExit(10000)){throw 'OLD_DAEMON_STILL_RUNNING'}
 }
 Stop-Service $sun;(Get-Service $sun).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(25))
 Start-Sleep -Seconds 2
 if(@(Get-Process 'web-server','sunshine' -ErrorAction SilentlyContinue).Count){throw 'OWNED_SERVICES_NOT_STOPPED'}
}
function StartServices {Start-Service $sun;Start-Service $service}
# Read-only bounded snapshot called BEFORE service shutdown/rollback, never during measurement.
# Failures are recorded by the caller but must not prevent safety rollback.
function Save-CgPreRollbackEvidence([string]$Root,[string]$Live){
 if($Root-notmatch'^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$'-or$Live-cne'C:\ProgramData\Cloudgime\Host'){throw 'SNAPSHOT_SCOPE'}
 $dest=Join-Path $Root ('pre-rollback-'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($dest)|Out-Null
 $files=@(@{name='sunshine.log';path="$Live\sunshine\config\sunshine.log";max=1048576},@{name='host-stream.log';path="$Live\moonlight\server\host-stream-live.log";max=2097152},@{name='supervisor.log';path="$Live\moonlight\server\host_supervisor.log";max=524288},@{name='capability.json';path="$Live\moonlight\server\host_capability_profile.json";max=262144})
 $rows=@();$total=[Diagnostics.Stopwatch]::StartNew()
 foreach($x in $files){if($total.ElapsedMilliseconds-gt5000){$rows+=@{name=$x.name;error='SNAPSHOT_TIME_LIMIT'};continue};$src=$null;try{
  $src=[IO.File]::Open($x.path,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$length=$src.Length;$start=[Math]::Max(0,$length-$x.max);[void]$src.Seek($start,'Begin');$bytes=New-Object byte[] ([Math]::Min($length,$x.max));$read=0;while($read-lt$bytes.Length){$n=$src.Read($bytes,$read,$bytes.Length-$read);if($n-eq0){break};$read+=$n};$text=[Text.Encoding]::UTF8.GetString($bytes,0,$read)
  if($x.name-eq'capability.json'){if($start-ne0){throw 'PROFILE_TOO_LARGE'};$profile=$text|ConvertFrom-Json;$text=@{selected_encoder=[string]$profile.selected_encoder;selected_capture=[string]$profile.selected_capture;force_nvenc_enabled=$profile.force_nvenc_enabled;sourceLength=$length}|ConvertTo-Json -Compress}
  else{$text=[regex]::Replace($text,'(?i)((?:launchToken|streamTicket|sessionToken|Authorization|password|ticket)[=:]\s*)[^\s"&]+','$1[redacted]')}
  $out=Join-Path $dest $x.name;[IO.File]::WriteAllText($out,$text,(New-Object Text.UTF8Encoding($false)));$rows+=@{name=$x.name;sourceBytes=$length;offset=$start;readBytes=$read;sha256=(Get-FileHash $out).Hash;bounded=$true}
 }catch{$rows+=@{name=$x.name;error=$_.Exception.GetType().Name}}finally{if($src){$src.Dispose()}}}
 [IO.File]::WriteAllText((Join-Path $dest 'manifest.json'),(@{at=[DateTime]::UtcNow.ToString('o');beforeServiceStop=$true;rows=$rows;elapsedMs=$total.ElapsedMilliseconds;notFullLog=$true}|ConvertTo-Json -Depth 5),(New-Object Text.UTF8Encoding($false)))
}

function Restore {
 Idle
 try{Save-CgPreRollbackEvidence -Root $Root -Live $live}catch{try{[IO.File]::WriteAllText((Join-Path $Root "pre-rollback-snapshot-error.txt"),$_.Exception.GetType().Name)}catch{}}
 foreach($b in $m.binaries){$p=Join-Path $live $b.relative;$h=Hash $p;if($h-ne$b.baseline-and$h-ne$b.candidate){throw 'UNKNOWN_LIVE_HASH'};if((Hash "$Root\baseline-$($b.name).exe")-ne$b.baseline){throw 'BACKUP_HASH'}}
 StopServices
 try{foreach($b in $m.binaries){$p=Join-Path $live $b.relative;if((Hash $p)-ne$b.baseline){Copy-Item "$Root\baseline-$($b.name).exe" ($p+'.cg-restore') -Force;[IO.File]::Replace(($p+'.cg-restore'),$p,"$Root\tested-$($b.name).exe")};if((Hash $p)-ne$b.baseline){throw 'RESTORE_VERIFY'}}
 $e=Get-Content "$Root\environment-before.json" -Raw|ConvertFrom-Json;if($e.exists){New-ItemProperty -Path $reg -Name Environment -PropertyType MultiString -Value @($e.value) -Force|Out-Null}else{Remove-ItemProperty -Path $reg -Name Environment -ErrorAction SilentlyContinue}
 }finally{StartServices}
 @{restored=$true;at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content "$Root\restored.json"
}
if($Action-eq'watch'){$deadline=[DateTime]::UtcNow.AddMinutes(25);while([DateTime]::UtcNow-lt$deadline){if(Test-Path "$Root\restored.json"){exit};Start-Sleep 3};for($i=0;$i-lt120;$i++){try{Restore;exit}catch{$_.Exception.Message|Set-Content "$Root\watchdog-last-error.txt";Start-Sleep 5}};throw 'WATCHDOG_RESTORE_BLOCKED'}
if($Action-eq'restore'){Restore;Write-Output 'RESTORED';exit}
Idle
if(Test-Path "$Root\apply-started.json"){throw 'ALREADY_ATTEMPTED'}
foreach($b in $m.binaries){$p=Join-Path $live $b.relative;if((Hash $p)-ne$b.baseline-or(Hash "$Root\candidate-$($b.name).exe")-ne$b.candidate){throw 'PREFLIGHT_HASH'};Copy-Item $p "$Root\baseline-$($b.name).exe";if((Hash "$Root\baseline-$($b.name).exe")-ne$b.baseline){throw 'BACKUP_VERIFY'}}
if((Hash "$live\sunshine\config\sunshine.conf")-ne$m.config){throw 'CONFIG_CHANGED'}
$props=Get-ItemProperty $reg;$exists=$props.PSObject.Properties.Name-contains'Environment';@{exists=$exists;value=if($exists){@($props.Environment)}else{@()}}|ConvertTo-Json -Depth 4|Set-Content "$Root\environment-before.json"
$settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 40)
$a=New-ScheduledTaskAction -Execute powershell.exe -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$Root\deploy.ps1`" -Action watch -Root `"$Root`""
$p=New-ScheduledTaskPrincipal -UserId SYSTEM -LogonType ServiceAccount -RunLevel Highest
Register-ScheduledTask $m.task -Action $a -Principal $p -Settings $settings|Out-Null;Start-ScheduledTask $m.task
@{at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content "$Root\apply-started.json"
try{StopServices
 foreach($b in $m.binaries){$p=Join-Path $live $b.relative;Copy-Item "$Root\candidate-$($b.name).exe" ($p+'.cg-new') -Force;[IO.File]::Replace(($p+'.cg-new'),$p,"$Root\replaced-$($b.name).exe");if((Hash $p)-ne$b.candidate){throw 'INSTALL_HASH'}}
 $values=@();if($exists){$values+=@($props.Environment|Where-Object{$_-notmatch'^CG_QA_(ENABLE_SCOPED_TRACE|SCOPED_TRACE_ROOT)='})};$values+='CG_QA_ENABLE_SCOPED_TRACE=1';$values+="CG_QA_SCOPED_TRACE_ROOT=$Root\traces";New-ItemProperty $reg -Name Environment -PropertyType MultiString -Value $values -Force|Out-Null
 StartServices
 @{at=[DateTime]::UtcNow.ToString('o');installed=$true}|ConvertTo-Json|Set-Content "$Root\installed.json";Write-Output 'INSTALLED'
}catch{$errorMessage=$_.Exception.Message;try{Restore}catch{$_|Out-String|Set-Content "$Root\restore-error.txt"};throw $errorMessage}
