param(
    [int]$PcNumber = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$makeScript = Join-Path $scriptRoot "make_portable_mon.ps1"

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

if ($PcNumber -eq 0) {
    $PcNumber = Read-ValidPcNumber
}

if (-not (Test-Path $makeScript)) {
    throw "Builder wrapper not found: $makeScript"
}

$hostLabel = "mon$PcNumber"
$bundleRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptRoot "..\..\export\$hostLabel"))
$stopScript = Join-Path $bundleRoot "stop-all.bat"
$startScript = Join-Path $bundleRoot "start-all.bat"

if (Test-Path $stopScript) {
    Write-Host "Menghentikan bundle lama $hostLabel..." -ForegroundColor Cyan
    & $stopScript
    Start-Sleep -Seconds 2
}

& $makeScript -PcNumber $PcNumber

if (-not (Test-Path $startScript)) {
    throw "Start script not found: $startScript"
}

Write-Host ""
Write-Host "Menjalankan bundle $hostLabel..." -ForegroundColor Cyan
Start-Process -FilePath $startScript | Out-Null
Write-Host "Selesai start: https://$hostLabel.pc.cloudgime.my.id/moonlight/"
