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
