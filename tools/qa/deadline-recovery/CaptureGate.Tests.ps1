param([string]$Core,[string]$Fixture,[string]$Output)
$ErrorActionPreference='Stop';. $Core
$f=Read-CgGateJson $Fixture 8192
$cases=@(
 @{name='valid';code=$null;mutate={param($x)}},
 @{name='stale-backend';code='STALE_BACKEND';mutate={param($x)$x.evidence.backendObservedHostLowerMs=-3000}},
 @{name='stale-review';code='STALE_REVIEW';mutate={param($x)$x.evidence.reviewedHostLowerMs=-5000}},
 @{name='stale-progress';code='STALE_PROGRESS';mutate={param($x)$x.snapshot.progress.observed_at_ms=1}},
 @{name='wrong-claim';code='CONTEXT_MISMATCH';mutate={param($x)$x.snapshot.peer.context.claim_id='claim_other'}},
 @{name='wrong-hash';code='PROCESS_HASH';mutate={param($x)$x.snapshot.process.sha256='c'*64}},
 @{name='wrong-process';code='PROCESS_NOT_OWNED';mutate={param($x)$x.snapshot.process.owned=$false}},
 @{name='ended-rental';code='BILLING_SCOPE';mutate={param($x)$x.evidence.backend.endedAt='ended'}},
 @{name='null-commit';code='BILLING_SCOPE';mutate={param($x)$x.evidence.backend.billingCommitId=$null}},
 @{name='route-relay';code='VISUAL_MODE';mutate={param($x)$x.evidence.route='relay'}},
 @{name='unreviewed';code='VISUAL_MODE';mutate={param($x)$x.evidence.visualApproved=$false}},
 @{name='wrong-local-rental';code='RENTAL_LOCAL_SCOPE';mutate={param($x)$x.snapshot.rental.RentalSessionId='other'}},
 @{name='fixture-background';code='FIXTURE_CHANGED';mutate={param($x)$x.snapshot.fixture.foreground=$false}},
 @{name='existing-request';code='CAPTURE_ALREADY_USED';mutate={param($x)$x.snapshot.requestExists=$true}},
 @{name='quota';code='CAPTURE_BUDGET';mutate={param($x)$x.snapshot.progress.packets_per_second=20000}},
 @{name='nan-rate';code='PROGRESS_INVALID';mutate={param($x)$x.snapshot.progress.packets_per_second=[double]::NaN}},
 @{name='future-time';code='STALE_BACKEND';mutate={param($x)$x.evidence.backendObservedHostLowerMs=1300}}
)
$rows=@();foreach($c in $cases){$x=$f|ConvertTo-Json -Depth 15|ConvertFrom-Json;& $c.mutate $x;$errorCode=$null;$r=$null;try{$r=Test-CgCaptureGate $x.evidence $x.snapshot $x.now}catch{$errorCode=$_.Exception.Message};$pass=($errorCode-ceq$c.code)-and(($c.code)-or$r.eligible);$rows+=@{name=$c.name;pass=[bool]$pass;expected=$c.code;error=$errorCode;ifValidRequest=$r}}
@{pass=(@($rows|Where-Object{!$_.pass}).Count-eq0);cases=$rows.Count;rows=$rows;noRequestsWritten=$true}|ConvertTo-Json -Depth 12|Set-Content $Output
if(@($rows|Where-Object{!$_.pass}).Count){throw 'GATE_TESTS_FAILED'}
Write-Output ('PASS '+$rows.Count+' cases')
