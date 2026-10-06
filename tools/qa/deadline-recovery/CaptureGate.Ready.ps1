param([string]$Root,[string]$CaseDir,[string]$Nonce,[string]$CoreSha256,[string]$EntrySha256,[string]$ExchangeNonce,[switch]$DryRun)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:COMPUTERNAME-ne'RDPGRID-COM122'-or$Root-notmatch'^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$'-or$ExchangeNonce-cnotmatch'^[a-f0-9]{32}$'-or$CoreSha256-cnotmatch'^[a-f0-9]{64}$'-or$EntrySha256-cnotmatch'^[a-f0-9]{64}$'){throw 'READY_GATE_SCOPE'}
$entry=Join-Path $Root $(if($DryRun){'CaptureGate.DryRun.ps1'}else{'CaptureGate.Live.ps1'});$core=Join-Path $Root 'CaptureGate.Core.ps1'
if((Get-FileHash $entry).Hash.ToLowerInvariant()-cne$EntrySha256-or(Get-FileHash $core).Hash.ToLowerInvariant()-cne$CoreSha256){throw 'READY_GATE_HASH'}
if($DryRun){if($CaseDir-notmatch('^'+[regex]::Escape($Root)+'\\dryrun-[a-f0-9]{32}$')){throw 'READY_GATE_CASE'}}elseif($Nonce-cnotmatch'^[a-f0-9]{32}$'){throw 'READY_GATE_NONCE'}
. $core
Add-Type -TypeDefinition @'
using System;using System.Threading;
public static class CgGateLifetime {
 static Timer timer;
 public static void Arm(){timer=new Timer(_=>Environment.Exit(3),null,15000,Timeout.Infinite);}
 public static void Complete(){if(timer!=null)timer.Dispose();}
}
'@
$inbox=Join-Path $Root ('gate-inbox-'+$ExchangeNonce+'.json');if(Test-Path $inbox){throw 'EXISTING_INBOX'}
# Warm module/provider initialization before READY. This is never reused as live
# process/hash authority: the selected entry re-reads all mutable evidence afterward.
$warmWeb=@(Get-CimInstance Win32_Process -Filter "Name='web-server.exe'"|Where-Object{$_.ExecutablePath-eq'C:\ProgramData\Cloudgime\Host\moonlight\web-server.exe'})
if($warmWeb.Count-ne1){throw 'WARMUP_WEB_COUNT'}
$null=Get-FileHash $warmWeb[0].ExecutablePath
try{
 [CgGateLifetime]::Arm();$deadline=[Diagnostics.Stopwatch]::StartNew()
 [Console]::Out.WriteLine((@{type='gate-ready';schema=1;exchangeNonce=$ExchangeNonce;dryRun=[bool]$DryRun;idleTimeoutMs=10000;lifetimeMs=15000;delivery='hashed-private-inbox'}|ConvertTo-Json -Compress));[Console]::Out.Flush()
 $payload=$null
 while($deadline.ElapsedMilliseconds-lt10000){
  if(Test-Path $inbox){$q=$null;try{$q=Read-CgGateJson $inbox 16384}catch{};if($null-ne$q){
   if(@($q.PSObject.Properties.Name).Count-ne4-or$q.exchangeNonce-cne$ExchangeNonce-or$q.sha256-cnotmatch'^[a-f0-9]{64}$'-or$q.payloadBase64-isnot[string]){throw 'INBOX_SCOPE'}
   $bytes=$null;try{$bytes=[Convert]::FromBase64String($q.payloadBase64)}catch{}
   if($null-ne$bytes){if($bytes.Length-gt8192-or$bytes.Length-ne$q.bytes){throw 'INBOX_SIZE'};$sha=[Security.Cryptography.SHA256]::Create();try{$digest=([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()};if($digest-cne$q.sha256){throw 'INBOX_HASH'};if([Convert]::ToBase64String($bytes)-cne$q.payloadBase64){throw 'INBOX_CANONICAL'};$payload=$q.payloadBase64;break;}
  }};Start-Sleep -Milliseconds 20
 }
 if($null-eq$payload){throw 'INBOX_TIMEOUT'}
 if((Get-FileHash $entry).Hash.ToLowerInvariant()-cne$EntrySha256){throw 'ENTRY_CHANGED'}
 if($DryRun){& $entry -CaseDir $CaseDir -EnvelopeBase64 $payload -CoreSha256 $CoreSha256}
 else{& $entry -Root $Root -Nonce $Nonce -EnvelopeBase64 $payload -CoreSha256 $CoreSha256}
 [Console]::Out.Flush();[CgGateLifetime]::Complete()
}catch{[Console]::Out.WriteLine((@{type='gate-error';schema=1;exchangeNonce=$ExchangeNonce;code=$_.Exception.Message.Substring(0,[Math]::Min(200,$_.Exception.Message.Length))}|ConvertTo-Json -Compress));[Console]::Out.Flush();[CgGateLifetime]::Complete();exit 2}
