param(
    [int]$PcNumber = 1,
    [string]$OutputDir = "..\..\export",
    [string]$HostPrefix = "mon",
    [string]$HostSlug = "",
    [string]$BaseDomain = "pc.cloudgime.my.id",
    [string]$FrpServerAddr = "64.120.92.154",
    [int]$FrpServerPort = 7001,
    [string]$FrpToken = "Shelgy678789",
    [string]$MoonlightSourceRoot = "",
    [string]$FrpSourceRoot = "",
    [string]$SunshineSourceRoot = "",
    [string]$LegacySunshineSourceRoot = "",
    [int]$LocalWebPort = 0,
    [int]$SunshineHttpPort = 0,
    [int]$WebRtcBasePort = 40000,
    [int]$WebRtcBlockSize = 50,
    [string]$UrlPathPrefix = "/stream",
    [string]$TurnHost = "64.120.92.154",
    [int]$TurnPort = 3478,
    [string]$TurnUsername = "mlturn",
    [string]$TurnCredential = "Shelgy678789",
    [string]$WebRtcNat1To1Ips = "",
    [ValidateSet("host", "srflx")][string]$WebRtcNat1To1CandidateType = "srflx",
    [string]$SunshineUsername = "admin",
    [string]$SunshinePassword = "",
    [ValidateSet("production", "staging", "development", "canary")][string]$DeploymentEnvironment = "production",
    [string]$PromotionGroup = "default",
    [switch]$DisableTurn,
    [switch]$Zip
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-SecretOverride {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$CurrentValue,
        [Parameter(Mandatory = $true)][string]$EnvName
    )

    $override = [Environment]::GetEnvironmentVariable($EnvName)
    if (-not [string]::IsNullOrWhiteSpace($override)) {
        return $override
    }

    return $CurrentValue
}

function Resolve-StringOverride {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$CurrentValue,
        [Parameter(Mandatory = $true)][string]$EnvName
    )

    $override = [Environment]::GetEnvironmentVariable($EnvName)
    if (-not [string]::IsNullOrWhiteSpace($override)) {
        return $override
    }

    return $CurrentValue
}

function Resolve-RuntimeRoot {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$RequestedPath,
        [Parameter(Mandatory = $true)][string]$PreferredRelativePath
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedPath)) {
        return $RequestedPath
    }

    return (Join-Path $PSScriptRoot $PreferredRelativePath)
}

function Resolve-OptionalRuntimeRoot {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$RequestedPath,
        [Parameter(Mandatory = $true)][string]$PreferredRelativePath
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedPath)) {
        return $RequestedPath
    }

    $candidate = Join-Path $PSScriptRoot $PreferredRelativePath
    if (Test-Path $candidate) {
        return $candidate
    }

    return ""
}

function Test-SunshineRuntimeRoot {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Path
    )

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $false
    }

    if (-not (Test-Path $Path)) {
        return $false
    }

    return (Test-Path (Join-Path $Path "sunshine.exe"))
}

$MoonlightSourceRoot = Resolve-RuntimeRoot `
    -RequestedPath $MoonlightSourceRoot `
    -PreferredRelativePath "..\..\runtime\moonlight"

$FrpSourceRoot = Resolve-RuntimeRoot `
    -RequestedPath $FrpSourceRoot `
    -PreferredRelativePath "..\..\runtime\frp"

$SunshineSourceRoot = Resolve-RuntimeRoot `
    -RequestedPath $SunshineSourceRoot `
    -PreferredRelativePath "..\..\runtime\sunshine"

$LegacySunshineSourceRoot = Resolve-OptionalRuntimeRoot `
    -RequestedPath $LegacySunshineSourceRoot `
    -PreferredRelativePath "..\..\runtime\sunshine-legacy"

$FrpToken = Resolve-SecretOverride -CurrentValue $FrpToken -EnvName "ML_FRP_TOKEN"
$TurnUsername = Resolve-SecretOverride -CurrentValue $TurnUsername -EnvName "ML_TURN_USERNAME"
$TurnCredential = Resolve-SecretOverride -CurrentValue $TurnCredential -EnvName "ML_TURN_CREDENTIAL"
$WebRtcNat1To1Ips = Resolve-StringOverride -CurrentValue $WebRtcNat1To1Ips -EnvName "ML_WEBRTC_NAT_1TO1_IPS"
$WebRtcNat1To1CandidateType = Resolve-StringOverride -CurrentValue $WebRtcNat1To1CandidateType -EnvName "ML_WEBRTC_NAT_1TO1_CANDIDATE_TYPE"
$SunshineUsername = Resolve-SecretOverride -CurrentValue $SunshineUsername -EnvName "ML_SUNSHINE_USERNAME"
$SunshinePassword = Resolve-SecretOverride -CurrentValue $SunshinePassword -EnvName "ML_SUNSHINE_PASSWORD"

if (-not (Test-SunshineRuntimeRoot $LegacySunshineSourceRoot)) {
    $LegacySunshineSourceRoot = ""
}

$BundleProcessManagerExe = Join-Path $PSScriptRoot "..\..\runtime\tools\bundle-process-manager.exe"
$BundleProcessManagerExe = [System.IO.Path]::GetFullPath($BundleProcessManagerExe)
$KeeperTunnelSourceDir = Join-Path $PSScriptRoot "..\..\runtime\tools\keeper-tunnel"
$KeeperTunnelSourceDir = [System.IO.Path]::GetFullPath($KeeperTunnelSourceDir)
$GamepadSidecarProject = Join-Path $PSScriptRoot "GamepadSidecar\GamepadSidecar.csproj"
$GamepadSidecarPublishRoot = Join-Path $PSScriptRoot "..\..\runtime\tools\gamepad-sidecar-publish"
$GamepadSidecarExe = Join-Path $GamepadSidecarPublishRoot "gamepad_sidecar.exe"
$DisplayPrepareHelperProject = Join-Path $PSScriptRoot "DisplayPrepareHelper\DisplayPrepareHelper.csproj"
$DisplayPrepareHelperPublishRoot = Join-Path $PSScriptRoot "..\..\runtime\tools\display-prepare-helper-publish"
$DisplayPrepareHelperExe = Join-Path $DisplayPrepareHelperPublishRoot "display-prepare-helper.exe"
$HostKeepAwakeAgentProject = Join-Path $PSScriptRoot "HostKeepAwakeAgent\HostKeepAwakeAgent.csproj"
$HostKeepAwakeAgentPublishRoot = Join-Path $PSScriptRoot "..\..\runtime\tools\host-keep-awake-agent-publish"
$HostKeepAwakeAgentExe = Join-Path $HostKeepAwakeAgentPublishRoot "cloudgime-keep-awake-agent.exe"
$HostControlTauriRoot = Join-Path $PSScriptRoot "HostControlApp.Tauri"
$HostControlTauriPublishScript = Join-Path $HostControlTauriRoot "publish-release.cmd"
$HostControlPublishRoot = Join-Path $HostControlTauriRoot "release"
$HostControlExe = Join-Path $HostControlPublishRoot "cloudgime-host-control.exe"
$HostControlOpenCmd = Join-Path $HostControlPublishRoot "open-host-control.cmd"
$HostControlOpenFolderCmd = Join-Path $HostControlPublishRoot "open-host-control-folder.cmd"
$ManagedBundleSeedRoot = Join-Path $PSScriptRoot "managed-bundle-seed"
$ManagedSharedPairInfoPath = Join-Path $ManagedBundleSeedRoot "shared_pair_info.json"
$ManagedSunshineSharedRoot = Join-Path $ManagedBundleSeedRoot "sunshine-shared"
$DriverSeedRoot = Join-Path $PSScriptRoot "..\..\payload-seed\drivers"

if ($PcNumber -lt 1) {
    throw "PcNumber must be at least 1."
}
if ($LocalWebPort -eq 0) {
    $LocalWebPort = 18080 + ($PcNumber - 1)
}
if ($SunshineHttpPort -eq 0) {
    $SunshineHttpPort = 49000 + (($PcNumber - 1) * 10)
}
if ($FrpServerPort -lt 1 -or $FrpServerPort -gt 65535) {
    throw "FrpServerPort must be within 1..65535."
}
if ($LocalWebPort -lt 1 -or $LocalWebPort -gt 65535) {
    throw "LocalWebPort must be within 1..65535."
}
if ($SunshineHttpPort -lt 1 -or $SunshineHttpPort -gt 65535) {
    throw "SunshineHttpPort must be within 1..65535."
}
if ($WebRtcBasePort -lt 1 -or $WebRtcBasePort -gt 65535) {
    throw "WebRtcBasePort must be within 1..65535."
}
if ($WebRtcBlockSize -lt 1) {
    throw "WebRtcBlockSize must be at least 1."
}
if (-not (Test-Path $MoonlightSourceRoot)) {
    throw "MoonlightSourceRoot does not exist: $MoonlightSourceRoot"
}
if (-not (Test-Path $SunshineSourceRoot)) {
    throw "SunshineSourceRoot does not exist: $SunshineSourceRoot"
}
if (-not (Test-Path $BundleProcessManagerExe)) {
    throw "Bundle process manager not found: $BundleProcessManagerExe"
}
if (-not $DisableTurn -and $TurnCredential -eq "Shelgy678789") {
    Write-Warning "TurnCredential is still using the builder default. Prefer ML_TURN_CREDENTIAL or an explicit parameter."
}
if ($SunshineUsername -eq "admin") {
    Write-Warning "SunshineUsername is still using the builder default. Prefer ML_SUNSHINE_USERNAME or an explicit parameter."
}

if (-not [System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir = Join-Path $PSScriptRoot $OutputDir
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Content
    )

    $parent = Split-Path -Parent $Path
    if ($parent) {
        [System.IO.Directory]::CreateDirectory($parent) | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Resolve-PreferredMoonlightBinaryPath {
    param(
        [Parameter(Mandatory = $true)][string]$MoonlightRoot,
        [Parameter(Mandatory = $true)][string]$BinaryName,
        [Parameter(Mandatory = $false)][AllowEmptyString()][string]$PreferredPath = ""
    )

    if (-not [string]::IsNullOrWhiteSpace($PreferredPath) -and (Test-Path $PreferredPath)) {
        return [System.IO.Path]::GetFullPath($PreferredPath)
    }

    return [System.IO.Path]::GetFullPath((Join-Path $MoonlightRoot $BinaryName))
}

function Resolve-WebServerBinaryPath {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$MoonlightRoot
    )

    $candidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        $candidates.Add((Join-Path $env:CARGO_TARGET_DIR "release\web-server.exe"))
    }

    $candidates.Add((Join-Path $RepoRoot "target\release\web-server.exe"))
    $candidates.Add((Join-Path $MoonlightRoot "web-server.exe"))

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return [System.IO.Path]::GetFullPath($candidate)
        }
    }

    throw "web-server.exe tidak ditemukan. Jalankan cargo build --release --bin web-server dulu."
}

function Resolve-MoonlightBuildBinaryPath {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$MoonlightRoot,
        [Parameter(Mandatory = $true)][string]$BinaryName
    )

    $candidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        $candidates.Add((Join-Path $env:CARGO_TARGET_DIR "release\$BinaryName"))
    }

    $candidates.Add((Join-Path $RepoRoot "target\release\$BinaryName"))
    $candidates.Add((Join-Path $MoonlightRoot $BinaryName))

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return [System.IO.Path]::GetFullPath($candidate)
        }
    }

    throw "$BinaryName tidak ditemukan. Jalankan cargo build --release -p streamer dulu."
}

function Get-GitValue {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Fallback
    )

    try {
        $repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
        $output = & git -C $repoRoot @Arguments 2>$null
        if ($LASTEXITCODE -eq 0) {
            $text = (($output | Out-String).Trim())
            if (-not [string]::IsNullOrWhiteSpace($text)) {
                return $text
            }
        }
    } catch {
    }

    return $Fallback
}

function New-ReleaseInfoJson {
    param(
        [Parameter(Mandatory = $true)][string]$DeploymentEnvironment,
        [Parameter(Mandatory = $true)][string]$ReleaseChannel,
        [Parameter(Mandatory = $true)][string]$SourceBranch,
        [Parameter(Mandatory = $true)][string]$SourceCommit,
        [Parameter(Mandatory = $true)][string]$SourceCommitShort,
        [Parameter(Mandatory = $true)][bool]$SourceDirty,
        [Parameter(Mandatory = $true)][string]$BuildProfile,
        [Parameter(Mandatory = $true)][Int64]$BuiltAtUnixMs
    )

    $builtAtUtc = [DateTimeOffset]::FromUnixTimeMilliseconds($BuiltAtUnixMs).UtcDateTime
    $versionTimestamp = $builtAtUtc.ToString("yyyyMMddHHmmss")
    $bundleVersion = "$DeploymentEnvironment.$ReleaseChannel.$versionTimestamp.$SourceCommitShort"
    $buildId = "$BuiltAtUnixMs-$SourceCommitShort"

    return ([ordered]@{
        schema_version = 1
        deployment_environment = $DeploymentEnvironment
        release_channel = $ReleaseChannel
        bundle_version = $bundleVersion
        build_id = $buildId
        source_branch = $SourceBranch
        source_commit = $SourceCommit
        source_commit_short = $SourceCommitShort
        source_dirty = $SourceDirty
        build_profile = $BuildProfile
        built_at_unix_ms = $BuiltAtUnixMs
    } | ConvertTo-Json -Depth 10)
}

function New-PromotionPolicyJson {
    param(
        [Parameter(Mandatory = $true)][string]$DeploymentEnvironment,
        [Parameter(Mandatory = $true)][string]$BundleName,
        [Parameter(Mandatory = $true)][string]$PromotionGroup
    )

    return ([ordered]@{
        schema_version = 1
        policy_name = "progressive-rings-v1"
        ring_order = @("development", "canary", "staging", "production")
        bundle_name = $BundleName
        promotion_group = $PromotionGroup
        deployment_environment = $DeploymentEnvironment
    } | ConvertTo-Json -Depth 10)
}

function Get-RelativePathCompat {
    param(
        [Parameter(Mandatory = $true)][string]$BasePath,
        [Parameter(Mandatory = $true)][string]$TargetPath
    )

    $baseFull = [System.IO.Path]::GetFullPath($BasePath)
    $targetFull = [System.IO.Path]::GetFullPath($TargetPath)

    if ((Get-Item $baseFull).PSIsContainer) {
        $baseFull = $baseFull.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    }

    $baseUri = [System.Uri]$baseFull
    $targetUri = [System.Uri]$targetFull
    $relative = $baseUri.MakeRelativeUri($targetUri).ToString()
    return [System.Uri]::UnescapeDataString($relative).Replace('/', [System.IO.Path]::DirectorySeparatorChar)
}

function Copy-DirectoryContents {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    [System.IO.Directory]::CreateDirectory($Destination) | Out-Null
    Copy-Item -Path (Join-Path $Source "*") -Destination $Destination -Recurse -Force
}

function Copy-SunshineRuntime {
    param(
        [Parameter(Mandatory = $true)][string]$SourceRoot,
        [Parameter(Mandatory = $true)][string]$DestinationRoot,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][string]$SunshineLoginName,
        [Parameter(Mandatory = $true)][string]$SunshineLoginSecret,
        [Parameter(Mandatory = $false)][string]$ManagedSunshineSharedSource = ""
    )

    [System.IO.Directory]::CreateDirectory($DestinationRoot) | Out-Null
    foreach ($directoryName in @("assets", "scripts", "tools")) {
        $sourceDir = Join-Path $SourceRoot $directoryName
        if (Test-Path $sourceDir) {
            Copy-DirectoryContents -Source $sourceDir -Destination (Join-Path $DestinationRoot $directoryName)
        }
    }

    foreach ($file in Get-ChildItem $SourceRoot -File) {
        if ($file.Name -eq "Uninstall.exe") {
            continue
        }
        Copy-Item $file.FullName (Join-Path $DestinationRoot $file.Name) -Force
    }

    [System.IO.Directory]::CreateDirectory((Join-Path $DestinationRoot "config")) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $DestinationRoot "config\credentials")) | Out-Null
    $bundleRoot = Split-Path -Parent $DestinationRoot
    $sharedRuntimeRoot = Join-Path $bundleRoot "moonlight\server\sunshine-shared"
    [System.IO.Directory]::CreateDirectory((Join-Path $sharedRuntimeRoot "credentials")) | Out-Null

    $sunshineAppsSource = Join-Path $SourceRoot "config\apps.json"
    if (-not (Test-Path $sunshineAppsSource)) {
        $sunshineAppsSource = Join-Path $SourceRoot "assets\apps.json"
    }
    if (Test-Path $sunshineAppsSource) {
        Copy-Item $sunshineAppsSource (Join-Path $DestinationRoot "config\apps.json") -Force
    }
    if (-not (Test-Path (Join-Path $DestinationRoot "config\apps.json"))) {
        Write-Utf8NoBom -Path (Join-Path $DestinationRoot "config\apps.json") -Content "{`"env`":{},`"apps`":[]}"
    }

    Write-Utf8NoBom -Path (Join-Path $DestinationRoot "config\sunshine.conf") -Content ((New-SunshineConfig -SunshineHttpPort $SunshineHttpPort) + [Environment]::NewLine)
    if (-not [string]::IsNullOrWhiteSpace($ManagedSunshineSharedSource) -and (Test-Path $ManagedSunshineSharedSource)) {
        Copy-DirectoryContents -Source $ManagedSunshineSharedSource -Destination $sharedRuntimeRoot
    } else {
        Push-Location $DestinationRoot
        try {
            & (Join-Path $DestinationRoot "sunshine.exe") --creds $SunshineLoginName $SunshineLoginSecret | Out-Null
        } finally {
            Pop-Location
        }
    }
}

function Get-ManagedSharedPairSeed {
    param(
        [Parameter(Mandatory = $true)][string]$SeedPath,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort
    )

    if (-not (Test-Path $SeedPath)) {
        return $null
    }

    $root = Get-Content $SeedPath -Raw | ConvertFrom-Json
    if ($null -eq $root -or $null -eq $root.hosts -or $root.hosts.Count -lt 1) {
        return $null
    }

    foreach ($hostEntry in $root.hosts) {
        $hostEntry.address = "127.0.0.1"
        $hostEntry.http_port = $SunshineHttpPort
    }

    return $root
}

function Get-SunshineRuntimeMetadata {
    param(
        [Parameter(Mandatory = $true)][string]$RuntimeRoot,
        [Parameter(Mandatory = $true)][string]$RuntimeKey,
        [Parameter(Mandatory = $true)][bool]$Legacy
    )

    $metadataPath = Join-Path $RuntimeRoot "sunshine_runtime_info.json"
    if (Test-Path $metadataPath) {
        try {
            $metadata = Get-Content -Raw -Path $metadataPath | ConvertFrom-Json
            $autoSelect = if ($null -ne $metadata.PSObject.Properties["auto_select"]) { [bool]$metadata.auto_select } else { $true }
            $startupValidationStatus = $metadata.startup_validation_status
            $startupValidationReason = $metadata.startup_validation_reason
            $startupValidationCheckedAt = $metadata.startup_validation_checked_at
            if ($Legacy -and $autoSelect) {
                $startupValidationStatus = "pending"
                $startupValidationReason = "runtime_start_validation_required"
                $startupValidationCheckedAt = $null
            }

            return [ordered]@{
                display_name = $metadata.display_name
                runtime_version = $metadata.runtime_version
                requires_bundled_ffmpeg = [bool]$metadata.requires_bundled_ffmpeg
                auto_select = $autoSelect
                startup_validation_status = $startupValidationStatus
                startup_validation_reason = $startupValidationReason
                startup_validation_checked_at = $startupValidationCheckedAt
            }
        } catch {
        }
    }

    $sunshineExe = Join-Path $RuntimeRoot "sunshine.exe"
    $version = $null
    if (Test-Path $sunshineExe) {
        try {
            $versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($sunshineExe)
            if (-not [string]::IsNullOrWhiteSpace($versionInfo.ProductVersion)) {
                $version = $versionInfo.ProductVersion
            } elseif (-not [string]::IsNullOrWhiteSpace($versionInfo.FileVersion)) {
                $version = $versionInfo.FileVersion
            }
        } catch {
        }
    }

    $displayName = if ($RuntimeKey -eq "default") {
        "Cloudgime Modern Runtime"
    } elseif ($Legacy) {
        "Cloudgime Compatibility Runtime ($RuntimeKey)"
    } else {
        "Cloudgime Runtime ($RuntimeKey)"
    }

    return [ordered]@{
        display_name = $displayName
        runtime_version = $version
        requires_bundled_ffmpeg = $Legacy
        auto_select = $true
        startup_validation_status = if ($Legacy) { "pending" } else { $null }
        startup_validation_reason = if ($Legacy) { "runtime_start_validation_required" } else { $null }
        startup_validation_checked_at = $null
    }
}

function Reset-LegacySunshineRuntimeMetadata {
    param(
        [Parameter(Mandatory = $true)][string]$RuntimeRoot
    )

    $metadataPath = Join-Path $RuntimeRoot "sunshine_runtime_info.json"
    $metadata = $null
    if (Test-Path $metadataPath) {
        try {
            $metadata = Get-Content -Raw -Path $metadataPath | ConvertFrom-Json
        } catch {
            $metadata = $null
        }
    }

    $runtimeVersion = "0.20.0"
    $displayName = "Cloudgime Compatibility Runtime"
    $requiresBundledFfmpeg = $true
    $autoSelect = $true
    if ($null -ne $metadata) {
        try {
            if ($null -ne $metadata.PSObject.Properties["runtime_version"] -and -not [string]::IsNullOrWhiteSpace($metadata.runtime_version)) {
                $runtimeVersion = $metadata.runtime_version
            }
            if ($null -ne $metadata.PSObject.Properties["display_name"] -and -not [string]::IsNullOrWhiteSpace($metadata.display_name)) {
                $displayName = $metadata.display_name
            }
            if ($null -ne $metadata.PSObject.Properties["requires_bundled_ffmpeg"]) {
                $requiresBundledFfmpeg = [bool]$metadata.requires_bundled_ffmpeg
            }
            if ($null -ne $metadata.PSObject.Properties["auto_select"]) {
                $autoSelect = [bool]$metadata.auto_select
            }
        } catch {
        }
    }

    $sanitized = [ordered]@{
        runtime_version = $runtimeVersion
        requires_bundled_ffmpeg = $requiresBundledFfmpeg
        display_name = $displayName
        auto_select = $autoSelect
        startup_validation_status = if ($autoSelect) { "pending" } else { "disabled" }
        startup_validation_reason = if ($autoSelect) { "runtime_start_validation_required" } else { "auto_select_disabled" }
        startup_validation_checked_at = $null
    }

    Write-Utf8NoBom -Path $metadataPath -Content (($sanitized | ConvertTo-Json -Depth 10) + [Environment]::NewLine)
}

function New-SunshineRuntimeManifestJson {
    param(
        [Parameter(Mandatory = $true)][string]$DefaultRuntimeRoot,
        [Parameter(Mandatory = $true)][bool]$IncludeLegacy,
        [Parameter(Mandatory = $false)][AllowEmptyString()][string]$LegacyRuntimeRoot = ""
    )

    function Resolve-ManifestFfmpegRelativePath {
        param(
            [Parameter(Mandatory = $true)][string]$RuntimeRoot,
            [Parameter(Mandatory = $true)][string]$RuntimeDirectory
        )

        $candidates = @(
            (Join-Path $RuntimeRoot "tools\ffmpeg.exe"),
            (Join-Path $RuntimeRoot "ffmpeg.exe")
        )

        foreach ($candidate in $candidates) {
            if (Test-Path $candidate) {
                $relativeWithinRuntime = Get-RelativePathCompat -BasePath $RuntimeRoot -TargetPath $candidate
                return [System.IO.Path]::Combine($RuntimeDirectory, $relativeWithinRuntime)
            }
        }

        return $null
    }

    $defaultMetadata = Get-SunshineRuntimeMetadata -RuntimeRoot $DefaultRuntimeRoot -RuntimeKey "default" -Legacy $false
    $runtimes = @(
        [ordered]@{
            key = "default"
            relative_directory = "sunshine"
            ffmpeg_relative_path = Resolve-ManifestFfmpegRelativePath -RuntimeRoot $DefaultRuntimeRoot -RuntimeDirectory "sunshine"
            requires_bundled_ffmpeg = [bool]$defaultMetadata.requires_bundled_ffmpeg
            legacy = $false
            display_name = $defaultMetadata.display_name
            runtime_version = $defaultMetadata.runtime_version
            priority = 0
            auto_select = [bool]$defaultMetadata.auto_select
            startup_validation_status = $defaultMetadata.startup_validation_status
            startup_validation_reason = $defaultMetadata.startup_validation_reason
            startup_validation_checked_at = $defaultMetadata.startup_validation_checked_at
        }
    )

    if ($IncludeLegacy) {
        $legacyMetadata = Get-SunshineRuntimeMetadata -RuntimeRoot $LegacyRuntimeRoot -RuntimeKey "legacy" -Legacy $true
        $runtimes += [ordered]@{
            key = "legacy"
            relative_directory = "sunshine-legacy"
            ffmpeg_relative_path = Resolve-ManifestFfmpegRelativePath -RuntimeRoot $LegacyRuntimeRoot -RuntimeDirectory "sunshine-legacy"
            requires_bundled_ffmpeg = [bool]$legacyMetadata.requires_bundled_ffmpeg
            legacy = $true
            display_name = $legacyMetadata.display_name
            runtime_version = $legacyMetadata.runtime_version
            priority = 10
            auto_select = [bool]$legacyMetadata.auto_select
            startup_validation_status = $legacyMetadata.startup_validation_status
            startup_validation_reason = $legacyMetadata.startup_validation_reason
            startup_validation_checked_at = $legacyMetadata.startup_validation_checked_at
        }
    }

    return ([ordered]@{
        version = 1
        runtimes = $runtimes
    } | ConvertTo-Json -Depth 10)
}

function New-MoonlightConfigJson {
    param(
        [Parameter(Mandatory = $true)][string]$UrlPathPrefix,
        [Parameter(Mandatory = $true)][int]$LocalWebPort,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][string]$PairDeviceName,
        [Parameter(Mandatory = $true)][int]$WebRtcMinPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMaxPort,
        [Parameter(Mandatory = $true)][bool]$DisableTurn,
        [Parameter(Mandatory = $true)][string]$TurnHost,
        [Parameter(Mandatory = $true)][int]$TurnPort,
        [Parameter(Mandatory = $true)][string]$TurnUsername,
        [Parameter(Mandatory = $true)][string]$TurnCredential,
        [Parameter(Mandatory = $false)][AllowEmptyString()][string]$WebRtcNat1To1Ips = "",
        [Parameter(Mandatory = $false)][ValidateSet("host", "srflx")][string]$WebRtcNat1To1CandidateType = "srflx"
    )

    $iceServers = @(
        [ordered]@{
            urls = @(
                "stun:stun.l.google.com:19302",
                "stun:stun1.l.google.com:19302",
                "stun:stun2.l.google.com:19302",
                "stun:stun3.l.google.com:19302",
                "stun:stun4.l.google.com:19302",
                "stun:stun.cloudflare.com:3478"
            )
        }
    )

    if (-not $DisableTurn) {
        $iceServers += [ordered]@{
            urls = @(
                "turn:${TurnHost}:${TurnPort}?transport=udp",
                "turn:${TurnHost}:${TurnPort}?transport=tcp"
            )
            username = $TurnUsername
            credential = $TurnCredential
        }
    }

    $nat1To1 = $null
    $normalizedNatIps = @($WebRtcNat1To1Ips -split "[,;`r`n`t ]+" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_.Trim() } | Select-Object -Unique)
    if ($normalizedNatIps.Count -gt 0) {
        $nat1To1 = [ordered]@{
            ips = $normalizedNatIps
            ice_candidate_type = $WebRtcNat1To1CandidateType
        }
    }

    $config = [ordered]@{
        data_storage = [ordered]@{
            type = "json"
            path = "server/data.json"
            session_expiration_check_interval = [ordered]@{
                secs = 300
                nanos = 0
            }
        }
        webrtc = [ordered]@{
            ice_servers = $iceServers
            port_range = [ordered]@{
                min = $WebRtcMinPort
                max = $WebRtcMaxPort
            }
            nat_1to1 = $nat1To1
            network_types = @("udp4")
            include_loopback_candidates = $false
        }
        web_server = [ordered]@{
            bind_address = "127.0.0.1:${LocalWebPort}"
            certificate = $null
            url_path_prefix = $UrlPathPrefix
            session_cookie_secure = $true
            session_cookie_expiration = [ordered]@{
                secs = 86400
                nanos = 0
            }
            first_login_create_admin = $false
            first_login_assign_global_hosts = $false
            default_user_id = 4081573497
            forwarded_header = $null
        }
        moonlight = [ordered]@{
            default_http_port = $SunshineHttpPort
            pair_device_name = $PairDeviceName
        }
        streamer_path = "./streamer.exe"
        mic_sidecar_path = "./mic_sidecar.exe"
        gamepad_sidecar_path = "./gamepad_sidecar.exe"
        log = [ordered]@{
            level_filter = "INFO"
            file_path = $null
        }
    }

    return ($config | ConvertTo-Json -Depth 20)
}

function New-MoonlightDataJson {
    param(
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][string]$HostName,
        [Parameter(Mandatory = $false)]$PairInfo = $null
    )

    $data = [ordered]@{
        version = "2"
        users = [ordered]@{
            "4081573497" = [ordered]@{
                role = "Admin"
                name = "admin"
                password = $null
                client_unique_id = "admin"
            }
        }
        hosts = [ordered]@{
            "4100364999" = [ordered]@{
                owner = 4081573497
                address = "127.0.0.1"
                http_port = $SunshineHttpPort
                pair_info = $PairInfo
                cache = [ordered]@{
                    name = $HostName
                    mac = $null
                }
            }
        }
    }

    return ($data | ConvertTo-Json -Depth 20)
}

function New-SunshineConfig {
    param(
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort
    )

    return @"
port = $SunshineHttpPort
system_tray = disabled
upnp = disabled
origin_web_ui_allowed = pc
controller = enabled
gamepad = auto
motion_as_ds4 = enabled
touchpad_as_ds4 = enabled
ds4_back_as_touchpad_click = enabled
dd_configuration_option = disabled
dd_resolution_option = disabled
dd_refresh_rate_option = disabled
dd_hdr_option = disabled
dd_config_revert_delay = 3000
dd_config_revert_on_disconnect = disabled
file_apps = apps.json
credentials_file = ../../moonlight/server/sunshine-shared/sunshine_state.json
file_state = ../../moonlight/server/sunshine-shared/sunshine_state.json
log_path = sunshine.log
pkey = ../../moonlight/server/sunshine-shared/credentials/cakey.pem
cert = ../../moonlight/server/sunshine-shared/credentials/cacert.pem
"@
}

function New-FrpcConfig {
    param(
        [Parameter(Mandatory = $true)][string]$FrpServerAddr,
        [Parameter(Mandatory = $true)][int]$FrpServerPort,
        [Parameter(Mandatory = $true)][string]$FrpToken,
        [Parameter(Mandatory = $true)][string]$ProxyName,
        [Parameter(Mandatory = $true)][int]$LocalWebPort,
        [Parameter(Mandatory = $true)][string]$Subdomain
    )

    return @"
serverAddr = "$FrpServerAddr"
serverPort = $FrpServerPort

auth.method = "token"
auth.token = "$FrpToken"
transport.tls.enable = false

[[proxies]]
name = "$ProxyName"
type = "http"
localIP = "127.0.0.1"
localPort = $LocalWebPort
subdomain = "$Subdomain"
"@
}

function New-StartAllBat {
    param(
        [Parameter(Mandatory = $true)][string]$PublicUrl,
        [Parameter(Mandatory = $true)][int]$LocalWebPort,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][bool]$DisableTurn
    )

    $sunshineUiPort = $SunshineHttpPort + 1
    $transportPreset = if ($DisableTurn) { "direct-strong" } else { "direct-safe" }
    $transportModeText = if ($DisableTurn) { "Direct-only (TURN off)" } else { "Auto direct -> TURN fallback" }

    return @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
"%~dp0host-installer.exe" --bundle-root "%~dp0." prepare-host
exit /b %ERRORLEVEL%
)
call "%~dp0stop-all.bat"
if errorlevel 1 (
echo.
echo Failed to stop old bundle processes cleanly.
echo Run this bundle as Administrator once, then try again.
echo Expected local web port: $LocalWebPort
echo.
exit /b 1
)
powershell -ExecutionPolicy Bypass -File "%~dp0ensure-firewall.ps1"
call "%~dp0start-bundle.bat"
powershell -ExecutionPolicy Bypass -File "%~dp0verify-startup.ps1"
if errorlevel 1 (
echo.
echo Bundle started incompletely. Check console output above.
echo Expected local web port: $LocalWebPort
echo.
exit /b 1
)
echo.
echo Portable host started.
echo Transport Preset: $transportPreset
echo Transport Mode  : $transportModeText
echo Public URL: $PublicUrl
echo Local runtime admin UI: https://localhost:$sunshineUiPort
echo Local runtime admin credentials are intentionally not written to disk.
echo.
"@
}

function New-StopBundlePs1 {
    param(
        [Parameter(Mandatory = $true)][int]$LocalWebPort,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort
    )

    $sunshineUiPort = $SunshineHttpPort + 1

    return @"
param(
    [switch]`$SkipElevation,
    [switch]`$Quiet
)

Set-StrictMode -Version Latest
`$ErrorActionPreference = "SilentlyContinue"

function Write-Status {
    param([string]`$Message)

    if (-not `$Quiet) {
        Write-Host "[stop-bundle] `$Message"
    }
}

function Test-IsAdministrator {
    `$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not `$identity) {
        return `$false
    }

    `$principal = New-Object Security.Principal.WindowsPrincipal(`$identity)
    return `$principal.IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
}

function Get-UniqueIntArray {
    param([object[]]`$Values)

    return @(
        `$Values |
            Where-Object { `$_ -ne `$null } |
            ForEach-Object { [int]`$_ } |
            Sort-Object -Unique
    )
}

`$bundleRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent `$MyInvocation.MyCommand.Path))
`$targetPaths = @(
    [System.IO.Path]::GetFullPath((Join-Path `$bundleRoot "sunshine\sunshine.exe")),
    [System.IO.Path]::GetFullPath((Join-Path `$bundleRoot "moonlight\web-server.exe")),
    [System.IO.Path]::GetFullPath((Join-Path `$bundleRoot "moonlight\streamer.exe")),
    [System.IO.Path]::GetFullPath((Join-Path `$bundleRoot "moonlight\mic_sidecar.exe"))
)
`$targetNames = @("sunshine.exe", "web-server.exe", "streamer.exe", "mic_sidecar.exe")
`$targetPorts = @($LocalWebPort, $SunshineHttpPort, $sunshineUiPort)

function Get-PortOwnerIds {
    param([int[]]`$Ports)

    `$processIds = New-Object System.Collections.Generic.List[int]
    foreach (`$port in Get-UniqueIntArray `$Ports) {
        foreach (`$tcp in @(Get-NetTCPConnection -LocalPort `$port -State Listen)) {
            if (`$tcp -and `$tcp.OwningProcess) {
                [void]`$processIds.Add([int]`$tcp.OwningProcess)
            }
        }
        foreach (`$udp in @(Get-NetUDPEndpoint -LocalPort `$port)) {
            if (`$udp -and `$udp.OwningProcess) {
                [void]`$processIds.Add([int]`$udp.OwningProcess)
            }
        }
    }

    return Get-UniqueIntArray `$processIds
}

function Get-BundleProcessInfo {
    `$portOwners = Get-PortOwnerIds -Ports `$targetPorts

    return @(
        Get-CimInstance Win32_Process |
            Where-Object {
                `$name = ([string]`$_.Name).ToLowerInvariant()
                `$exe = if (`$_.ExecutablePath) { [System.IO.Path]::GetFullPath(`$_.ExecutablePath) } else { `$null }
                `$cmd = [string]`$_.CommandLine
                `$isTargetName = `$targetNames -contains `$name
                `$pathMatch = `$exe -and (`$targetPaths -contains `$exe)
                `$cmdMatch = `$isTargetName -and `$cmd -and (`$cmd -like "*`$bundleRoot*")
                `$portMatch = `$isTargetName -and (`$portOwners -contains [int]`$_.ProcessId)

                (`$pathMatch -or `$cmdMatch -or `$portMatch) -and ([int]`$_.ProcessId -ne `$PID)
            } |
            Select-Object ProcessId, Name, ExecutablePath, CommandLine
    )
}

function Stop-BundlePid {
    param([int]`$ProcessId)

    if (`$ProcessId -eq `$PID) {
        return
    }

    try {
        Stop-Process -Id `$ProcessId -Force -ErrorAction Stop
        Start-Sleep -Milliseconds 250
        return
    } catch {
        Write-Status "Stop-Process failed for PID `$ProcessId, trying taskkill."
    }

    try {
        `$taskkill = Join-Path `$env:SystemRoot "System32\taskkill.exe"
        & `$taskkill /PID `$ProcessId /T /F | Out-Null
    } catch {
        Write-Status "taskkill failed for PID `$ProcessId."
    }

    Start-Sleep -Milliseconds 350
}

function Invoke-CleanupPass {
    `$items = Get-BundleProcessInfo
    foreach (`$item in (`$items | Sort-Object ProcessId -Descending)) {
        Write-Status ("Stopping PID {0} ({1})" -f `$item.ProcessId, `$item.Name)
        Stop-BundlePid -ProcessId ([int]`$item.ProcessId)
    }
}

function Get-RemainingState {
    `$remaining = Get-BundleProcessInfo
    `$remainingPorts = Get-PortOwnerIds -Ports `$targetPorts
    return [PSCustomObject]@{
        Processes = @(`$remaining)
        Ports = @(`$remainingPorts)
    }
}

Invoke-CleanupPass

for (`$attempt = 0; `$attempt -lt 5; `$attempt++) {
    `$state = Get-RemainingState
    if (`$state.Processes.Count -eq 0 -and `$state.Ports.Count -eq 0) {
        exit 0
    }

    Start-Sleep -Milliseconds 500
    Invoke-CleanupPass
}

`$state = Get-RemainingState
if ((`$state.Processes.Count -gt 0 -or `$state.Ports.Count -gt 0) -and -not `$SkipElevation -and -not (Test-IsAdministrator)) {
    Write-Status "Cleanup still blocked. Requesting Administrator elevation."
    `$arguments = @(
        "-ExecutionPolicy", "Bypass",
        "-File", "`"`$PSCommandPath`"",
        "-SkipElevation"
    )
    if (`$Quiet) {
        `$arguments += "-Quiet"
    }

    `$process = Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList `$arguments -Wait -PassThru
    exit `$process.ExitCode
}

`$state = Get-RemainingState
if (`$state.Processes.Count -gt 0 -or `$state.Ports.Count -gt 0) {
    `$processList = if (`$state.Processes.Count -gt 0) {
        (`$state.Processes | ForEach-Object { "`$(`$_.ProcessId):`$(`$_.Name)" }) -join ", "
    } else {
        "none"
    }
    `$portList = if (`$state.Ports.Count -gt 0) {
        (`$state.Ports | Sort-Object -Unique) -join ", "
    } else {
        "none"
    }

    Write-Status "Cleanup incomplete. Remaining processes: `$processList"
    Write-Status "Ports still occupied by matching process names: `$portList"
    exit 1
}

exit 0
"@
}

function New-VerifyStartupPs1 {
    param(
        [Parameter(Mandatory = $true)][int]$LocalWebPort,
        [Parameter(Mandatory = $true)][string]$UrlPathPrefix
    )

    return @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = "Stop"

function Test-BundleProcessPresent {
    param([Parameter(Mandatory = `$true)][string]`$ExecutablePath)

    `$normalizedPath = [System.IO.Path]::GetFullPath(`$ExecutablePath)
    return @(
        Get-CimInstance Win32_Process |
            Where-Object {
                `$_.ExecutablePath -and
                [System.IO.Path]::GetFullPath([string]`$_.ExecutablePath) -eq `$normalizedPath
            }
    ).Count -gt 0
}

`$bundleRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent `$MyInvocation.MyCommand.Path))
`$checks = @(
    @{ Name = "Host Runtime"; Path = (Join-Path `$bundleRoot "sunshine\sunshine.exe") },
    @{ Name = "Cloudgime Host Web"; Path = (Join-Path `$bundleRoot "moonlight\web-server.exe") }
)

`$missing = @()
`$deadline = (Get-Date).AddSeconds(20)
do {
    `$missing = @(
        `$checks |
            Where-Object { -not (Test-BundleProcessPresent -ExecutablePath `$_.Path) } |
            ForEach-Object { `$_.Name }
    )

    if (`$missing.Count -eq 0) {
        break
    }

    Start-Sleep -Milliseconds 500
} while ((Get-Date) -lt `$deadline)

if (`$missing.Count -gt 0) {
    Write-Error ("Bundle startup incomplete. Missing processes: " + ((`$missing | Sort-Object -Unique) -join ", "))
}

`$localUrl = "http://127.0.0.1:$LocalWebPort$UrlPathPrefix/"
`$deadline = (Get-Date).AddSeconds(20)
do {
    try {
        `$response = Invoke-WebRequest -UseBasicParsing -Uri `$localUrl -TimeoutSec 5
        if (`$response.StatusCode -ge 200 -and `$response.StatusCode -lt 500) {
            exit 0
        }
    } catch {
    }

    Start-Sleep -Milliseconds 500
} while ((Get-Date) -lt `$deadline)

Write-Error "Cloudgime Host web surface did not become ready at `$localUrl"
"@
}

function New-ReadmeText {
    param(
        [Parameter(Mandatory = $true)][string]$PublicUrl,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMinPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMaxPort,
        [Parameter(Mandatory = $true)][string]$HostLabel,
        [Parameter(Mandatory = $true)][bool]$DisableTurn
    )

    $sunshineUiPort = $SunshineHttpPort + 1
    $transportModeLine = if ($DisableTurn) {
        "- Transport preset: direct-strong (direct-only STUN, TURN off)."
    } else {
        "- Transport preset: direct-safe (direct first, TURN fallback enabled)."
    }

    return @"
Portable bundle for $HostLabel

What is included:
- Host runtime (portable user-mode launch)
- Cloudgime Host web surface
- Setup-token activation flow from CloudRental master admin
- Config already pointed to the local host runtime
- Self-contained runtime from this project folder

How to use:
1. Extract the zip anywhere.
2. Run start-all.bat
3. Open cloudgime-host-control.exe for local admin work
4. Open $PublicUrl from another PC

Notes:
- First run may trigger Windows Firewall prompts.
- start-all.bat may request Administrator permission once to register firewall rules for the bundle's WebRTC UDP range.
- The host runtime is launched in user mode for now, not as the final Windows service model.
- Before the runtime starts, the bundle runs a host preflight probe to pick the best available encoder/capture path for that PC.
- If the bundle contains multiple runtime variants such as sunshine and sunshine-legacy, preflight picks the healthiest one automatically.
- The host web surface is preconfigured as 127.0.0.1:$SunshineHttpPort.
- Local runtime admin UI: https://localhost:$sunshineUiPort
- Local runtime admin credentials are intentionally not written to disk.
- Managed pairing state is already bundled for the local host runtime.
- If the PC card is removed in the web client, it should reappear automatically without manual pairing.
- Public routing now comes from the control plane and keeper tunnel. FRP is not included in this bundle.
$transportModeLine
- Optional router UDP range for this PC: $WebRtcMinPort-$WebRtcMaxPort
- Host capability profile is written to moonlight\server\host_capability_profile.json
- If a target PC is missing Visual C++ runtime, install Microsoft Visual C++ Redistributable 2015-2022 x64.
"@
}

function New-SetupText {
    param(
        [Parameter(Mandatory = $true)][string]$HostLabel,
        [Parameter(Mandatory = $true)][string]$PublicUrl,
        [Parameter(Mandatory = $true)][int]$SunshineHttpPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMinPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMaxPort,
        [Parameter(Mandatory = $true)][bool]$DisableTurn
    )

    $sunshineUiPort = $SunshineHttpPort + 1
    $transportModeLine = if ($DisableTurn) {
        "- Transport preset: direct-strong (TURN off)."
    } else {
        "- Transport preset: direct-safe (TURN fallback on)."
    }

    return @"
START:
- Run start-all.bat
- Open cloudgime-host-control.exe for local admin tasks
- Paste the setup token from CloudRental master admin

TEST FROM ANOTHER PC:
- Open $PublicUrl

LOCAL RUNTIME ADMIN:
- URL: https://localhost:$sunshineUiPort
- Secret handling: local runtime admin credentials are intentionally not written to disk.

NETWORK:
- WebRTC host label: $HostLabel
- UDP range for this PC: $WebRtcMinPort-$WebRtcMaxPort
- start-all.bat may ask for Administrator permission once so the bundle can add Windows Firewall rules automatically.
- start-all.bat runs a host preflight probe first and writes moonlight\server\host_capability_profile.json
- If sunshine-legacy is bundled too, start-all.bat will auto-select it when that host needs the older NVENC compatibility path.
- Public routing is now managed by the control plane and keeper tunnel.
$transportModeLine
- Router port forward is optional fallback only.
- Manual FRP server setup is no longer part of the normal path.
"@
}

function New-EnsureFirewallPs1 {
    param(
        [Parameter(Mandatory = $true)][string]$HostLabel,
        [Parameter(Mandatory = $true)][int]$WebRtcMinPort,
        [Parameter(Mandatory = $true)][int]$WebRtcMaxPort
    )

    return @"
param(
    [switch]`$Elevated
)

`$ErrorActionPreference = "Stop"
`$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not `$isAdmin) {
    if (-not `$Elevated) {
        try {
            `$child = Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList @(
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "`"`$PSCommandPath`"",
                "-Elevated"
            ) -PassThru -Wait

            exit `$child.ExitCode
        } catch {
            Write-Host "[WARN] Firewall rules were not updated because Administrator access was denied." -ForegroundColor Yellow
            Write-Host "[WARN] Direct P2P WebRTC may fail until you allow the UAC prompt once." -ForegroundColor Yellow
            exit 0
        }
    }

    Write-Host "[WARN] Firewall rules were not updated because this script is not running as Administrator." -ForegroundColor Yellow
    exit 0
}

`$root = Split-Path -Parent `$MyInvocation.MyCommand.Path
`$rules = @(
    @{
        Name = "Cloudgime Host $HostLabel WebServer UDP"
        Program = Join-Path `$root "moonlight\web-server.exe"
        Protocol = "UDP"
        LocalPort = "Any"
    },
    @{
        Name = "Cloudgime Host $HostLabel WebServer TCP"
        Program = Join-Path `$root "moonlight\web-server.exe"
        Protocol = "TCP"
        LocalPort = "Any"
    },
    @{
        Name = "Cloudgime Host $HostLabel Streamer UDP"
        Program = Join-Path `$root "moonlight\streamer.exe"
        Protocol = "UDP"
        LocalPort = "Any"
    },
    @{
        Name = "Cloudgime Host $HostLabel Streamer TCP"
        Program = Join-Path `$root "moonlight\streamer.exe"
        Protocol = "TCP"
        LocalPort = "Any"
    },
    @{
        Name = "Cloudgime Host $HostLabel WebRTC UDP Range"
        Program = `$null
        Protocol = "UDP"
        LocalPort = "$WebRtcMinPort-$WebRtcMaxPort"
    }
)

foreach (`$rule in `$rules) {
    & netsh advfirewall firewall delete rule name="`$(`$rule.Name)" *> `$null

    `$arguments = @(
        "advfirewall",
        "firewall",
        "add",
        "rule",
        "name=`"`$(`$rule.Name)`"",
        "dir=in",
        "action=allow",
        "profile=private,public",
        "protocol=`$(`$rule.Protocol)"
    )

    if (`$rule.Program) {
        `$arguments += "program=`"`$(`$rule.Program)`""
    } else {
        `$arguments += "program=any"
    }

    if (`$rule.LocalPort -eq "Any") {
        `$arguments += "localport=any"
    } else {
        `$arguments += "localport=`$(`$rule.LocalPort)"
    }

    & netsh @arguments | Out-Null
}

Write-Host "[OK] Firewall rules ensured for $HostLabel (UDP $WebRtcMinPort-$WebRtcMaxPort)." -ForegroundColor Green
"@
}

[System.IO.Directory]::CreateDirectory($OutputDir) | Out-Null

$normalizedHostSlug = $HostSlug.Trim().ToLowerInvariant()
if (-not [string]::IsNullOrWhiteSpace($normalizedHostSlug)) {
    if ($normalizedHostSlug.Contains(".")) {
        throw "HostSlug must be a single label without dots."
    }
    if ($normalizedHostSlug -notmatch '^[a-z0-9-]+$') {
        throw "HostSlug may only contain lowercase letters, numbers, and hyphens."
    }
    $hostLabel = $normalizedHostSlug
} else {
    $hostLabel = "$HostPrefix$PcNumber"
}
$hostname = "$hostLabel.$BaseDomain"
$publicUrl = "https://$hostname$UrlPathPrefix/"
$proxyName = "moonlight-$hostLabel"
$bundleRoot = Join-Path $OutputDir $hostLabel
$moonlightBundle = Join-Path $bundleRoot "moonlight"
$moonlightSystemBundle = Join-Path $moonlightBundle "system"
$frpBundle = Join-Path $bundleRoot "frp"
$sunshineBundle = Join-Path $bundleRoot "sunshine"
$webRtcMinPort = $WebRtcBasePort + (($PcNumber - 1) * $WebRtcBlockSize)
$webRtcMaxPort = $webRtcMinPort + $WebRtcBlockSize - 1

if ([string]::IsNullOrWhiteSpace($SunshinePassword)) {
    Write-Warning "SunshinePassword was empty. Builder will fall back to a predictable host-label password. Prefer ML_SUNSHINE_PASSWORD or an explicit parameter."
    $SunshinePassword = "$hostLabel-login"
}

$releaseChannel = Get-GitValue -Arguments @("branch", "--show-current") -Fallback "unknown"
$sourceBranch = $releaseChannel
$sourceCommit = Get-GitValue -Arguments @("rev-parse", "HEAD") -Fallback "unknown"
$sourceCommitShort = Get-GitValue -Arguments @("rev-parse", "--short", "HEAD") -Fallback "unknown"
$sourceDirtyText = Get-GitValue -Arguments @("status", "--porcelain") -Fallback ""
$sourceDirty = -not [string]::IsNullOrWhiteSpace($sourceDirtyText)
$builtAtUnixMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

if ($webRtcMaxPort -gt 65535) {
    throw "Computed WebRTC port range exceeds 65535 for $hostLabel."
}

if (Test-Path $bundleRoot) {
    Remove-Item -Recurse -Force $bundleRoot
}

[System.IO.Directory]::CreateDirectory($bundleRoot) | Out-Null

if (Test-Path $DriverSeedRoot) {
    Copy-DirectoryContents -Source $DriverSeedRoot -Destination (Join-Path $bundleRoot "drivers")
}

# Copy Moonlight runtime (portable subset)
[System.IO.Directory]::CreateDirectory($moonlightBundle) | Out-Null
[System.IO.Directory]::CreateDirectory($moonlightSystemBundle) | Out-Null
if (Test-Path $GamepadSidecarProject) {
    dotnet publish $GamepadSidecarProject `
        -c Release `
        -r win-x64 `
        --self-contained true `
        -p:PublishSingleFile=true `
        -p:EnableCompressionInSingleFile=true `
        -o $GamepadSidecarPublishRoot | Out-Host
}
if (Test-Path $DisplayPrepareHelperProject) {
    dotnet publish $DisplayPrepareHelperProject `
        -c Release `
        -r win-x64 `
        --self-contained true `
        -p:PublishSingleFile=true `
        -p:EnableCompressionInSingleFile=true `
        -o $DisplayPrepareHelperPublishRoot | Out-Host
}
if (Test-Path $HostKeepAwakeAgentProject) {
    dotnet publish $HostKeepAwakeAgentProject `
        -c Release `
        -r win-x64 `
        --self-contained true `
        -p:PublishSingleFile=true `
        -p:EnableCompressionInSingleFile=true `
        -o $HostKeepAwakeAgentPublishRoot | Out-Host
}
if (Test-Path $HostControlTauriPublishScript) {
    Push-Location $HostControlTauriRoot
    try {
        npm run tauri build | Out-Host
    } finally {
        Pop-Location
    }

    & $HostControlTauriPublishScript --bundle-root $bundleRoot | Out-Host
}
$webServerSourceExe = Resolve-WebServerBinaryPath `
    -RepoRoot $RepoRoot `
    -MoonlightRoot $MoonlightSourceRoot
Write-Host "[BUILD] Bundling web-server from: $webServerSourceExe" -ForegroundColor DarkCyan
Copy-Item $webServerSourceExe (Join-Path $moonlightBundle "web-server.exe") -Force
if (Test-Path (Join-Path $MoonlightSourceRoot "host_supervisor.exe")) {
    Copy-Item (Join-Path $MoonlightSourceRoot "host_supervisor.exe") (Join-Path $moonlightSystemBundle "cloudgime-runtime-agent.exe") -Force
}
if (Test-Path (Join-Path $MoonlightSourceRoot "host_installer.exe")) {
    Copy-Item (Join-Path $MoonlightSourceRoot "host_installer.exe") (Join-Path $bundleRoot "host-installer.exe") -Force
}
$streamerSourceExe = Resolve-MoonlightBuildBinaryPath `
    -RepoRoot $RepoRoot `
    -MoonlightRoot $MoonlightSourceRoot `
    -BinaryName "streamer.exe"
Write-Host "[BUILD] Bundling streamer from: $streamerSourceExe" -ForegroundColor DarkCyan
Copy-Item $streamerSourceExe (Join-Path $moonlightBundle "streamer.exe") -Force
$micSidecarSourceExe = Resolve-MoonlightBuildBinaryPath `
    -RepoRoot $RepoRoot `
    -MoonlightRoot $MoonlightSourceRoot `
    -BinaryName "mic_sidecar.exe"
Write-Host "[BUILD] Bundling mic sidecar from: $micSidecarSourceExe" -ForegroundColor DarkCyan
Copy-Item $micSidecarSourceExe (Join-Path $moonlightBundle "mic_sidecar.exe") -Force
if (Test-Path $GamepadSidecarExe) {
    Copy-Item $GamepadSidecarExe (Join-Path $moonlightBundle "gamepad_sidecar.exe") -Force
}
if (Test-Path $HostControlExe) {
    Copy-Item $HostControlExe (Join-Path $bundleRoot "cloudgime-host-control.exe") -Force
    if (Test-Path $HostControlOpenCmd) {
        Copy-Item $HostControlOpenCmd (Join-Path $bundleRoot "open-host-control.cmd") -Force
    }
    if (Test-Path $HostControlOpenFolderCmd) {
        Copy-Item $HostControlOpenFolderCmd (Join-Path $bundleRoot "open-host-control-folder.cmd") -Force
    }
}
if (Test-Path $HostKeepAwakeAgentExe) {
    Copy-Item $HostKeepAwakeAgentExe (Join-Path $moonlightSystemBundle "cloudgime-keep-awake-agent.exe") -Force
}
Copy-DirectoryContents -Source (Join-Path $MoonlightSourceRoot "static") -Destination (Join-Path $moonlightBundle "static")
[System.IO.Directory]::CreateDirectory((Join-Path $moonlightBundle "server")) | Out-Null
if (Test-Path $DisplayPrepareHelperExe) {
    Copy-Item $DisplayPrepareHelperExe (Join-Path $moonlightBundle "server\\display-prepare-helper.exe") -Force
}

Remove-Item (Join-Path $moonlightSystemBundle "launch-runtime-agent-hidden.vbs") -Force -ErrorAction SilentlyContinue

$moonlightConfig = New-MoonlightConfigJson `
    -UrlPathPrefix $UrlPathPrefix `
    -LocalWebPort $LocalWebPort `
    -SunshineHttpPort $SunshineHttpPort `
    -PairDeviceName "$hostLabel-web" `
    -WebRtcMinPort $webRtcMinPort `
    -WebRtcMaxPort $webRtcMaxPort `
    -DisableTurn ([bool]$DisableTurn) `
    -TurnHost $TurnHost `
    -TurnPort $TurnPort `
    -TurnUsername $TurnUsername `
    -TurnCredential $TurnCredential `
    -WebRtcNat1To1Ips $WebRtcNat1To1Ips `
    -WebRtcNat1To1CandidateType $WebRtcNat1To1CandidateType
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\config.json") -Content ($moonlightConfig + [Environment]::NewLine)

$managedPairSeed = Get-ManagedSharedPairSeed -SeedPath $ManagedSharedPairInfoPath -SunshineHttpPort $SunshineHttpPort
$managedPairInfo = $null
if ($managedPairSeed -and $managedPairSeed.hosts.Count -gt 0) {
    $managedPairInfo = $managedPairSeed.hosts[0].pair_info
    Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\shared_pair_info.json") -Content (($managedPairSeed | ConvertTo-Json -Depth 20) + [Environment]::NewLine)
}

$moonlightData = New-MoonlightDataJson -SunshineHttpPort $SunshineHttpPort -HostName "This PC" -PairInfo $managedPairInfo
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\data.json") -Content ($moonlightData + [Environment]::NewLine)
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\release_info.json") -Content ((New-ReleaseInfoJson -DeploymentEnvironment $DeploymentEnvironment -ReleaseChannel $releaseChannel -SourceBranch $sourceBranch -SourceCommit $sourceCommit -SourceCommitShort $sourceCommitShort -SourceDirty ([bool]$sourceDirty) -BuildProfile "release" -BuiltAtUnixMs $builtAtUnixMs) + [Environment]::NewLine)
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\promotion_policy.json") -Content ((New-PromotionPolicyJson -DeploymentEnvironment $DeploymentEnvironment -BundleName $hostLabel -PromotionGroup $PromotionGroup) + [Environment]::NewLine)
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\hard_reset_mode.txt") -Content ("auto" + [Environment]::NewLine)

# Copy Sunshine runtime (portable subset, fresh config)
Copy-SunshineRuntime `
    -SourceRoot $SunshineSourceRoot `
    -DestinationRoot $sunshineBundle `
    -SunshineHttpPort $SunshineHttpPort `
    -SunshineLoginName $SunshineUsername `
    -SunshineLoginSecret $SunshinePassword `
    -ManagedSunshineSharedSource $ManagedSunshineSharedRoot

if (-not [string]::IsNullOrWhiteSpace($LegacySunshineSourceRoot) -and (Test-Path $LegacySunshineSourceRoot)) {
    $legacyBundleRoot = Join-Path $bundleRoot "sunshine-legacy"
    Copy-SunshineRuntime `
        -SourceRoot $LegacySunshineSourceRoot `
        -DestinationRoot $legacyBundleRoot `
        -SunshineHttpPort $SunshineHttpPort `
        -SunshineLoginName $SunshineUsername `
        -SunshineLoginSecret $SunshinePassword `
        -ManagedSunshineSharedSource $ManagedSunshineSharedRoot
    Reset-LegacySunshineRuntimeMetadata -RuntimeRoot $legacyBundleRoot
}

$runtimeManifestJson = New-SunshineRuntimeManifestJson `
    -DefaultRuntimeRoot $SunshineSourceRoot `
    -IncludeLegacy (-not [string]::IsNullOrWhiteSpace($LegacySunshineSourceRoot) -and (Test-Path $LegacySunshineSourceRoot)) `
    -LegacyRuntimeRoot $LegacySunshineSourceRoot
Write-Utf8NoBom -Path (Join-Path $moonlightBundle "server\sunshine_runtime_manifest.json") -Content ($runtimeManifestJson + [Environment]::NewLine)

# Start / stop helpers
$startHostRuntime = @"
@echo off
setlocal
if exist "%~dp0moonlight\system\cloudgime-runtime-agent.exe" (
    "%~dp0moonlight\system\cloudgime-runtime-agent.exe" --bundle-root "%~dp0." restart-runtime
    exit /b %ERRORLEVEL%
)
echo cloudgime-runtime-agent.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "start-host-runtime.bat") -Content $startHostRuntime

$startMoonlight = @"
@echo off
setlocal
pushd "%~dp0moonlight"
start "" "%~dp0moonlight\web-server.exe" --config-path "%~dp0moonlight\server\config.json" --bind-address 127.0.0.1:${LocalWebPort} --webrtc-port-range ${webRtcMinPort}:${webRtcMaxPort}
popd
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "start-host-web.bat") -Content $startMoonlight

$startBundle = @"
@echo off
setlocal
if exist "%~dp0moonlight\system\cloudgime-runtime-agent.exe" (
    "%~dp0moonlight\system\cloudgime-runtime-agent.exe" --bundle-root "%~dp0." start-bundle
    exit /b %ERRORLEVEL%
)
echo cloudgime-runtime-agent.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "start-bundle.bat") -Content $startBundle

$stopBundle = @"
@echo off
setlocal
if exist "%~dp0moonlight\system\cloudgime-runtime-agent.exe" (
    "%~dp0moonlight\system\cloudgime-runtime-agent.exe" --bundle-root "%~dp0." stop-bundle
    exit /b %ERRORLEVEL%
)
echo cloudgime-runtime-agent.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "stop-bundle.bat") -Content $stopBundle

$hardenHostUserDaemonTask = @"
`$ErrorActionPreference = 'Stop'
`$taskName = 'CloudgimeHostUser-Host'
`$bundleRoot = Split-Path -Parent `$MyInvocation.MyCommand.Path
`$daemonPath = Join-Path `$bundleRoot 'moonlight\system\cloudgime-runtime-agent.exe'
`$healthPath = Join-Path `$bundleRoot 'moonlight\server\host_user_daemon_task_health.json'
`$action = New-ScheduledTaskAction -Execute `$daemonPath -Argument ('--bundle-root "' + `$bundleRoot + '" run-daemon')
`$startupTrigger = New-ScheduledTaskTrigger -AtStartup
`$logonTrigger = New-ScheduledTaskTrigger -AtLogOn
`$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
`$settingsSeed = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName `$taskName -Action `$action -Trigger @(`$startupTrigger, `$logonTrigger) -Principal `$principal -Settings `$settingsSeed -Force | Out-Null
`$xmlRaw = & schtasks.exe /Query /TN `$taskName /XML 2>`$null | Out-String
if ([string]::IsNullOrWhiteSpace(`$xmlRaw)) {
    throw 'Scheduled task was created but could not be queried back from Task Scheduler.'
}
[xml]`$taskXml = `$xmlRaw
`$ns = New-Object System.Xml.XmlNamespaceManager(`$taskXml.NameTable)
`$ns.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
`$settings = `$taskXml.SelectSingleNode('/t:Task/t:Settings', `$ns)
if (`$null -eq `$settings) {
    throw 'Settings node missing in scheduled task XML.'
}
function New-TaskNode([string]`$name, [string]`$value) {
    `$node = `$taskXml.CreateElement(`$name, `$taskXml.DocumentElement.NamespaceURI)
    `$node.InnerText = `$value
    return `$node
}
function New-RestartOnFailureNode([string]`$count, [string]`$interval) {
    `$node = `$taskXml.CreateElement('RestartOnFailure', `$taskXml.DocumentElement.NamespaceURI)
    `$countNode = `$taskXml.CreateElement('Count', `$taskXml.DocumentElement.NamespaceURI)
    `$countNode.InnerText = `$count
    [void]`$node.AppendChild(`$countNode)
    `$intervalNode = `$taskXml.CreateElement('Interval', `$taskXml.DocumentElement.NamespaceURI)
    `$intervalNode.InnerText = `$interval
    [void]`$node.AppendChild(`$intervalNode)
    return `$node
}
`$taskXml.DocumentElement.SetAttribute('version', '1.3')
while (`$settings.HasChildNodes) {
    [void]`$settings.RemoveChild(`$settings.FirstChild)
}
[void]`$settings.AppendChild((New-TaskNode 'DisallowStartIfOnBatteries' 'false'))
[void]`$settings.AppendChild((New-TaskNode 'StopIfGoingOnBatteries' 'false'))
[void]`$settings.AppendChild((New-TaskNode 'ExecutionTimeLimit' 'PT0S'))
[void]`$settings.AppendChild((New-TaskNode 'Hidden' 'true'))
[void]`$settings.AppendChild((New-TaskNode 'MultipleInstancesPolicy' 'StopExisting'))
[void]`$settings.AppendChild((New-RestartOnFailureNode '999' 'PT1M'))
[void]`$settings.AppendChild((New-TaskNode 'StartWhenAvailable' 'true'))
[void]`$settings.AppendChild((New-TaskNode 'UseUnifiedSchedulingEngine' 'true'))
`$tempXml = Join-Path `$env:TEMP ('cloudgime-host-user-task-' + [guid]::NewGuid().ToString('N') + '.xml')
try {
    `$taskXml.Save(`$tempXml)
    & schtasks.exe /Create /TN `$taskName /XML `$tempXml /F | Out-Null
} finally {
    Remove-Item `$tempXml -Force -ErrorAction SilentlyContinue
}
`$task = Get-ScheduledTask -TaskName `$taskName -ErrorAction SilentlyContinue
`$daemon = Get-CimInstance Win32_Process | Where-Object {
    `$_.ExecutablePath -eq `$daemonPath -and
    `$_.CommandLine -like '* run-daemon*'
} | Select-Object -First 1
if (`$null -ne `$task -and `$null -eq `$daemon) {
    if (`$task.State -eq 'Running') {
        Stop-ScheduledTask -TaskName `$taskName -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
    Start-ScheduledTask -TaskName `$taskName -ErrorAction SilentlyContinue
}
function Get-TaskTextValue(`$parent, [string]`$xpath, [System.Xml.XmlNamespaceManager]`$xmlNs) {
    `$node = `$parent.SelectSingleNode(`$xpath, `$xmlNs)
    if (`$null -eq `$node) {
        return ''
    }
    return [string]`$node.InnerText
}
`$exportRaw = Export-ScheduledTask -TaskName `$taskName
[xml]`$exportXml = `$exportRaw
`$exportNs = New-Object System.Xml.XmlNamespaceManager(`$exportXml.NameTable)
`$exportNs.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
`$exportSettings = `$exportXml.SelectSingleNode('/t:Task/t:Settings', `$exportNs)
`$taskInfo = Get-ScheduledTaskInfo -TaskName `$taskName -ErrorAction SilentlyContinue
`$daemon = Get-CimInstance Win32_Process | Where-Object {
    `$_.ExecutablePath -eq `$daemonPath -and
    `$_.CommandLine -like '* run-daemon*'
} | Select-Object -First 1
`$issues = New-Object System.Collections.Generic.List[string]
`$multipleInstances = Get-TaskTextValue `$exportSettings 't:MultipleInstancesPolicy' `$exportNs
`$restartCount = Get-TaskTextValue `$exportSettings 't:RestartOnFailure/t:Count' `$exportNs
`$restartInterval = Get-TaskTextValue `$exportSettings 't:RestartOnFailure/t:Interval' `$exportNs
`$executionTimeLimit = Get-TaskTextValue `$exportSettings 't:ExecutionTimeLimit' `$exportNs
`$startWhenAvailable = Get-TaskTextValue `$exportSettings 't:StartWhenAvailable' `$exportNs
`$hidden = Get-TaskTextValue `$exportSettings 't:Hidden' `$exportNs
`$disallowBattery = Get-TaskTextValue `$exportSettings 't:DisallowStartIfOnBatteries' `$exportNs
`$stopBattery = Get-TaskTextValue `$exportSettings 't:StopIfGoingOnBatteries' `$exportNs
`$useUnifiedSchedulingEngine = Get-TaskTextValue `$exportSettings 't:UseUnifiedSchedulingEngine' `$exportNs
`$idleStopOnIdleEnd = Get-TaskTextValue `$exportSettings 't:IdleSettings/t:StopOnIdleEnd' `$exportNs
`$idleRestartOnIdle = Get-TaskTextValue `$exportSettings 't:IdleSettings/t:RestartOnIdle' `$exportNs
if (`$multipleInstances -ne 'StopExisting') { [void]`$issues.Add("multiple_instances:`$multipleInstances") }
if (`$restartCount -ne '999') { [void]`$issues.Add("restart_count:`$restartCount") }
if (`$restartInterval -ne 'PT1M') { [void]`$issues.Add("restart_interval:`$restartInterval") }
if (`$executionTimeLimit -ne 'PT0S') { [void]`$issues.Add("execution_time_limit:`$executionTimeLimit") }
if (`$startWhenAvailable -ne 'true') { [void]`$issues.Add("start_when_available:`$startWhenAvailable") }
if (`$hidden -ne 'true') { [void]`$issues.Add("hidden:`$hidden") }
if (`$disallowBattery -ne 'false') { [void]`$issues.Add("disallow_start_if_on_batteries:`$disallowBattery") }
if (`$stopBattery -ne 'false') { [void]`$issues.Add("stop_if_going_on_batteries:`$stopBattery") }
if (`$useUnifiedSchedulingEngine -ne 'true') { [void]`$issues.Add("use_unified_scheduling_engine:`$useUnifiedSchedulingEngine") }
`$health = [ordered]@{
    schemaVersion = 1
    taskName = `$taskName
    bundleRoot = `$bundleRoot
    daemonPath = `$daemonPath
    checkedAtUtc = [DateTime]::UtcNow.ToString('o')
    policyValid = (`$issues.Count -eq 0)
    taskState = if (`$null -ne `$task) { [string]`$task.State } else { '' }
    lastTaskResult = if (`$null -ne `$taskInfo) { [int]`$taskInfo.LastTaskResult } else { 0 }
    lastRunTimeUtc = if (`$null -ne `$taskInfo -and `$taskInfo.LastRunTime -is [DateTime]) { `$taskInfo.LastRunTime.ToUniversalTime().ToString('o') } else { '' }
    daemonRunning = (`$null -ne `$daemon)
    daemonPid = if (`$null -ne `$daemon) { [int]`$daemon.ProcessId } else { 0 }
    taskSettings = [ordered]@{
        multipleInstancesPolicy = `$multipleInstances
        restartCount = `$restartCount
        restartInterval = `$restartInterval
        executionTimeLimit = `$executionTimeLimit
        startWhenAvailable = `$startWhenAvailable
        hidden = `$hidden
        disallowStartIfOnBatteries = `$disallowBattery
        stopIfGoingOnBatteries = `$stopBattery
        useUnifiedSchedulingEngine = `$useUnifiedSchedulingEngine
        idleStopOnIdleEnd = `$idleStopOnIdleEnd
        idleRestartOnIdle = `$idleRestartOnIdle
    }
    issues = @(`$issues)
}
`$healthDir = Split-Path -Parent `$healthPath
if (-not (Test-Path `$healthDir)) {
    New-Item -ItemType Directory -Path `$healthDir -Force | Out-Null
}
`$health | ConvertTo-Json -Depth 8 | Set-Content -Path `$healthPath -Encoding UTF8
if (`$issues.Count -gt 0) {
    throw ('Host user-daemon task policy validation failed: ' + (`$issues -join ', '))
}
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "harden-host-user-daemon-task.ps1") -Content $hardenHostUserDaemonTask

$installService = @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
    "%~dp0host-installer.exe" --bundle-root "%~dp0." install-service
    set "CG_INSTALL_EXIT=%ERRORLEVEL%"
    if not "%CG_INSTALL_EXIT%"=="0" exit /b %CG_INSTALL_EXIT%
    sc.exe config "CloudgimeHost-Host" start= auto >nul 2>&1
    sc.exe failure "CloudgimeHost-Host" reset= 86400 actions= restart/60000/restart/60000/restart/120000 >nul 2>&1
    sc.exe failureflag "CloudgimeHost-Host" 1 >nul 2>&1
    sc.exe failure "CloudgimeRuntime-Host" reset= 86400 actions= restart/60000/restart/60000/restart/120000 >nul 2>&1
    sc.exe failureflag "CloudgimeRuntime-Host" 1 >nul 2>&1
    if exist "%~dp0harden-host-user-daemon-task.ps1" (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0harden-host-user-daemon-task.ps1" >nul 2>&1
        if not "%ERRORLEVEL%"=="0" exit /b %ERRORLEVEL%
    )
    exit /b 0
)
echo host-installer.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "install-service.bat") -Content $installService

$uninstallService = @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
    "%~dp0host-installer.exe" --bundle-root "%~dp0." uninstall-service
    exit /b %ERRORLEVEL%
)
echo host-installer.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "uninstall-service.bat") -Content $uninstallService

$startService = @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
    "%~dp0host-installer.exe" --bundle-root "%~dp0." start-service
    exit /b %ERRORLEVEL%
)
echo host-installer.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "start-service.bat") -Content $startService

$stopService = @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
    "%~dp0host-installer.exe" --bundle-root "%~dp0." stop-service
    exit /b %ERRORLEVEL%
)
echo host-installer.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "stop-service.bat") -Content $stopService

$serviceStatus = @"
@echo off
setlocal
if exist "%~dp0host-installer.exe" (
    "%~dp0host-installer.exe" --bundle-root "%~dp0." service-status
    exit /b %ERRORLEVEL%
)
echo host-installer.exe was not found. 1>&2
exit /b 1
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "service-status.bat") -Content $serviceStatus

$openHostRuntimeUi = @"
@echo off
start "" "https://localhost:$($SunshineHttpPort + 1)"
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "open-host-runtime-ui.bat") -Content $openHostRuntimeUi
if (Test-Path $HostControlExe) {
    $openHostControlCompat = @"
@echo off
call "%~dp0open-host-control.cmd"
"@
    Write-Utf8NoBom -Path (Join-Path $bundleRoot "open-host-control.bat") -Content $openHostControlCompat
}
Copy-Item $BundleProcessManagerExe (Join-Path $bundleRoot "bundle-process-manager.exe") -Force
if (Test-Path $KeeperTunnelSourceDir) {
    Copy-Item -Path $KeeperTunnelSourceDir -Destination $bundleRoot -Recurse -Force
}

$stopBundlePs1 = New-StopBundlePs1 -LocalWebPort $LocalWebPort -SunshineHttpPort $SunshineHttpPort
Write-Utf8NoBom -Path (Join-Path $bundleRoot "stop-bundle.ps1") -Content $stopBundlePs1

$stopAllBat = @"
@echo off
if exist "%~dp0host-installer.exe" (
"%~dp0host-installer.exe" --bundle-root "%~dp0." stop-bundle
) else if exist "%~dp0moonlight\system\cloudgime-runtime-agent.exe" (
"%~dp0moonlight\system\cloudgime-runtime-agent.exe" --bundle-root "%~dp0." stop-bundle
) else if exist "%~dp0bundle-process-manager.exe" (
"%~dp0bundle-process-manager.exe" stop --bundle-root "%~dp0." --web-port $LocalWebPort --sunshine-port $SunshineHttpPort
) else (
powershell -ExecutionPolicy Bypass -File "%~dp0stop-bundle.ps1"
)
exit /b %ERRORLEVEL%
"@
Write-Utf8NoBom -Path (Join-Path $bundleRoot "stop-all.bat") -Content $stopAllBat

Write-Utf8NoBom -Path (Join-Path $bundleRoot "ensure-firewall.ps1") -Content (New-EnsureFirewallPs1 -HostLabel $hostLabel -WebRtcMinPort $webRtcMinPort -WebRtcMaxPort $webRtcMaxPort)
Write-Utf8NoBom -Path (Join-Path $bundleRoot "verify-startup.ps1") -Content (New-VerifyStartupPs1 -LocalWebPort $LocalWebPort -UrlPathPrefix $UrlPathPrefix)

Write-Utf8NoBom -Path (Join-Path $bundleRoot "start-all.bat") -Content (New-StartAllBat -PublicUrl $publicUrl -LocalWebPort $LocalWebPort -SunshineHttpPort $SunshineHttpPort -DisableTurn ([bool]$DisableTurn))
Write-Utf8NoBom -Path (Join-Path $bundleRoot "PUBLIC_URL.txt") -Content ($publicUrl + [Environment]::NewLine)
Write-Utf8NoBom -Path (Join-Path $bundleRoot "README.txt") -Content (New-ReadmeText -PublicUrl $publicUrl -SunshineHttpPort $SunshineHttpPort -WebRtcMinPort $webRtcMinPort -WebRtcMaxPort $webRtcMaxPort -HostLabel $hostLabel -DisableTurn ([bool]$DisableTurn))
Write-Utf8NoBom -Path (Join-Path $bundleRoot "SETUP.txt") -Content (New-SetupText -HostLabel $hostLabel -PublicUrl $publicUrl -SunshineHttpPort $SunshineHttpPort -WebRtcMinPort $webRtcMinPort -WebRtcMaxPort $webRtcMaxPort -DisableTurn ([bool]$DisableTurn))
Write-Utf8NoBom -Path (Join-Path $bundleRoot "HOST_RUNTIME_LOGIN.txt") -Content ("Local host runtime credentials are intentionally not written to disk.`r`nUse your secure deployment secret source to manage local runtime admin credentials.`r`n")

if ($Zip) {
    $zipPath = Join-Path $OutputDir "$hostLabel.zip"
    if (Test-Path $zipPath) {
        Remove-Item -Force $zipPath
    }
    Compress-Archive -Path (Join-Path $bundleRoot "*") -DestinationPath $zipPath -Force
    Write-Host "[OK] Portable zip created at $zipPath"
} else {
    Write-Host "[OK] Portable bundle created at $bundleRoot"
}
