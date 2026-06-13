param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptDir ".."))
$toolchainFile = Join-Path $repoRoot "rust-toolchain.toml"
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
    throw "rustup tidak ditemukan. Host Rust build harus lewat rustup agar toolchain nightly repo dipakai."
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

$hasExplicitTargetDir = $false
for ($i = 0; $i -lt $CargoArgs.Count; $i++) {
    if ($CargoArgs[$i] -eq "--target-dir" -or $CargoArgs[$i].StartsWith("--target-dir=")) {
        $hasExplicitTargetDir = $true
        break
    }
}
if (-not $hasExplicitTargetDir -and [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR = "C:\cg-host-target"
}

Push-Location $repoRoot
try {
    & $cargoExe @CargoArgs
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
