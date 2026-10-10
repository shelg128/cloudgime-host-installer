[CmdletBinding()]
param(
    [string]$ApiBaseUrl = 'https://cloudgime.my.id',
    [string]$ExpectedHostId = 'host-pc01-mtk6sz2v',
    [string]$ExpectedPcId = '1',
    [string]$ExpectedDeviceId = 'PC-01',
    [string]$ResultPath = '',
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
[void][Reflection.Assembly]::LoadWithPartialName('System.Security')
$provisioningRoot = 'C:\ProgramData\Cloudgime\provisioning'
$hostRoot = 'C:\ProgramData\Cloudgime\Host'
$renewalPath = Join-Path $provisioningRoot 'installer_renewal.credential.dpapi'
$identityPath = Join-Path $provisioningRoot 'machine-identity.dpapi'
$manifestPath = Join-Path $provisioningRoot 'manifest.json'
$statePath = Join-Path $hostRoot 'moonlight\server\host_activation_state.json'
$backupRoot = 'C:\ProgramData\Cloudgime\backups\universal-migration-pc01-20260910'

function Read-MachineDpapiJson([string]$Path) {
    $encrypted = [IO.File]::ReadAllBytes($Path)
    $plain = [Security.Cryptography.ProtectedData]::Unprotect($encrypted,$null,[Security.Cryptography.DataProtectionScope]::LocalMachine)
    try { return [Text.Encoding]::UTF8.GetString($plain) | ConvertFrom-Json }
    finally { [Array]::Clear($plain,0,$plain.Length); [Array]::Clear($encrypted,0,$encrypted.Length) }
}
function Write-MachineDpapiJsonNew([string]$Path,[object]$Value) {
    if (Test-Path $Path) { throw 'DPAPI_TARGET_ALREADY_EXISTS' }
    $plain = [Text.Encoding]::UTF8.GetBytes(($Value|ConvertTo-Json -Compress))
    try {
        $encrypted = [Security.Cryptography.ProtectedData]::Protect($plain,$null,[Security.Cryptography.DataProtectionScope]::LocalMachine)
        $temp = "$Path.$PID.tmp"
        [IO.File]::WriteAllBytes($temp,$encrypted)
        & icacls.exe $temp /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' /remove:g '*S-1-5-32-545' '*S-1-5-11' '*S-1-1-0' | Out-Null
        if ($LASTEXITCODE -ne 0) { Remove-Item $temp -Force -ErrorAction SilentlyContinue; throw 'DPAPI_ACL_FAILED' }
        [IO.File]::Move($temp,$Path)
    } finally { [Array]::Clear($plain,0,$plain.Length); if($encrypted){[Array]::Clear($encrypted,0,$encrypted.Length)} }
}
function Write-RestrictedJsonAtomic([string]$Path,[object]$Value) {
    $temp="$Path.$PID.next"
    [IO.File]::WriteAllText($temp,($Value|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
    & icacls.exe $temp /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' /remove:g '*S-1-5-32-545' '*S-1-5-11' '*S-1-1-0' | Out-Null
    if($LASTEXITCODE -ne 0){Remove-Item $temp -Force -ErrorAction SilentlyContinue;throw 'STATE_ACL_FAILED'}
    Move-Item $temp $Path -Force
    & icacls.exe $Path /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' /remove:g '*S-1-5-32-545' '*S-1-5-11' '*S-1-1-0' | Out-Null
    if($LASTEXITCODE -ne 0){throw 'FINAL_STATE_ACL_FAILED'}
}
function Normalize-PcId([string]$Value){$v=$Value.Trim();if($v -match '^(?i:PC-)?0*([0-9]+)$'){return ([int64]$Matches[1]).ToString()};return $v}
function Require-Compatible([string]$Name,[string]$Existing,[string]$Expected){if($Existing -and $Existing -ne $Expected){throw "${Name}_CONFLICT"}}

$principal=[Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'ADMIN_OR_SYSTEM_REQUIRED'}
if(-not $ValidateOnly -and -not $env:CG_MIGRATION_JWT){throw 'MISSING_MIGRATION_JWT'}
foreach($p in @($identityPath,$manifestPath,$statePath)){if(-not(Test-Path $p)){throw "MISSING_REQUIRED_FILE:$([IO.Path]::GetFileName($p))"}}
$base=[Uri]$ApiBaseUrl;if($base.Scheme -ne 'https' -or $base.UserInfo){throw 'HTTPS_API_REQUIRED'}

$identity=Read-MachineDpapiJson $identityPath
$machineIdentity=[string]$identity.machineIdentity;if(-not $machineIdentity){$machineIdentity=[string]$identity.machine_identity}
$installInstanceId=[string]$identity.installInstanceId;if(-not $installInstanceId){$installInstanceId=[string]$identity.install_instance_id}
if(-not $machineIdentity -or -not $installInstanceId){throw 'INVALID_EXISTING_MACHINE_IDENTITY'}
$manifest=Get-Content $manifestPath -Raw|ConvertFrom-Json
if([int]$manifest.schemaVersion -ne 2){throw 'MANIFEST_SCHEMA_INVALID'}
if(([Uri][string]$manifest.controlPlaneUrl).GetLeftPart([UriPartial]::Authority) -ne $base.GetLeftPart([UriPartial]::Authority)){throw 'MANIFEST_CONTROL_PLANE_MISMATCH'}
if([string]$manifest.machineIdentity -ne $machineIdentity -or [string]$manifest.installInstanceId -ne $installInstanceId){throw 'MANIFEST_IDENTITY_MISMATCH'}
$hostId=[string]$manifest.binding.hostId;$pcId=Normalize-PcId ([string]$manifest.binding.pcId);$deviceId=[string]$manifest.binding.deviceId
if($hostId -ne $ExpectedHostId -or $pcId -ne (Normalize-PcId $ExpectedPcId) -or $deviceId -ne $ExpectedDeviceId){throw 'MANIFEST_BINDING_MISMATCH'}
$hostComponents=@($manifest.components|Where-Object componentType -eq 'host_app')
if($hostComponents.Count -ne 1 -or -not [string]$hostComponents[0].applicationActivationId){throw 'HOST_COMPONENT_AMBIGUOUS'}
$applicationActivationId=[string]$hostComponents[0].applicationActivationId

$state=Get-Content $statePath -Raw|ConvertFrom-Json
Require-Compatible 'STATE_HOST_ID' ([string]$state.HostId) $hostId
Require-Compatible 'STATE_MACHINE_IDENTITY' ([string]$state.MachineIdentity) $machineIdentity
Require-Compatible 'STATE_INSTALL_IDENTITY' ([string]$state.InstallInstanceId) $installInstanceId
Require-Compatible 'STATE_PC_ID' (Normalize-PcId ([string]$state.SentinelPcId)) $pcId
Require-Compatible 'STATE_DEVICE_ID' ([string]$state.SentinelDeviceId) $deviceId
Require-Compatible 'STATE_APPLICATION_ACTIVATION' ([string]$state.ApplicationActivationId) $applicationActivationId

if($ValidateOnly){
 $result=[pscustomobject]@{ok=$true;validateOnly=$true;hostId=$hostId;pcId=$pcId;deviceId=$deviceId;identityPreserved=$true;manifestBindingValid=$true;stateCompatible=$true;renewalExists=(Test-Path $renewalPath)}
 if($ResultPath){[IO.File]::WriteAllText([IO.Path]::GetFullPath($ResultPath),($result|ConvertTo-Json -Compress),[Text.UTF8Encoding]::new($false))}
 $result|ConvertTo-Json -Compress
 return
}

New-Item -ItemType Directory -Force $backupRoot|Out-Null
& icacls.exe $backupRoot /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if($LASTEXITCODE -ne 0){throw 'BACKUP_ACL_FAILED'}
foreach($p in @($identityPath,$manifestPath,$statePath)){Copy-Item $p (Join-Path $backupRoot ([IO.Path]::GetFileName($p))) -Force}

if(Test-Path $renewalPath){$renewal=Read-MachineDpapiJson $renewalPath;$renewalCredential=[string]$renewal.renewalCredential;if([int64]$renewal.repairGeneration -ne 0 -or $renewal.pendingOperationId){throw 'EXISTING_RENEWAL_STATE_NOT_MIGRATION_BASELINE'}}
else{$random=New-Object byte[] 32;$rng=[Security.Cryptography.RandomNumberGenerator]::Create();try{$rng.GetBytes($random)}finally{$rng.Dispose()};$renewalCredential='cgrenew_'+[Convert]::ToBase64String($random).TrimEnd('=').Replace('+','-').Replace('/','_');Write-MachineDpapiJsonNew $renewalPath ([ordered]@{renewalCredential=$renewalCredential;repairGeneration=0;pendingOperationId=$null});[Array]::Clear($random,0,$random.Length)}
if($renewalCredential.Length -lt 32){throw 'INVALID_RENEWAL_CREDENTIAL'}

try{
 $body=@{machineIdentity=$machineIdentity;installInstanceId=$installInstanceId;requestedDeviceId=$deviceId;renewalCredential=$renewalCredential}|ConvertTo-Json -Compress
 $headers=@{Authorization="Bearer $env:CG_MIGRATION_JWT"}
 $uri="$($base.GetLeftPart([UriPartial]::Authority))/api/v1/admin/provisioning/universal-node/$hostId/repair-migration"
 $first=Invoke-RestMethod -Method Post -Uri $uri -Headers $headers -ContentType 'application/json' -Body $body
 if(-not $first.ok -or $first.hostId -ne $hostId){throw 'MIGRATION_NOT_CONFIRMED'}
 $second=Invoke-RestMethod -Method Post -Uri $uri -Headers $headers -ContentType 'application/json' -Body $body
 if(-not $second.ok -or $second.hostId -ne $hostId -or $second.migrated){throw 'MIGRATION_RETRY_NOT_IDEMPOTENT'}

 $state.SchemaVersion=1;$state.HostId=$hostId;$state.MachineIdentity=$machineIdentity;$state.InstallInstanceId=$installInstanceId;$state.ActivationState='activated';$state.ControlPlaneUrl=$base.GetLeftPart([UriPartial]::Authority);$state.SentinelPcId=$pcId;$state.SentinelDeviceId=$deviceId;$state.ApplicationActivationId=$applicationActivationId;$state.ApplicationType='CLOUDGIME_HOST';$state.LastHeartbeatAtUtc='';$state.LastReadyForStream=$false;$state.UpdatedAtUtc=[DateTimeOffset]::UtcNow.ToString('O')
 Write-RestrictedJsonAtomic $statePath $state
 $result=[pscustomobject]@{ok=$true;migrated=[bool]$first.migrated;retryIdempotent=$true;hostId=$hostId;pcId=$pcId;identityPreserved=$true;renewalStored=$true;stateReconciled=$true;backup=$backupRoot}
 if($ResultPath){[IO.File]::WriteAllText([IO.Path]::GetFullPath($ResultPath),($result|ConvertTo-Json -Compress),[Text.UTF8Encoding]::new($false))}
 $result|ConvertTo-Json -Compress
}finally{Remove-Item Env:CG_MIGRATION_JWT -ErrorAction SilentlyContinue;$renewalCredential=$null;$body=$null;$headers=$null}
