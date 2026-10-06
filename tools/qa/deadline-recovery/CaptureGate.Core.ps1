# Shared, side-effect-free gate. No file writes, no billing calls, no process control.
Set-StrictMode -Version 2
function Read-CgGateJson([string]$Path,[int]$MaxBytes=8192){
 $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete))
 try{if($stream.Length-le0-or$stream.Length-gt$MaxBytes){throw 'GATE_FILE_SIZE'};$bytes=New-Object byte[] ($MaxBytes+1);$count=0;while($count-lt$bytes.Length){$n=$stream.Read($bytes,$count,$bytes.Length-$count);if($n-eq0){break};$count+=$n};if($count-gt$MaxBytes){throw 'GATE_FILE_GREW'};[Text.Encoding]::UTF8.GetString($bytes,0,$count)|ConvertFrom-Json}finally{$stream.Dispose()}
}
function Get-CgField($Object,[string]$Name){if($null-eq$Object){return $null};$prop=$Object.PSObject.Properties[$Name];if($null-eq$prop){return $null};return $prop.Value}
function Test-CgNumber($Value){return ($Value-is[byte]-or$Value-is[int16]-or$Value-is[int32]-or$Value-is[int64]-or$Value-is[uint32]-or$Value-is[uint64]-or$Value-is[double]-or$Value-is[decimal])-and![double]::IsNaN([double]$Value)-and![double]::IsInfinity([double]$Value)}
function Test-CgInteger($Value){return (Test-CgNumber $Value)-and[double]$Value-ge0-and[double]$Value-le9007199254740991-and[Math]::Floor([double]$Value)-eq[double]$Value}
function Assert-CgFresh($At,[long]$Now,[long]$Budget,[string]$Label){if(!(Test-CgNumber $At)-or[double]$At-lt0-or[double]$At-gt$Now-or$Now-[double]$At-gt$Budget){throw ('STALE_'+$Label)}}
function Test-CgCaptureGate($Evidence,$Snapshot,[long]$Now){
 $e=$Evidence;$s=$Snapshot
 $contextKeys=@('native_session_id','claim_id','handoff_binding_id','rental_session_id','connection_nonce','generation','authority_expires_ms')
 $ctx=Get-CgField $e 'context';$peer=Get-CgField $s 'peer';$ready=Get-CgField $s 'ready';$progress=Get-CgField $s 'progress';$proc=Get-CgField $s 'process';$fixture=Get-CgField $s 'fixture';$rental=Get-CgField $s 'rental';$backend=Get-CgField $e 'backend'
 foreach($k in $contextKeys){$wanted=Get-CgField $ctx $k;if($null-eq$wanted){throw 'CONTEXT_MISSING'};if($k-in@('generation','authority_expires_ms')){if(!(Test-CgInteger $wanted)-or$wanted-le0){throw 'CONTEXT_NUMBER'}}elseif($wanted-isnot[string]-or$wanted.Length-lt1-or$wanted.Length-gt256-or$wanted-notmatch'^[A-Za-z0-9_-]+$'){throw 'CONTEXT_ID'};foreach($row in @($peer,$ready,$progress)){if((Get-CgField (Get-CgField $row 'context') $k)-cne$wanted){throw 'CONTEXT_MISMATCH'}}}
 if($ctx.connection_nonce-cnotmatch'^[a-f0-9]{32}$'-or(Get-CgField $s 'nonce')-cne$ctx.connection_nonce){throw 'NONCE_SCOPE'}
 $pidValue=Get-CgField $peer 'process';$ssrc=Get-CgField $ready 'ssrc';$epoch=Get-CgField $ready 'epoch'
 if(!(Test-CgInteger $pidValue)-or$pidValue-le0-or$pidValue-ne(Get-CgField $ready 'process')-or$pidValue-ne(Get-CgField $progress 'process')-or$pidValue-ne(Get-CgField $proc 'pid')-or!(Test-CgInteger $ssrc)-or$ssrc-le0-or$ssrc-gt4294967295-or$ssrc-ne(Get-CgField $progress 'ssrc')-or!(Test-CgInteger $epoch)-or$epoch-le0-or$epoch-ne(Get-CgField $progress 'epoch')){throw 'PEER_IDENTITY'}
 if((Get-CgField $proc 'owned')-ne$true-or(Get-CgField $proc 'alive')-ne$true){throw 'PROCESS_NOT_OWNED'}
 $expectedHash=Get-CgField $e 'streamerSha256';$actualHash=Get-CgField $proc 'sha256';if($expectedHash-isnot[string]-or$expectedHash-notmatch'^[a-fA-F0-9]{64}$'-or$actualHash-isnot[string]-or$actualHash.ToLowerInvariant()-cne$expectedHash.ToLowerInvariant()){throw 'PROCESS_HASH'}
 Assert-CgFresh (Get-CgField $e 'backendObservedHostLowerMs') $Now 3000 'BACKEND'
 Assert-CgFresh (Get-CgField $e 'reviewedHostLowerMs') $Now 5000 'REVIEW'
 Assert-CgFresh (Get-CgField $progress 'observed_at_ms') $Now 1000 'PROGRESS'
 if((Get-CgField $backend 'billingStatus')-cne'BILLING_RUNNING'-or$null-ne(Get-CgField $backend 'endedAt')-or(Get-CgField $backend 'sessionId')-cne$ctx.rental_session_id-or(Get-CgField $backend 'claimId')-cne$ctx.claim_id-or(Get-CgField $backend 'nativeRoot')-cne$ctx.native_session_id-or(Get-CgField $backend 'billingCommitId')-isnot[string]-or![string](Get-CgField $backend 'billingCommitId')){throw 'BILLING_SCOPE'}
 if((Get-CgField $e 'visualApproved')-ne$true-or(Get-CgField $e 'route')-cne'direct'-or(Get-CgField $e 'width')-ne720-or(Get-CgField $e 'height')-ne1570-or(Get-CgField $e 'fps')-ne30-or(Get-CgField $e 'targetKbps')-ne5000){throw 'VISUAL_MODE'}
 if((Get-CgField $rental 'RentalSessionId')-cne$ctx.rental_session_id-or(Get-CgField $rental 'ClaimId')-cne$ctx.claim_id-or(Get-CgField $rental 'PcId')-cne'2'){throw 'RENTAL_LOCAL_SCOPE'}
 if((Get-CgField $fixture 'sessionId')-cne$ctx.rental_session_id-or(Get-CgField $fixture 'claimId')-cne$ctx.claim_id-or(Get-CgField $fixture 'foreground')-ne$true-or(Get-CgField $fixture 'error')-or(Get-CgField $fixture 'width')-ne720-or(Get-CgField $fixture 'height')-ne1570){throw 'FIXTURE_CHANGED'}
 Assert-CgFresh (Get-CgField $s 'fixtureAtMs') $Now 3000 'FIXTURE'
 if((Get-CgField $progress 'active_trace')-ne$false-or(Get-CgField $s 'requestExists')-ne$false-or(Get-CgField $s 'armedExists')-ne$false-or(Get-CgField $s 'stopExists')-ne$false-or(Get-CgField $s 'stoppedExists')-ne$false){throw 'CAPTURE_ALREADY_USED'}
 $pps=Get-CgField $progress 'packets_per_second';$frames=Get-CgField $progress 'progress_frames';if(!(Test-CgNumber $pps)-or$pps-le0-or!(Test-CgInteger $frames)-or$frames-lt3){throw 'PROGRESS_INVALID'}
 $duration=[Math]::Min(25000,[Math]::Floor(43000/([double]$pps*3+250))*1000);if($duration-lt10000-or$Now+$duration+2000-ge$ctx.authority_expires_ms){throw 'CAPTURE_BUDGET'}
 $nonce=Get-CgField $e 'requestNonce';if($nonce-isnot[string]-or$nonce-cnotmatch'^[a-f0-9]{32}$'){throw 'REQUEST_NONCE'}
 return [pscustomobject]@{eligible=$true;durationMs=[long]$duration;projectedEvents=[Math]::Ceiling(($pps*3+250)*$duration/1000);backendAgeUpperMs=$Now-$e.backendObservedHostLowerMs;reviewAgeUpperMs=$Now-$e.reviewedHostLowerMs;progressAgeMs=$Now-$progress.observed_at_ms;request=[pscustomobject]@{context=$ctx;request_nonce=$nonce;expires_ms=$Now+2000;duration_ms=[long]$duration}}
}
