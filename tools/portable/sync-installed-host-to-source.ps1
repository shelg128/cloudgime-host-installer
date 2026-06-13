param(
    [string]$InstalledRoot = "",
    [string]$RepoRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Join-Path $scriptDir "..\.."
}
$RepoRoot = [System.IO.Path]::GetFullPath($RepoRoot)

if ([string]::IsNullOrWhiteSpace($InstalledRoot)) {
    $InstalledRoot = Join-Path $env:ProgramData "Cloudgime\Host"
}
$InstalledRoot = [System.IO.Path]::GetFullPath($InstalledRoot)

if (-not (Test-Path -LiteralPath $InstalledRoot)) {
    throw "Installed host root tidak ditemukan: $InstalledRoot"
}

function Assert-UnderRepoRoot([string]$Path) {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $normalizedRepoRoot = $RepoRoot
    if (-not $normalizedRepoRoot.EndsWith('\')) {
        $normalizedRepoRoot += '\'
    }

    if (-not $fullPath.StartsWith($normalizedRepoRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Target di luar repo source tidak diizinkan: $fullPath"
    }
}

function Reset-Dir([string]$Path) {
    Assert-UnderRepoRoot $Path
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
}

function Copy-FileSafe([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source)) {
        throw "Source file tidak ditemukan: $Source"
    }

    Assert-UnderRepoRoot $Destination
    $parent = Split-Path -Parent $Destination
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Copy-TreeFiltered(
    [string]$SourceRoot,
    [string]$DestRoot,
    [string[]]$ExcludeDirNames = @(),
    [string[]]$ExcludeFilePatterns = @()
) {
    if (-not (Test-Path -LiteralPath $SourceRoot)) {
        throw "Source directory tidak ditemukan: $SourceRoot"
    }

    Assert-UnderRepoRoot $DestRoot
    $normalizedSource = [System.IO.Path]::GetFullPath($SourceRoot)
    if (-not $normalizedSource.EndsWith('\')) {
        $normalizedSource += '\'
    }

    Get-ChildItem -LiteralPath $SourceRoot -Recurse -File -Force | ForEach-Object {
        $fullName = [System.IO.Path]::GetFullPath($_.FullName)
        $relative = if ($fullName.StartsWith($normalizedSource, [System.StringComparison]::OrdinalIgnoreCase)) {
            $fullName.Substring($normalizedSource.Length)
        } else {
            $_.Name
        }

        $skip = $false
        $segments = $relative -split '[\\/]'
        foreach ($segment in $segments) {
            if ($ExcludeDirNames -contains $segment) {
                $skip = $true
                break
            }
        }
        if ($skip) {
            return
        }

        foreach ($pattern in $ExcludeFilePatterns) {
            if ($_.Name -like $pattern) {
                $skip = $true
                break
            }
        }
        if ($skip) {
            return
        }

        $destPath = Join-Path $DestRoot $relative
        $destDir = Split-Path -Parent $destPath
        if ($destDir) {
            New-Item -ItemType Directory -Force -Path $destDir | Out-Null
        }
        Copy-Item -LiteralPath $_.FullName -Destination $destPath -Force
    }
}

function Sync-DirectoryFiltered(
    [string]$SourceRoot,
    [string]$DestRoot,
    [string[]]$ExcludeDirNames = @(),
    [string[]]$ExcludeFilePatterns = @()
) {
    Reset-Dir $DestRoot
    Copy-TreeFiltered -SourceRoot $SourceRoot -DestRoot $DestRoot -ExcludeDirNames $ExcludeDirNames -ExcludeFilePatterns $ExcludeFilePatterns
}

function Restore-NativeBridgePanel([string]$RepoRoot, [string]$MoonlightStaticRoot) {
    $candidates = @(
        (Join-Path $RepoRoot "..\android native app\app\src\main\assets\native_bridge_panel.html"),
        (Join-Path $RepoRoot "dist\native_bridge_panel.html")
    ) | ForEach-Object { [System.IO.Path]::GetFullPath($_) }

    $source = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($source)) {
        Write-Warning "native_bridge_panel.html tidak ditemukan di source kandidat. Runtime static host akan mengikuti hasil sync live."
        return
    }

    $target = Join-Path $MoonlightStaticRoot "native_bridge_panel.html"
    Copy-FileSafe $source $target
}

$moonlightRoot = Join-Path $RepoRoot "runtime\moonlight"
$sunshineRoot = Join-Path $RepoRoot "runtime\sunshine"
$legacySunshineRoot = Join-Path $RepoRoot "runtime\sunshine-legacy"
$toolsRoot = Join-Path $RepoRoot "runtime\tools"
$driverSeedRoot = Join-Path $RepoRoot "payload-seed\drivers"

Write-Host "[SYNC] Menyalin runtime Moonlight aktif ke source..." -ForegroundColor Cyan
Reset-Dir $moonlightRoot
Copy-FileSafe (Join-Path $InstalledRoot "moonlight\web-server.exe") (Join-Path $moonlightRoot "web-server.exe")
Copy-FileSafe (Join-Path $InstalledRoot "moonlight\streamer.exe") (Join-Path $moonlightRoot "streamer.exe")
if (Test-Path -LiteralPath (Join-Path $InstalledRoot "moonlight\mic_sidecar.exe")) {
    Copy-FileSafe (Join-Path $InstalledRoot "moonlight\mic_sidecar.exe") (Join-Path $moonlightRoot "mic_sidecar.exe")
}
Copy-FileSafe (Join-Path $InstalledRoot "host-installer.exe") (Join-Path $moonlightRoot "host_installer.exe")
Copy-FileSafe (Join-Path $InstalledRoot "moonlight\system\cloudgime-runtime-agent.exe") (Join-Path $moonlightRoot "host_supervisor.exe")
Copy-TreeFiltered -SourceRoot (Join-Path $InstalledRoot "moonlight\static") -DestRoot (Join-Path $moonlightRoot "static") -ExcludeDirNames @() -ExcludeFilePatterns @("*.bak*", "*.prepatch*")
Restore-NativeBridgePanel -RepoRoot $RepoRoot -MoonlightStaticRoot (Join-Path $moonlightRoot "static")

Write-Host "[SYNC] Menyalin runtime Sunshine aktif ke source..." -ForegroundColor Cyan
Sync-DirectoryFiltered -SourceRoot (Join-Path $InstalledRoot "sunshine") -DestRoot $sunshineRoot -ExcludeDirNames @("credentials", "data") -ExcludeFilePatterns @("*.bak*", "*.disabled-*", "*.log", "*.tmp")

if (Test-Path -LiteralPath (Join-Path $InstalledRoot "sunshine-legacy")) {
    Write-Host "[SYNC] Menyalin runtime Sunshine legacy aktif ke source..." -ForegroundColor Cyan
    Sync-DirectoryFiltered -SourceRoot (Join-Path $InstalledRoot "sunshine-legacy") -DestRoot $legacySunshineRoot -ExcludeDirNames @("credentials", "data") -ExcludeFilePatterns @("*.bak*", "*.disabled-*", "*.log", "*.tmp")
}

Write-Host "[SYNC] Menyalin seed driver aktif ke source..." -ForegroundColor Cyan
Sync-DirectoryFiltered -SourceRoot (Join-Path $InstalledRoot "drivers") -DestRoot $driverSeedRoot -ExcludeDirNames @("logs") -ExcludeFilePatterns @("*.bak*", "*.log", "*.tmp")

Write-Host "[SYNC] Menyalin helper runtime aktif ke source..." -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $toolsRoot | Out-Null
Copy-FileSafe (Join-Path $InstalledRoot "bundle-process-manager.exe") (Join-Path $toolsRoot "bundle-process-manager.exe")

$keeperTunnelExe = Join-Path $InstalledRoot "keeper-tunnel\KeeperTunnelAgent.exe"
if (Test-Path -LiteralPath $keeperTunnelExe) {
    Copy-FileSafe $keeperTunnelExe (Join-Path $toolsRoot "keeper-tunnel\KeeperTunnelAgent.exe")
}

Write-Host "[SYNC] Source host sudah diselaraskan dengan runtime stabil dari mesin ini." -ForegroundColor Green
Write-Host "[SYNC] Repo source: $RepoRoot"
Write-Host "[SYNC] Referensi live: $InstalledRoot"
