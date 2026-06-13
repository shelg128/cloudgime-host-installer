set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "tools/just_shell_entry.ps1"]

bundle_root := "export/mon1"

default:
    @just --list

status:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} status

ops-status:
    @Invoke-RestMethod -Uri 'http://127.0.0.1:18080/moonlight/api/admin/host-ops-status' | ConvertTo-Json -Depth 8

gate duration="60000":
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -DurationMs {{duration}}

gate-smoke:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile smoke-60s

gate-scenario profile="smoke-60s" scenario="mixed" duration="":
    @if ('{{duration}}' -eq '') { & 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} } else { & 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} -DurationMs {{duration}} }

gate-rotate:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile smoke-60s -GateScenario rotate

gate-fullscreen:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile smoke-60s -GateScenario fullscreen

gate-reconnect:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile smoke-60s -GateScenario reconnect

gate-startup-recovery:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile smoke-60s -GateScenario startup-recovery

gate-10m:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile standard-10m

gate-30m:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile endurance-30m

gate-longhaul:
    @& 'tools/run_release_gate_total_stress.ps1' -BundleRoot {{bundle_root}} -GateProfile longhaul

self-test profile="smoke-60s" scenario="mixed" duration="":
    @if ('{{duration}}' -eq '') { & 'tools/run_host_self_test.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} } else { & 'tools/run_host_self_test.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} -DurationMs {{duration}} }

diagnostic-pack profile="smoke-60s" scenario="mixed" duration="":
    @if ('{{duration}}' -eq '') { & 'tools/run_host_self_test.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} } else { & 'tools/run_host_self_test.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} -DurationMs {{duration}} }

refresh-capability:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} refresh-host-capability

adopt-runtime:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} adopt-recommended-runtime

stabilize-runtime profile="smoke-60s" scenario="mixed" duration="":
    @if ('{{duration}}' -eq '') { & 'tools/run_runtime_stabilization.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} } else { & 'tools/run_runtime_stabilization.ps1' -BundleRoot {{bundle_root}} -GateProfile {{profile}} -GateScenario {{scenario}} -DurationMs {{duration}} }

shell-doctor:
    @$currentShellVersion = $PSVersionTable.PSVersion.ToString(); $pwshPath = if (Test-Path 'C:\Program Files\PowerShell\7\pwsh.exe') { 'C:\Program Files\PowerShell\7\pwsh.exe' } else { $null }; $pwshVersion = if ($pwshPath) { & $pwshPath -NoLogo -NoProfile -Command '$PSVersionTable.PSVersion.ToString()' } else { $null }; $pssa = Get-Module -ListAvailable PSScriptAnalyzer | Sort-Object Version -Descending | Select-Object -First 1; [pscustomobject]@{ current_repo_shell = $currentShellVersion; pwsh_path = $pwshPath; pwsh_version = $pwshVersion; psscriptanalyzer_version = if ($pssa) { $pssa.Version.ToString() } else { $null }; just = (& just.exe --version 2>$null | Select-Object -First 1); cargo_nextest = (& cargo-nextest.exe --version 2>$null | Select-Object -First 1) } | ConvertTo-Json -Depth 4

lint-powershell:
    @& 'tools\invoke_script_analyzer.ps1'

apply-upgrade:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} apply-release-upgrade

backup-config-state:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} backup-config-state

restore-config-state:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} restore-latest-config-state

collect-support-bundle:
    @.\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} collect-support-bundle

prepare-promotion target="":
    @if ('{{target}}' -eq '') { .\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} prepare-release-promotion } else { .\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} prepare-release-promotion --target-environment {{target}} }

apply-promotion target="":
    @if ('{{target}}' -eq '') { .\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} apply-release-promotion } else { .\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} apply-release-promotion --target-environment {{target}} }

fleet-status inventory="ops/promotion_inventory.json":
    @cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} status

fleet-prepare-promotion target group="" inventory="ops/promotion_inventory.json":
    @if ('{{group}}' -eq '') { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} prepare-promotion --target-environment {{target}} } else { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} prepare-promotion --target-environment {{target}} --promotion-group {{group}} }

fleet-apply-promotion target group="" inventory="ops/promotion_inventory.json" max_hosts="":
    @if ('{{group}}' -eq '') { if ('{{max_hosts}}' -eq '') { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} apply-promotion --target-environment {{target}} } else { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} apply-promotion --target-environment {{target}} --max-hosts {{max_hosts}} } } else { if ('{{max_hosts}}' -eq '') { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} apply-promotion --target-environment {{target}} --promotion-group {{group}} } else { cargo run --quiet -p web-server --bin release_coordinator -- --inventory {{inventory}} apply-promotion --target-environment {{target}} --promotion-group {{group}} --max-hosts {{max_hosts}} } }

snapshot-memory:
    @& 'tools/write_operational_memory_snapshot.ps1' -BundleRoot {{bundle_root}}

release-health:
    @& 'tools/write_operational_memory_snapshot.ps1' -BundleRoot {{bundle_root}}; .\{{bundle_root}}\host-installer.exe --bundle-root {{bundle_root}} status
