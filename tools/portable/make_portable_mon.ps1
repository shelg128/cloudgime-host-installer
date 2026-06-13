param(
    [int]$PcNumber = 0,
    [ValidateSet("direct-strong", "direct-safe")]
    [string]$TransportPreset = "direct-strong",
    [switch]$UseTurnFallback
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$builder = Join-Path $scriptRoot "build_portable_host_bundle.ps1"
$hostPrefix = "mon"
$baseDomain = "pc.cloudgime.my.id"
$webRtcBasePort = 40000
$webRtcBlockSize = 50
$exportDir = [System.IO.Path]::GetFullPath((Join-Path $scriptRoot "..\..\export"))

function Read-ValidPcNumber {
    while ($true) {
        $rawValue = Read-Host "PC keberapa"
        $parsedNumber = 0

        if ([int]::TryParse($rawValue, [ref]$parsedNumber) -and $parsedNumber -ge 1) {
            return $parsedNumber
        }

        Write-Host "Input harus angka 1 atau lebih." -ForegroundColor Yellow
    }
}

if ($PcNumber -lt 0) {
    throw "PcNumber cannot be negative."
}

if ($UseTurnFallback) {
    $TransportPreset = "direct-safe"
}

if ($PcNumber -eq 0) {
    $PcNumber = Read-ValidPcNumber
}

if (-not (Test-Path $builder)) {
    throw "Builder script not found: $builder"
}

$hostLabel = "$hostPrefix$PcNumber"
$hostname = "$hostLabel.$baseDomain"
$webRtcMinPort = $webRtcBasePort + (($PcNumber - 1) * $webRtcBlockSize)
$webRtcMaxPort = $webRtcMinPort + $webRtcBlockSize - 1
$localWebPort = 18080 + ($PcNumber - 1)
$sunshineHttpPort = 49000 + (($PcNumber - 1) * 10)
$sunshinePassword = "$hostLabel-login"
$zipPath = Join-Path $exportDir "$hostLabel.zip"

Write-Host ""
Write-Host "Membuat portable bundle..." -ForegroundColor Cyan
Write-Host "PC Number  : $PcNumber"
Write-Host "Host       : $hostname"
Write-Host "Public URL : https://$hostname/moonlight/"
Write-Host "Local Web  : $localWebPort"
Write-Host "Sunshine   : $sunshineHttpPort / $($sunshineHttpPort + 1)"
Write-Host "Login      : admin / $sunshinePassword"
Write-Host "WebRTC UDP : $webRtcMinPort-$webRtcMaxPort"
Write-Host "Preset     : $TransportPreset"
Write-Host "Mode       : $(if ($TransportPreset -eq 'direct-safe') { 'Auto direct -> TURN fallback' } else { 'Direct-only (TURN off)' })"
Write-Host ""

& $builder `
    -PcNumber $PcNumber `
    -OutputDir "..\..\export" `
    -LocalWebPort $localWebPort `
    -SunshineHttpPort $sunshineHttpPort `
    -SunshinePassword $sunshinePassword `
    -DisableTurn:($TransportPreset -eq "direct-strong") `
    -Zip

Write-Host ""
Write-Host "Selesai." -ForegroundColor Green
Write-Host "Zip file   : $zipPath"

if (Test-Path $exportDir) {
    Write-Host "Membuka folder export..."
    Start-Process -FilePath "explorer.exe" -ArgumentList $exportDir | Out-Null
}
