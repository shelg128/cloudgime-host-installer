[CmdletBinding()]
param(
 [Parameter(Mandatory=$true)][string]$InputBundleRoot,
 [Parameter(Mandatory=$true)][string]$OutputBundleRoot,
 [Parameter(Mandatory=$true)][string]$SentinelInstallerPath,
 [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$SentinelInstallerSha256,
 [Parameter(Mandatory=$true)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$ReleaseVersion,
 [switch]$FixtureOnly
)
Set-StrictMode -Version Latest;$ErrorActionPreference='Stop'
$InputBundleRoot=[IO.Path]::GetFullPath($InputBundleRoot);$OutputBundleRoot=[IO.Path]::GetFullPath($OutputBundleRoot);$SentinelInstallerPath=[IO.Path]::GetFullPath($SentinelInstallerPath)
if($OutputBundleRoot.StartsWith($InputBundleRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or $InputBundleRoot.StartsWith($OutputBundleRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'INPUT_OUTPUT_OVERLAP'}
if(-not(Test-Path $InputBundleRoot -PathType Container)){throw 'INPUT_BUNDLE_MISSING'}
if((Get-Item -LiteralPath $InputBundleRoot -Force).Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'REPARSE_INPUT_ROOT'}
if(Test-Path $OutputBundleRoot){throw 'OUTPUT_BUNDLE_MUST_BE_NEW'}
if((Get-FileHash $SentinelInstallerPath -Algorithm SHA256).Hash-ne$SentinelInstallerSha256){throw 'SENTINEL_HASH_MISMATCH'}
# Copy without following reparse points.
$queue=New-Object 'Collections.Generic.Queue[object]';$queue.Enqueue(@($InputBundleRoot,$OutputBundleRoot));New-Item -ItemType Directory $OutputBundleRoot|Out-Null
while($queue.Count){$pair=$queue.Dequeue();foreach($e in Get-ChildItem -LiteralPath $pair[0] -Force){if($e.Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'REPARSE_INPUT_ENTRY'};$dst=Join-Path $pair[1] $e.Name;if($e.PSIsContainer){New-Item -ItemType Directory $dst -Force|Out-Null;$queue.Enqueue(@($e.FullName,$dst))}else{Copy-Item -LiteralPath $e.FullName -Destination $dst}}}
# Delete all machine/enrollment state from the candidate, even if a legacy builder emitted it.
$blocked=@('provisioning','keeper-tunnel\data','moonlight\server\sunshine-shared','sunshine\config\credentials','sunshine-legacy\config\credentials')
foreach($r in $blocked){$p=Join-Path $OutputBundleRoot $r;if(Test-Path $p){Remove-Item $p -Recurse -Force}}
foreach($pattern in @('*.dpapi','*.pem','*.key','.env*','*.env')){Get-ChildItem $OutputBundleRoot -Recurse -Force -File -Filter $pattern -ErrorAction SilentlyContinue|Remove-Item -Force}
foreach($relative in @('PUBLIC_URL.txt','moonlight\server\host_activation_state.json','moonlight\server\shared_pair_info.json','sunshine\config\sunshine_state.json','sunshine-legacy\config\sunshine_state.json','cloudgime-host-bundle.json')){$p=Join-Path $OutputBundleRoot $relative;if(Test-Path $p){Remove-Item $p -Force}}
# Reset web runtime to first-run loopback state; never preserve a builder's user/pairing.
$configPath=Join-Path $OutputBundleRoot 'moonlight\server\config.json';$config=Get-Content $configPath -Raw|ConvertFrom-Json
$web=$config.web_server
foreach($entry in @(
    @('bind_address','127.0.0.1:18081'),
    @('url_path_prefix','/stream'),
    @('default_user_id',$null),
    @('first_login_create_admin',$true),
    @('first_login_assign_global_hosts',$false),
    @('rental_stream_ticket_required',$true),
    @('rental_access_revocation_enabled',$true)
)){ $web | Add-Member -NotePropertyName $entry[0] -NotePropertyValue $entry[1] -Force }
[IO.File]::WriteAllText($configPath,($config|ConvertTo-Json -Depth 50),[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $OutputBundleRoot 'moonlight\server\data.json'),'{"users":{},"hosts":{}}',[Text.UTF8Encoding]::new($false))
$pairSource=Join-Path $PSScriptRoot 'auto-pair-host.ps1';if(-not(Test-Path $pairSource)){throw 'PAIRING_SCRIPT_MISSING'};Copy-Item $pairSource (Join-Path $OutputBundleRoot 'auto-pair-host.ps1')
New-Item -ItemType Directory (Join-Path $OutputBundleRoot 'sentinel') -Force|Out-Null;Copy-Item $SentinelInstallerPath (Join-Path $OutputBundleRoot 'sentinel\SentinelLauncherSetup.exe')
# Seal last. The manifest itself is intentionally not listed in files.
$packProject=Join-Path $PSScriptRoot 'HostControlPackaging\HostControlPackaging.csproj';& dotnet build $packProject -c Release;if($LASTEXITCODE-ne0){throw 'PACKAGING_BUILD_FAILED'}
$dll=Join-Path $PSScriptRoot 'HostControlPackaging\bin\Release\net8.0\HostControlPackaging.dll'
$sealArgs=@('seal-universal-bundle','--bundle-root',$OutputBundleRoot,'--release-version',$ReleaseVersion)
if($FixtureOnly){$sealArgs+='--fixture-only'}
& dotnet $dll @sealArgs;if($LASTEXITCODE-ne0){throw 'UNIVERSAL_SEAL_FAILED'}
Write-Output 'UNIVERSAL_BUNDLE_FINALIZED_NEUTRAL_AND_VALIDATED'
