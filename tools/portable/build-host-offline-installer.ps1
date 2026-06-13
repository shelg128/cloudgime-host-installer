param(
    [string]$RepoRoot = "",
    [string]$InstalledRoot = "",
    [string]$BundleRoot = "",
    [string]$OutputDir = "",
    [string]$KeeperTunnelProject = "",
    [string]$KeeperTunnelExe = "",
    [switch]$SyncInstalledRuntime,
    [switch]$SkipBundleBuild
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

if ([string]::IsNullOrWhiteSpace($BundleRoot)) {
    $BundleRoot = Join-Path $RepoRoot "export\mon1"
}
$BundleRoot = [System.IO.Path]::GetFullPath($BundleRoot)

$portableRoot = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot "tools\portable"))
$hostControlRoot = [System.IO.Path]::GetFullPath((Join-Path $portableRoot "HostControlApp.Tauri"))
$preparePayloadCmd = Join-Path $hostControlRoot "installer\prepare-nsis-payload.cmd"
$buildNsisCmd = Join-Path $hostControlRoot "installer\build-nsis.cmd"
$bundleBuilderScript = Join-Path $portableRoot "build_portable_host_bundle.ps1"
$syncScript = Join-Path $portableRoot "sync-installed-host-to-source.ps1"

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $hostControlRoot "installer\output"
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)

function Add-ToolPathIfExists([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return
    }

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    $pathEntries = @($env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($pathEntries -contains $Path) {
        return
    }

    $env:Path = "$Path;$env:Path"
}

function Import-VsDevEnvironment {
    $candidates = New-Object System.Collections.Generic.List[string]
    $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $installationPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Workload.VCTools -property installationPath 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($installationPath)) {
            $candidates.Add((Join-Path $installationPath.Trim() 'Common7\Tools\VsDevCmd.bat'))
        }
    }

    foreach ($candidate in @(
        'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'
    )) {
        $candidates.Add($candidate)
    }

    $vsDevCmd = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($vsDevCmd)) {
        return
    }

    $cmdLine = '"' + $vsDevCmd + '" -arch=amd64 -host_arch=amd64 >nul && set'
    $seenEnvNames = @{}
    cmd /d /c $cmdLine | ForEach-Object {
        $parts = $_ -split '=', 2
        if ($parts.Length -eq 2) {
            $envName = $parts[0]
            $envKey = $envName.ToUpperInvariant()
            if (-not $seenEnvNames.ContainsKey($envKey)) {
                $seenEnvNames[$envKey] = $true
                Set-Item -Path ("Env:" + $envName) -Value $parts[1]
            }
        }
    }
}

function Assert-Command([string]$CommandName, [string]$FriendlyName) {
    if (-not (Get-Command $CommandName -ErrorAction SilentlyContinue)) {
        throw "$FriendlyName tidak ditemukan di mesin build."
    }
}

function Assert-Makensis {
    $command = Get-Command makensis -ErrorAction SilentlyContinue
    if ($command) {
        return
    }

    foreach ($candidate in @(
        "C:\Program Files (x86)\NSIS\makensis.exe",
        "C:\Program Files\NSIS\makensis.exe"
    )) {
        if (Test-Path -LiteralPath $candidate) {
            return
        }
    }

    throw "makensis.exe tidak ditemukan di mesin build."
}

function Invoke-Checked([string]$FilePath, [string[]]$Arguments) {
    & $FilePath @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Perintah gagal: $FilePath $($Arguments -join ' ')"
    }
}

function Resolve-ReleaseBinary([string]$BinaryName) {
    $candidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        $candidates.Add((Join-Path $env:CARGO_TARGET_DIR "release\$BinaryName"))
    }
    $candidates.Add((Join-Path $RepoRoot "target\release\$BinaryName"))

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return [System.IO.Path]::GetFullPath($candidate)
        }
    }

    throw "$BinaryName hasil build tidak ditemukan."
}

function Get-Sha256([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "File tidak ditemukan untuk hash: $Path"
    }

    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToUpperInvariant()
}

function Assert-SameBinary([string]$ExpectedPath, [string]$ActualPath, [string]$Label) {
    $expectedHash = Get-Sha256 $ExpectedPath
    $actualHash = Get-Sha256 $ActualPath
    if ($expectedHash -ne $actualHash) {
        throw "$Label stale: $ActualPath tidak sama dengan hasil build terbaru $ExpectedPath. Jalankan ulang tanpa -SkipBundleBuild agar bundle dibuat ulang."
    }
}

function Use-PinnedHostRustToolchain {
    $toolchainFile = Join-Path $RepoRoot "rust-toolchain.toml"
    if (-not (Test-Path -LiteralPath $toolchainFile)) {
        throw "rust-toolchain.toml tidak ditemukan: $toolchainFile"
    }

    $toolchain = $null
    foreach ($line in Get-Content -LiteralPath $toolchainFile) {
        if ($line -match '^\s*channel\s*=\s*"([^"]+)"') {
            $toolchain = $Matches[1]
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($toolchain)) {
        throw "channel toolchain tidak ditemukan di $toolchainFile"
    }

    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if (-not $rustup) {
        throw "rustup tidak ditemukan. Build host harus memakai rustup agar toolchain nightly repo dipakai."
    }

    $cargoExe = (& $rustup.Source which --toolchain $toolchain cargo).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($cargoExe)) {
        throw "cargo untuk toolchain $toolchain tidak ditemukan."
    }

    $rustcExe = (& $rustup.Source which --toolchain $toolchain rustc).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($rustcExe)) {
        throw "rustc untuk toolchain $toolchain tidak ditemukan."
    }

    $toolchainBin = Split-Path -Parent $cargoExe
    $pathEntries = @($env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $pathEntries = @($toolchainBin) + @($pathEntries | Where-Object { $_ -ne $toolchainBin })
    $env:Path = ($pathEntries -join ';')
    $env:RUSTC = $rustcExe
    if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        $env:CARGO_TARGET_DIR = "C:\cg-host-target"
    }
    Write-Host "[BUILD] Rust toolchain terkunci: $toolchain" -ForegroundColor Cyan
}

Add-ToolPathIfExists 'C:\Program Files\nodejs'
Add-ToolPathIfExists 'C:\Program Files (x86)\NSIS'

$rustToolRoot = Get-ChildItem 'C:\Program Files' -Filter 'Rust stable MSVC*' -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    Select-Object -First 1
if ($rustToolRoot) {
    Add-ToolPathIfExists (Join-Path $rustToolRoot.FullName 'bin')
}

if (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.cargo\bin')) {
    Add-ToolPathIfExists (Join-Path $env:USERPROFILE '.cargo\bin')
}

Import-VsDevEnvironment
Use-PinnedHostRustToolchain

Assert-Command "dotnet" "dotnet"
Assert-Command "node" "node"
Assert-Command "npm" "npm"
Assert-Command "cargo" "cargo"
Assert-Command "rustc" "rustc"
Assert-Makensis
Assert-Command "cl" "Visual Studio C++ Build Tools"

if ($SyncInstalledRuntime) {
    if (-not (Test-Path -LiteralPath $InstalledRoot)) {
        throw "Installed host root tidak ditemukan untuk sync: $InstalledRoot"
    }

    Write-Host "[BUILD] Menyelaraskan source host dari runtime stabil..." -ForegroundColor Cyan
    Invoke-Checked "powershell" @(
        "-ExecutionPolicy", "Bypass",
        "-File", $syncScript,
        "-RepoRoot", $RepoRoot,
        "-InstalledRoot", $InstalledRoot
    )
}

Write-Host "[BUILD] Membuild web-server host terbaru..." -ForegroundColor Cyan
Push-Location $RepoRoot
try {
    cargo build --release --bin web-server | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build web-server gagal dengan exit code $LASTEXITCODE"
    }

    Write-Host "[BUILD] Membuild streamer host terbaru..." -ForegroundColor Cyan
    cargo build --release -p streamer | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build streamer gagal dengan exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$webServerSourceExe = Resolve-ReleaseBinary "web-server.exe"
$streamerSourceExe = Resolve-ReleaseBinary "streamer.exe"
$micSidecarSourceExe = Resolve-ReleaseBinary "mic_sidecar.exe"

$runtimeMoonlightRoot = Join-Path $RepoRoot "runtime\moonlight"
if (-not (Test-Path -LiteralPath $runtimeMoonlightRoot)) {
    throw "Runtime moonlight root tidak ditemukan: $runtimeMoonlightRoot"
}
Copy-Item -LiteralPath $webServerSourceExe -Destination (Join-Path $runtimeMoonlightRoot "web-server.exe") -Force
Copy-Item -LiteralPath $streamerSourceExe -Destination (Join-Path $runtimeMoonlightRoot "streamer.exe") -Force
Copy-Item -LiteralPath $micSidecarSourceExe -Destination (Join-Path $runtimeMoonlightRoot "mic_sidecar.exe") -Force

if ([string]::IsNullOrWhiteSpace($KeeperTunnelProject)) {
    $defaultKeeperTunnelProject = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot "..\power panel\KeeperTunnelAgent\KeeperTunnelAgent.csproj"))
    if (Test-Path -LiteralPath $defaultKeeperTunnelProject) {
        $KeeperTunnelProject = $defaultKeeperTunnelProject
    }
}

if (-not $SkipBundleBuild) {
    Write-Host "[BUILD] Membuat bundle host final dari source..." -ForegroundColor Cyan
    Invoke-Checked "powershell" @(
        "-ExecutionPolicy", "Bypass",
        "-File", $bundleBuilderScript,
        "-PcNumber", "1",
        "-OutputDir", (Join-Path $RepoRoot "export")
    )
} else {
    Write-Host "[BUILD] SkipBundleBuild aktif; memvalidasi bundle existing terhadap binary hasil build terbaru..." -ForegroundColor Yellow
}

if (-not (Test-Path -LiteralPath (Join-Path $BundleRoot "host-installer.exe"))) {
    throw "Bundle host tidak ditemukan atau belum lengkap: $BundleRoot"
}

Assert-SameBinary $webServerSourceExe (Join-Path $BundleRoot "moonlight\web-server.exe") "web-server.exe bundle"
Assert-SameBinary $streamerSourceExe (Join-Path $BundleRoot "moonlight\streamer.exe") "streamer.exe bundle"
Assert-SameBinary $micSidecarSourceExe (Join-Path $BundleRoot "moonlight\mic_sidecar.exe") "mic_sidecar.exe bundle"

$prepareArgs = @("--bundle-root", $BundleRoot)
if (-not [string]::IsNullOrWhiteSpace($KeeperTunnelProject)) {
    $prepareArgs += @("--keeper-tunnel-project", $KeeperTunnelProject)
} elseif (-not [string]::IsNullOrWhiteSpace($KeeperTunnelExe)) {
    $prepareArgs += @("--keeper-tunnel-exe", $KeeperTunnelExe)
}

Write-Host "[BUILD] Menyiapkan payload NSIS host..." -ForegroundColor Cyan
Invoke-Checked $preparePayloadCmd $prepareArgs

Write-Host "[BUILD] Membuat installer offline host..." -ForegroundColor Cyan
Invoke-Checked $buildNsisCmd @("--output-dir", $OutputDir)

$finalInstaller = Join-Path $OutputDir "CloudgimeHostSetup.exe"
if (-not (Test-Path -LiteralPath $finalInstaller)) {
    throw "Installer final host tidak ditemukan: $finalInstaller"
}

Write-Host "[BUILD] Host offline installer siap: $finalInstaller" -ForegroundColor Green
