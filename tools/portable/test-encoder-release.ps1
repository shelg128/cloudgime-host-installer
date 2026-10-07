[CmdletBinding()]
param([string]$OutputRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$source=Join-Path $PSScriptRoot 'DisplayPrepareHelper'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if([string]::IsNullOrWhiteSpace($OutputRoot)){$OutputRoot=Join-Path ([IO.Path]::GetTempPath()) ('cg-encoder-release-'+[Guid]::NewGuid().ToString('N'))}
$OutputRoot=[IO.Path]::GetFullPath($OutputRoot)
if(Test-Path $OutputRoot){throw 'ENCODER_OUTPUT_MUST_BE_NEW'}
New-Item -ItemType Directory $OutputRoot|Out-Null
$result=Join-Path $OutputRoot 'encoder-regression.json'
& dotnet run --project (Join-Path $repo 'tools\qa\encoder-profile-tests\EncoderProfileTests.csproj') -c Release -- $result
if($LASTEXITCODE-ne0){throw 'ENCODER_REGRESSION_FAILED'}
$tests=Get-Content $result -Raw|ConvertFrom-Json
if($tests.failed-ne0-or$tests.passed-lt42-or$tests.tests-ne$tests.passed){throw 'ENCODER_REGRESSION_INCOMPLETE'}
$publish=Join-Path $OutputRoot 'publish'
& dotnet publish (Join-Path $source 'DisplayPrepareHelper.csproj') -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:EnableCompressionInSingleFile=true -o $publish
if($LASTEXITCODE-ne0){throw 'ENCODER_HELPER_PUBLISH_FAILED'}
$exe=Join-Path $publish 'display-prepare-helper.exe'
& node (Join-Path $PSScriptRoot 'verify-display-runtime.mjs') record $exe $source
if($LASTEXITCODE-ne0){throw 'DISPLAY_CONTRACT_FAILED'}
& node (Join-Path $PSScriptRoot 'verify-encoder-runtime.mjs') record $exe $source
if($LASTEXITCODE-ne0){throw 'ENCODER_CONTRACT_FAILED'}
@{schema=1;helper=$exe;source=$source;tests=$tests.passed;binarySha256=(Get-FileHash $exe).Hash.ToLowerInvariant();installed=$false;qualityAccepted=$false}|ConvertTo-Json|Set-Content (Join-Path $OutputRoot 'qualification.json') -Encoding UTF8
Write-Output "QUALIFIED_HELPER_NOT_INSTALLED $exe"
