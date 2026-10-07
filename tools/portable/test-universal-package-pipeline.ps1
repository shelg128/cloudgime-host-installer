param([string]$EvidenceRoot)
$ErrorActionPreference='Stop'
$root=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if(-not$EvidenceRoot){$EvidenceRoot=Join-Path $root ('test-results\package-pipeline-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))}
$base=[IO.Path]::GetFullPath($EvidenceRoot);if(Test-Path $base){throw 'TEST_OUTPUT_MUST_BE_NEW'};$raw=Join-Path $base 'raw';$app=Join-Path $base 'app';$out=Join-Path $base 'out';New-Item -ItemType Directory $raw,$app|Out-Null
$required=@('host-installer.exe','moonlight\web-server.exe','moonlight\streamer.exe','moonlight\system\cloudgime-runtime-agent.exe','moonlight\static\stream.js','moonlight\static\stream\transport\webrtc.js','sunshine\sunshine.exe','keeper-tunnel\cloudgimehosttunnel.exe')
foreach($r in $required){$p=Join-Path $raw $r;New-Item -ItemType Directory (Split-Path $p) -Force|Out-Null;[IO.File]::WriteAllText($p,'INERT_TEST_FIXTURE_NOT_EXECUTABLE',[Text.Encoding]::ASCII)}
New-Item -ItemType Directory (Join-Path $raw 'moonlight\server') -Force|Out-Null
[IO.File]::WriteAllText((Join-Path $raw 'moonlight\server\config.json'),'{"web_server":{"bind_address":"127.0.0.1:18081","url_path_prefix":"/stream","default_user_id":42,"first_login_create_admin":false},"moonlight":{"default_http_port":49000}}',[Text.Encoding]::ASCII)
[IO.File]::WriteAllText((Join-Path $raw 'moonlight\server\data.json'),'{"users":{"42":{"name":"builder"}},"hosts":{"7":{"pair_info":{"private":"must-be-erased"}}}}',[Text.Encoding]::ASCII)
# Seed files that finalizer must remove.
foreach($r in @('PUBLIC_URL.txt','moonlight\server\host_capability_profile.json','moonlight\server\selected_sunshine_runtime.txt','moonlight\server\stream_display_preferences.json','moonlight\server\host_activation_state.json','moonlight\server\shared_pair_info.json','keeper-tunnel\data\cloudrental.env','provisioning\host_app.credential.dpapi','sunshine\config\credentials\key.pem')){$p=Join-Path $raw $r;New-Item -ItemType Directory (Split-Path $p) -Force|Out-Null;[IO.File]::WriteAllText($p,'MACHINE_STATE_FIXTURE',[Text.Encoding]::ASCII)}
$sentinel=Join-Path $base 'SentinelLauncherSetup.exe';[IO.File]::WriteAllText($sentinel,'INERT_SENTINEL_FIXTURE',[Text.Encoding]::ASCII);$sha=(Get-FileHash $sentinel).Hash
foreach($n in @('cloudgime-host-control.exe','open-host-control.exe','uninstaller-cloudgime.exe')){[IO.File]::WriteAllText((Join-Path $app $n),'INERT_APP_FIXTURE',[Text.Encoding]::ASCII)}
& (Join-Path $root 'tools\portable\build-universal-host-installer.ps1') -RawBundleRoot $raw -AppRoot $app -SentinelInstallerPath $sentinel -SentinelInstallerSha256 $sha -ReleaseVersion '1.2.0' -OutputDir $out -InternalUnsignedCandidate -FixtureOnly
if($LASTEXITCODE-ne0){throw 'PIPELINE_FAILED'}
$report=Get-Content (Join-Path $out 'candidate-report.json') -Raw|ConvertFrom-Json
if(-not$report.candidateOnly-or-not$report.fixtureOnly-or$report.installable-or$report.installed-or$report.cleanInstallTested-or$report.coldBootTested){throw 'CANDIDATE_REPORT_INVALID'}
if(Test-Path (Join-Path $out 'sealed-bundle\provisioning')){throw 'PROVISIONING_LEAKED'}
if(Test-Path (Join-Path $out 'sealed-bundle\PUBLIC_URL.txt')){throw 'PUBLIC_URL_LEAKED'}
foreach($n in @('host_capability_profile.json','selected_sunshine_runtime.txt','stream_display_preferences.json')){if(Test-Path (Join-Path $out ('sealed-bundle\moonlight\server\'+$n))){throw 'BUILDER_MACHINE_CACHE_LEAKED'}}
$config=Get-Content (Join-Path $out 'sealed-bundle\moonlight\server\config.json') -Raw|ConvertFrom-Json;$data=Get-Content (Join-Path $out 'sealed-bundle\moonlight\server\data.json') -Raw|ConvertFrom-Json
if($config.web_server.default_user_id-ne$null-or-not$config.web_server.first_login_create_admin-or@($data.users.psobject.Properties).Count-or@($data.hosts.psobject.Properties).Count){throw 'NEUTRALIZATION_FAILED'}
$fixtureExe=Join-Path $out 'CloudgimeHostPackageFixture-NOT-INSTALLABLE.exe'
if(Test-Path (Join-Path $out 'CloudgimeHostSetup.exe')){throw 'FIXTURE_MISLABELLED_AS_INSTALLER'}
[pscustomobject]@{ok=$true;installerBytes=(Get-Item $fixtureExe).Length;installerSha256=$report.sha256;candidateOnly=$report.candidateOnly;fixtureOnly=$report.fixtureOnly;installable=$report.installable;payloadValidated=$true}|ConvertTo-Json|Set-Content (Join-Path $base 'result.json');Get-Content (Join-Path $base 'result.json')
