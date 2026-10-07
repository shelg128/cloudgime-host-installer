[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$RawBundleRoot,
    [Parameter(Mandatory=$true)][string]$AppRoot,
    [Parameter(Mandatory=$true)][string]$SentinelInstallerPath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$SentinelInstallerSha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$ReleaseVersion,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [switch]$InternalUnsignedCandidate,
    [switch]$FixtureOnly
)
if($FixtureOnly -and -not$InternalUnsignedCandidate){throw 'FIXTURE_REQUIRES_INTERNAL_CANDIDATE'}
Set-StrictMode -Version Latest;$ErrorActionPreference='Stop'
$RawBundleRoot=[IO.Path]::GetFullPath($RawBundleRoot);$AppRoot=[IO.Path]::GetFullPath($AppRoot);$SentinelInstallerPath=[IO.Path]::GetFullPath($SentinelInstallerPath);$OutputDir=[IO.Path]::GetFullPath($OutputDir)
if(Test-Path $OutputDir){throw 'OUTPUT_MUST_BE_NEW'}
foreach($p in @($RawBundleRoot,$AppRoot)){if(-not(Test-Path $p -PathType Container)){throw 'INPUT_ROOT_MISSING'};if($OutputDir.StartsWith($p.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'OUTPUT_NESTED_IN_INPUT'}}
if((Get-FileHash $SentinelInstallerPath -Algorithm SHA256).Hash-ne$SentinelInstallerSha256){throw 'SENTINEL_HASH_MISMATCH'}
if(-not$InternalUnsignedCandidate-and(Get-AuthenticodeSignature $SentinelInstallerPath).Status-ne'Valid'){throw 'SIGNED_SENTINEL_REQUIRED'}
if(-not$FixtureOnly){
 $helper=Join-Path $RawBundleRoot 'moonlight\server\display-prepare-helper.exe'
 & node (Join-Path $PSScriptRoot 'verify-encoder-runtime.mjs') verify $helper (Join-Path $PSScriptRoot 'DisplayPrepareHelper')
 if($LASTEXITCODE-ne0){throw 'RELEASE_ENCODER_SOURCE_OR_BINARY_MISMATCH'}
}
foreach($n in @('cloudgime-host-control.exe','open-host-control.exe','uninstaller-cloudgime.exe')){if(-not(Test-Path (Join-Path $AppRoot $n) -PathType Leaf)){throw "APP_PAYLOAD_MISSING:$n"}}
# Seal a sanitized copy; RawBundleRoot is never mutated.
$bundle=Join-Path $OutputDir 'sealed-bundle';New-Item -ItemType Directory $OutputDir|Out-Null
& (Join-Path $PSScriptRoot 'finalize-universal-bundle.ps1') -InputBundleRoot $RawBundleRoot -OutputBundleRoot $bundle -SentinelInstallerPath $SentinelInstallerPath -SentinelInstallerSha256 $SentinelInstallerSha256 -ReleaseVersion $ReleaseVersion -FixtureOnly:$FixtureOnly
if($LASTEXITCODE-ne0){throw 'FINALIZE_UNIVERSAL_FAILED'}
$packProject=Join-Path $PSScriptRoot 'HostControlPackaging\HostControlPackaging.csproj';& dotnet build $packProject -c Release;if($LASTEXITCODE-ne0){throw 'PACKAGING_BUILD_FAILED'}
$dll=Join-Path $PSScriptRoot 'HostControlPackaging\bin\Release\net8.0\HostControlPackaging.dll';& dotnet $dll validate-universal-bundle --bundle-root $bundle;if($LASTEXITCODE-ne0){throw 'UNIVERSAL_BUNDLE_INVALID'}
$payload=Join-Path $OutputDir 'payload';$app=Join-Path $payload 'app';$bundlePayload=Join-Path $payload 'bundle';New-Item -ItemType Directory $app,$bundlePayload -Force|Out-Null
foreach($n in @('cloudgime-host-control.exe','open-host-control.exe','uninstaller-cloudgime.exe')){Copy-Item (Join-Path $AppRoot $n) $app}
Copy-Item (Join-Path $bundle '*') $bundlePayload -Recurse
$bootstrapProject=Join-Path $PSScriptRoot 'HostControlBootstrap\HostControlBootstrap.csproj';$bootstrapOut=Join-Path $OutputDir 'bootstrap-build'
& dotnet publish $bootstrapProject -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -o $bootstrapOut;if($LASTEXITCODE-ne0){throw 'BOOTSTRAP_PUBLISH_FAILED'}
Copy-Item (Join-Path $bootstrapOut 'cloudgime-host-bootstrap.exe') $app
& dotnet $dll validate-universal-bundle --bundle-root $bundlePayload;if($LASTEXITCODE-ne0){throw 'STAGED_BUNDLE_INVALID'}
if(-not$FixtureOnly){& dotnet $dll validate-installable-bundle --bundle-root $bundlePayload --app-root $app;if($LASTEXITCODE-ne0){throw 'NON_INSTALLABLE_PAYLOAD'}}
$makensis=@('C:\Program Files (x86)\NSIS\makensis.exe','C:\Program Files\NSIS\makensis.exe')|Where-Object{Test-Path $_}|Select-Object -First 1;if(-not$makensis){$makensis=(Get-Command makensis.exe -ErrorAction Stop).Source}
$nsis=Join-Path $PSScriptRoot 'HostControlApp.Tauri\installer\CloudgimeHostControl.Universal.nsi'
$installerName=if($FixtureOnly){'CloudgimeHostPackageFixture-NOT-INSTALLABLE.exe'}else{'CloudgimeHostSetup.exe'}
& $makensis /V2 ('/DINSTALLER_FILE='+$installerName) ('/DOUTPUT_DIR='+$OutputDir) ('/DPAYLOAD_APP='+$app) ('/DPAYLOAD_BUNDLE='+$bundlePayload) ('/DPRODUCT_VERSION='+$ReleaseVersion) ('/DPRODUCT_VERSION_QUAD='+$ReleaseVersion+'.0') $nsis;if($LASTEXITCODE-ne0){throw 'NSIS_BUILD_FAILED'}
$exe=Join-Path $OutputDir $installerName;if(-not(Test-Path $exe)){throw 'INSTALLER_NOT_CREATED'}
$report=[ordered]@{schemaVersion=1;releaseVersion=$ReleaseVersion;candidateOnly=$true;fixtureOnly=[bool]$FixtureOnly;installable=(-not[bool]$FixtureOnly);installerFile=$installerName;installed=$false;cleanInstallTested=$false;upgradeIdentityPreservationTested=$false;coldBootTested=$false;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;sentinelSha256=$SentinelInstallerSha256;authenticode=(Get-AuthenticodeSignature $exe).Status.ToString();createdAt=[DateTime]::UtcNow.ToString('o')}
[IO.File]::WriteAllText((Join-Path $OutputDir 'candidate-report.json'),($report|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
Write-Output 'UNIVERSAL_INSTALLER_CANDIDATE_BUILT_NOT_DEPLOYED_OR_RELEASED'
