# setup-build-environment.ps1
# Automates setting up the compilation/build environment for Cloudgime Host on a new Windows machine.
# Must be run as Administrator!

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "Script ini harus dijalankan sebagai Administrator! Silakan buka PowerShell/CMD sebagai Administrator dan jalankan ulang."
    Exit
}

Write-Host "===============================================" -ForegroundColor Cyan
Write-Host "=== SETUP LINGKUNGAN BUILD CLOUDGIME HOST ===" -ForegroundColor Cyan
Write-Host "===============================================" -ForegroundColor Cyan
Write-Host ""

# 1. Install toolchain via winget
$tools = @(
    @{ Id = "Git.Git"; Name = "Git" },
    @{ Id = "OpenJS.NodeJS.LTS"; Name = "Node.js LTS" },
    @{ Id = "Microsoft.DotNet.SDK.8"; Name = ".NET 8 SDK" },
    @{ Id = "Rustlang.Rustup"; Name = "Rustup (Rust)" },
    @{ Id = "Nullsoft.NSIS"; Name = "NSIS (Nullsoft Installer)" },
    @{ Id = "ShiningLight.OpenSSL.Dev"; Name = "OpenSSL Developer Edition" }
)

Write-Host "[1/4] Menginstal Toolchain Utama via Winget..." -ForegroundColor Cyan
foreach ($tool in $tools) {
    Write-Host "Memeriksa $($tool.Name)..." -NoNewline
    $chk = & winget list --id $tool.Id -e 2>$null
    if ($chk -like "*$($tool.Id)*") {
        Write-Host " [SUDAH TERPASANG]" -ForegroundColor Green
    } else {
        Write-Host " [MENGUNDUH/MENGINSTAL]" -ForegroundColor Yellow
        & winget install --id $tool.Id -e --accept-package-agreements --accept-source-agreements --silent
        if ($LASTEXITCODE -eq 0) {
            Write-Host " -> $($tool.Name) berhasil diinstal!" -ForegroundColor Green
        } else {
            Write-Warning " Gagal menginstal $($tool.Name) secara otomatis. Silakan pasang secara manual."
        }
    }
}

# 2. Check for Visual Studio Build Tools / MSVC C++ Workload
Write-Host "`n[2/4] Memeriksa Visual Studio C++ Build Tools (MSVC)..." -ForegroundColor Cyan
$vsInstalled = $false
$vsPathCandidates = @(
    "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe",
    "${env:ProgramFiles}\Microsoft Visual Studio\Installer\vswhere.exe"
)
$vswhere = $null
foreach ($cand in $vsPathCandidates) {
    if (Test-Path $cand) { $vswhere = $cand; break }
}

if ($vswhere) {
    $vsInfo = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vsInfo) {
        Write-Host "[OK] MSVC C++ Build Tools terdeteksi di: $vsInfo" -ForegroundColor Green
        $vsInstalled = $true
    }
}

if (-not $vsInstalled) {
    Write-Host "[WARNING] MSVC C++ Build Tools tidak ditemukan!" -ForegroundColor Yellow
    Write-Host "Rust memerlukan linker C++ MSVC. Mencoba memasang Visual Studio Build Tools via winget..."
    & winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK] VS Build Tools berhasil diinstal dengan C++ Workload!" -ForegroundColor Green
    } else {
        Write-Host "[INFO] Jika build Rust gagal nanti, pasang Visual Studio 2022 Community / Build Tools dan pilih beban kerja 'Desktop development with C++'." -ForegroundColor Yellow
    }
}

# 3. Configure OpenSSL Environment Variable (Critical for Rust Compiler)
Write-Host "`n[3/4] Mengonfigurasi Variabel Lingkungan OpenSSL..." -ForegroundColor Cyan
$opensslDirs = @(
    "C:\Program Files\OpenSSL-Win64",
    "C:\Program Files\OpenSSL-Dev-Win64",
    "C:\Program Files\OpenSSL",
    "C:\OpenSSL-Win64",
    "C:\OpenSSL-Dev-Win64"
)

$detectedOpenSSL = $null
foreach ($dir in $opensslDirs) {
    if (Test-Path (Join-Path $dir "include\openssl\ssl.h")) {
        $detectedOpenSSL = $dir
        break
    }
}

if ($detectedOpenSSL) {
    Write-Host "[OK] Menemukan OpenSSL di: $detectedOpenSSL" -ForegroundColor Green
    
    # Set OPENSSL_DIR permanently
    [Environment]::SetEnvironmentVariable("OPENSSL_DIR", $detectedOpenSSL, "Machine")
    $env:OPENSSL_DIR = $detectedOpenSSL
    Write-Host "  -> System Environment Variable OPENSSL_DIR = $detectedOpenSSL (Diatur)" -ForegroundColor Green
    
    # Add to Machine PATH if not already there
    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $binPath = Join-Path $detectedOpenSSL "bin"
    if ($machinePath -notlike "*$binPath*") {
        [Environment]::SetEnvironmentVariable("Path", "$machinePath;$binPath", "Machine")
        $env:Path = "$env:Path;$binPath"
        Write-Host "  -> Menambahkan OpenSSL bin ke System PATH" -ForegroundColor Green
    }
} else {
    Write-Host "[X] OpenSSL Development headers tidak terdeteksi!" -ForegroundColor Red
    Write-Host "Silakan jalankan ulang script ini setelah ShiningLight.OpenSSL.Dev selesai dipasang secara penuh." -ForegroundColor Yellow
}

# 4. Verify Toolchain
Write-Host "`n[4/4] Memverifikasi Keaktifan Toolchain..." -ForegroundColor Cyan
Write-Host "Node.js      : " -NoNewline; & node -v 2>$null | Out-Host
Write-Host ".NET SDK     : " -NoNewline; & dotnet --version 2>$null | Out-Host
Write-Host "Rust (cargo) : " -NoNewline; & cargo --version 2>$null | Out-Host
Write-Host "NSIS compiler: " -NoNewline; & makensis.exe /VERSION 2>$null | Out-Host

Write-Host ""
Write-Host "=== PROSES SELESAI ===" -ForegroundColor Green
Write-Host "Lingkungan siap! Anda sekarang dapat menjalankan build-installer.bat dengan aman."
Read-Host "Tekan Enter untuk keluar..."
