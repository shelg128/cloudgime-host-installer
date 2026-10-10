param(
    [string]$OutputDir = "",
    [string]$InstallerOutputDir = "",
    [string]$MoonlightSourceRoot = "",
    [string]$SunshineSourceRoot = "",
    [string]$LegacySunshineSourceRoot = "",
    [string]$KeeperTunnelProject = "",
    [switch]$DisableTurn,
    [switch]$Zip
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptDir "..\.."))
# Use one deterministic Rust output directory for bundle creation and installer validation.
$env:CARGO_TARGET_DIR = "C:\cg-host-target"
$bundleBuilder = Join-Path $scriptDir "build_portable_host_bundle.ps1"
$installerBuilder = Join-Path $scriptDir "build-host-offline-installer.ps1"

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $repoRoot "export"
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
$bundleRoot = Join-Path $OutputDir "cloudgime-host"

if ([string]::IsNullOrWhiteSpace($InstallerOutputDir)) {
    $InstallerOutputDir = Join-Path $scriptDir "HostControlApp.Tauri\installer\output-universal"
}
$InstallerOutputDir = [System.IO.Path]::GetFullPath($InstallerOutputDir)

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: $FilePath $($Arguments -join ' ')"
    }
}

$bundleArgs = @(
    "-ExecutionPolicy", "Bypass",
    "-File", $bundleBuilder,
    "-PcNumber", "1",
    "-OutputDir", $OutputDir,
    "-Universal",
    "-SkipHostControlBuild"
)
if (-not [string]::IsNullOrWhiteSpace($MoonlightSourceRoot)) {
    $bundleArgs += @("-MoonlightSourceRoot", $MoonlightSourceRoot)
}
if (-not [string]::IsNullOrWhiteSpace($SunshineSourceRoot)) {
    $bundleArgs += @("-SunshineSourceRoot", $SunshineSourceRoot)
}
if (-not [string]::IsNullOrWhiteSpace($LegacySunshineSourceRoot)) {
    $bundleArgs += @("-LegacySunshineSourceRoot", $LegacySunshineSourceRoot)
}
if (-not [string]::IsNullOrWhiteSpace($KeeperTunnelProject)) {
    $bundleArgs += @("-KeeperTunnelProject", $KeeperTunnelProject)
}
if ($DisableTurn) {
    $bundleArgs += "-DisableTurn"
}
if ($Zip) {
    $bundleArgs += "-Zip"
}

Write-Host "[UNIVERSAL] Building neutral Cloudgime Host bundle..." -ForegroundColor Cyan
Invoke-Checked "powershell" $bundleArgs

$manifestPath = Join-Path $bundleRoot "cloudgime-host-bundle.json"
if (-not (Test-Path -LiteralPath $manifestPath)) {
    throw "Universal bundle manifest was not created: $manifestPath"
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.bundleMode -ne "universal") {
    throw "Builder produced a non-universal bundle: $bundleRoot"
}

$installerArgs = @(
    "-ExecutionPolicy", "Bypass",
    "-File", $installerBuilder,
    "-BundleRoot", $bundleRoot,
    "-OutputDir", $InstallerOutputDir,
    "-SkipBundleBuild",
    "-Universal"
)
if (-not [string]::IsNullOrWhiteSpace($KeeperTunnelProject)) {
    $installerArgs += @("-KeeperTunnelProject", $KeeperTunnelProject)
}

Write-Host "[UNIVERSAL] Building one installer for all PCs..." -ForegroundColor Cyan
Invoke-Checked "powershell" $installerArgs

$installerPath = Join-Path $InstallerOutputDir "CloudgimeHostSetup.exe"
if (-not (Test-Path -LiteralPath $installerPath)) {
    throw "Universal installer was not created: $installerPath"
}

Write-Host "[UNIVERSAL] Installer ready: $installerPath" -ForegroundColor Green
Write-Host "[UNIVERSAL] Install on any PC, then paste its CloudRental activation license in Host Control." -ForegroundColor Green
