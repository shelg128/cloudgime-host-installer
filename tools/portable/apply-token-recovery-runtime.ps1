$ErrorActionPreference = 'Stop'
$source = 'C:\Users\Admin\Documents\CLOUDGIME WORKSPACE\01 SOURCE AKTIF\01 CLOUDGIME HOST\export\mon1\moonlight\system\cloudgime-runtime-agent.exe'
$target = 'C:\ProgramData\CloudGime\Host\moonlight\system\cloudgime-runtime-agent.exe'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$log = 'C:\Windows\Temp\cloudgime-token-recovery-install.log'
try {
  "[$(Get-Date -Format o)] begin" | Set-Content -LiteralPath $log -Encoding UTF8
  if (-not (Test-Path -LiteralPath $source)) { throw "source binary not found: $source" }
  Stop-Service -Name 'CloudgimeHost-Host' -Force -ErrorAction Stop
  Start-Sleep -Seconds 8
  Get-Process -Name 'cloudgime-runtime-agent' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 3
  Copy-Item -LiteralPath $target -Destination ($target + '.bak-token-recovery-' + $stamp) -Force
  Copy-Item -LiteralPath $source -Destination $target -Force
  Start-Service -Name 'CloudgimeHost-Host' -ErrorAction Stop
  Start-Sleep -Seconds 20
  $sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
  $targetHash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
  $service = (Get-Service -Name 'CloudgimeHost-Host').Status.ToString()
  "[$(Get-Date -Format o)] OK service=$service sourceHash=$sourceHash targetHash=$targetHash" | Add-Content -LiteralPath $log -Encoding UTF8
  if ($sourceHash -ne $targetHash) { throw 'hash mismatch after installation' }
} catch {
  "[$(Get-Date -Format o)] ERROR $($_.Exception.Message)" | Add-Content -LiteralPath $log -Encoding UTF8
  exit 1
}
