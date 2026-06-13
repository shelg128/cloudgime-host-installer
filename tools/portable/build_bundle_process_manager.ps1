Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectPath = Join-Path $scriptRoot "BundleProcessManager\BundleProcessManager.csproj"
$publishDir = Join-Path $scriptRoot "..\..\runtime\tools\bundle-process-manager-publish"
$finalExe = Join-Path $scriptRoot "..\..\runtime\tools\bundle-process-manager.exe"

if (-not (Test-Path $projectPath)) {
    throw "BundleProcessManager project not found: $projectPath"
}

if (Test-Path $publishDir) {
    Remove-Item -Recurse -Force $publishDir
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $finalExe) | Out-Null

dotnet publish $projectPath -c Release -o $publishDir

$publishedExe = Join-Path $publishDir "bundle-process-manager.exe"
if (-not (Test-Path $publishedExe)) {
    throw "Published helper exe not found: $publishedExe"
}

Copy-Item $publishedExe $finalExe -Force
Write-Host "[OK] Built bundle process manager at $finalExe"
