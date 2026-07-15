use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::{Command, ExitStatus, Output, Stdio},
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use common::api_bindings::{
    HostCapabilityProfile, HostCapabilityRuntimeCandidate, HostDiagnosticPackSummary,
    HostReleaseGateSummary, HostReleaseHistoryEntry, HostReleaseInfo, HostReleaseUpgradeState,
    HostRuntimeAdoptionHistoryEntry, HostRuntimeAdoptionState,
};
use common::config::{Config, PortRange, WebRtcNetworkType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const VIRTUAL_AUDIO_DRIVER_RELEASE_TAG: &str = "25.7.23";
const VIRTUAL_AUDIO_DRIVER_PACKAGE_URL: &str = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualAudioDriver-x86.Driver.Only.zip";
const VDD_CONTROL_PACKAGE_URL: &str = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VDD.Control.25.7.23.zip";
const VIRTUAL_AUDIO_DRIVER_MIN_WINDOWS_BUILD: u32 = 22000;
const VIRTUAL_DISPLAY_DRIVER_HARDWARE_ID: &str = r"Root\MttVDD";
const VIRTUAL_DISPLAY_SETTINGS_TARGET_DIR: &str = r"C:\VirtualDisplayDriver";
const VIRTUAL_DISPLAY_SETTINGS_FILE_NAME: &str = "vdd_settings.xml";
const STREAMER_MEDIA_QOS_DSCP: u8 = 46;
const STREAMER_MEDIA_QOS_PRECEDENCE: u32 = 255;

#[derive(Parser)]
#[command(version, about = "Cloudgime Host installer / operator entrypoint")]
struct Cli {
    #[arg(long)]
    bundle_root: Option<PathBuf>,

    #[command(subcommand)]
    command: InstallerCommand,
}

#[derive(Subcommand)]
enum InstallerCommand {
    PrepareHost,
    PrepareInstall,
    PreflightHost {
        #[arg(long, default_value_t = false)]
        fix: bool,
    },
    RefreshHostCapability,
    PrepareReleaseUpgrade,
    AdoptRecommendedRuntime,
    BackupConfigState,
    RestoreLatestConfigState,
    CollectSupportBundle,
    PrepareReleasePromotion {
        #[arg(long)]
        target_environment: Option<String>,
    },
    ApplyReleasePromotion {
        #[arg(long)]
        target_environment: Option<String>,
    },
    ApplyReleaseUpgrade,
    PromoteReleaseMetadata {
        #[arg(long)]
        deployment_environment: String,
        #[arg(long)]
        release_channel: String,
        #[arg(long)]
        source_branch: String,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        source_commit_short: String,
        #[arg(long, default_value_t = false)]
        source_dirty: bool,
        #[arg(long, default_value = "release")]
        build_profile: String,
        #[arg(long)]
        built_at_unix_ms: Option<u64>,
    },
    ConfigureFirewall,
    ConfigureQos,
    RemoveQos,
    RemediateConfigHygiene,
    CaptureReleaseSnapshot,
    RecordReleaseGate {
        #[arg(long)]
        gate_name: String,
        #[arg(long)]
        gate_profile: Option<String>,
        #[arg(long)]
        gate_scenario: Option<String>,
        #[arg(long)]
        status: String,
        #[arg(long)]
        summary_path: Option<PathBuf>,
        #[arg(long)]
        reason: Option<String>,
    },
    RecordDiagnosticPack {
        #[arg(long)]
        pack_name: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        summary_path: Option<PathBuf>,
        #[arg(long)]
        reason: Option<String>,
    },
    RollbackLatestReleaseSnapshot,
    InstallService,
    UninstallService,
    StartService,
    StopService,
    ServiceStatus,
    StartBundle,
    StopBundle,
    RestartRuntime,
    VerifyStartup,
    Status,
}

#[derive(Debug, Clone)]
struct BundlePaths {
    bundle_root: PathBuf,
    host_installer_path: PathBuf,
    static_root: PathBuf,
    config_path: PathBuf,
    data_path: PathBuf,
    release_info_path: PathBuf,
    promotion_policy_path: PathBuf,
    release_gate_summary_path: PathBuf,
    release_gate_history_path: PathBuf,
    diagnostic_pack_summary_path: PathBuf,
    diagnostic_pack_history_path: PathBuf,
    release_upgrade_state_path: PathBuf,
    release_history_path: PathBuf,
    runtime_adoption_state_path: PathBuf,
    runtime_adoption_history_path: PathBuf,
    server_root: PathBuf,
    host_capability_profile_path: PathBuf,
    audio_dependency_state_path: PathBuf,
    driver_cache_root: PathBuf,
    config_state_backups_root: PathBuf,
    support_bundles_root: PathBuf,
    hard_reset_mode_path: PathBuf,
    release_snapshots_root: PathBuf,
    selected_runtime_path: PathBuf,
    supervisor_path: PathBuf,
    web_server_path: PathBuf,
    streamer_path: PathBuf,
}

fn resolve_supervisor_path(moonlight_root: &std::path::Path) -> PathBuf {
    let internal_path = moonlight_root
        .join("system")
        .join("cloudgime-runtime-agent.exe");
    if internal_path.exists() {
        internal_path
    } else {
        moonlight_root.join("host-supervisor.exe")
    }
}

#[derive(Debug, Serialize)]
struct InstallerIncidentRecord {
    kind: String,
    reason: String,
    strategy: Option<String>,
    escalated: bool,
    at_unix_ms: u64,
}

#[derive(Debug, Serialize)]
struct InstallerStatus {
    bundle_root: String,
    selected_runtime_key: String,
    universal_bundle_grade: String,
    universal_bundle_reason: String,
    capability_probe_mode: Option<String>,
    capability_updated_at: Option<String>,
    capability_selection_reason: Option<String>,
    selected_runtime_display_name: Option<String>,
    selected_runtime_version: Option<String>,
    recommended_runtime_key: Option<String>,
    recommended_runtime_display_name: Option<String>,
    recommended_runtime_version: Option<String>,
    recommended_runtime_reason: Option<String>,
    recommended_runtime_switch_required: bool,
    alternate_ready_runtime_count: u32,
    selected_encoder: Option<String>,
    selected_capture: Option<String>,
    selected_capture_reason: Option<String>,
    preferred_audio_driver: String,
    audio_dependency_status: String,
    audio_dependency_reason: String,
    audio_dependency_package_source: Option<String>,
    audio_dependency_package_inf_path: Option<String>,
    audio_fallback_installer_ready: bool,
    audio_fallback_installer_path: Option<String>,
    audio_endpoint_count: u32,
    selected_audio_sink_name: Option<String>,
    selected_virtual_sink_name: Option<String>,
    selected_microphone_name: Option<String>,
    audio_selection_reason: Option<String>,
    audio_routing_status: String,
    audio_routing_reason: String,
    selected_ffmpeg_source: Option<String>,
    selected_runtime_startup_validation_status: Option<String>,
    selected_runtime_startup_validation_reason: Option<String>,
    service_name: String,
    user_agent_task_name: String,
    user_agent_task_status: String,
    health_grade: String,
    health_reason: String,
    config_hygiene_grade: String,
    config_hygiene_warnings: Vec<String>,
    required_processes_ready: bool,
    local_http_ready: bool,
    lifecycle_phase: String,
    lifecycle_reason: Option<String>,
    lifecycle_updated_at_unix_ms: Option<u64>,
    failure_recovery_attempt_count: u32,
    failure_recovery_window_started_at_unix_ms: Option<u64>,
    last_failure_recovery_reason: Option<String>,
    last_failure_recovery_strategy: Option<String>,
    last_failure_recovery_escalated: bool,
    total_failure_recovery_count: u32,
    total_failure_recovery_escalation_count: u32,
    total_service_watchdog_trigger_count: u32,
    daemon_started_at_unix_ms: Option<u64>,
    boot_failure_recovery_count: u32,
    boot_service_watchdog_trigger_count: u32,
    ready_since_unix_ms: Option<u64>,
    current_ready_streak_ms: Option<u64>,
    daemon_uptime_ms: Option<u64>,
    last_incident_kind: Option<String>,
    last_incident_at_unix_ms: Option<u64>,
    last_failure_recovery_completed_at_unix_ms: Option<u64>,
    last_failure_recovery_budget_cleared_at_unix_ms: Option<u64>,
    last_service_watchdog_reason: Option<String>,
    last_service_watchdog_at_unix_ms: Option<u64>,
    recent_incidents: Vec<InstallerIncidentRecord>,
    release_info: Option<HostReleaseInfo>,
    current_release_id: Option<String>,
    promotion_policy_name: String,
    promotion_ring_order: Vec<String>,
    promotion_bundle_name: String,
    promotion_group: String,
    promotion_stage: String,
    promotion_reason: String,
    promotion_target_environment: Option<String>,
    next_promotion_target_environment: Option<String>,
    next_promotion_readiness: String,
    next_promotion_reason: String,
    next_promotion_required_ready_streak_ms: Option<u64>,
    next_promotion_current_ready_streak_ms: Option<u64>,
    rollback_ready: bool,
    release_snapshot_count: u32,
    last_release_snapshot_id: Option<String>,
    last_release_snapshot_at_unix_ms: Option<u64>,
    config_state_backup_count: u32,
    last_config_state_backup_id: Option<String>,
    last_config_state_backup_at_unix_ms: Option<u64>,
    support_bundle_count: u32,
    last_support_bundle_id: Option<String>,
    last_support_bundle_at_unix_ms: Option<u64>,
    release_gate_status: String,
    release_gate_reason: String,
    release_gate_summary: Option<HostReleaseGateSummary>,
    release_gate_history_count: u32,
    recent_release_gate_history: Vec<HostReleaseGateSummary>,
    diagnostic_pack_status: String,
    diagnostic_pack_reason: String,
    diagnostic_pack_summary: Option<HostDiagnosticPackSummary>,
    diagnostic_pack_history_count: u32,
    recent_diagnostic_pack_history: Vec<HostDiagnosticPackSummary>,
    release_upgrade_state: Option<HostReleaseUpgradeState>,
    recent_release_history: Vec<HostReleaseHistoryEntry>,
    runtime_adoption_state: Option<HostRuntimeAdoptionState>,
    runtime_adoption_history_count: u32,
    recent_runtime_adoption_history: Vec<HostRuntimeAdoptionHistoryEntry>,
    migration_readiness: String,
    migration_reason: String,
    local_url: String,
    supervisor_path: String,
    supervisor_status: Value,
}

#[derive(Debug, Serialize)]
struct PreflightCheck {
    name: String,
    status: String,
    detail: Option<String>,
    fix_applied: bool,
}

#[derive(Debug, Serialize)]
struct PreflightResult {
    ok: bool,
    fix_applied: bool,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
    checks: Vec<PreflightCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AudioDependencyState {
    preferred_audio_driver: String,
    status: String,
    reason: String,
    windows_build_number: Option<u32>,
    package_root: Option<String>,
    package_inf_path: Option<String>,
    package_source: Option<String>,
    control_root: Option<String>,
    control_source: Option<String>,
    download_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostHealthGrade {
    Healthy,
    Recovering,
    Degraded,
    Failed,
}

impl HostHealthGrade {
    fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Recovering => "recovering",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct FirewallRuleSpec {
    name: String,
    program: Option<PathBuf>,
    protocol: &'static str,
    local_port: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct HostLicenseAssignmentState {
    application_activation_id: String,
    application_type: String,
    host_http_port: i32,
    host_stream_udp_start: i32,
    host_stream_udp_end: i32,
}

#[derive(Debug, Serialize)]
struct HygieneRemediationResult {
    changed: bool,
    config_changed: bool,
    changed_paths: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReleaseSnapshotResult {
    snapshot_id: String,
    created_at_unix_ms: u64,
    snapshot_root: String,
    file_count: u32,
}

#[derive(Debug, Serialize)]
struct ConfigStateBackupResult {
    backup_id: String,
    created_at_unix_ms: u64,
    backup_root: String,
    file_count: u32,
}

#[derive(Debug, Serialize)]
struct ConfigStateRestoreResult {
    backup_id: String,
    restored_at_unix_ms: u64,
    backup_root: String,
    verification_reason: String,
}

#[derive(Debug, Serialize)]
struct SupportBundleResult {
    support_bundle_id: String,
    created_at_unix_ms: u64,
    support_bundle_root: String,
    file_count: u32,
}

#[derive(Debug, Serialize)]
struct ReleaseUpgradePreparationResult {
    migration_readiness: String,
    migration_reason: String,
    snapshot: Option<ReleaseSnapshotResult>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostCapabilityRefreshResult {
    ok: bool,
    changed: bool,
    restored: bool,
    skipped: bool,
    reason: String,
    profile_path: Option<String>,
    config_path: Option<String>,
    selected_encoder: Option<String>,
    selected_capture: Option<String>,
    selected_runtime: Option<String>,
}

#[derive(Debug, Serialize)]
struct RuntimeRecommendationAdoptionResult {
    previous_runtime_key: String,
    previous_runtime_display_name: Option<String>,
    adopted_runtime_key: String,
    adopted_runtime_directory: String,
    adopted_runtime_display_name: Option<String>,
    adopted_runtime_version: Option<String>,
    switch_required: bool,
    changed: bool,
    alternate_ready_runtime_count: u32,
    recommendation_reason: String,
    verification_reason: String,
}

#[derive(Debug, Serialize)]
struct ReleasePromotionPreparationResult {
    current_release_id: Option<String>,
    current_environment: Option<String>,
    target_environment: Option<String>,
    promotion_policy_name: String,
    promotion_ring_order: Vec<String>,
    promotion_bundle_name: String,
    promotion_group: String,
    promotion_readiness: String,
    promotion_reason: String,
    required_ready_streak_ms: Option<u64>,
    current_ready_streak_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ReleasePromotionApplyResult {
    previous_release_id: Option<String>,
    promoted_release_id: String,
    previous_environment: Option<String>,
    target_environment: String,
    promotion_policy_name: String,
    promotion_bundle_name: String,
    promotion_group: String,
    release_info: HostReleaseInfo,
    release_upgrade_state: HostReleaseUpgradeState,
}

#[derive(Debug, Serialize)]
struct ReleaseUpgradeApplyResult {
    migration_readiness: String,
    migration_reason: String,
    snapshot: Option<ReleaseSnapshotResult>,
    release_upgrade_state: HostReleaseUpgradeState,
    post_apply_verification_status: String,
    post_apply_verification_reason: String,
    auto_rollback_performed: bool,
}

#[derive(Debug, Serialize)]
struct ReleaseMetadataPromotionResult {
    release_info: HostReleaseInfo,
    current_release_id: String,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseGateInputSummary {
    gate_profile: Option<String>,
    gate_scenario: Option<String>,
    duration_ms: Option<u64>,
    route_lost_count: Option<u32>,
    reconnect_count: Option<u32>,
    stall_recoveries: Option<u32>,
    gameplay_degrade_count: Option<u32>,
    frame_advance_failures: Option<u32>,
    effective_presented_fps: Option<f64>,
    avg_streamer_output_fps: Option<f64>,
    min_streamer_output_fps: Option<f64>,
    avg_receiver_fps: Option<f64>,
    min_receiver_fps: Option<f64>,
    max_play_estimate_ms: Option<f64>,
    max_effective_buffer_ms: Option<f64>,
    max_jitter_buffer_delay_ms: Option<f64>,
    max_decode_time_ms: Option<f64>,
    max_processing_delay_ms: Option<f64>,
    frames_dropped_delta: Option<u32>,
    nack_count_delta: Option<u32>,
    freeze_count_delta: Option<u32>,
    final_route_title: Option<String>,
    final_route_note: Option<String>,
    final_receiver_route: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct DiagnosticPackInputSummary {
    gate_profile: Option<String>,
    gate_scenario: Option<String>,
    requested_duration_ms: Option<u64>,
    gate_exit_code: Option<i32>,
    failure_step: Option<String>,
    failure_reason: Option<String>,
    support_bundle_id: Option<String>,
    health_grade_before: Option<String>,
    health_grade_after: Option<String>,
    lifecycle_before: Option<String>,
    lifecycle_after: Option<String>,
    release_gate_status: Option<String>,
    release_gate_reason: Option<String>,
    release_gate_name: Option<String>,
    verify_startup_status: Option<String>,
    verify_startup_reason: Option<String>,
    started_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
    duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct ReleaseSnapshotManifest {
    schema_version: u32,
    snapshot_id: String,
    created_at_unix_ms: u64,
    selected_runtime_key: String,
    release_info: Option<HostReleaseInfo>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct ConfigStateBackupManifest {
    schema_version: u32,
    backup_id: String,
    created_at_unix_ms: u64,
    selected_runtime_key: String,
    current_release_id: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct SupportBundleManifest {
    schema_version: u32,
    support_bundle_id: String,
    created_at_unix_ms: u64,
    current_release_id: Option<String>,
    health_grade: String,
    lifecycle_phase: String,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct ReleaseHistoryDocument {
    schema_version: u32,
    entries: Vec<HostReleaseHistoryEntry>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct ReleaseGateHistoryDocument {
    schema_version: u32,
    entries: Vec<HostReleaseGateSummary>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct DiagnosticPackHistoryDocument {
    schema_version: u32,
    entries: Vec<HostDiagnosticPackSummary>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct RuntimeAdoptionHistoryDocument {
    schema_version: u32,
    entries: Vec<HostRuntimeAdoptionHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct BundlePromotionPolicy {
    schema_version: u32,
    policy_name: String,
    ring_order: Vec<String>,
    bundle_name: String,
    promotion_group: String,
    deployment_environment: Option<String>,
}

const PROMOTION_POLICY_NAME: &str = "progressive-rings-v1";
const PROMOTION_RING_ORDER: [&str; 4] = ["development", "canary", "staging", "production"];

fn default_promotion_ring_order() -> Vec<String> {
    PROMOTION_RING_ORDER
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

fn default_bundle_promotion_policy(
    paths: &BundlePaths,
    release_info: Option<&HostReleaseInfo>,
) -> BundlePromotionPolicy {
    let bundle_name = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    BundlePromotionPolicy {
        schema_version: 1,
        policy_name: PROMOTION_POLICY_NAME.to_string(),
        ring_order: default_promotion_ring_order(),
        bundle_name,
        promotion_group: "default".to_string(),
        deployment_environment: release_info.and_then(|value| value.deployment_environment.clone()),
    }
}

fn read_bundle_promotion_policy(
    paths: &BundlePaths,
    release_info: Option<&HostReleaseInfo>,
) -> BundlePromotionPolicy {
    let default_policy = default_bundle_promotion_policy(paths, release_info);
    let raw = match fs::read_to_string(&paths.promotion_policy_path) {
        Ok(value) => value,
        Err(_) => return default_policy,
    };

    let Ok(mut parsed) = serde_json::from_str::<BundlePromotionPolicy>(&raw) else {
        return default_policy;
    };

    if parsed.policy_name.trim().is_empty() {
        parsed.policy_name = default_policy.policy_name;
    }
    parsed.ring_order = parsed
        .ring_order
        .into_iter()
        .map(|value| normalize_environment_name(&value))
        .filter(|value| !value.is_empty())
        .collect();
    if parsed.ring_order.is_empty() {
        parsed.ring_order = default_policy.ring_order;
    }
    if parsed.bundle_name.trim().is_empty() {
        parsed.bundle_name = default_policy.bundle_name;
    }
    if parsed.promotion_group.trim().is_empty() {
        parsed.promotion_group = default_policy.promotion_group;
    }
    if parsed.deployment_environment.is_none() {
        parsed.deployment_environment = default_policy.deployment_environment;
    }

    parsed
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let bundle_root = resolve_bundle_root(cli.bundle_root)?;
    let paths = BundlePaths::new(bundle_root)?;

    match cli.command {
        InstallerCommand::PrepareHost => {
            prepare_install_environment(&paths)?;
            apply_license_assignment_to_runtime_config(&paths)?;
            let start_bundle_result = run_supervisor_command(&paths, "start-bundle");
            match verify_startup(&paths) {
                Ok(_) => {
                    if let Err(err) = start_bundle_result {
                        eprintln!(
                            "[WARN] start-bundle returned early failure, but host recovered and passed startup verification: {err:#}"
                        );
                    }
                }
                Err(verify_err) => {
                    if let Err(start_err) = start_bundle_result {
                        bail!(
                            "prepare-host failed: start-bundle returned {start_err:#}; startup verification also failed: {verify_err:#}"
                        );
                    }
                    return Err(verify_err);
                }
            }
            Ok(())
        }
        InstallerCommand::PrepareInstall => {
            prepare_install_environment(&paths)?;
            Ok(())
        }
        InstallerCommand::PreflightHost { fix } => {
            let result = preflight_host(&paths, fix)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::RefreshHostCapability => {
            let result = refresh_host_capability(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::PrepareReleaseUpgrade => {
            let result = prepare_release_upgrade(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::AdoptRecommendedRuntime => {
            let result = adopt_recommended_runtime(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::BackupConfigState => {
            let result = backup_config_state(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::RestoreLatestConfigState => {
            let result = restore_latest_config_state(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::CollectSupportBundle => {
            let result = collect_support_bundle(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::PrepareReleasePromotion { target_environment } => {
            let result = prepare_release_promotion(&paths, target_environment)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::ApplyReleasePromotion { target_environment } => {
            let result = apply_release_promotion(&paths, target_environment)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::ApplyReleaseUpgrade => {
            let result = apply_release_upgrade(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::PromoteReleaseMetadata {
            deployment_environment,
            release_channel,
            source_branch,
            source_commit,
            source_commit_short,
            source_dirty,
            build_profile,
            built_at_unix_ms,
        } => {
            let result = promote_release_metadata(
                &paths,
                &deployment_environment,
                &release_channel,
                &source_branch,
                &source_commit,
                &source_commit_short,
                source_dirty,
                &build_profile,
                built_at_unix_ms,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::ConfigureFirewall => {
            for warning in configure_firewall(&paths)? {
                eprintln!("{warning}");
            }
            Ok(())
        }
        InstallerCommand::ConfigureQos => {
            for warning in configure_qos(&paths)? {
                eprintln!("{warning}");
            }
            Ok(())
        }
        InstallerCommand::RemoveQos => {
            remove_qos(&paths)?;
            Ok(())
        }
        InstallerCommand::RemediateConfigHygiene => {
            let result = remediate_config_hygiene(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::CaptureReleaseSnapshot => {
            let result = capture_release_snapshot(&paths)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::RecordReleaseGate {
            gate_name,
            gate_profile,
            gate_scenario,
            status,
            summary_path,
            reason,
        } => {
            let result = record_release_gate(
                &paths,
                &gate_name,
                gate_profile,
                gate_scenario,
                &status,
                summary_path,
                reason,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::RecordDiagnosticPack {
            pack_name,
            status,
            summary_path,
            reason,
        } => {
            let result = record_diagnostic_pack(&paths, &pack_name, &status, summary_path, reason)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        InstallerCommand::RollbackLatestReleaseSnapshot => rollback_latest_release_snapshot(&paths),
        InstallerCommand::InstallService => install_service(&paths),
        InstallerCommand::UninstallService => uninstall_service(&paths),
        InstallerCommand::StartService => service_control(&paths, "start"),
        InstallerCommand::StopService => service_control(&paths, "stop"),
        InstallerCommand::ServiceStatus => {
            println!("{}", query_service_status(&paths)?);
            Ok(())
        }
        InstallerCommand::StartBundle => {
            apply_license_assignment_to_runtime_config(&paths)?;
            run_supervisor_command(&paths, "start-bundle")
        }
        InstallerCommand::StopBundle => run_supervisor_command(&paths, "stop-bundle"),
        InstallerCommand::RestartRuntime => run_supervisor_command(&paths, "restart-runtime"),
        InstallerCommand::VerifyStartup => verify_startup(&paths),
        InstallerCommand::Status => {
            let status = build_installer_status(&paths)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
    }
}

fn prepare_install_environment(paths: &BundlePaths) -> Result<()> {
    let display_dependency = ensure_display_dependency(paths)?;
    eprintln!(
        "Display dependency: {} ({})",
        display_dependency.status, display_dependency.reason
    );
    let audio_dependency = ensure_audio_dependency(paths)?;
    eprintln!(
        "Audio dependency: {} ({})",
        audio_dependency.status, audio_dependency.reason
    );
    refresh_host_capability(paths)?;
    if let Some(profile) = read_host_capability_profile(paths)? {
        if let Some(summary) = format_audio_prepare_summary(&profile) {
            eprintln!("{summary}");
        }
    }
    let remediation = remediate_config_hygiene(paths)?;
    for note in remediation.notes {
        eprintln!("{note}");
    }
    let warnings = configure_firewall(paths)?;
    for warning in warnings {
        eprintln!("{warning}");
    }
    let warnings = configure_qos(paths)?;
    for warning in warnings {
        eprintln!("{warning}");
    }
    ensure_user_agent_task(paths)?;
    Ok(())
}

#[derive(Debug, Default)]
struct DisplayDependencyState {
    status: String,
    reason: String,
}

impl BundlePaths {
    fn new(bundle_root: PathBuf) -> Result<Self> {
        let moonlight_root = bundle_root.join("moonlight");
        let server_root = moonlight_root.join("server");
        let config_path = server_root.join("config.json");
        if !config_path.exists() {
            bail!("missing config.json at {}", config_path.display());
        }

        Ok(Self {
            host_installer_path: bundle_root.join("host-installer.exe"),
            static_root: moonlight_root.join("static"),
            data_path: server_root.join("data.json"),
            release_info_path: server_root.join("release_info.json"),
            promotion_policy_path: server_root.join("promotion_policy.json"),
            release_gate_summary_path: server_root.join("release_gate_summary.json"),
            release_gate_history_path: server_root.join("release_gate_history.json"),
            diagnostic_pack_summary_path: server_root.join("diagnostic_pack_summary.json"),
            diagnostic_pack_history_path: server_root.join("diagnostic_pack_history.json"),
            release_upgrade_state_path: server_root.join("release_upgrade_state.json"),
            release_history_path: server_root.join("release_history.json"),
            runtime_adoption_state_path: server_root.join("runtime_adoption_state.json"),
            runtime_adoption_history_path: server_root.join("runtime_adoption_history.json"),
            server_root: server_root.clone(),
            host_capability_profile_path: server_root.join("host_capability_profile.json"),
            audio_dependency_state_path: server_root.join("audio_dependency_state.json"),
            driver_cache_root: server_root.join("driver_cache"),
            config_state_backups_root: server_root.join("config_state_backups"),
            support_bundles_root: server_root.join("support_bundles"),
            hard_reset_mode_path: server_root.join("hard_reset_mode.txt"),
            release_snapshots_root: server_root.join("release_snapshots"),
            selected_runtime_path: server_root.join("selected_sunshine_runtime.txt"),
            supervisor_path: resolve_supervisor_path(&moonlight_root),
            web_server_path: moonlight_root.join("web-server.exe"),
            streamer_path: moonlight_root.join("streamer.exe"),
            bundle_root,
            config_path,
        })
    }
}

fn resolve_bundle_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(bundle_root) = explicit {
        return canonicalize_without_verbatim_prefix(bundle_root);
    }

    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve installer directory"))?;

    let bundle_root = if exe_dir.join("moonlight").exists() {
        exe_dir.to_path_buf()
    } else if exe_dir
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("moonlight"))
    {
        exe_dir
            .parent()
            .ok_or_else(|| anyhow!("failed to resolve bundle root from moonlight dir"))?
            .to_path_buf()
    } else {
        bail!(
            "failed to infer bundle root from {}; pass --bundle-root explicitly",
            current_exe.display()
        );
    };

    canonicalize_without_verbatim_prefix(bundle_root)
}

fn build_installer_status(paths: &BundlePaths) -> Result<InstallerStatus> {
    let now = now_unix_ms();
    let config = load_config(paths)?;
    let supervisor_status = run_supervisor_status(paths)?;
    let selected_runtime_key = read_selected_runtime_key(paths);
    let required_processes_ready = has_required_processes(&supervisor_status)?;
    let local_http_ready = local_http_ready(&config)?;
    let (config_hygiene_grade, config_hygiene_warnings) =
        derive_config_hygiene_status(paths, &config)?;
    let host_capability_profile = read_host_capability_profile(paths)?;
    let audio_dependency_state = read_audio_dependency_state(paths)?;
    let snapshot_manifests = read_release_snapshot_manifests(paths)?;
    let last_snapshot = snapshot_manifests.first();
    let config_state_backup_manifests = read_config_state_backup_manifests(paths)?;
    let last_config_state_backup = config_state_backup_manifests.first();
    let support_bundle_manifests = read_support_bundle_manifests(paths)?;
    let last_support_bundle = support_bundle_manifests.first();
    let release_info = read_release_info(paths)?;
    let promotion_policy = read_bundle_promotion_policy(paths, release_info.as_ref());
    let release_gate_summary = read_release_gate_summary(paths)?;
    let recent_release_gate_history = read_release_gate_history(paths)?;
    let diagnostic_pack_summary = read_diagnostic_pack_summary(paths)?;
    let recent_diagnostic_pack_history = read_diagnostic_pack_history(paths)?;
    let release_upgrade_state = read_release_upgrade_state(paths)?;
    let recent_release_history = read_release_history(paths)?;
    let runtime_adoption_state = read_runtime_adoption_state(paths)?;
    let recent_runtime_adoption_history = read_runtime_adoption_history(paths)?;
    let current_release_id = release_info.as_ref().map(derive_release_id);
    let lifecycle_phase = supervisor_status
        .get("state")
        .and_then(|state| state.get("lifecycle_phase"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let lifecycle_reason = supervisor_status
        .get("state")
        .and_then(|state| state.get("lifecycle_reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let lifecycle_updated_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("lifecycle_updated_at_unix_ms"))
        .and_then(Value::as_u64);
    let failure_recovery_attempt_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("failure_recovery_attempt_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let failure_recovery_window_started_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("failure_recovery_window_started_at_unix_ms"))
        .and_then(Value::as_u64);
    let last_failure_recovery_reason = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_failure_recovery_reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let last_failure_recovery_strategy = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_failure_recovery_strategy"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let last_failure_recovery_escalated = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_failure_recovery_escalated"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let total_failure_recovery_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("total_failure_recovery_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let total_failure_recovery_escalation_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("total_failure_recovery_escalation_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let total_service_watchdog_trigger_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("total_service_watchdog_trigger_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let daemon_started_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("daemon_started_at_unix_ms"))
        .and_then(Value::as_u64);
    let boot_failure_recovery_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("boot_failure_recovery_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let boot_service_watchdog_trigger_count = supervisor_status
        .get("state")
        .and_then(|state| state.get("boot_service_watchdog_trigger_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let ready_since_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("ready_since_unix_ms"))
        .and_then(Value::as_u64);
    let last_failure_recovery_completed_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_failure_recovery_completed_at_unix_ms"))
        .and_then(Value::as_u64);
    let last_failure_recovery_budget_cleared_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_failure_recovery_budget_cleared_at_unix_ms"))
        .and_then(Value::as_u64);
    let last_service_watchdog_reason = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_service_watchdog_reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let last_service_watchdog_at_unix_ms = supervisor_status
        .get("state")
        .and_then(|state| state.get("last_service_watchdog_at_unix_ms"))
        .and_then(Value::as_u64);
    let recent_incidents = supervisor_status
        .get("state")
        .and_then(|state| state.get("recent_incidents"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(InstallerIncidentRecord {
                        kind: item.get("kind")?.as_str()?.to_string(),
                        reason: item.get("reason")?.as_str()?.to_string(),
                        strategy: item
                            .get("strategy")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        escalated: item
                            .get("escalated")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        at_unix_ms: item.get("at_unix_ms")?.as_u64()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let current_ready_streak_ms = if lifecycle_phase.eq_ignore_ascii_case("ready") {
        ready_since_unix_ms.map(|value| now.saturating_sub(value))
    } else {
        None
    };
    let daemon_uptime_ms = daemon_started_at_unix_ms.map(|value| now.saturating_sub(value));
    let last_meaningful_incident = recent_incidents
        .iter()
        .find(|incident| incident.kind != "recovery_budget_cleared");
    let last_incident_kind = last_meaningful_incident.map(|incident| incident.kind.clone());
    let last_incident_at_unix_ms = last_meaningful_incident.map(|incident| incident.at_unix_ms);
    let (health_grade, health_reason) = derive_host_health_grade(
        &lifecycle_phase,
        required_processes_ready,
        local_http_ready,
        failure_recovery_attempt_count,
        last_failure_recovery_escalated,
    );
    let (release_gate_status, release_gate_reason) =
        derive_release_gate_status(release_info.as_ref(), release_gate_summary.as_ref());
    let (diagnostic_pack_status, diagnostic_pack_reason) =
        derive_diagnostic_pack_status(release_info.as_ref(), diagnostic_pack_summary.as_ref());
    let (universal_bundle_grade, universal_bundle_reason) =
        derive_universal_bundle_status(host_capability_profile.as_ref(), &selected_runtime_key);
    let (audio_routing_status, audio_routing_reason) =
        derive_audio_routing_status(host_capability_profile.as_ref());
    let (
        recommended_runtime_capability,
        recommended_runtime_reason,
        recommended_runtime_switch_required,
        alternate_ready_runtime_count,
    ) = derive_runtime_recommendation(host_capability_profile.as_ref(), &selected_runtime_key);
    let selected_runtime_capability = host_capability_profile
        .as_ref()
        .and_then(find_selected_runtime_candidate);
    let promotion_policy_name = promotion_policy.policy_name.clone();
    let promotion_ring_order = promotion_policy.ring_order.clone();
    let promotion_bundle_name = promotion_policy.bundle_name.clone();
    let promotion_group = promotion_policy.promotion_group.clone();
    let (promotion_stage, promotion_reason, promotion_target_environment) =
        derive_release_promotion_status(
            release_info.as_ref(),
            &release_gate_status,
            &recent_release_history,
        );
    let (
        next_promotion_target_environment,
        next_promotion_readiness,
        next_promotion_reason,
        next_promotion_required_ready_streak_ms,
        next_promotion_current_ready_streak_ms,
    ) = derive_next_promotion_status(
        release_info.as_ref(),
        &promotion_ring_order,
        health_grade,
        &config_hygiene_grade,
        &release_gate_status,
        current_ready_streak_ms,
        failure_recovery_attempt_count,
    );
    let (migration_readiness, migration_reason) = derive_migration_readiness(
        release_info.as_ref(),
        health_grade,
        &universal_bundle_grade,
        &universal_bundle_reason,
        &config_hygiene_grade,
        !snapshot_manifests.is_empty(),
        &release_gate_status,
        &release_gate_reason,
    );
    let fallback_audio_installer = find_fallback_audio_installer(paths);
    let audio_fallback_installer_ready = fallback_audio_installer.is_some();
    let audio_fallback_installer_path =
        fallback_audio_installer.map(|path| path.display().to_string());
    Ok(InstallerStatus {
        bundle_root: paths.bundle_root.display().to_string(),
        selected_runtime_key,
        universal_bundle_grade,
        universal_bundle_reason,
        capability_probe_mode: host_capability_profile
            .as_ref()
            .map(|profile| profile.probe_mode.clone()),
        capability_updated_at: host_capability_profile
            .as_ref()
            .map(|profile| profile.updated_at.clone()),
        capability_selection_reason: host_capability_profile
            .as_ref()
            .map(|profile| profile.selection_reason.clone()),
        selected_runtime_display_name: host_capability_profile
            .as_ref()
            .and_then(|profile| profile.selected_runtime_display_name.clone()),
        selected_runtime_version: host_capability_profile
            .as_ref()
            .and_then(|profile| profile.selected_runtime_version.clone()),
        recommended_runtime_key: recommended_runtime_capability
            .map(|candidate| candidate.key.clone()),
        recommended_runtime_display_name: recommended_runtime_capability
            .and_then(|candidate| candidate.display_name.clone()),
        recommended_runtime_version: recommended_runtime_capability
            .and_then(|candidate| candidate.runtime_version.clone()),
        recommended_runtime_reason,
        recommended_runtime_switch_required,
        alternate_ready_runtime_count,
        selected_encoder: host_capability_profile
            .as_ref()
            .map(|profile| profile.selected_encoder.clone()),
        selected_capture: host_capability_profile
            .as_ref()
            .map(|profile| profile.selected_capture.clone()),
        selected_capture_reason: host_capability_profile
            .as_ref()
            .and_then(|profile| profile.selected_capture_reason.clone()),
        preferred_audio_driver: audio_dependency_state
            .as_ref()
            .map(|state| state.preferred_audio_driver.clone())
            .unwrap_or_else(|| "virtual-audio-driver-preferred".to_string()),
        audio_dependency_status: audio_dependency_state
            .as_ref()
            .map(|state| state.status.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        audio_dependency_reason: audio_dependency_state
            .as_ref()
            .map(|state| state.reason.clone())
            .unwrap_or_else(|| "audio dependency has not been prepared yet".to_string()),
        audio_dependency_package_source: audio_dependency_state
            .as_ref()
            .and_then(|state| state.package_source.clone()),
        audio_dependency_package_inf_path: audio_dependency_state
            .as_ref()
            .and_then(|state| state.package_inf_path.clone()),
        audio_fallback_installer_ready,
        audio_fallback_installer_path,
        audio_endpoint_count: host_capability_profile
            .as_ref()
            .map(|profile| profile.audio_endpoints.len() as u32)
            .unwrap_or(0),
        selected_audio_sink_name: host_capability_profile.as_ref().and_then(|profile| {
            profile
                .selected_audio_sink_name
                .as_deref()
                .map(display_audio_endpoint_name)
        }),
        selected_virtual_sink_name: host_capability_profile.as_ref().and_then(|profile| {
            profile
                .selected_virtual_sink_name
                .as_deref()
                .map(display_audio_endpoint_name)
        }),
        selected_microphone_name: host_capability_profile.as_ref().and_then(|profile| {
            profile
                .selected_microphone_name
                .as_deref()
                .map(display_audio_endpoint_name)
        }),
        audio_selection_reason: host_capability_profile
            .as_ref()
            .and_then(|profile| profile.audio_selection_reason.clone()),
        audio_routing_status,
        audio_routing_reason,
        selected_ffmpeg_source: host_capability_profile
            .as_ref()
            .and_then(|profile| profile.selected_ffmpeg_source.clone()),
        selected_runtime_startup_validation_status: selected_runtime_capability
            .and_then(|candidate| candidate.startup_validation_status.clone()),
        selected_runtime_startup_validation_reason: selected_runtime_capability
            .and_then(|candidate| candidate.startup_validation_reason.clone()),
        service_name: default_service_name(paths),
        user_agent_task_name: default_user_agent_task_name(paths),
        user_agent_task_status: query_user_agent_task_status(paths)?,
        health_grade: health_grade.as_str().to_string(),
        health_reason,
        config_hygiene_grade,
        config_hygiene_warnings,
        required_processes_ready,
        local_http_ready,
        lifecycle_phase,
        lifecycle_reason,
        lifecycle_updated_at_unix_ms,
        failure_recovery_attempt_count,
        failure_recovery_window_started_at_unix_ms,
        last_failure_recovery_reason,
        last_failure_recovery_strategy,
        last_failure_recovery_escalated,
        total_failure_recovery_count,
        total_failure_recovery_escalation_count,
        total_service_watchdog_trigger_count,
        daemon_started_at_unix_ms,
        boot_failure_recovery_count,
        boot_service_watchdog_trigger_count,
        ready_since_unix_ms,
        current_ready_streak_ms,
        daemon_uptime_ms,
        last_incident_kind,
        last_incident_at_unix_ms,
        last_failure_recovery_completed_at_unix_ms,
        last_failure_recovery_budget_cleared_at_unix_ms,
        last_service_watchdog_reason,
        last_service_watchdog_at_unix_ms,
        recent_incidents,
        release_info,
        current_release_id,
        promotion_policy_name,
        promotion_ring_order,
        promotion_bundle_name,
        promotion_group,
        promotion_stage,
        promotion_reason,
        promotion_target_environment,
        next_promotion_target_environment,
        next_promotion_readiness,
        next_promotion_reason,
        next_promotion_required_ready_streak_ms,
        next_promotion_current_ready_streak_ms,
        rollback_ready: !snapshot_manifests.is_empty(),
        release_snapshot_count: snapshot_manifests.len() as u32,
        last_release_snapshot_id: last_snapshot.map(|item| item.snapshot_id.clone()),
        last_release_snapshot_at_unix_ms: last_snapshot.map(|item| item.created_at_unix_ms),
        config_state_backup_count: config_state_backup_manifests.len() as u32,
        last_config_state_backup_id: last_config_state_backup.map(|item| item.backup_id.clone()),
        last_config_state_backup_at_unix_ms: last_config_state_backup
            .map(|item| item.created_at_unix_ms),
        support_bundle_count: support_bundle_manifests.len() as u32,
        last_support_bundle_id: last_support_bundle.map(|item| item.support_bundle_id.clone()),
        last_support_bundle_at_unix_ms: last_support_bundle.map(|item| item.created_at_unix_ms),
        release_gate_status,
        release_gate_reason,
        release_gate_summary,
        release_gate_history_count: recent_release_gate_history.len() as u32,
        recent_release_gate_history,
        diagnostic_pack_status,
        diagnostic_pack_reason,
        diagnostic_pack_summary,
        diagnostic_pack_history_count: recent_diagnostic_pack_history.len() as u32,
        recent_diagnostic_pack_history,
        release_upgrade_state,
        recent_release_history,
        runtime_adoption_state,
        runtime_adoption_history_count: recent_runtime_adoption_history.len() as u32,
        recent_runtime_adoption_history,
        migration_readiness,
        migration_reason,
        local_url: build_local_url(&config),
        supervisor_path: paths.supervisor_path.display().to_string(),
        supervisor_status,
    })
}

fn is_elevated() -> bool {
    Command::new("cmd")
        .args(["/c", "net", "session"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn preflight_host(paths: &BundlePaths, fix: bool) -> Result<PreflightResult> {
    let started_at_unix_ms = now_unix_ms();
    let mut checks: Vec<PreflightCheck> = Vec::new();
    let mut fix_applied = false;

    let elevated = is_elevated();
    checks.push(PreflightCheck {
        name: "admin_privileges".into(),
        status: if elevated { "ok" } else { "warn" }.into(),
        detail: Some(
            if elevated {
                "Running elevated."
            } else {
                "Run as Administrator for full fixes."
            }
            .into(),
        ),
        fix_applied: false,
    });

    for (name, path) in [
        ("host_supervisor", &paths.supervisor_path),
        ("web_server", &paths.web_server_path),
        ("streamer", &paths.streamer_path),
        ("host_installer", &paths.host_installer_path),
    ] {
        let exists = path.exists();
        checks.push(PreflightCheck {
            name: format!("binary_{name}"),
            status: if exists { "ok" } else { "fail" }.into(),
            detail: Some(path.display().to_string()),
            fix_applied: false,
        });
    }

    let mut service_status_raw = query_service_status(paths).unwrap_or_else(|err| err.to_string());
    let service_installed = !service_status_raw.to_lowercase().contains("does not exist");
    if !service_installed && fix {
        if install_service(paths).is_ok() {
            fix_applied = true;
            service_status_raw = query_service_status(paths).unwrap_or_else(|err| err.to_string());
        }
    }

    checks.push(PreflightCheck {
        name: "service_installed".into(),
        status: if service_installed { "ok" } else { "fail" }.into(),
        detail: Some(service_status_raw.clone()),
        fix_applied: fix_applied,
    });

    let service_running = service_status_raw.to_uppercase().contains("RUNNING");
    if !service_running && fix {
        if service_control(paths, "start").is_ok() {
            fix_applied = true;
            service_status_raw = query_service_status(paths).unwrap_or_else(|err| err.to_string());
        }
    }

    checks.push(PreflightCheck {
        name: "service_running".into(),
        status: if service_running { "ok" } else { "warn" }.into(),
        detail: Some(service_status_raw.clone()),
        fix_applied: fix_applied,
    });

    if fix {
        let _ = configure_firewall(paths);
        fix_applied = true;
        checks.push(PreflightCheck {
            name: "firewall_rules".into(),
            status: "fixed".into(),
            detail: Some("Firewall rules ensured.".into()),
            fix_applied: true,
        });
        let _ = ensure_user_agent_task(paths);
    } else {
        checks.push(PreflightCheck {
            name: "firewall_rules".into(),
            status: "warn".into(),
            detail: Some("Not checked. Run preflight with --fix to configure firewall.".into()),
            fix_applied: false,
        });
    }

    let status = build_installer_status(paths)?;
    let ready = status.required_processes_ready && status.local_http_ready;
    if !ready && fix {
        let _ = run_supervisor_command(paths, "start-bundle");
        if verify_startup(paths).is_ok() {
            fix_applied = true;
        }
    }

    checks.push(PreflightCheck {
        name: "runtime_ready".into(),
        status: if ready { "ok" } else { "warn" }.into(),
        detail: Some(format!(
            "lifecycle={} local_http_ready={} required_processes_ready={}",
            status.lifecycle_phase, status.local_http_ready, status.required_processes_ready
        )),
        fix_applied: fix_applied,
    });

    let audio_ok = status.audio_dependency_status.eq_ignore_ascii_case("ready");
    checks.push(PreflightCheck {
        name: "audio_dependency".into(),
        status: if audio_ok { "ok" } else { "warn" }.into(),
        detail: Some(format!(
            "{} ({})",
            status.audio_dependency_status, status.audio_dependency_reason
        )),
        fix_applied: false,
    });

    let ok = checks.iter().all(|check| check.status != "fail");
    Ok(PreflightResult {
        ok,
        fix_applied,
        started_at_unix_ms,
        finished_at_unix_ms: now_unix_ms(),
        checks,
    })
}

fn format_audio_prepare_summary(profile: &HostCapabilityProfile) -> Option<String> {
    let sink = profile
        .selected_audio_sink_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())?;
    let virtual_sink = profile
        .selected_virtual_sink_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(sink);
    let microphone = profile
        .selected_microphone_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("none");
    Some(format!(
        "Audio auto-select: speaker/output='{sink}', virtual_sink='{virtual_sink}', mic/input='{microphone}'"
    ))
}

fn derive_audio_routing_status(profile: Option<&HostCapabilityProfile>) -> (String, String) {
    let Some(profile) = profile else {
        return (
            "unknown".to_string(),
            "host capability profile is missing".to_string(),
        );
    };

    let selected_audio_sink = profile
        .selected_audio_sink_name
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let selected_virtual_sink = profile
        .selected_virtual_sink_name
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let selected_microphone = profile
        .selected_microphone_name
        .as_deref()
        .filter(|value| !value.trim().is_empty());

    if let (Some(audio_sink), Some(virtual_sink), Some(microphone)) = (
        selected_audio_sink,
        selected_virtual_sink,
        selected_microphone,
    ) {
        return (
            "ready".to_string(),
            format!(
                "speaker/output={audio_sink}, virtual_sink={virtual_sink}, mic/input={microphone}"
            ),
        );
    }

    if let (Some(audio_sink), Some(virtual_sink)) = (selected_audio_sink, selected_virtual_sink) {
        return (
            "partial".to_string(),
            format!(
                "speaker/output={audio_sink} and virtual_sink={virtual_sink} are ready, but mic/input pairing is missing"
            ),
        );
    }

    if profile.audio_endpoints.is_empty() {
        return (
            "missing".to_string(),
            "no virtual audio endpoint was detected during preflight".to_string(),
        );
    }

    (
        "missing".to_string(),
        profile.audio_selection_reason.clone().unwrap_or_else(|| {
            "virtual audio endpoints were detected, but no valid input/output routing was selected"
                .to_string()
        }),
    )
}

fn derive_host_health_grade(
    lifecycle_phase: &str,
    required_processes_ready: bool,
    local_http_ready: bool,
    failure_recovery_attempt_count: u32,
    last_failure_recovery_escalated: bool,
) -> (HostHealthGrade, String) {
    if lifecycle_phase.eq_ignore_ascii_case("failed") {
        return (
            HostHealthGrade::Failed,
            "lifecycle phase is failed".to_string(),
        );
    }

    if !required_processes_ready || !local_http_ready {
        if lifecycle_phase.eq_ignore_ascii_case("starting")
            || lifecycle_phase.eq_ignore_ascii_case("recovering")
            || lifecycle_phase.eq_ignore_ascii_case("stopping")
        {
            return (
                HostHealthGrade::Recovering,
                "bundle is still transitioning to ready".to_string(),
            );
        }

        return (
            HostHealthGrade::Degraded,
            "required processes or local web health are not ready".to_string(),
        );
    }

    if lifecycle_phase.eq_ignore_ascii_case("starting")
        || lifecycle_phase.eq_ignore_ascii_case("recovering")
        || lifecycle_phase.eq_ignore_ascii_case("stopping")
    {
        return (
            HostHealthGrade::Recovering,
            format!("lifecycle phase is {lifecycle_phase}"),
        );
    }

    if last_failure_recovery_escalated {
        return (
            HostHealthGrade::Degraded,
            "failure recovery has escalated recently".to_string(),
        );
    }

    if failure_recovery_attempt_count > 0 {
        return (
            HostHealthGrade::Degraded,
            format!(
                "failure recovery budget is active ({failure_recovery_attempt_count} attempt(s))"
            ),
        );
    }

    (
        HostHealthGrade::Healthy,
        "bundle is ready and health checks are passing".to_string(),
    )
}

fn derive_migration_readiness(
    release_info: Option<&HostReleaseInfo>,
    health_grade: HostHealthGrade,
    universal_bundle_grade: &str,
    universal_bundle_reason: &str,
    config_hygiene_grade: &str,
    rollback_ready: bool,
    release_gate_status: &str,
    release_gate_reason: &str,
) -> (String, String) {
    let Some(release_info) = release_info else {
        return (
            "blocked".to_string(),
            "release metadata is missing".to_string(),
        );
    };

    if release_info.source_dirty {
        return (
            "blocked".to_string(),
            "release metadata is marked dirty".to_string(),
        );
    }

    if config_hygiene_grade != "clean" {
        return (
            "blocked".to_string(),
            "config hygiene must be clean before upgrade".to_string(),
        );
    }

    if health_grade != HostHealthGrade::Healthy {
        return (
            "blocked".to_string(),
            "host must be healthy before upgrade".to_string(),
        );
    }

    if !universal_bundle_grade.eq_ignore_ascii_case("ready") {
        return (
            "blocked".to_string(),
            format!(
                "universal bundle readiness must be ready before upgrade ({universal_bundle_reason})"
            ),
        );
    }

    if !rollback_ready {
        return (
            "pending_snapshot".to_string(),
            "capture a rollback snapshot before upgrade".to_string(),
        );
    }

    if release_gate_status != "passed" {
        return (
            "blocked".to_string(),
            format!("release gate must pass for this build ({release_gate_reason})"),
        );
    }

    (
        "ready".to_string(),
        "host is healthy, rollback snapshot is available, and release gate passed".to_string(),
    )
}

fn normalize_environment_name(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "dev" => "development".to_string(),
        "prod" => "production".to_string(),
        other => other.to_string(),
    }
}

fn next_promotion_target_for_environment(
    ring_order: &[String],
    environment: &str,
) -> Option<String> {
    let normalized_environment = normalize_environment_name(environment);
    let current_index = ring_order
        .iter()
        .position(|item| item.eq_ignore_ascii_case(&normalized_environment))?;
    ring_order.get(current_index + 1).cloned()
}

fn required_ready_streak_ms_for_environment(environment: &str) -> Option<u64> {
    match normalize_environment_name(environment).as_str() {
        "canary" => Some(5 * 60 * 1000),
        "staging" => Some(10 * 60 * 1000),
        "production" => Some(15 * 60 * 1000),
        _ => None,
    }
}

fn history_matches_release_build(
    entry: &HostReleaseHistoryEntry,
    release_info: &HostReleaseInfo,
) -> bool {
    if let (Some(entry_build_id), Some(release_build_id)) = (
        entry.build_id.as_deref().filter(|value| !value.is_empty()),
        release_info
            .build_id
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) {
        return entry_build_id == release_build_id;
    }

    match (
        entry
            .source_commit_short
            .as_deref()
            .filter(|value| !value.is_empty()),
        release_info
            .source_commit_short
            .as_deref()
            .filter(|value| !value.is_empty()),
        entry
            .release_channel
            .as_deref()
            .filter(|value| !value.is_empty()),
        release_info
            .release_channel
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) {
        (Some(entry_commit), Some(release_commit), Some(entry_channel), Some(release_channel)) => {
            entry_commit == release_commit && entry_channel == release_channel
        }
        (Some(entry_commit), Some(release_commit), _, _) => entry_commit == release_commit,
        _ => false,
    }
}

fn derive_release_promotion_status(
    release_info: Option<&HostReleaseInfo>,
    release_gate_status: &str,
    recent_release_history: &[HostReleaseHistoryEntry],
) -> (String, String, Option<String>) {
    let Some(release_info) = release_info else {
        return (
            "unknown".to_string(),
            "release metadata is missing".to_string(),
            None,
        );
    };

    let environment = release_info
        .deployment_environment
        .as_deref()
        .map(normalize_environment_name)
        .unwrap_or_else(|| "unknown".to_string());
    let current_build_promoted_or_applied = recent_release_history.iter().any(|entry| {
        entry.status.eq_ignore_ascii_case("succeeded")
            && (entry.action.eq_ignore_ascii_case("apply")
                || entry.action.eq_ignore_ascii_case("promote"))
            && history_matches_release_build(entry, release_info)
    });

    if environment.eq_ignore_ascii_case("production") {
        if current_build_promoted_or_applied {
            return (
                "production_applied".to_string(),
                "current production build was applied or promoted successfully".to_string(),
                None,
            );
        }

        if release_gate_status.eq_ignore_ascii_case("passed") {
            return (
                "candidate".to_string(),
                "current production build passed the gate but has not been applied yet".to_string(),
                None,
            );
        }

        return (
            "candidate".to_string(),
            "current production build has not passed the release gate yet".to_string(),
            None,
        );
    }

    if environment.eq_ignore_ascii_case("canary") {
        if release_gate_status.eq_ignore_ascii_case("passed") {
            return (
                "canary_verified".to_string(),
                "canary build passed the release gate and is ready for staging promotion"
                    .to_string(),
                Some("staging".to_string()),
            );
        }

        return (
            "candidate".to_string(),
            "canary build has not passed the release gate yet".to_string(),
            Some("staging".to_string()),
        );
    }

    if environment.eq_ignore_ascii_case("staging") {
        if release_gate_status.eq_ignore_ascii_case("passed") {
            return (
                "staging_verified".to_string(),
                "staging build passed the release gate and is ready for production promotion"
                    .to_string(),
                Some("production".to_string()),
            );
        }

        return (
            "candidate".to_string(),
            "staging build has not passed the release gate yet".to_string(),
            Some("production".to_string()),
        );
    }

    if environment.eq_ignore_ascii_case("development") {
        if release_gate_status.eq_ignore_ascii_case("passed") {
            return (
                "development_verified".to_string(),
                "development build passed the release gate and is ready for canary promotion"
                    .to_string(),
                Some("canary".to_string()),
            );
        }

        return (
            "candidate".to_string(),
            "development build has not passed the release gate yet".to_string(),
            Some("canary".to_string()),
        );
    }

    if release_gate_status.eq_ignore_ascii_case("passed") {
        return (
            "verified".to_string(),
            format!("{environment} build passed the release gate"),
            None,
        );
    }

    (
        "candidate".to_string(),
        format!("{environment} build has not passed the release gate yet"),
        None,
    )
}

fn derive_next_promotion_status(
    release_info: Option<&HostReleaseInfo>,
    promotion_ring_order: &[String],
    health_grade: HostHealthGrade,
    config_hygiene_grade: &str,
    release_gate_status: &str,
    current_ready_streak_ms: Option<u64>,
    failure_recovery_attempt_count: u32,
) -> (Option<String>, String, String, Option<u64>, Option<u64>) {
    let Some(release_info) = release_info else {
        return (
            None,
            "unknown".to_string(),
            "release metadata is missing".to_string(),
            None,
            current_ready_streak_ms,
        );
    };

    let environment = release_info
        .deployment_environment
        .as_deref()
        .map(normalize_environment_name)
        .unwrap_or_else(|| "unknown".to_string());

    let target = next_promotion_target_for_environment(promotion_ring_order, &environment);
    let required_ready_streak_ms = target
        .as_deref()
        .and_then(required_ready_streak_ms_for_environment);

    let Some(target_environment) = target else {
        return (
            None,
            "settled".to_string(),
            if promotion_ring_order
                .iter()
                .any(|item| item.eq_ignore_ascii_case(&environment))
            {
                "current release already sits at the highest promotion environment".to_string()
            } else {
                format!("current environment {environment} is outside the active promotion policy")
            },
            None,
            current_ready_streak_ms,
        );
    };

    if release_info.source_dirty {
        return (
            Some(target_environment),
            "blocked".to_string(),
            "source metadata is dirty; build cannot be promoted".to_string(),
            required_ready_streak_ms,
            current_ready_streak_ms,
        );
    }

    if config_hygiene_grade != "clean" {
        return (
            Some(target_environment),
            "blocked".to_string(),
            "config hygiene must be clean before promotion".to_string(),
            required_ready_streak_ms,
            current_ready_streak_ms,
        );
    }

    if health_grade != HostHealthGrade::Healthy {
        return (
            Some(target_environment),
            "blocked".to_string(),
            "host must be healthy before promotion".to_string(),
            required_ready_streak_ms,
            current_ready_streak_ms,
        );
    }

    if !release_gate_status.eq_ignore_ascii_case("passed") {
        return (
            Some(target_environment),
            "blocked".to_string(),
            "release gate must pass before promotion".to_string(),
            required_ready_streak_ms,
            current_ready_streak_ms,
        );
    }

    if failure_recovery_attempt_count > 0 {
        return (
            Some(target_environment),
            "blocked".to_string(),
            "active recovery budget must clear before promotion".to_string(),
            required_ready_streak_ms,
            current_ready_streak_ms,
        );
    }

    let required_ready_streak_ms = required_ready_streak_ms.unwrap_or(0);
    let current_ready_streak_ms = current_ready_streak_ms.unwrap_or(0);
    if current_ready_streak_ms < required_ready_streak_ms {
        return (
            Some(target_environment),
            "observing".to_string(),
            format!(
                "release gate passed, but host needs a stable ready streak of at least {} before promotion",
                format_duration_ms(required_ready_streak_ms)
            ),
            Some(required_ready_streak_ms),
            Some(current_ready_streak_ms),
        );
    }

    (
        Some(target_environment),
        "ready".to_string(),
        "release passed gate and is ready for next environment promotion".to_string(),
        Some(required_ready_streak_ms),
        Some(current_ready_streak_ms),
    )
}

fn format_duration_ms(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn derive_config_hygiene_status(
    paths: &BundlePaths,
    config: &Config,
) -> Result<(String, Vec<String>)> {
    let mut warnings = Vec::new();

    if !config.web_server.session_cookie_secure {
        warnings.push("session_cookie_secure_disabled".to_string());
    }
    if config.web_server.first_login_create_admin {
        warnings.push("web_first_login_bootstrap_enabled".to_string());
    }

    for file_name in [
        "SUNSHINE_LOGIN.txt",
        "SETUP.txt",
        "README.txt",
        "start-all.bat",
    ] {
        let path = paths.bundle_root.join(file_name);
        if !path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&path).unwrap_or_default();
        let contains_sunshine_password = raw.contains("Password:")
            || raw.contains("Local Sunshine login:")
            || raw.contains("Local Sunshine Login:");
        if contains_sunshine_password {
            match file_name {
                "SUNSHINE_LOGIN.txt" => warnings.push("plaintext_sunshine_login_file".to_string()),
                "SETUP.txt" => warnings.push("plaintext_setup_credentials".to_string()),
                "README.txt" => warnings.push("plaintext_readme_credentials".to_string()),
                "start-all.bat" => warnings.push("plaintext_start_script_credentials".to_string()),
                _ => {}
            }
        }
    }

    let grade = if warnings
        .iter()
        .any(|warning| warning.starts_with("plaintext_"))
    {
        "risk"
    } else if warnings.is_empty() {
        "clean"
    } else {
        "warning"
    };

    Ok((grade.to_string(), warnings))
}

fn load_config(paths: &BundlePaths) -> Result<Config> {
    let raw = fs::read_to_string(&paths.config_path)
        .with_context(|| format!("failed to read {}", paths.config_path.display()))?;
    Ok(serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", paths.config_path.display()))?)
}

fn activation_state_path(paths: &BundlePaths) -> PathBuf {
    paths.server_root.join("host_activation_state.json")
}

fn read_host_license_assignment_state(
    paths: &BundlePaths,
) -> Result<Option<HostLicenseAssignmentState>> {
    let path = activation_state_path(paths);
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostLicenseAssignmentState>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn valid_runtime_port(value: i32) -> Option<u16> {
    if (1..=u16::MAX as i32).contains(&value) {
        Some(value as u16)
    } else {
        None
    }
}

fn apply_license_assignment_to_runtime_config(paths: &BundlePaths) -> Result<bool> {
    let Some(assignment) = read_host_license_assignment_state(paths)? else {
        return Ok(false);
    };
    let has_license_identity = !assignment.application_activation_id.trim().is_empty()
        || assignment
            .application_type
            .trim()
            .eq_ignore_ascii_case("CLOUDGIME_HOST");
    if !has_license_identity {
        return Ok(false);
    }

    let mut config = load_config(paths)?;
    let mut changed = false;
    if let Some(host_http_port) = valid_runtime_port(assignment.host_http_port) {
        let current = config.web_server.bind_address;
        let next = SocketAddr::new(current.ip(), host_http_port);
        if current != next {
            config.web_server.bind_address = next;
            changed = true;
        }
    }

    let udp_start = valid_runtime_port(assignment.host_stream_udp_start);
    let udp_end = valid_runtime_port(assignment.host_stream_udp_end);
    if let (Some(min), Some(max)) = (udp_start, udp_end) {
        if min <= max {
            let needs_update = config
                .webrtc
                .port_range
                .as_ref()
                .map(|range| range.min != min || range.max != max)
                .unwrap_or(true);
            if needs_update {
                config.webrtc.port_range = Some(PortRange { min, max });
                changed = true;
            }
        }
    }

    if changed {
        save_config(paths, &config)?;
    }
    Ok(changed)
}

fn read_release_info(paths: &BundlePaths) -> Result<Option<HostReleaseInfo>> {
    let path = paths.release_info_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostReleaseInfo>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_host_capability_profile(paths: &BundlePaths) -> Result<Option<HostCapabilityProfile>> {
    let path = paths.host_capability_profile_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostCapabilityProfile>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_audio_dependency_state(paths: &BundlePaths) -> Result<Option<AudioDependencyState>> {
    let path = paths.audio_dependency_state_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<AudioDependencyState>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn write_audio_dependency_state(paths: &BundlePaths, state: &AudioDependencyState) -> Result<()> {
    if let Some(parent) = paths.audio_dependency_state_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(
        &paths.audio_dependency_state_path,
        serde_json::to_string_pretty(state)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            paths.audio_dependency_state_path.display()
        )
    })?;
    Ok(())
}

fn is_managed_virtual_audio_sink_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.contains("symo virtual audio output")
        || normalized.contains("symo virtual audio")
        || normalized.contains("virtual audio driver by mtt")
        || normalized.contains("virtual audio driver output")
}

fn is_managed_virtual_audio_route(profile: Option<&HostCapabilityProfile>) -> bool {
    profile
        .and_then(|profile| profile.selected_audio_sink_name.as_deref())
        .is_some_and(is_managed_virtual_audio_sink_name)
}

fn is_audio_route_ready(profile: Option<&HostCapabilityProfile>) -> bool {
    let Some(profile) = profile else {
        return false;
    };

    profile
        .selected_audio_sink_name
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && profile
            .selected_virtual_sink_name
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && profile
            .selected_microphone_name
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn display_audio_endpoint_name(name: &str) -> String {
    let trimmed = name.trim();
    let normalized = trimmed.to_ascii_lowercase();

    if normalized.contains("virtual speakers for audiorelay")
        || normalized.contains("virtual mic for audiorelay")
    {
        return format!("SYMO Audio Driver ({trimmed})");
    }

    trimmed.to_string()
}

fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

fn cleanup_conflicting_virtual_audio_devices(
    profile: Option<&HostCapabilityProfile>,
) -> Result<Option<String>> {
    let Some(profile) = profile else {
        return Ok(None);
    };

    let keep_names = [
        profile.selected_audio_sink_name.as_deref(),
        profile.selected_virtual_sink_name.as_deref(),
        profile.selected_microphone_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();

    if keep_names.is_empty() {
        return Ok(None);
    }

    let cleanup_targets = [
        "Virtual Speakers for AudioRelay",
        "Virtual Mic for AudioRelay",
        "SYMO Virtual Audio Output",
        "SYMO Virtual Audio Input",
        "Virtual Audio Driver by MTT",
        "Virtual Mic Driver by MTT",
        "Virtual Audio Driver Output",
        "Virtual Audio Driver Input",
    ]
    .into_iter()
    .filter(|candidate| {
        !keep_names
            .iter()
            .any(|active| active.eq_ignore_ascii_case(candidate))
    })
    .collect::<Vec<_>>();

    if cleanup_targets.is_empty() {
        return Ok(None);
    }

    let keep_names_ps = keep_names
        .iter()
        .map(|value| format!("'{}'", escape_powershell_single_quoted(value)))
        .collect::<Vec<_>>()
        .join(", ");
    let cleanup_targets_ps = cleanup_targets
        .iter()
        .map(|value| format!("'{}'", escape_powershell_single_quoted(value)))
        .collect::<Vec<_>>()
        .join(", ");

    let script = format!(
        r#"
$keep = @({keep_names_ps})
$targets = @({cleanup_targets_ps})
$removed = New-Object System.Collections.Generic.List[string]
$failed = New-Object System.Collections.Generic.List[string]
$devices = @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {{ $_.FriendlyName -ne $null -and $_.InstanceId -ne $null }})

foreach ($target in $targets) {{
    $matches = @($devices | Where-Object {{
        $friendly = [string]$_.FriendlyName
        $friendly.Equals($target, [System.StringComparison]::OrdinalIgnoreCase) -or
        $friendly.IndexOf($target, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
    }})

    foreach ($device in $matches) {{
        $friendly = [string]$device.FriendlyName
        $instanceId = [string]$device.InstanceId
        if ([string]::IsNullOrWhiteSpace($instanceId)) {{
            continue
        }}
        if ($keep -contains $friendly) {{
            continue
        }}

        & pnputil.exe /remove-device "$instanceId" | Out-Null
        if ($LASTEXITCODE -eq 0) {{
            $removed.Add("$friendly [$instanceId]")
        }}
        else {{
            $failed.Add("$friendly [$instanceId]")
        }}
    }}
}}

$driverEntries = @(Get-CimInstance Win32_PnPSignedDriver -ErrorAction SilentlyContinue | Where-Object {{ $_.DeviceName -ne $null -and $_.InfName -ne $null }})
$deletedInfNames = New-Object System.Collections.Generic.HashSet[string]([System.StringComparer]::OrdinalIgnoreCase)

foreach ($target in $targets) {{
    $driverMatches = @($driverEntries | Where-Object {{
        $deviceName = [string]$_.DeviceName
        $deviceName.Equals($target, [System.StringComparison]::OrdinalIgnoreCase) -or
        $deviceName.IndexOf($target, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
    }})

    foreach ($driver in $driverMatches) {{
        $deviceName = [string]$driver.DeviceName
        if ($keep -contains $deviceName) {{
            continue
        }}

        $infName = [string]$driver.InfName
        if ([string]::IsNullOrWhiteSpace($infName)) {{
            continue
        }}
        if (-not $deletedInfNames.Add($infName)) {{
            continue
        }}

        & pnputil.exe /delete-driver "$infName" /uninstall /force | Out-Null
        if ($LASTEXITCODE -eq 0) {{
            $removed.Add("driver:$deviceName [$infName]")
        }}
        else {{
            $failed.Add("driver:$deviceName [$infName]")
        }}
    }}
}}

[pscustomobject]@{{
    removed = @($removed)
    failed = @($failed)
}} | ConvertTo-Json -Compress
"#
    );

    let (ok, output) = run_command_capture(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]),
        "virtual audio cleanup",
    )?;

    if !ok {
        bail!(
            "failed to clean conflicting virtual audio devices: {}",
            output
        );
    }

    let parsed = serde_json::from_str::<Value>(output.trim())
        .context("failed to parse virtual audio cleanup result")?;
    let removed = parsed
        .get("removed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let failed = parsed
        .get("failed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();

    if removed.is_empty() && failed.is_empty() {
        return Ok(None);
    }

    let mut parts = Vec::new();
    if !removed.is_empty() {
        parts.push(format!(
            "cleaned conflicting virtual audio devices: {}",
            removed.join(", ")
        ));
    }
    if !failed.is_empty() {
        parts.push(format!(
            "some conflicting devices could not be removed: {}",
            failed.join(", ")
        ));
    }
    Ok(Some(parts.join(" ; ")))
}

fn enrich_audio_reason_with_cleanup(
    paths: &BundlePaths,
    profile: Option<HostCapabilityProfile>,
    base_reason: String,
) -> Result<(Option<HostCapabilityProfile>, String)> {
    let cleanup_note = cleanup_conflicting_virtual_audio_devices(profile.as_ref())?;
    let refreshed_profile = if cleanup_note.is_some() {
        refresh_host_capability(paths)?;
        read_host_capability_profile(paths)?
    } else {
        profile
    };
    let reason = if let Some(note) = cleanup_note {
        format!("{base_reason}; {note}")
    } else {
        base_reason
    };
    Ok((refreshed_profile, reason))
}

fn summarize_audio_route(profile: Option<&HostCapabilityProfile>) -> String {
    let Some(profile) = profile else {
        return "no host capability profile is available yet".to_string();
    };

    match (
        profile.selected_audio_sink_name.as_deref(),
        profile.selected_virtual_sink_name.as_deref(),
        profile.selected_microphone_name.as_deref(),
    ) {
        (Some(sink), Some(virtual_sink), Some(microphone)) => format!(
            "speaker/output={}, virtual_sink={}, mic/input={}",
            display_audio_endpoint_name(sink),
            display_audio_endpoint_name(virtual_sink),
            display_audio_endpoint_name(microphone)
        ),
        (Some(sink), Some(virtual_sink), None) => {
            format!(
                "speaker/output={}, virtual_sink={}, mic/input=missing",
                display_audio_endpoint_name(sink),
                display_audio_endpoint_name(virtual_sink)
            )
        }
        _ => "no valid virtual audio route was selected".to_string(),
    }
}

fn has_virtual_display_driver(profile: Option<&HostCapabilityProfile>) -> bool {
    let Some(profile) = profile else {
        return false;
    };

    if profile
        .selected_capture_reason
        .as_deref()
        .is_some_and(|reason| reason.eq_ignore_ascii_case("virtual_display_driver_present"))
    {
        return true;
    }

    profile.gpu_controllers.iter().any(|gpu| {
        gpu.name
            .to_ascii_lowercase()
            .contains("virtual display driver")
    })
}

fn summarize_display_route(profile: Option<&HostCapabilityProfile>) -> String {
    let Some(profile) = profile else {
        return "no host capability profile is available yet".to_string();
    };

    if has_virtual_display_driver(Some(profile)) {
        let capture = profile.selected_capture.trim();
        let reason = profile
            .selected_capture_reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("virtual_display_driver_present");
        return format!("capture={} ({reason})", capture);
    }

    format!(
        "capture={} ({})",
        profile.selected_capture.trim(),
        profile
            .selected_capture_reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("virtual display driver not detected")
    )
}

fn find_offline_virtual_display_driver_inf(paths: &BundlePaths) -> Option<PathBuf> {
    let candidates = [
        paths
            .server_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("MttVDD.inf"),
        paths
            .server_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("MttVDD")
            .join("MttVDD.inf"),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("MttVDD.inf"),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("MttVDD")
            .join("MttVDD.inf"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn find_offline_virtual_display_settings(paths: &BundlePaths) -> Option<PathBuf> {
    let candidates = [
        paths
            .server_root
            .join("drivers")
            .join("virtual-display-driver")
            .join(VIRTUAL_DISPLAY_SETTINGS_FILE_NAME),
        paths
            .server_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("config")
            .join(VIRTUAL_DISPLAY_SETTINGS_FILE_NAME),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-display-driver")
            .join(VIRTUAL_DISPLAY_SETTINGS_FILE_NAME),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("config")
            .join(VIRTUAL_DISPLAY_SETTINGS_FILE_NAME),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn find_offline_vdd_control_tool(paths: &BundlePaths) -> Option<PathBuf> {
    let candidates = [
        paths
            .server_root
            .join("drivers")
            .join("vdd-control")
            .join("x64")
            .join("nefconw.exe"),
        paths
            .bundle_root
            .join("drivers")
            .join("vdd-control")
            .join("x64")
            .join("nefconw.exe"),
        paths
            .server_root
            .join("drivers")
            .join("vdd-control")
            .join("Dependencies")
            .join("devcon.exe"),
        paths
            .bundle_root
            .join("drivers")
            .join("vdd-control")
            .join("Dependencies")
            .join("devcon.exe"),
        paths
            .server_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("Dependencies")
            .join("devcon.exe"),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-display-driver")
            .join("Dependencies")
            .join("devcon.exe"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn install_bundled_virtual_display_settings(settings_source: &PathBuf) -> Result<()> {
    let target_dir = PathBuf::from(VIRTUAL_DISPLAY_SETTINGS_TARGET_DIR);
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;
    let target_path = target_dir.join(VIRTUAL_DISPLAY_SETTINGS_FILE_NAME);
    fs::copy(settings_source, &target_path).with_context(|| {
        format!(
            "failed to copy virtual display settings {} -> {}",
            settings_source.display(),
            target_path.display()
        )
    })?;
    Ok(())
}

fn install_offline_virtual_display_driver(
    inf_path: &PathBuf,
    control_tool_path: &PathBuf,
    settings_source: Option<&PathBuf>,
) -> Result<()> {
    let inf_parent = inf_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let (ok, output, timed_out) = run_command_capture_with_timeout(
        Command::new("pnputil.exe").current_dir(&inf_parent).args([
            "/add-driver",
            inf_path.to_string_lossy().as_ref(),
            "/install",
        ]),
        "pnputil /add-driver virtual display",
        Duration::from_secs(90),
    )?;
    if timed_out {
        bail!("pnputil /add-driver virtual display timed out");
    }
    if !ok {
        bail!("pnputil /add-driver virtual display failed: {output}");
    }

    let tool_description = control_tool_path.display().to_string();
    let (ok, output, timed_out) = run_command_capture_with_timeout(
        Command::new(control_tool_path)
            .current_dir(&inf_parent)
            .args([
                "install",
                inf_path.to_string_lossy().as_ref(),
                VIRTUAL_DISPLAY_DRIVER_HARDWARE_ID,
            ]),
        &tool_description,
        Duration::from_secs(60),
    )?;
    if timed_out {
        bail!(
            "offline virtual display installer {} timed out",
            control_tool_path.display()
        );
    }
    if !ok {
        bail!(
            "offline virtual display installer {} failed: {}",
            control_tool_path.display(),
            output
        );
    }

    let (ok, output, timed_out) = run_command_capture_with_timeout(
        Command::new("pnputil.exe").args(["/scan-devices"]),
        "pnputil /scan-devices",
        Duration::from_secs(30),
    )?;
    if timed_out {
        bail!("pnputil /scan-devices timed out");
    }
    if !ok {
        bail!("pnputil /scan-devices failed: {output}");
    }

    if let Some(settings_source) = settings_source {
        install_bundled_virtual_display_settings(settings_source)?;
    }

    Ok(())
}

fn ensure_display_dependency(paths: &BundlePaths) -> Result<DisplayDependencyState> {
    refresh_host_capability(paths)?;
    let current_profile = read_host_capability_profile(paths)?;

    if has_virtual_display_driver(current_profile.as_ref()) {
        return Ok(DisplayDependencyState {
            status: "ready".to_string(),
            reason: format!(
                "virtual display driver is already active: {}",
                summarize_display_route(current_profile.as_ref())
            ),
        });
    }

    let inf_path = find_offline_virtual_display_driver_inf(paths);
    let control_tool_path = find_offline_vdd_control_tool(paths);
    let settings_source = find_offline_virtual_display_settings(paths);

    match (&inf_path, &control_tool_path) {
        (Some(inf_path), Some(control_tool_path)) => {
            if let Err(err) = install_offline_virtual_display_driver(
                inf_path,
                control_tool_path,
                settings_source.as_ref(),
            ) {
                return Ok(DisplayDependencyState {
                    status: "manual-install-required".to_string(),
                    reason: format!(
                        "offline virtual display setup did not complete: {err:#}. Install or repair the display driver manually, then refresh Host Control."
                    ),
                });
            }
            refresh_host_capability(paths)?;
            let refreshed_profile = read_host_capability_profile(paths)?;
            if has_virtual_display_driver(refreshed_profile.as_ref()) {
                return Ok(DisplayDependencyState {
                    status: "installed".to_string(),
                    reason: format!(
                        "bundled virtual display driver was installed successfully: {}",
                        summarize_display_route(refreshed_profile.as_ref())
                    ),
                });
            }

            return Ok(DisplayDependencyState {
                status: "manual-install-required".to_string(),
                reason: format!(
                    "bundled virtual display installer ran but the driver is still not active. Install or repair the display driver manually, then refresh Host Control. Current route: {}",
                    summarize_display_route(refreshed_profile.as_ref())
                ),
            });
        }
        _ => {}
    }

    let payload_hint = match (inf_path, control_tool_path) {
        (None, None) => {
            "bundle is missing the offline virtual display payload and installer tool".to_string()
        }
        (None, Some(tool)) => format!(
            "bundle has installer tool {} but is missing the virtual display driver package",
            tool.display()
        ),
        (Some(inf), None) => format!(
            "bundle has virtual display driver package {} but is missing devcon/nefcon",
            inf.display()
        ),
        (Some(_), Some(_)) => {
            "virtual display payload exists but the driver is still not active".to_string()
        }
    };

    Ok(DisplayDependencyState {
        status: "manual-install-required".to_string(),
        reason: format!(
            "{}. Install the display driver manually or add an offline payload under drivers/virtual-display-driver plus drivers/vdd-control so resize/match-device can use the virtual display path.",
            payload_hint
        ),
    })
}

fn read_windows_build_number() -> Result<Option<u32>> {
    let output = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output()
        .context("failed to query Windows build number")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let build = stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("CurrentBuildNumber") {
            return None;
        }
        trimmed
            .split_whitespace()
            .last()
            .and_then(|value| value.parse::<u32>().ok())
    });

    Ok(build)
}

fn run_command_capture(command: &mut Command, description: &str) -> Result<(bool, String)> {
    let output =
        output_hidden(command).with_context(|| format!("failed to invoke {description}"))?;
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .trim()
    .to_string();
    Ok((output.status.success(), combined))
}

fn run_command_capture_with_timeout(
    command: &mut Command,
    description: &str,
    timeout: Duration,
) -> Result<(bool, String, bool)> {
    let (output, timed_out) = output_hidden_with_timeout(command, timeout)
        .with_context(|| format!("failed to invoke {description}"))?;
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .trim()
    .to_string();
    Ok((output.status.success(), combined, timed_out))
}

fn output_hidden(command: &mut Command) -> std::io::Result<Output> {
    command.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.output()
}

fn output_hidden_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<(Output, bool)> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });

    let started_at = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }

        if started_at.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }

        sleep(Duration::from_millis(200));
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Ok((
        Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
    ))
}

fn status_hidden(command: &mut Command) -> std::io::Result<ExitStatus> {
    command.stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.status()
}

fn download_file_with_curl(url: &str, target_path: &PathBuf) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let (ok, output) = run_command_capture(
        Command::new("curl.exe").args([
            "-fL",
            "-H",
            "User-Agent: moonlight-web-host-installer",
            "-o",
            target_path.to_string_lossy().as_ref(),
            url,
        ]),
        "curl.exe",
    )?;

    if !ok {
        bail!(
            "failed to download {} to {}: {}",
            url,
            target_path.display(),
            output
        );
    }

    Ok(())
}

fn expand_zip_archive(zip_path: &PathBuf, destination_root: &PathBuf) -> Result<()> {
    if destination_root.exists() {
        fs::remove_dir_all(destination_root)
            .with_context(|| format!("failed to remove existing {}", destination_root.display()))?;
    }
    fs::create_dir_all(destination_root)
        .with_context(|| format!("failed to create {}", destination_root.display()))?;

    let script = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        zip_path.display(),
        destination_root.display()
    );
    let (ok, output) = run_command_capture(
        Command::new("powershell.exe").args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]),
        "Expand-Archive",
    )?;
    if !ok {
        bail!(
            "failed to expand {} into {}: {}",
            zip_path.display(),
            destination_root.display(),
            output
        );
    }

    Ok(())
}

fn is_fallback_audio_installer_ready(path: &PathBuf) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };

    if parent.join("package.json").exists() || parent.join("payload").exists() {
        return true;
    }

    fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .any(|candidate| {
            candidate.is_file()
                && candidate
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "exe" | "msi" | "inf" | "zip"
                        )
                    })
        })
}

fn find_fallback_audio_installer(paths: &BundlePaths) -> Option<PathBuf> {
    let candidates = [
        paths
            .server_root
            .join("drivers")
            .join("fallback-audio")
            .join("install-audio.ps1"),
        paths
            .server_root
            .join("drivers")
            .join("fallback-audio")
            .join("install-audio.bat"),
        paths
            .bundle_root
            .join("drivers")
            .join("fallback-audio")
            .join("install-audio.ps1"),
        paths
            .bundle_root
            .join("drivers")
            .join("fallback-audio")
            .join("install-audio.bat"),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists() && is_fallback_audio_installer_ready(path))
}

fn try_install_fallback_audio_dependency(paths: &BundlePaths) -> Result<Option<String>> {
    let Some(installer_path) = find_fallback_audio_installer(paths) else {
        return Ok(None);
    };

    let description = installer_path.display().to_string();
    let installer_dir = installer_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.bundle_root.clone());
    let (ok, output) = if installer_path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
    {
        run_command_capture(
            Command::new("powershell.exe")
                .current_dir(&installer_dir)
                .env(
                    "MOONLIGHT_BUNDLE_ROOT",
                    paths.bundle_root.to_string_lossy().to_string(),
                )
                .env(
                    "MOONLIGHT_SERVER_ROOT",
                    paths.server_root.to_string_lossy().to_string(),
                )
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    installer_path.to_string_lossy().as_ref(),
                    "-BundleRoot",
                    paths.bundle_root.to_string_lossy().as_ref(),
                    "-ServerRoot",
                    paths.server_root.to_string_lossy().as_ref(),
                ]),
            &description,
        )?
    } else {
        run_command_capture(
            Command::new("cmd.exe")
                .current_dir(&installer_dir)
                .env(
                    "MOONLIGHT_BUNDLE_ROOT",
                    paths.bundle_root.to_string_lossy().to_string(),
                )
                .env(
                    "MOONLIGHT_SERVER_ROOT",
                    paths.server_root.to_string_lossy().to_string(),
                )
                .args(["/c", installer_path.to_string_lossy().as_ref()]),
            &description,
        )?
    };

    if !ok {
        bail!(
            "fallback audio installer {} failed: {}",
            installer_path.display(),
            output
        );
    }

    Ok(Some(installer_path.display().to_string()))
}

fn ensure_virtual_audio_driver_package(paths: &BundlePaths) -> Result<(PathBuf, String)> {
    let bundled_candidates = [
        paths
            .server_root
            .join("drivers")
            .join("virtual-audio-driver")
            .join("VirtualAudioDriver.inf"),
        paths
            .bundle_root
            .join("drivers")
            .join("virtual-audio-driver")
            .join("VirtualAudioDriver.inf"),
    ];
    for candidate in bundled_candidates {
        if candidate.exists() {
            let root = candidate
                .parent()
                .ok_or_else(|| anyhow!("invalid audio driver package path"))?
                .to_path_buf();
            return Ok((root, "bundle-local".to_string()));
        }
    }

    let cache_root = paths
        .driver_cache_root
        .join("virtual-audio-driver")
        .join(VIRTUAL_AUDIO_DRIVER_RELEASE_TAG);
    let package_root = cache_root.join("VirtualAudioDriver");
    let inf_path = package_root.join("VirtualAudioDriver.inf");
    if inf_path.exists() {
        return Ok((package_root, "cache-download".to_string()));
    }

    let zip_path = cache_root.join("VirtualAudioDriver-x86.Driver.Only.zip");
    download_file_with_curl(VIRTUAL_AUDIO_DRIVER_PACKAGE_URL, &zip_path)?;
    expand_zip_archive(&zip_path, &cache_root)?;
    if !inf_path.exists() {
        bail!(
            "downloaded virtual audio driver package is missing {}",
            inf_path.display()
        );
    }

    Ok((package_root, "release-download".to_string()))
}

fn ensure_vdd_control_root(paths: &BundlePaths) -> Result<(PathBuf, String)> {
    let bundled_candidates = [
        paths.server_root.join("drivers").join("vdd-control"),
        paths.bundle_root.join("drivers").join("vdd-control"),
    ];
    for candidate in bundled_candidates {
        if candidate.join("Dependencies").join("devcon.exe").exists() {
            return Ok((candidate, "bundle-local".to_string()));
        }
    }

    let cache_root = paths
        .driver_cache_root
        .join("vdd-control")
        .join(VIRTUAL_AUDIO_DRIVER_RELEASE_TAG);
    let control_root = cache_root;
    if control_root
        .join("Dependencies")
        .join("devcon.exe")
        .exists()
    {
        return Ok((control_root, "cache-download".to_string()));
    }

    let zip_path = paths.driver_cache_root.join("vdd-control").join(format!(
        "VDD.Control.{VIRTUAL_AUDIO_DRIVER_RELEASE_TAG}.zip"
    ));
    download_file_with_curl(VDD_CONTROL_PACKAGE_URL, &zip_path)?;
    expand_zip_archive(
        &zip_path,
        &paths
            .driver_cache_root
            .join("vdd-control")
            .join(VIRTUAL_AUDIO_DRIVER_RELEASE_TAG),
    )?;
    if !control_root
        .join("Dependencies")
        .join("devcon.exe")
        .exists()
    {
        bail!(
            "downloaded VDD Control package is missing {}",
            control_root
                .join("Dependencies")
                .join("devcon.exe")
                .display()
        );
    }

    Ok((control_root, "release-download".to_string()))
}

fn wait_for_mtt_audio_route(paths: &BundlePaths, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        refresh_host_capability(paths)?;
        let profile = read_host_capability_profile(paths)?;
        if is_managed_virtual_audio_route(profile.as_ref()) {
            return Ok(true);
        }

        if Instant::now() >= deadline {
            return Ok(false);
        }

        sleep(Duration::from_secs(1));
    }
}

fn ensure_audio_dependency(paths: &BundlePaths) -> Result<AudioDependencyState> {
    refresh_host_capability(paths)?;
    let mut state = AudioDependencyState {
        preferred_audio_driver: "virtual-audio-driver-preferred".to_string(),
        ..AudioDependencyState::default()
    };

    let current_profile = read_host_capability_profile(paths)?;
    if is_managed_virtual_audio_route(current_profile.as_ref()) {
        let (_, reason) = enrich_audio_reason_with_cleanup(
            paths,
            current_profile,
            format!(
                "managed virtual audio route is already active: {}",
                summarize_audio_route(read_host_capability_profile(paths)?.as_ref())
            ),
        )?;
        state.status = "ready".to_string();
        state.reason = reason;
        write_audio_dependency_state(paths, &state)?;
        return Ok(state);
    }

    state.windows_build_number = read_windows_build_number()?;
    if is_audio_route_ready(current_profile.as_ref()) {
        let (current_profile, reason) = enrich_audio_reason_with_cleanup(
            paths,
            current_profile,
            format!(
                "installer kept the existing audio route and skipped automatic driver setup: {}",
                summarize_audio_route(read_host_capability_profile(paths)?.as_ref())
            ),
        )?;
        state.status = "ready".to_string();
        state.reason = reason;
        let _ = current_profile;
        write_audio_dependency_state(paths, &state)?;
        return Ok(state);
    }

    let windows_build = state
        .windows_build_number
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let fallback_ready =
        find_fallback_audio_installer(paths).map(|path| path.display().to_string());
    state.status = "manual-install-required".to_string();
    state.reason = match fallback_ready {
        Some(installer_path) => format!(
            "no working audio route was detected on Windows build {windows_build}. Automatic audio driver setup is disabled during install. Install or configure the audio driver manually later, then refresh/select the route in Host Control. Bundled fallback installer is available at {installer_path}."
        ),
        None => format!(
            "no working audio route was detected on Windows build {windows_build}. Automatic audio driver setup is disabled during install. Install or configure SYMO/VB-CABLE/AudioRelay manually later, then refresh/select the route in Host Control."
        ),
    };
    write_audio_dependency_state(paths, &state)?;
    Ok(state)
}

fn find_selected_runtime_candidate<'a>(
    profile: &'a HostCapabilityProfile,
) -> Option<&'a HostCapabilityRuntimeCandidate> {
    profile.runtime_candidates.iter().find(|candidate| {
        candidate
            .key
            .eq_ignore_ascii_case(profile.selected_runtime_key.as_str())
    })
}

fn runtime_candidate_operational_score(candidate: &HostCapabilityRuntimeCandidate) -> i32 {
    let runtime_status = candidate.runtime_status.trim().to_ascii_lowercase();
    let mut score = match runtime_status.as_str() {
        "ready" => 300,
        "software_only" => 220,
        _ => return 0,
    };

    match candidate
        .startup_validation_status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("passed") => score += 80,
        Some("pending") => score += 20,
        Some("failed") => return 0,
        Some(_) => return 0,
        None => score += 10,
    }

    if candidate.auto_select {
        score += 8;
    }
    score += candidate.healthy_encoders.len().min(8) as i32;
    score
}

fn derive_runtime_recommendation<'a>(
    profile: Option<&'a HostCapabilityProfile>,
    selected_runtime_key: &str,
) -> (
    Option<&'a HostCapabilityRuntimeCandidate>,
    Option<String>,
    bool,
    u32,
) {
    let Some(profile) = profile else {
        return (
            None,
            Some("host capability profile is missing".to_string()),
            false,
            0,
        );
    };

    let selected_candidate = profile.runtime_candidates.iter().find(|candidate| {
        candidate.key.eq_ignore_ascii_case(selected_runtime_key)
            || candidate
                .relative_directory
                .eq_ignore_ascii_case(selected_runtime_key)
    });
    let selected_score = selected_candidate
        .map(runtime_candidate_operational_score)
        .unwrap_or_default();

    let best_candidate = profile
        .runtime_candidates
        .iter()
        .max_by_key(|candidate| {
            let mut score = runtime_candidate_operational_score(candidate);
            if candidate.key.eq_ignore_ascii_case(selected_runtime_key)
                || candidate
                    .relative_directory
                    .eq_ignore_ascii_case(selected_runtime_key)
            {
                score += 1;
            }
            score
        })
        .filter(|candidate| runtime_candidate_operational_score(candidate) > 0);

    let alternate_ready_runtime_count = profile
        .runtime_candidates
        .iter()
        .filter(|candidate| runtime_candidate_operational_score(candidate) > 0)
        .filter(|candidate| {
            !candidate.key.eq_ignore_ascii_case(selected_runtime_key)
                && !candidate
                    .relative_directory
                    .eq_ignore_ascii_case(selected_runtime_key)
        })
        .count() as u32;

    let Some(best_candidate) = best_candidate else {
        return (
            None,
            Some(
                "no ready runtime candidate is available in the host capability profile"
                    .to_string(),
            ),
            false,
            0,
        );
    };

    let best_label = best_candidate
        .display_name
        .as_deref()
        .unwrap_or(best_candidate.key.as_str());
    let switch_required = !best_candidate
        .key
        .eq_ignore_ascii_case(selected_runtime_key)
        && !best_candidate
            .relative_directory
            .eq_ignore_ascii_case(selected_runtime_key);
    let reason = if switch_required {
        if selected_score > 0 {
            Some(format!(
                "strongest ready runtime candidate is {best_label}; switch if the current slot keeps misbehaving"
            ))
        } else {
            Some(format!(
                "selected runtime is not healthy enough; strongest ready candidate is {best_label}"
            ))
        }
    } else if alternate_ready_runtime_count > 0 {
        Some(format!(
            "selected runtime already matches the strongest ready candidate; {alternate_ready_runtime_count} alternate ready runtime(s) also exist"
        ))
    } else {
        Some("selected runtime already matches the strongest ready candidate".to_string())
    };

    (
        Some(best_candidate),
        reason,
        switch_required,
        alternate_ready_runtime_count,
    )
}

fn derive_universal_bundle_status(
    profile: Option<&HostCapabilityProfile>,
    selected_runtime_key: &str,
) -> (String, String) {
    let Some(profile) = profile else {
        return (
            "unknown".to_string(),
            "host capability profile is missing".to_string(),
        );
    };

    if profile.selected_runtime_key.trim().is_empty() {
        return (
            "degraded".to_string(),
            "host capability profile does not specify a selected runtime".to_string(),
        );
    }

    if !profile
        .selected_runtime_key
        .eq_ignore_ascii_case(selected_runtime_key)
        && !profile
            .selected_runtime_directory
            .eq_ignore_ascii_case(selected_runtime_key)
    {
        return (
            "degraded".to_string(),
            format!(
                "selected runtime cache ({selected_runtime_key}) does not match capability profile ({}/{})",
                profile.selected_runtime_key, profile.selected_runtime_directory
            ),
        );
    }

    if !profile.config_applied {
        return (
            "degraded".to_string(),
            "host capability profile has not been applied to the live runtime config".to_string(),
        );
    }

    let Some(selected_candidate) = find_selected_runtime_candidate(profile) else {
        return (
            "degraded".to_string(),
            "selected runtime is missing from runtime candidates".to_string(),
        );
    };

    match selected_candidate
        .runtime_status
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "ready" | "software_only" => {}
        status => {
            return (
                "degraded".to_string(),
                format!(
                    "selected runtime status is {status}{}",
                    selected_candidate
                        .runtime_status_reason
                        .as_deref()
                        .map(|reason| format!(" ({reason})"))
                        .unwrap_or_default()
                ),
            );
        }
    }

    if let Some(startup_validation_status) = selected_candidate
        .startup_validation_status
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        if !startup_validation_status.eq_ignore_ascii_case("passed") {
            return (
                "degraded".to_string(),
                format!(
                    "selected runtime startup validation is {startup_validation_status}{}",
                    selected_candidate
                        .startup_validation_reason
                        .as_deref()
                        .map(|reason| format!(" ({reason})"))
                        .unwrap_or_default()
                ),
            );
        }
    }

    let runtime_label = profile
        .selected_runtime_display_name
        .as_deref()
        .unwrap_or(profile.selected_runtime_key.as_str());
    let mut reason = format!(
        "preflight selected {runtime_label} / {} / {}",
        profile.selected_encoder, profile.selected_capture
    );
    if let Some(reason_text) = profile.selected_capture_reason.as_deref() {
        if !reason_text.trim().is_empty() {
            reason.push_str(&format!(" ({reason_text})"));
        }
    }
    if !profile.warnings.is_empty() {
        reason.push_str(&format!(" with {} warning(s)", profile.warnings.len()));
    }

    ("ready".to_string(), reason)
}

fn read_release_gate_summary(paths: &BundlePaths) -> Result<Option<HostReleaseGateSummary>> {
    let path = paths.release_gate_summary_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostReleaseGateSummary>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_release_gate_history(paths: &BundlePaths) -> Result<Vec<HostReleaseGateSummary>> {
    let path = paths.release_gate_history_path.clone();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<ReleaseGateHistoryDocument>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed.entries)
}

fn read_diagnostic_pack_summary(paths: &BundlePaths) -> Result<Option<HostDiagnosticPackSummary>> {
    let path = paths.diagnostic_pack_summary_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostDiagnosticPackSummary>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_diagnostic_pack_history(paths: &BundlePaths) -> Result<Vec<HostDiagnosticPackSummary>> {
    let path = paths.diagnostic_pack_history_path.clone();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<DiagnosticPackHistoryDocument>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed.entries)
}

fn read_release_upgrade_state(paths: &BundlePaths) -> Result<Option<HostReleaseUpgradeState>> {
    let path = paths.release_upgrade_state_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostReleaseUpgradeState>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_runtime_adoption_state(paths: &BundlePaths) -> Result<Option<HostRuntimeAdoptionState>> {
    let path = paths.runtime_adoption_state_path.clone();
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<HostRuntimeAdoptionState>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn read_release_history(paths: &BundlePaths) -> Result<Vec<HostReleaseHistoryEntry>> {
    let path = paths.release_history_path.clone();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<ReleaseHistoryDocument>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed.entries)
}

fn read_runtime_adoption_history(
    paths: &BundlePaths,
) -> Result<Vec<HostRuntimeAdoptionHistoryEntry>> {
    let path = paths.runtime_adoption_history_path.clone();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<RuntimeAdoptionHistoryDocument>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed.entries)
}

fn derive_release_gate_status(
    release_info: Option<&HostReleaseInfo>,
    release_gate_summary: Option<&HostReleaseGateSummary>,
) -> (String, String) {
    let Some(summary) = release_gate_summary else {
        return (
            "missing".to_string(),
            "no release gate result has been recorded for this bundle".to_string(),
        );
    };

    if !summary.gate_status.eq_ignore_ascii_case("passed") {
        return (summary.gate_status.clone(), summary.gate_reason.clone());
    }

    if let Some(release_info) = release_info {
        if let (Some(current_commit), Some(summary_commit)) = (
            release_info.source_commit_short.as_deref(),
            summary.source_commit_short.as_deref(),
        ) {
            if !current_commit.is_empty()
                && !summary_commit.is_empty()
                && !current_commit.eq_ignore_ascii_case(summary_commit)
            {
                return (
                    "stale".to_string(),
                    "release gate result belongs to a different source commit".to_string(),
                );
            }
        }

        if let (Some(current_built_at), Some(summary_built_at)) =
            (release_info.built_at_unix_ms, summary.built_at_unix_ms)
        {
            if summary_built_at < current_built_at || summary.checked_at_unix_ms < current_built_at
            {
                return (
                    "stale".to_string(),
                    "release gate result is older than the active build metadata".to_string(),
                );
            }
        }
    }

    ("passed".to_string(), summary.gate_reason.clone())
}

fn derive_diagnostic_pack_status(
    release_info: Option<&HostReleaseInfo>,
    diagnostic_pack_summary: Option<&HostDiagnosticPackSummary>,
) -> (String, String) {
    let Some(summary) = diagnostic_pack_summary else {
        return (
            "missing".to_string(),
            "no diagnostic pack result has been recorded for this bundle".to_string(),
        );
    };

    if !summary.pack_status.eq_ignore_ascii_case("passed") {
        return (summary.pack_status.clone(), summary.pack_reason.clone());
    }

    if let Some(release_info) = release_info {
        if let (Some(current_commit), Some(summary_commit)) = (
            release_info.source_commit_short.as_deref(),
            summary.source_commit_short.as_deref(),
        ) {
            if !current_commit.is_empty()
                && !summary_commit.is_empty()
                && !current_commit.eq_ignore_ascii_case(summary_commit)
            {
                return (
                    "stale".to_string(),
                    "diagnostic pack belongs to a different source commit".to_string(),
                );
            }
        }

        if let (Some(current_built_at), Some(summary_built_at)) =
            (release_info.built_at_unix_ms, summary.built_at_unix_ms)
        {
            if summary_built_at < current_built_at || summary.checked_at_unix_ms < current_built_at
            {
                return (
                    "stale".to_string(),
                    "diagnostic pack result is older than the active build metadata".to_string(),
                );
            }
        }
    }

    ("passed".to_string(), summary.pack_reason.clone())
}

fn derive_bundle_artifact_suffix(
    paths: &BundlePaths,
    release_info: Option<&HostReleaseInfo>,
    fallback: &str,
) -> String {
    release_info
        .and_then(|info| info.source_commit_short.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            paths
                .bundle_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn backup_config_state(paths: &BundlePaths) -> Result<ConfigStateBackupResult> {
    fs::create_dir_all(&paths.config_state_backups_root).with_context(|| {
        format!(
            "failed to create config backup root {}",
            paths.config_state_backups_root.display()
        )
    })?;

    let created_at_unix_ms = now_unix_ms();
    let release_info = read_release_info(paths)?;
    let selected_runtime_key = read_selected_runtime_key(paths);
    let backup_suffix = derive_bundle_artifact_suffix(paths, release_info.as_ref(), "config");
    let backup_id = format!("{created_at_unix_ms}-{backup_suffix}");
    let backup_root = paths.config_state_backups_root.join(&backup_id);
    if backup_root.exists() {
        fs::remove_dir_all(&backup_root)
            .with_context(|| format!("failed to clear backup {}", backup_root.display()))?;
    }
    fs::create_dir_all(&backup_root)
        .with_context(|| format!("failed to create backup {}", backup_root.display()))?;

    let mut file_count = 0u32;
    file_count += copy_optional_file(&paths.config_path, &backup_root.join("config.json"))?;
    file_count += copy_optional_file(&paths.data_path, &backup_root.join("data.json"))?;
    file_count += copy_optional_file(
        &paths.selected_runtime_path,
        &backup_root.join("selected_sunshine_runtime.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.hard_reset_mode_path,
        &backup_root.join("hard_reset_mode.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.promotion_policy_path,
        &backup_root.join("promotion_policy.json"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("force_legacy_nvenc.txt"),
        &backup_root.join("force_legacy_nvenc.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("shared_pair_info.json"),
        &backup_root.join("shared_pair_info.json"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("dynamic_display_state.json"),
        &backup_root.join("dynamic_display_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.bundle_root.join("PUBLIC_URL.txt"),
        &backup_root.join("PUBLIC_URL.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.bundle_root.join("frp").join("frpc.toml"),
        &backup_root.join("frp").join("frpc.toml"),
    )?;
    file_count += copy_optional_directory(
        &paths.bundle_root.join("sunshine").join("config"),
        &backup_root.join("sunshine").join("config"),
    )?;
    file_count += copy_optional_directory(
        &paths.bundle_root.join("sunshine-legacy").join("config"),
        &backup_root.join("sunshine-legacy").join("config"),
    )?;

    let manifest = ConfigStateBackupManifest {
        schema_version: 1,
        backup_id: backup_id.clone(),
        created_at_unix_ms,
        selected_runtime_key,
        current_release_id: release_info.as_ref().map(derive_release_id),
    };
    fs::write(
        backup_root.join("config_state_backup_manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write config state backup manifest {}",
            backup_root.display()
        )
    })?;
    file_count += 1;

    Ok(ConfigStateBackupResult {
        backup_id,
        created_at_unix_ms,
        backup_root: backup_root.display().to_string(),
        file_count,
    })
}

fn collect_support_bundle(paths: &BundlePaths) -> Result<SupportBundleResult> {
    fs::create_dir_all(&paths.support_bundles_root).with_context(|| {
        format!(
            "failed to create support bundle root {}",
            paths.support_bundles_root.display()
        )
    })?;

    let created_at_unix_ms = now_unix_ms();
    let installer_status = build_installer_status(paths)?;
    let supervisor_status = run_supervisor_status(paths)?;
    let service_status = query_service_status(paths).unwrap_or_else(|err| format!("{err:#}"));
    let release_info = installer_status.release_info.clone();
    let support_suffix = derive_bundle_artifact_suffix(paths, release_info.as_ref(), "support");
    let support_bundle_id = format!("{created_at_unix_ms}-{support_suffix}");
    let support_bundle_root = paths.support_bundles_root.join(&support_bundle_id);
    if support_bundle_root.exists() {
        fs::remove_dir_all(&support_bundle_root).with_context(|| {
            format!(
                "failed to clear support bundle {}",
                support_bundle_root.display()
            )
        })?;
    }
    fs::create_dir_all(&support_bundle_root).with_context(|| {
        format!(
            "failed to create support bundle {}",
            support_bundle_root.display()
        )
    })?;

    let mut file_count = 0u32;
    file_count += write_json_artifact(
        &support_bundle_root.join("installer_status.json"),
        &installer_status,
    )?;
    file_count += write_json_artifact(
        &support_bundle_root.join("supervisor_status.json"),
        &supervisor_status,
    )?;
    file_count += write_text_artifact(
        &support_bundle_root.join("service_status.txt"),
        &service_status,
    )?;
    file_count += copy_optional_file(
        &paths.release_info_path,
        &support_bundle_root.join("release_info.json"),
    )?;
    file_count += copy_optional_file(
        &paths.promotion_policy_path,
        &support_bundle_root.join("promotion_policy.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_gate_summary_path,
        &support_bundle_root.join("release_gate_summary.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_gate_history_path,
        &support_bundle_root.join("release_gate_history.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_upgrade_state_path,
        &support_bundle_root.join("release_upgrade_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_history_path,
        &support_bundle_root.join("release_history.json"),
    )?;
    file_count += copy_optional_file(
        &paths.runtime_adoption_state_path,
        &support_bundle_root.join("runtime_adoption_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.runtime_adoption_history_path,
        &support_bundle_root.join("runtime_adoption_history.json"),
    )?;
    file_count += copy_optional_file(
        &paths.selected_runtime_path,
        &support_bundle_root.join("selected_sunshine_runtime.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.hard_reset_mode_path,
        &support_bundle_root.join("hard_reset_mode.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("host_capability_profile.json"),
        &support_bundle_root.join("host_capability_profile.json"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("host_supervisor_state.json"),
        &support_bundle_root.join("host_supervisor_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("dynamic_display_state.json"),
        &support_bundle_root.join("dynamic_display_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.server_root.join("shared_pair_info.json"),
        &support_bundle_root.join("shared_pair_info.json"),
    )?;
    file_count += copy_optional_top_level_files_by_extension(
        &paths.server_root,
        &support_bundle_root.join("server_logs"),
        &["log", "jsonl"],
    )?;
    file_count += copy_optional_file(
        &paths
            .bundle_root
            .join("sunshine")
            .join("config")
            .join("sunshine.log"),
        &support_bundle_root.join("sunshine").join("sunshine.log"),
    )?;
    file_count += copy_optional_file(
        &paths
            .bundle_root
            .join("sunshine")
            .join("config")
            .join("sunshine_state.json"),
        &support_bundle_root
            .join("sunshine")
            .join("sunshine_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths
            .bundle_root
            .join("sunshine-legacy")
            .join("config")
            .join("sunshine.log"),
        &support_bundle_root
            .join("sunshine-legacy")
            .join("sunshine.log"),
    )?;
    file_count += copy_optional_file(
        &paths
            .bundle_root
            .join("sunshine-legacy")
            .join("config")
            .join("sunshine_state.json"),
        &support_bundle_root
            .join("sunshine-legacy")
            .join("sunshine_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths
            .bundle_root
            .join("sunshine-legacy")
            .join("sunshine_runtime_info.json"),
        &support_bundle_root
            .join("sunshine-legacy")
            .join("sunshine_runtime_info.json"),
    )?;
    file_count += copy_optional_file(
        &paths.bundle_root.join("frp").join("frpc-live.out.log"),
        &support_bundle_root.join("frp").join("frpc-live.out.log"),
    )?;
    file_count += copy_optional_file(
        &paths.bundle_root.join("frp").join("frpc-live.err.log"),
        &support_bundle_root.join("frp").join("frpc-live.err.log"),
    )?;

    let manifest = SupportBundleManifest {
        schema_version: 1,
        support_bundle_id: support_bundle_id.clone(),
        created_at_unix_ms,
        current_release_id: installer_status.current_release_id.clone(),
        health_grade: installer_status.health_grade.clone(),
        lifecycle_phase: installer_status.lifecycle_phase.clone(),
    };
    fs::write(
        support_bundle_root.join("support_bundle_manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write support bundle manifest {}",
            support_bundle_root.display()
        )
    })?;
    file_count += 1;

    Ok(SupportBundleResult {
        support_bundle_id,
        created_at_unix_ms,
        support_bundle_root: support_bundle_root.display().to_string(),
        file_count,
    })
}

fn capture_release_snapshot(paths: &BundlePaths) -> Result<ReleaseSnapshotResult> {
    fs::create_dir_all(&paths.release_snapshots_root).with_context(|| {
        format!(
            "failed to create release snapshot root {}",
            paths.release_snapshots_root.display()
        )
    })?;

    let created_at_unix_ms = now_unix_ms();
    let release_info = read_release_info(paths)?;
    let selected_runtime_key = read_selected_runtime_key(paths);
    let snapshot_suffix = release_info
        .as_ref()
        .and_then(|info| info.source_commit_short.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "snapshot".to_string());
    let snapshot_id = format!("{created_at_unix_ms}-{snapshot_suffix}");
    let snapshot_root = paths.release_snapshots_root.join(&snapshot_id);
    if snapshot_root.exists() {
        fs::remove_dir_all(&snapshot_root).with_context(|| {
            format!(
                "failed to clear existing snapshot {}",
                snapshot_root.display()
            )
        })?;
    }
    fs::create_dir_all(&snapshot_root)
        .with_context(|| format!("failed to create snapshot {}", snapshot_root.display()))?;

    let mut file_count = 0u32;
    file_count += copy_optional_file(&paths.config_path, &snapshot_root.join("config.json"))?;
    file_count += copy_optional_file(&paths.data_path, &snapshot_root.join("data.json"))?;
    file_count += copy_optional_file(
        &paths.selected_runtime_path,
        &snapshot_root.join("selected_sunshine_runtime.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.hard_reset_mode_path,
        &snapshot_root.join("hard_reset_mode.txt"),
    )?;
    file_count += copy_optional_file(
        &paths.release_info_path,
        &snapshot_root.join("release_info.json"),
    )?;
    file_count += copy_optional_file(
        &paths.promotion_policy_path,
        &snapshot_root.join("promotion_policy.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_gate_summary_path,
        &snapshot_root.join("release_gate_summary.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_gate_history_path,
        &snapshot_root.join("release_gate_history.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_upgrade_state_path,
        &snapshot_root.join("release_upgrade_state.json"),
    )?;
    file_count += copy_optional_file(
        &paths.release_history_path,
        &snapshot_root.join("release_history.json"),
    )?;
    file_count += copy_optional_file(
        &paths.host_installer_path,
        &snapshot_root.join("host-installer.exe"),
    )?;
    file_count += copy_optional_directory(&paths.static_root, &snapshot_root.join("static"))?;

    let manifest = ReleaseSnapshotManifest {
        schema_version: 1,
        snapshot_id: snapshot_id.clone(),
        created_at_unix_ms,
        selected_runtime_key,
        release_info,
    };
    fs::write(
        snapshot_root.join("snapshot_manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write snapshot manifest {}",
            snapshot_root.display()
        )
    })?;
    file_count += 1;

    Ok(ReleaseSnapshotResult {
        snapshot_id,
        created_at_unix_ms,
        snapshot_root: snapshot_root.display().to_string(),
        file_count,
    })
}

fn prepare_release_upgrade(paths: &BundlePaths) -> Result<ReleaseUpgradePreparationResult> {
    let status = build_installer_status(paths)?;
    if status.migration_readiness.eq_ignore_ascii_case("blocked") {
        bail!("release upgrade is blocked: {}", status.migration_reason);
    }

    let snapshot = if status.rollback_ready {
        None
    } else {
        Some(capture_release_snapshot(paths)?)
    };

    Ok(ReleaseUpgradePreparationResult {
        migration_readiness: "ready".to_string(),
        migration_reason: if snapshot.is_some() {
            "rollback snapshot captured and host is ready for upgrade".to_string()
        } else {
            "host is already ready for upgrade".to_string()
        },
        snapshot,
    })
}

fn refresh_host_capability(paths: &BundlePaths) -> Result<HostCapabilityRefreshResult> {
    let helper_path = paths.server_root.join("display-prepare-helper.exe");
    if !helper_path.exists() {
        bail!(
            "missing display-prepare-helper.exe at {}",
            helper_path.display()
        );
    }

    let mut helper_command = Command::new(&helper_path);
    helper_command
        .arg("preflight")
        .arg("--bundle-root")
        .arg(&paths.bundle_root)
        .arg("--refresh")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = output_hidden(&mut helper_command)
        .with_context(|| format!("failed to run {}", helper_path.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed =
        serde_json::from_str::<HostCapabilityRefreshResult>(&stdout).with_context(|| {
            format!(
                "failed to parse host capability refresh result from {}",
                helper_path.display()
            )
        })?;

    if !output.status.success() || !parsed.ok {
        bail!(
            "host capability refresh failed: {}{}",
            parsed.reason,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", stderr.trim())
            }
        );
    }

    Ok(parsed)
}

fn adopt_recommended_runtime(paths: &BundlePaths) -> Result<RuntimeRecommendationAdoptionResult> {
    let previous_status = build_installer_status(paths)?;
    let profile = read_host_capability_profile(paths)?
        .ok_or_else(|| anyhow!("host capability profile is missing"))?;
    let previous_runtime_key = read_selected_runtime_key(paths);
    let previous_runtime_display_name = previous_status.selected_runtime_display_name.clone();
    let (
        recommended_candidate,
        recommendation_reason,
        switch_required,
        alternate_ready_runtime_count,
    ) = derive_runtime_recommendation(Some(&profile), &previous_runtime_key);
    let recommended_candidate = recommended_candidate.ok_or_else(|| {
        anyhow!(
            "{}",
            recommendation_reason
                .clone()
                .unwrap_or_else(|| "no ready runtime recommendation is available".to_string())
        )
    })?;
    let recommendation_reason = recommendation_reason.unwrap_or_else(|| {
        "selected runtime already matches the strongest ready candidate".to_string()
    });
    let adopted_runtime_directory = recommended_candidate.relative_directory.clone();
    let changed = !adopted_runtime_directory.eq_ignore_ascii_case(&previous_runtime_key);
    let started_at_unix_ms = now_unix_ms();

    let service_status = query_service_status(paths).unwrap_or_default();
    let service_running = service_status.contains("STATE") && service_status.contains("RUNNING");
    let bundle_running = run_supervisor_status(paths)
        .ok()
        .and_then(|status| has_required_processes(&status).ok())
        .unwrap_or(false);
    let runtime_was_active = bundle_running || service_running;

    let finalize_state =
        |last_status: &str, last_reason: &str, reverted: bool| -> HostRuntimeAdoptionState {
            build_runtime_adoption_state(
                "adopt",
                last_status,
                last_reason,
                Some(previous_runtime_key.clone()),
                previous_runtime_display_name.clone(),
                Some(recommended_candidate.key.clone()),
                Some(adopted_runtime_directory.clone()),
                recommended_candidate.display_name.clone(),
                recommended_candidate.runtime_version.clone(),
                switch_required,
                changed,
                reverted,
                started_at_unix_ms,
                Some(now_unix_ms()),
                None,
            )
        };

    if !changed {
        let state = finalize_state("noop", &recommendation_reason, false);
        write_runtime_adoption_state(paths, &state)?;
        append_runtime_adoption_history(paths, &build_runtime_adoption_history_entry(&state))?;
        return Ok(RuntimeRecommendationAdoptionResult {
            previous_runtime_key,
            previous_runtime_display_name,
            adopted_runtime_key: recommended_candidate.key.clone(),
            adopted_runtime_directory,
            adopted_runtime_display_name: recommended_candidate.display_name.clone(),
            adopted_runtime_version: recommended_candidate.runtime_version.clone(),
            switch_required,
            changed,
            alternate_ready_runtime_count,
            recommendation_reason: recommendation_reason.clone(),
            verification_reason: recommendation_reason,
        });
    }

    let started_state = build_runtime_adoption_state(
        "adopt",
        "started",
        &format!(
            "runtime adoption started toward {}",
            recommended_candidate
                .display_name
                .clone()
                .unwrap_or_else(|| recommended_candidate.key.clone())
        ),
        Some(previous_runtime_key.clone()),
        previous_runtime_display_name.clone(),
        Some(recommended_candidate.key.clone()),
        Some(adopted_runtime_directory.clone()),
        recommended_candidate.display_name.clone(),
        recommended_candidate.runtime_version.clone(),
        switch_required,
        changed,
        false,
        started_at_unix_ms,
        None,
        None,
    );
    write_runtime_adoption_state(paths, &started_state)?;

    let mut bundle_stopped = false;
    let mut runtime_cache_updated = false;
    let switch_result: Result<String> = (|| {
        if bundle_running {
            run_supervisor_command(paths, "stop-bundle")?;
            wait_for_bundle_process_state(paths, false, Duration::from_secs(45))?;
            bundle_stopped = true;
        }

        fs::write(
            &paths.selected_runtime_path,
            format!("{adopted_runtime_directory}\r\n"),
        )
        .with_context(|| format!("failed to write {}", paths.selected_runtime_path.display()))?;
        runtime_cache_updated = true;

        if runtime_was_active {
            run_supervisor_command(paths, "start-bundle")?;
            wait_for_bundle_process_state(paths, true, Duration::from_secs(75))?;
            sleep(Duration::from_secs(8));
            verify_startup(paths)?;
            verify_post_apply_health(paths, Duration::from_secs(30))
        } else {
            Ok("recommended runtime adopted while bundle/service remained stopped".to_string())
        }
    })();

    let verification_reason = match switch_result {
        Ok(verification_reason) => {
            let state = finalize_state("succeeded", &verification_reason, false);
            write_runtime_adoption_state(paths, &state)?;
            append_runtime_adoption_history(paths, &build_runtime_adoption_history_entry(&state))?;
            verification_reason
        }
        Err(switch_error) => {
            let rollback_needed = bundle_stopped || runtime_cache_updated || runtime_was_active;
            if rollback_needed {
                let rollback_result: Result<String> = (|| {
                    fs::write(
                        &paths.selected_runtime_path,
                        format!("{previous_runtime_key}\r\n"),
                    )
                    .with_context(|| {
                        format!(
                            "failed to restore {}",
                            paths.selected_runtime_path.display()
                        )
                    })?;

                    if runtime_was_active {
                        run_supervisor_command(paths, "start-bundle")?;
                        wait_for_bundle_process_state(paths, true, Duration::from_secs(75))?;
                        sleep(Duration::from_secs(8));
                        verify_startup(paths)?;
                        verify_post_apply_health(paths, Duration::from_secs(30))
                    } else {
                        Ok(
                            "previous runtime restored while bundle/service remained stopped"
                                .to_string(),
                        )
                    }
                })();

                match rollback_result {
                    Ok(rollback_reason) => {
                        let failure_reason = format!(
                            "runtime adoption failed: {switch_error}; reverted to previous runtime: {rollback_reason}"
                        );
                        let state = finalize_state("reverted", &failure_reason, true);
                        write_runtime_adoption_state(paths, &state)?;
                        append_runtime_adoption_history(
                            paths,
                            &build_runtime_adoption_history_entry(&state),
                        )?;
                        return Err(anyhow!(failure_reason));
                    }
                    Err(rollback_error) => {
                        let failure_reason = format!(
                            "runtime adoption failed: {switch_error}; rollback failed: {rollback_error}"
                        );
                        let state = finalize_state("failed", &failure_reason, false);
                        let _ = write_runtime_adoption_state(paths, &state);
                        let _ = append_runtime_adoption_history(
                            paths,
                            &build_runtime_adoption_history_entry(&state),
                        );
                        return Err(anyhow!(failure_reason));
                    }
                }
            }

            let failure_reason = format!("runtime adoption failed: {switch_error}");
            let state = finalize_state("failed", &failure_reason, false);
            let _ = write_runtime_adoption_state(paths, &state);
            let _ = append_runtime_adoption_history(
                paths,
                &build_runtime_adoption_history_entry(&state),
            );
            return Err(anyhow!(failure_reason));
        }
    };

    Ok(RuntimeRecommendationAdoptionResult {
        previous_runtime_key,
        previous_runtime_display_name,
        adopted_runtime_key: recommended_candidate.key.clone(),
        adopted_runtime_directory,
        adopted_runtime_display_name: recommended_candidate.display_name.clone(),
        adopted_runtime_version: recommended_candidate.runtime_version.clone(),
        switch_required,
        changed,
        alternate_ready_runtime_count,
        recommendation_reason,
        verification_reason,
    })
}

fn prepare_release_promotion(
    paths: &BundlePaths,
    requested_target_environment: Option<String>,
) -> Result<ReleasePromotionPreparationResult> {
    let status = build_installer_status(paths)?;
    let current_environment = status
        .release_info
        .as_ref()
        .and_then(|value| value.deployment_environment.clone());
    let default_target = status.next_promotion_target_environment.clone();
    let requested_target_environment =
        requested_target_environment.map(|value| normalize_environment_name(&value));
    let target_environment = requested_target_environment.or(default_target);

    let (promotion_readiness, promotion_reason) = match &target_environment {
        Some(target) => {
            if status.next_promotion_target_environment.as_deref() != Some(target.as_str()) {
                (
                    "blocked".to_string(),
                    format!(
                        "requested target environment {target} does not match the next allowed promotion target"
                    ),
                )
            } else {
                (
                    status.next_promotion_readiness.clone(),
                    status.next_promotion_reason.clone(),
                )
            }
        }
        None => (
            status.next_promotion_readiness.clone(),
            status.next_promotion_reason.clone(),
        ),
    };

    Ok(ReleasePromotionPreparationResult {
        current_release_id: status.current_release_id,
        current_environment,
        target_environment,
        promotion_policy_name: status.promotion_policy_name,
        promotion_ring_order: status.promotion_ring_order,
        promotion_bundle_name: status.promotion_bundle_name,
        promotion_group: status.promotion_group,
        promotion_readiness,
        promotion_reason,
        required_ready_streak_ms: status.next_promotion_required_ready_streak_ms,
        current_ready_streak_ms: status.next_promotion_current_ready_streak_ms,
    })
}

fn build_promoted_bundle_version(
    release_info: &HostReleaseInfo,
    target_environment: &str,
) -> String {
    let release_channel = release_info
        .release_channel
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let built_at_unix_ms = release_info.built_at_unix_ms.unwrap_or_else(now_unix_ms);
    let source_commit_short = release_info
        .source_commit_short
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    format!("{target_environment}.{release_channel}.{built_at_unix_ms}.{source_commit_short}")
}

fn apply_release_promotion(
    paths: &BundlePaths,
    requested_target_environment: Option<String>,
) -> Result<ReleasePromotionApplyResult> {
    let started_at_unix_ms = now_unix_ms();
    let preparation = prepare_release_promotion(paths, requested_target_environment)?;
    let Some(target_environment) = preparation.target_environment.clone() else {
        bail!("release promotion has no eligible target environment");
    };

    if !preparation
        .promotion_readiness
        .eq_ignore_ascii_case("ready")
    {
        bail!(
            "release promotion is not ready: {}",
            preparation.promotion_reason
        );
    }

    let mut release_info =
        read_release_info(paths)?.ok_or_else(|| anyhow!("release metadata is missing"))?;
    let previous_release_id = Some(derive_release_id(&release_info));
    let previous_environment = release_info.deployment_environment.clone();
    let normalized_target_environment = normalize_environment_name(&target_environment);

    release_info.deployment_environment = Some(normalized_target_environment.clone());
    release_info.bundle_version = Some(build_promoted_bundle_version(
        &release_info,
        &normalized_target_environment,
    ));
    write_release_info(paths, &release_info)?;

    let success_reason = format!(
        "release promotion advanced build into {normalized_target_environment} using policy {}",
        preparation.promotion_policy_name
    );
    let release_upgrade_state = build_release_upgrade_state(
        Some(&release_info),
        "promote",
        "succeeded",
        &success_reason,
        None,
        started_at_unix_ms,
        Some(now_unix_ms()),
    );
    write_release_upgrade_state(paths, &release_upgrade_state)?;
    append_release_history(
        paths,
        &build_release_history_entry(Some(&release_info), &release_upgrade_state),
    )?;

    Ok(ReleasePromotionApplyResult {
        previous_release_id,
        promoted_release_id: derive_release_id(&release_info),
        previous_environment,
        target_environment: normalized_target_environment,
        promotion_policy_name: preparation.promotion_policy_name,
        promotion_bundle_name: preparation.promotion_bundle_name,
        promotion_group: preparation.promotion_group,
        release_info,
        release_upgrade_state,
    })
}

fn verify_post_apply_health(paths: &BundlePaths, timeout: Duration) -> Result<String> {
    let started_at = Instant::now();

    loop {
        let current_reason = match build_installer_status(paths) {
            Ok(status) => {
                if status.health_grade.eq_ignore_ascii_case("healthy")
                    && status.lifecycle_phase.eq_ignore_ascii_case("ready")
                    && status.required_processes_ready
                    && status.local_http_ready
                {
                    return Ok(
                        "bundle is healthy, ready, and passing required process/local HTTP checks"
                            .to_string(),
                    );
                }

                format!(
                    "health={} lifecycle={} required_processes_ready={} local_http_ready={}",
                    status.health_grade,
                    status.lifecycle_phase,
                    status.required_processes_ready,
                    status.local_http_ready
                )
            }
            Err(err) => {
                format!("status check failed: {err:#}")
            }
        };

        if started_at.elapsed() >= timeout {
            bail!(
                "post-apply verification did not reach healthy/ready within {}s ({})",
                timeout.as_secs(),
                current_reason
            );
        }

        sleep(Duration::from_secs(2));
    }
}

fn restore_release_snapshot(paths: &BundlePaths, snapshot: &ReleaseSnapshotManifest) -> Result<()> {
    let snapshot_root = paths.release_snapshots_root.join(&snapshot.snapshot_id);
    let service_status = query_service_status(paths)?;
    let service_was_running =
        service_status.contains("STATE") && service_status.contains("RUNNING");

    if service_was_running {
        run_supervisor_command(paths, "stop-bundle")?;
        wait_for_bundle_process_state(paths, false, Duration::from_secs(45))?;
    }

    restore_optional_file(&snapshot_root.join("config.json"), &paths.config_path)?;
    restore_optional_file(&snapshot_root.join("data.json"), &paths.data_path)?;
    restore_optional_file(
        &snapshot_root.join("selected_sunshine_runtime.txt"),
        &paths.selected_runtime_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("hard_reset_mode.txt"),
        &paths.hard_reset_mode_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("release_info.json"),
        &paths.release_info_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("promotion_policy.json"),
        &paths.promotion_policy_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("release_gate_summary.json"),
        &paths.release_gate_summary_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("release_gate_history.json"),
        &paths.release_gate_history_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("release_upgrade_state.json"),
        &paths.release_upgrade_state_path,
    )?;
    restore_optional_file(
        &snapshot_root.join("release_history.json"),
        &paths.release_history_path,
    )?;
    restore_optional_directory(&snapshot_root.join("static"), &paths.static_root)?;

    if service_was_running {
        service_control(paths, "start")?;
        sleep(Duration::from_secs(8));
        verify_startup(paths)?;
        verify_post_apply_health(paths, Duration::from_secs(30))?;
    }

    Ok(())
}

fn apply_release_upgrade(paths: &BundlePaths) -> Result<ReleaseUpgradeApplyResult> {
    let started_at_unix_ms = now_unix_ms();
    let preparation = prepare_release_upgrade(paths)?;
    let release_info = read_release_info(paths)?;
    let snapshot_manifests = read_release_snapshot_manifests(paths)?;
    let snapshot_manifest = match preparation.snapshot.as_ref() {
        Some(snapshot) => snapshot_manifests
            .iter()
            .find(|item| item.snapshot_id == snapshot.snapshot_id)
            .cloned(),
        None => snapshot_manifests.first().cloned(),
    };
    let snapshot_id = snapshot_manifest
        .as_ref()
        .map(|item| item.snapshot_id.clone());

    let apply_result = (|| -> Result<String> {
        let _ = run_supervisor_command(paths, "stop-bundle");
        sleep(Duration::from_secs(2));
        run_supervisor_command(paths, "start-bundle")?;
        sleep(Duration::from_secs(5));
        verify_startup(paths)?;
        verify_post_apply_health(paths, Duration::from_secs(30))
    })();

    match apply_result {
        Ok(verification_reason) => {
            let diagnostic_pack_suffix =
                match record_post_apply_diagnostic_pack(paths, &verification_reason) {
                    Ok(summary) => format!(
                        "; diagnostic pack {} recorded for active build",
                        summary.pack_name
                    ),
                    Err(err) => format!("; post-apply diagnostic pack recording failed ({err:#})"),
                };
            let success_reason = if preparation.snapshot.is_some() {
                format!(
                    "release upgrade applied, rollback snapshot captured, and post-apply verification passed ({verification_reason}){diagnostic_pack_suffix}"
                )
            } else {
                format!(
                    "release upgrade applied and post-apply verification passed ({verification_reason}){diagnostic_pack_suffix}"
                )
            };
            let release_upgrade_state = build_release_upgrade_state(
                release_info.as_ref(),
                "apply",
                "succeeded",
                &success_reason,
                snapshot_id,
                started_at_unix_ms,
                Some(now_unix_ms()),
            );
            write_release_upgrade_state(paths, &release_upgrade_state)?;
            append_release_history(
                paths,
                &build_release_history_entry(release_info.as_ref(), &release_upgrade_state),
            )?;
            Ok(ReleaseUpgradeApplyResult {
                migration_readiness: "ready".to_string(),
                migration_reason: release_upgrade_state.last_reason.clone(),
                snapshot: preparation.snapshot,
                release_upgrade_state,
                post_apply_verification_status: "passed".to_string(),
                post_apply_verification_reason: verification_reason,
                auto_rollback_performed: false,
            })
        }
        Err(err) => {
            let failure_reason = format!("{err:#}");

            if let Some(snapshot_manifest) = snapshot_manifest.as_ref() {
                match restore_release_snapshot(paths, snapshot_manifest) {
                    Ok(()) => {
                        let release_upgrade_state = build_release_upgrade_state(
                            release_info.as_ref(),
                            "apply",
                            "rolled_back",
                            &format!(
                                "post-apply verification failed ({failure_reason}); rollback snapshot restored successfully"
                            ),
                            snapshot_id,
                            started_at_unix_ms,
                            Some(now_unix_ms()),
                        );
                        write_release_upgrade_state(paths, &release_upgrade_state)?;
                        append_release_history(
                            paths,
                            &build_release_history_entry(
                                release_info.as_ref(),
                                &release_upgrade_state,
                            ),
                        )?;
                        return Ok(ReleaseUpgradeApplyResult {
                            migration_readiness: "blocked".to_string(),
                            migration_reason: release_upgrade_state.last_reason.clone(),
                            snapshot: preparation.snapshot,
                            release_upgrade_state,
                            post_apply_verification_status: "failed".to_string(),
                            post_apply_verification_reason: failure_reason,
                            auto_rollback_performed: true,
                        });
                    }
                    Err(rollback_err) => {
                        let release_upgrade_state = build_release_upgrade_state(
                            release_info.as_ref(),
                            "apply",
                            "failed",
                            &format!(
                                "post-apply verification failed ({failure_reason}); automatic rollback also failed ({rollback_err:#})"
                            ),
                            snapshot_id,
                            started_at_unix_ms,
                            Some(now_unix_ms()),
                        );
                        let _ = write_release_upgrade_state(paths, &release_upgrade_state);
                        let _ = append_release_history(
                            paths,
                            &build_release_history_entry(
                                release_info.as_ref(),
                                &release_upgrade_state,
                            ),
                        );
                        bail!(
                            "release apply failed and automatic rollback did not succeed: {failure_reason}; rollback error: {rollback_err:#}"
                        );
                    }
                }
            }

            let release_upgrade_state = build_release_upgrade_state(
                release_info.as_ref(),
                "apply",
                "failed",
                &failure_reason,
                snapshot_id,
                started_at_unix_ms,
                Some(now_unix_ms()),
            );
            let _ = write_release_upgrade_state(paths, &release_upgrade_state);
            let _ = append_release_history(
                paths,
                &build_release_history_entry(release_info.as_ref(), &release_upgrade_state),
            );
            Err(err)
        }
    }
}

fn promote_release_metadata(
    paths: &BundlePaths,
    deployment_environment: &str,
    release_channel: &str,
    source_branch: &str,
    source_commit: &str,
    source_commit_short: &str,
    source_dirty: bool,
    build_profile: &str,
    built_at_unix_ms: Option<u64>,
) -> Result<ReleaseMetadataPromotionResult> {
    let normalized_environment = normalize_environment_name(deployment_environment);
    let built_at_unix_ms = built_at_unix_ms.unwrap_or_else(now_unix_ms);
    let bundle_version = format!(
        "{normalized_environment}.{release_channel}.{built_at_unix_ms}.{source_commit_short}"
    );
    let build_id = format!("{built_at_unix_ms}-{source_commit_short}");
    let release_info = HostReleaseInfo {
        schema_version: 1,
        deployment_environment: Some(normalized_environment),
        release_channel: Some(release_channel.to_string()),
        bundle_version: Some(bundle_version),
        build_id: Some(build_id),
        source_branch: Some(source_branch.to_string()),
        source_commit: Some(source_commit.to_string()),
        source_commit_short: Some(source_commit_short.to_string()),
        source_dirty,
        build_profile: Some(build_profile.to_string()),
        built_at_unix_ms: Some(built_at_unix_ms),
    };
    write_release_info(paths, &release_info)?;
    let current_release_id = derive_release_id(&release_info);

    Ok(ReleaseMetadataPromotionResult {
        release_info,
        current_release_id,
    })
}

fn rollback_latest_release_snapshot(paths: &BundlePaths) -> Result<()> {
    let manifests = read_release_snapshot_manifests(paths)?;
    let Some(snapshot) = manifests.first() else {
        bail!(
            "no release snapshots found in {}",
            paths.release_snapshots_root.display()
        );
    };
    restore_release_snapshot(paths, snapshot)?;

    let restored_release_info = read_release_info(paths)?;
    let rollback_state = build_release_upgrade_state(
        restored_release_info.as_ref(),
        "rollback",
        "succeeded",
        "rollback snapshot restored and post-rollback verification passed",
        Some(snapshot.snapshot_id.clone()),
        now_unix_ms(),
        Some(now_unix_ms()),
    );
    write_release_upgrade_state(paths, &rollback_state)?;
    append_release_history(
        paths,
        &build_release_history_entry(restored_release_info.as_ref(), &rollback_state),
    )?;

    Ok(())
}

fn infer_release_gate_profile(
    gate_name: &str,
    duration_ms: Option<u64>,
    explicit_profile: Option<&str>,
) -> Option<String> {
    let normalized = explicit_profile
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if let Some(value) = normalized {
        return Some(match value.as_str() {
            "smoke" | "smoke-60s" | "smoke_60s" => "smoke-60s".to_string(),
            "10m" | "standard-10m" | "ten-minute" | "ten_minute" => "standard-10m".to_string(),
            "30m" | "endurance-30m" | "endurance_30m" => "endurance-30m".to_string(),
            "longhaul" | "long-haul" | "long_haul" | "longhaul-60m" => "longhaul".to_string(),
            _ => value,
        });
    }

    let normalized_gate_name = gate_name.to_ascii_lowercase();
    let gate_duration_ms = duration_ms.unwrap_or(0);

    if normalized_gate_name.contains("smoke")
        || (gate_duration_ms > 0 && gate_duration_ms <= 60_000)
    {
        return Some("smoke-60s".to_string());
    }
    if normalized_gate_name.contains("longhaul") || gate_duration_ms > 1_800_000 {
        return Some("longhaul".to_string());
    }
    if normalized_gate_name.contains("30m") || gate_duration_ms > 600_000 {
        return Some("endurance-30m".to_string());
    }
    if gate_duration_ms > 0 {
        return Some("standard-10m".to_string());
    }

    None
}

fn infer_release_gate_scenario(explicit_scenario: Option<&str>) -> Option<String> {
    explicit_scenario
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "mixed")
}

struct ReleaseGateThresholds {
    max_stall_recoveries: u32,
    min_effective_presented_fps: f64,
    max_frame_advance_failures: u32,
    max_gameplay_degrade_count: u32,
    max_play_estimate_ms: f64,
    max_effective_buffer_ms: f64,
}

fn collect_positive_fps_samples(values: &[f64]) -> Vec<f64> {
    values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect()
}

fn get_measured_output_fps_floor(
    profile_floor: f64,
    avg_receiver_fps: f64,
    avg_streamer_output_fps: f64,
) -> f64 {
    let samples = collect_positive_fps_samples(&[avg_receiver_fps, avg_streamer_output_fps]);
    if samples.is_empty() {
        return profile_floor;
    }

    let observed_floor = samples.into_iter().fold(f64::INFINITY, f64::min);
    let dynamic_floor = (observed_floor - 2.0).floor().max(20.0);
    profile_floor.min(dynamic_floor)
}

fn get_smoke_presented_fps_floor(avg_receiver_fps: f64, avg_streamer_output_fps: f64) -> f64 {
    let samples = collect_positive_fps_samples(&[avg_receiver_fps, avg_streamer_output_fps]);
    if samples.is_empty() {
        return 20.0;
    }

    let observed_floor = samples.into_iter().fold(f64::INFINITY, f64::min);
    if observed_floor >= 28.0 {
        return 20.0;
    }
    if observed_floor >= 24.0 {
        return 18.0;
    }
    if observed_floor >= 20.0 {
        return 16.0;
    }
    14.0
}

fn get_release_gate_thresholds(summary: &HostReleaseGateSummary) -> ReleaseGateThresholds {
    let scenario = summary
        .gate_scenario
        .as_deref()
        .unwrap_or("mixed")
        .trim()
        .to_ascii_lowercase();
    let scenario_frame_budget = match scenario.as_str() {
        "mixed" | "startup-recovery" => 0,
        "reconnect" => 6,
        _ => 4,
    };
    let live_output_floor = get_measured_output_fps_floor(
        24.0,
        summary.avg_receiver_fps,
        summary.avg_streamer_output_fps,
    );
    let relaxed_motion_budget = if live_output_floor < 24.0 {
        Some(
            if summary.avg_receiver_fps >= 20.0 || summary.avg_streamer_output_fps >= 20.0 {
                10
            } else {
                6
            } + scenario_frame_budget,
        )
    } else {
        None
    };
    let recovery_frame_budget = |max_stall_recoveries: u32| -> u32 {
        summary
            .stall_recoveries
            .min(max_stall_recoveries)
            .saturating_mul(2)
    };

    match summary
        .gate_profile
        .as_deref()
        .unwrap_or("smoke-60s")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "smoke-60s" => {
            let max_stall_recoveries = 1;
            ReleaseGateThresholds {
                max_stall_recoveries,
                min_effective_presented_fps: get_smoke_presented_fps_floor(
                    summary.avg_receiver_fps,
                    summary.avg_streamer_output_fps,
                ),
                max_frame_advance_failures: (if summary.avg_receiver_fps >= 20.0
                    || summary.avg_streamer_output_fps >= 20.0
                {
                    8
                } else {
                    4
                }) + scenario_frame_budget
                    + recovery_frame_budget(max_stall_recoveries),
                max_gameplay_degrade_count: 1,
                max_play_estimate_ms: 150.0,
                max_effective_buffer_ms: 80.0,
            }
        }
        "endurance-30m" => {
            let max_stall_recoveries = 2;
            ReleaseGateThresholds {
                max_stall_recoveries,
                min_effective_presented_fps: live_output_floor,
                max_frame_advance_failures: relaxed_motion_budget
                    .unwrap_or(4 + scenario_frame_budget)
                    + recovery_frame_budget(max_stall_recoveries),
                max_gameplay_degrade_count: 1,
                max_play_estimate_ms: 110.0,
                max_effective_buffer_ms: 65.0,
            }
        }
        "longhaul" => {
            let max_stall_recoveries = 3;
            ReleaseGateThresholds {
                max_stall_recoveries,
                min_effective_presented_fps: live_output_floor,
                max_frame_advance_failures: relaxed_motion_budget
                    .unwrap_or(6 + scenario_frame_budget)
                    + recovery_frame_budget(max_stall_recoveries),
                max_gameplay_degrade_count: 2,
                max_play_estimate_ms: 120.0,
                max_effective_buffer_ms: 70.0,
            }
        }
        _ => {
            let max_stall_recoveries = 1;
            ReleaseGateThresholds {
                max_stall_recoveries,
                min_effective_presented_fps: live_output_floor,
                max_frame_advance_failures: relaxed_motion_budget
                    .unwrap_or(2 + scenario_frame_budget)
                    + recovery_frame_budget(max_stall_recoveries),
                max_gameplay_degrade_count: 1,
                max_play_estimate_ms: 95.0,
                max_effective_buffer_ms: 55.0,
            }
        }
    }
}

fn evaluate_release_gate_quality(summary: &HostReleaseGateSummary) -> Option<String> {
    if summary.route_lost_count > 0 {
        return Some(format!(
            "release gate quality threshold failed: route lost {} time(s)",
            summary.route_lost_count
        ));
    }
    if summary.reconnect_count > 0 {
        return Some(format!(
            "release gate quality threshold failed: reconnect triggered {} time(s)",
            summary.reconnect_count
        ));
    }

    let thresholds = get_release_gate_thresholds(summary);

    if summary.stall_recoveries > thresholds.max_stall_recoveries {
        return Some(format!(
            "release gate quality threshold failed: stall recovery triggered {} time(s) above profile limit {}",
            summary.stall_recoveries, thresholds.max_stall_recoveries
        ));
    }

    if summary.frame_advance_failures > thresholds.max_frame_advance_failures {
        return Some(format!(
            "release gate quality threshold failed: frame advance failures {} exceeded profile limit {}",
            summary.frame_advance_failures, thresholds.max_frame_advance_failures
        ));
    }
    if summary.effective_presented_fps > 0.0
        && summary.effective_presented_fps < thresholds.min_effective_presented_fps
    {
        return Some(format!(
            "release gate quality threshold failed: effective fps {:.2} is below required {:.2}",
            summary.effective_presented_fps, thresholds.min_effective_presented_fps
        ));
    }
    if summary.gameplay_degrade_count > thresholds.max_gameplay_degrade_count {
        return Some(format!(
            "release gate quality threshold failed: gameplay degrade count {} exceeded profile limit {}",
            summary.gameplay_degrade_count, thresholds.max_gameplay_degrade_count
        ));
    }
    if let Some(max_play_estimate_ms) = summary.max_play_estimate_ms {
        if max_play_estimate_ms > thresholds.max_play_estimate_ms {
            return Some(format!(
                "release gate quality threshold failed: max play estimate {:.2} ms exceeded profile limit {:.2} ms",
                max_play_estimate_ms, thresholds.max_play_estimate_ms
            ));
        }
    }
    if let Some(max_effective_buffer_ms) = summary.max_effective_buffer_ms {
        if max_effective_buffer_ms > thresholds.max_effective_buffer_ms {
            return Some(format!(
                "release gate quality threshold failed: max effective buffer {:.2} ms exceeded profile limit {:.2} ms",
                max_effective_buffer_ms, thresholds.max_effective_buffer_ms
            ));
        }
    }

    None
}

fn record_release_gate(
    paths: &BundlePaths,
    gate_name: &str,
    gate_profile: Option<String>,
    gate_scenario: Option<String>,
    status: &str,
    summary_path: Option<PathBuf>,
    reason: Option<String>,
) -> Result<HostReleaseGateSummary> {
    let checked_at_unix_ms = now_unix_ms();
    let release_info = read_release_info(paths)?;
    let input = match summary_path {
        Some(path) => {
            let raw = fs::read_to_string(&path).with_context(|| {
                format!("failed to read release gate summary {}", path.display())
            })?;
            let raw = raw.trim_start_matches('\u{feff}');
            Some(
                serde_json::from_str::<ReleaseGateInputSummary>(&raw).with_context(|| {
                    format!("failed to parse release gate summary {}", path.display())
                })?,
            )
        }
        None => None,
    };

    let normalized_status = if status.eq_ignore_ascii_case("passed") {
        "passed"
    } else {
        "failed"
    };
    let gate_reason = reason.unwrap_or_else(|| {
        if normalized_status == "passed" {
            "release gate completed successfully".to_string()
        } else {
            "release gate execution failed".to_string()
        }
    });
    let normalized_profile = infer_release_gate_profile(
        gate_name,
        input.as_ref().and_then(|value| value.duration_ms),
        gate_profile.as_deref().or_else(|| {
            input
                .as_ref()
                .and_then(|value| value.gate_profile.as_deref())
        }),
    );
    let normalized_scenario = infer_release_gate_scenario(gate_scenario.as_deref().or_else(|| {
        input
            .as_ref()
            .and_then(|value| value.gate_scenario.as_deref())
    }));

    let mut summary = HostReleaseGateSummary {
        schema_version: 1,
        gate_name: gate_name.to_string(),
        gate_profile: normalized_profile,
        gate_scenario: normalized_scenario,
        gate_status: normalized_status.to_string(),
        gate_reason,
        checked_at_unix_ms,
        duration_ms: input
            .as_ref()
            .and_then(|value| value.duration_ms)
            .unwrap_or(0),
        source_commit_short: release_info
            .as_ref()
            .and_then(|value| value.source_commit_short.clone()),
        built_at_unix_ms: release_info
            .as_ref()
            .and_then(|value| value.built_at_unix_ms),
        route_lost_count: input
            .as_ref()
            .and_then(|value| value.route_lost_count)
            .unwrap_or(0),
        reconnect_count: input
            .as_ref()
            .and_then(|value| value.reconnect_count)
            .unwrap_or(0),
        stall_recoveries: input
            .as_ref()
            .and_then(|value| value.stall_recoveries)
            .unwrap_or(0),
        gameplay_degrade_count: input
            .as_ref()
            .and_then(|value| value.gameplay_degrade_count)
            .unwrap_or(0),
        frame_advance_failures: input
            .as_ref()
            .and_then(|value| value.frame_advance_failures)
            .unwrap_or(0),
        effective_presented_fps: input
            .as_ref()
            .and_then(|value| value.effective_presented_fps)
            .unwrap_or(0.0),
        avg_streamer_output_fps: input
            .as_ref()
            .and_then(|value| value.avg_streamer_output_fps)
            .unwrap_or(0.0),
        min_streamer_output_fps: input
            .as_ref()
            .and_then(|value| value.min_streamer_output_fps),
        avg_receiver_fps: input
            .as_ref()
            .and_then(|value| value.avg_receiver_fps)
            .unwrap_or(0.0),
        min_receiver_fps: input.as_ref().and_then(|value| value.min_receiver_fps),
        max_play_estimate_ms: input.as_ref().and_then(|value| value.max_play_estimate_ms),
        max_effective_buffer_ms: input
            .as_ref()
            .and_then(|value| value.max_effective_buffer_ms),
        max_jitter_buffer_delay_ms: input
            .as_ref()
            .and_then(|value| value.max_jitter_buffer_delay_ms)
            .unwrap_or(0.0),
        max_decode_time_ms: input
            .as_ref()
            .and_then(|value| value.max_decode_time_ms)
            .unwrap_or(0.0),
        max_processing_delay_ms: input
            .as_ref()
            .and_then(|value| value.max_processing_delay_ms)
            .unwrap_or(0.0),
        frames_dropped_delta: input
            .as_ref()
            .and_then(|value| value.frames_dropped_delta)
            .unwrap_or(0),
        nack_count_delta: input
            .as_ref()
            .and_then(|value| value.nack_count_delta)
            .unwrap_or(0),
        freeze_count_delta: input
            .as_ref()
            .and_then(|value| value.freeze_count_delta)
            .unwrap_or(0),
        final_route_title: input
            .as_ref()
            .and_then(|value| value.final_route_title.clone()),
        final_route_note: input
            .as_ref()
            .and_then(|value| value.final_route_note.clone()),
        final_receiver_route: input
            .as_ref()
            .and_then(|value| value.final_receiver_route.clone()),
        support_bundle_id: None,
    };

    if normalized_status.eq_ignore_ascii_case("passed") {
        if let Some(quality_failure_reason) = evaluate_release_gate_quality(&summary) {
            summary.gate_status = "failed".to_string();
            summary.gate_reason = quality_failure_reason;
        }
    }

    fs::write(
        &paths.release_gate_summary_path,
        serde_json::to_string_pretty(&summary)?,
    )
    .with_context(|| {
        format!(
            "failed to write release gate summary {}",
            paths.release_gate_summary_path.display()
        )
    })?;
    append_release_gate_history(paths, &summary)?;

    if !summary.gate_status.eq_ignore_ascii_case("passed") {
        let support_bundle = collect_support_bundle(paths)?;
        summary.support_bundle_id = Some(support_bundle.support_bundle_id);
        fs::write(
            &paths.release_gate_summary_path,
            serde_json::to_string_pretty(&summary)?,
        )
        .with_context(|| {
            format!(
                "failed to rewrite release gate summary {}",
                paths.release_gate_summary_path.display()
            )
        })?;
        append_release_gate_history(paths, &summary)?;
    }

    Ok(summary)
}

fn build_diagnostic_pack_summary(
    paths: &BundlePaths,
    pack_name: &str,
    status: &str,
    input: Option<&DiagnosticPackInputSummary>,
    reason: Option<String>,
) -> Result<HostDiagnosticPackSummary> {
    let checked_at_unix_ms = now_unix_ms();
    let release_info = read_release_info(paths)?;
    let normalized_status = if status.eq_ignore_ascii_case("passed") {
        "passed"
    } else {
        "failed"
    };
    let pack_reason = reason.unwrap_or_else(|| {
        if normalized_status == "passed" {
            "diagnostic pack completed successfully".to_string()
        } else {
            "diagnostic pack execution failed".to_string()
        }
    });

    Ok(HostDiagnosticPackSummary {
        schema_version: 1,
        pack_name: pack_name.to_string(),
        gate_profile: input.and_then(|value| value.gate_profile.clone()),
        gate_scenario: infer_release_gate_scenario(
            input.and_then(|value| value.gate_scenario.as_deref()),
        ),
        pack_status: normalized_status.to_string(),
        pack_reason,
        checked_at_unix_ms,
        duration_ms: input.and_then(|value| value.duration_ms).unwrap_or(0),
        source_commit_short: release_info
            .as_ref()
            .and_then(|value| value.source_commit_short.clone()),
        built_at_unix_ms: release_info
            .as_ref()
            .and_then(|value| value.built_at_unix_ms),
        verify_startup_status: input
            .and_then(|value| value.verify_startup_status.clone())
            .unwrap_or_else(|| {
                if normalized_status == "passed" {
                    "passed".to_string()
                } else {
                    "unknown".to_string()
                }
            }),
        verify_startup_reason: input.and_then(|value| value.verify_startup_reason.clone()),
        gate_exit_code: input
            .and_then(|value| value.gate_exit_code)
            .unwrap_or(if normalized_status == "passed" { 0 } else { 1 }),
        failure_step: input.and_then(|value| value.failure_step.clone()),
        release_gate_status: input.and_then(|value| value.release_gate_status.clone()),
        release_gate_reason: input.and_then(|value| value.release_gate_reason.clone()),
        release_gate_name: input.and_then(|value| value.release_gate_name.clone()),
        support_bundle_id: input.and_then(|value| value.support_bundle_id.clone()),
        health_grade_before: input.and_then(|value| value.health_grade_before.clone()),
        health_grade_after: input.and_then(|value| value.health_grade_after.clone()),
        lifecycle_before: input.and_then(|value| value.lifecycle_before.clone()),
        lifecycle_after: input.and_then(|value| value.lifecycle_after.clone()),
    })
}

fn persist_diagnostic_pack_summary(
    paths: &BundlePaths,
    summary: &HostDiagnosticPackSummary,
) -> Result<()> {
    fs::write(
        &paths.diagnostic_pack_summary_path,
        serde_json::to_string_pretty(summary)?,
    )
    .with_context(|| {
        format!(
            "failed to write diagnostic pack summary {}",
            paths.diagnostic_pack_summary_path.display()
        )
    })?;
    append_diagnostic_pack_history(paths, summary)?;
    Ok(())
}

fn record_diagnostic_pack(
    paths: &BundlePaths,
    pack_name: &str,
    status: &str,
    summary_path: Option<PathBuf>,
    reason: Option<String>,
) -> Result<HostDiagnosticPackSummary> {
    let input = match summary_path {
        Some(path) => {
            let raw = fs::read_to_string(&path).with_context(|| {
                format!("failed to read diagnostic pack summary {}", path.display())
            })?;
            let raw = raw.trim_start_matches('\u{feff}');
            Some(
                serde_json::from_str::<DiagnosticPackInputSummary>(&raw).with_context(|| {
                    format!("failed to parse diagnostic pack summary {}", path.display())
                })?,
            )
        }
        None => None,
    };

    let mut summary =
        build_diagnostic_pack_summary(paths, pack_name, status, input.as_ref(), reason)?;

    persist_diagnostic_pack_summary(paths, &summary)?;

    if !summary.pack_status.eq_ignore_ascii_case("passed") && summary.support_bundle_id.is_none() {
        let support_bundle = collect_support_bundle(paths)?;
        summary.support_bundle_id = Some(support_bundle.support_bundle_id);
        persist_diagnostic_pack_summary(paths, &summary)?;
    }

    Ok(summary)
}

fn record_post_apply_diagnostic_pack(
    paths: &BundlePaths,
    verification_reason: &str,
) -> Result<HostDiagnosticPackSummary> {
    let current_status = build_installer_status(paths)?;
    let release_info = current_status.release_info.as_ref();
    let release_gate_summary = current_status.release_gate_summary.as_ref();
    let (release_gate_status, release_gate_reason) =
        derive_release_gate_status(release_info, release_gate_summary);
    let input = DiagnosticPackInputSummary {
        gate_profile: release_gate_summary.and_then(|value| value.gate_profile.clone()),
        gate_scenario: release_gate_summary.and_then(|value| value.gate_scenario.clone()),
        requested_duration_ms: None,
        gate_exit_code: Some(0),
        failure_step: None,
        failure_reason: None,
        support_bundle_id: None,
        health_grade_before: Some(current_status.health_grade.clone()),
        health_grade_after: Some(current_status.health_grade.clone()),
        lifecycle_before: Some(current_status.lifecycle_phase.clone()),
        lifecycle_after: Some(current_status.lifecycle_phase.clone()),
        release_gate_status: Some(release_gate_status),
        release_gate_reason: Some(release_gate_reason),
        release_gate_name: release_gate_summary.map(|value| value.gate_name.clone()),
        verify_startup_status: Some("passed".to_string()),
        verify_startup_reason: Some(verification_reason.to_string()),
        started_at_unix_ms: None,
        completed_at_unix_ms: Some(now_unix_ms()),
        duration_ms: Some(0),
    };
    let summary = build_diagnostic_pack_summary(
        paths,
        "post-apply",
        "passed",
        Some(&input),
        Some("post-apply verification recorded successfully".to_string()),
    )?;
    persist_diagnostic_pack_summary(paths, &summary)?;
    Ok(summary)
}

fn build_release_upgrade_state(
    release_info: Option<&HostReleaseInfo>,
    last_action: &str,
    last_status: &str,
    last_reason: &str,
    snapshot_id: Option<String>,
    started_at_unix_ms: u64,
    completed_at_unix_ms: Option<u64>,
) -> HostReleaseUpgradeState {
    HostReleaseUpgradeState {
        schema_version: 1,
        last_action: last_action.to_string(),
        last_status: last_status.to_string(),
        last_reason: last_reason.to_string(),
        snapshot_id,
        started_at_unix_ms,
        completed_at_unix_ms,
        bundle_version: release_info.and_then(|value| value.bundle_version.clone()),
        build_id: release_info.and_then(|value| value.build_id.clone()),
        source_commit_short: release_info.and_then(|value| value.source_commit_short.clone()),
        deployment_environment: release_info.and_then(|value| value.deployment_environment.clone()),
        release_channel: release_info.and_then(|value| value.release_channel.clone()),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_runtime_adoption_state(
    last_action: &str,
    last_status: &str,
    last_reason: &str,
    previous_runtime_key: Option<String>,
    previous_runtime_display_name: Option<String>,
    adopted_runtime_key: Option<String>,
    adopted_runtime_directory: Option<String>,
    adopted_runtime_display_name: Option<String>,
    adopted_runtime_version: Option<String>,
    switch_required: bool,
    changed: bool,
    reverted: bool,
    started_at_unix_ms: u64,
    completed_at_unix_ms: Option<u64>,
    support_bundle_id: Option<String>,
) -> HostRuntimeAdoptionState {
    HostRuntimeAdoptionState {
        schema_version: 1,
        last_action: last_action.to_string(),
        last_status: last_status.to_string(),
        last_reason: last_reason.to_string(),
        previous_runtime_key,
        previous_runtime_display_name,
        adopted_runtime_key,
        adopted_runtime_directory,
        adopted_runtime_display_name,
        adopted_runtime_version,
        switch_required,
        changed,
        reverted,
        started_at_unix_ms,
        completed_at_unix_ms,
        support_bundle_id,
    }
}

fn build_runtime_adoption_history_entry(
    state: &HostRuntimeAdoptionState,
) -> HostRuntimeAdoptionHistoryEntry {
    HostRuntimeAdoptionHistoryEntry {
        schema_version: 1,
        action: state.last_action.clone(),
        status: state.last_status.clone(),
        reason: state.last_reason.clone(),
        previous_runtime_key: state.previous_runtime_key.clone(),
        previous_runtime_display_name: state.previous_runtime_display_name.clone(),
        adopted_runtime_key: state.adopted_runtime_key.clone(),
        adopted_runtime_directory: state.adopted_runtime_directory.clone(),
        adopted_runtime_display_name: state.adopted_runtime_display_name.clone(),
        adopted_runtime_version: state.adopted_runtime_version.clone(),
        switch_required: state.switch_required,
        changed: state.changed,
        reverted: state.reverted,
        started_at_unix_ms: state.started_at_unix_ms,
        completed_at_unix_ms: state.completed_at_unix_ms,
        support_bundle_id: state.support_bundle_id.clone(),
    }
}

fn derive_release_id(release_info: &HostReleaseInfo) -> String {
    if let Some(bundle_version) = release_info
        .bundle_version
        .clone()
        .filter(|value| !value.is_empty())
    {
        return bundle_version;
    }

    let environment = release_info
        .deployment_environment
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let channel = release_info
        .release_channel
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let commit = release_info
        .source_commit_short
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let built = release_info
        .built_at_unix_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("{environment}/{channel}@{commit}:{built}")
}

fn build_release_history_entry(
    release_info: Option<&HostReleaseInfo>,
    state: &HostReleaseUpgradeState,
) -> HostReleaseHistoryEntry {
    let release_id = release_info
        .map(derive_release_id)
        .unwrap_or_else(|| "unknown/unknown@unknown:unknown".to_string());
    HostReleaseHistoryEntry {
        schema_version: 1,
        release_id,
        action: state.last_action.clone(),
        status: state.last_status.clone(),
        reason: state.last_reason.clone(),
        snapshot_id: state.snapshot_id.clone(),
        started_at_unix_ms: state.started_at_unix_ms,
        completed_at_unix_ms: state.completed_at_unix_ms,
        bundle_version: state.bundle_version.clone(),
        build_id: state.build_id.clone(),
        source_commit_short: state.source_commit_short.clone(),
        deployment_environment: state.deployment_environment.clone(),
        release_channel: state.release_channel.clone(),
    }
}

fn write_release_upgrade_state(paths: &BundlePaths, state: &HostReleaseUpgradeState) -> Result<()> {
    fs::write(
        &paths.release_upgrade_state_path,
        serde_json::to_string_pretty(state)?,
    )
    .with_context(|| {
        format!(
            "failed to write release upgrade state {}",
            paths.release_upgrade_state_path.display()
        )
    })?;
    Ok(())
}

fn write_runtime_adoption_state(
    paths: &BundlePaths,
    state: &HostRuntimeAdoptionState,
) -> Result<()> {
    fs::write(
        &paths.runtime_adoption_state_path,
        serde_json::to_string_pretty(state)?,
    )
    .with_context(|| {
        format!(
            "failed to write runtime adoption state {}",
            paths.runtime_adoption_state_path.display()
        )
    })?;
    Ok(())
}

fn write_release_info(paths: &BundlePaths, info: &HostReleaseInfo) -> Result<()> {
    fs::write(
        &paths.release_info_path,
        serde_json::to_string_pretty(info)?,
    )
    .with_context(|| {
        format!(
            "failed to write release info {}",
            paths.release_info_path.display()
        )
    })?;
    Ok(())
}

fn append_runtime_adoption_history(
    paths: &BundlePaths,
    entry: &HostRuntimeAdoptionHistoryEntry,
) -> Result<()> {
    let mut entries = read_runtime_adoption_history(paths)?;
    entries.retain(|item| {
        !(item.action == entry.action
            && item.status == entry.status
            && item.started_at_unix_ms == entry.started_at_unix_ms
            && item.previous_runtime_key == entry.previous_runtime_key
            && item.adopted_runtime_directory == entry.adopted_runtime_directory)
    });
    entries.insert(0, entry.clone());
    if entries.len() > 10 {
        entries.truncate(10);
    }

    let document = RuntimeAdoptionHistoryDocument {
        schema_version: 1,
        entries,
    };
    fs::write(
        &paths.runtime_adoption_history_path,
        serde_json::to_string_pretty(&document)?,
    )
    .with_context(|| {
        format!(
            "failed to write runtime adoption history {}",
            paths.runtime_adoption_history_path.display()
        )
    })?;
    Ok(())
}

fn append_release_history(paths: &BundlePaths, entry: &HostReleaseHistoryEntry) -> Result<()> {
    let mut entries = read_release_history(paths)?;
    entries.retain(|item| {
        !(item.release_id == entry.release_id
            && item.action == entry.action
            && item.started_at_unix_ms == entry.started_at_unix_ms)
    });
    entries.insert(0, entry.clone());
    if entries.len() > 10 {
        entries.truncate(10);
    }

    let document = ReleaseHistoryDocument {
        schema_version: 1,
        entries,
    };
    fs::write(
        &paths.release_history_path,
        serde_json::to_string_pretty(&document)?,
    )
    .with_context(|| {
        format!(
            "failed to write release history {}",
            paths.release_history_path.display()
        )
    })?;
    Ok(())
}

fn append_release_gate_history(
    paths: &BundlePaths,
    summary: &HostReleaseGateSummary,
) -> Result<()> {
    let mut entries = read_release_gate_history(paths)?;
    entries.retain(|item| {
        !(item.gate_name == summary.gate_name
            && item.checked_at_unix_ms == summary.checked_at_unix_ms
            && item.source_commit_short == summary.source_commit_short)
    });
    entries.insert(0, summary.clone());
    if entries.len() > 20 {
        entries.truncate(20);
    }

    let document = ReleaseGateHistoryDocument {
        schema_version: 1,
        entries,
    };
    fs::write(
        &paths.release_gate_history_path,
        serde_json::to_string_pretty(&document)?,
    )
    .with_context(|| {
        format!(
            "failed to write release gate history {}",
            paths.release_gate_history_path.display()
        )
    })?;
    Ok(())
}

fn append_diagnostic_pack_history(
    paths: &BundlePaths,
    summary: &HostDiagnosticPackSummary,
) -> Result<()> {
    let mut entries = read_diagnostic_pack_history(paths)?;
    entries.retain(|item| {
        !(item.pack_name == summary.pack_name
            && item.checked_at_unix_ms == summary.checked_at_unix_ms
            && item.source_commit_short == summary.source_commit_short)
    });
    entries.insert(0, summary.clone());
    if entries.len() > 20 {
        entries.truncate(20);
    }

    let document = DiagnosticPackHistoryDocument {
        schema_version: 1,
        entries,
    };
    fs::write(
        &paths.diagnostic_pack_history_path,
        serde_json::to_string_pretty(&document)?,
    )
    .with_context(|| {
        format!(
            "failed to write diagnostic pack history {}",
            paths.diagnostic_pack_history_path.display()
        )
    })?;
    Ok(())
}

fn read_release_snapshot_manifests(paths: &BundlePaths) -> Result<Vec<ReleaseSnapshotManifest>> {
    if !paths.release_snapshots_root.exists() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();
    for entry in fs::read_dir(&paths.release_snapshots_root).with_context(|| {
        format!(
            "failed to read release snapshot root {}",
            paths.release_snapshots_root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("snapshot_manifest.json");
        if !manifest_path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest = serde_json::from_str::<ReleaseSnapshotManifest>(&raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        manifests.push(manifest);
    }

    manifests.sort_by(|a, b| b.created_at_unix_ms.cmp(&a.created_at_unix_ms));
    Ok(manifests)
}

fn read_config_state_backup_manifests(
    paths: &BundlePaths,
) -> Result<Vec<ConfigStateBackupManifest>> {
    if !paths.config_state_backups_root.exists() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();
    for entry in fs::read_dir(&paths.config_state_backups_root).with_context(|| {
        format!(
            "failed to read config backup root {}",
            paths.config_state_backups_root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("config_state_backup_manifest.json");
        if !manifest_path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest = serde_json::from_str::<ConfigStateBackupManifest>(&raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        manifests.push(manifest);
    }

    manifests.sort_by(|a, b| b.created_at_unix_ms.cmp(&a.created_at_unix_ms));
    Ok(manifests)
}

fn read_support_bundle_manifests(paths: &BundlePaths) -> Result<Vec<SupportBundleManifest>> {
    if !paths.support_bundles_root.exists() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();
    for entry in fs::read_dir(&paths.support_bundles_root).with_context(|| {
        format!(
            "failed to read support bundle root {}",
            paths.support_bundles_root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("support_bundle_manifest.json");
        if !manifest_path.exists() {
            continue;
        }

        let raw = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest = serde_json::from_str::<SupportBundleManifest>(&raw)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        manifests.push(manifest);
    }

    manifests.sort_by(|a, b| b.created_at_unix_ms.cmp(&a.created_at_unix_ms));
    Ok(manifests)
}

fn restore_config_state_backup(
    paths: &BundlePaths,
    backup: &ConfigStateBackupManifest,
) -> Result<String> {
    let backup_root = paths.config_state_backups_root.join(&backup.backup_id);
    let service_status = query_service_status(paths)?;
    let service_was_running =
        service_status.contains("STATE") && service_status.contains("RUNNING");

    if service_was_running {
        run_supervisor_command(paths, "stop-bundle")?;
        wait_for_bundle_process_state(paths, false, Duration::from_secs(45))?;
    }

    restore_optional_file(&backup_root.join("config.json"), &paths.config_path)?;
    restore_optional_file(&backup_root.join("data.json"), &paths.data_path)?;
    restore_optional_file(
        &backup_root.join("selected_sunshine_runtime.txt"),
        &paths.selected_runtime_path,
    )?;
    restore_optional_file(
        &backup_root.join("hard_reset_mode.txt"),
        &paths.hard_reset_mode_path,
    )?;
    restore_optional_file(
        &backup_root.join("promotion_policy.json"),
        &paths.promotion_policy_path,
    )?;
    restore_optional_file(
        &backup_root.join("force_legacy_nvenc.txt"),
        &paths.server_root.join("force_legacy_nvenc.txt"),
    )?;
    restore_optional_file(
        &backup_root.join("shared_pair_info.json"),
        &paths.server_root.join("shared_pair_info.json"),
    )?;
    restore_optional_file(
        &backup_root.join("dynamic_display_state.json"),
        &paths.server_root.join("dynamic_display_state.json"),
    )?;
    restore_optional_file(
        &backup_root.join("PUBLIC_URL.txt"),
        &paths.bundle_root.join("PUBLIC_URL.txt"),
    )?;
    restore_optional_file(
        &backup_root.join("frp").join("frpc.toml"),
        &paths.bundle_root.join("frp").join("frpc.toml"),
    )?;
    restore_optional_directory(
        &backup_root.join("sunshine").join("config"),
        &paths.bundle_root.join("sunshine").join("config"),
    )?;
    restore_optional_directory(
        &backup_root.join("sunshine-legacy").join("config"),
        &paths.bundle_root.join("sunshine-legacy").join("config"),
    )?;

    let verification_reason = if service_was_running {
        run_supervisor_command(paths, "start-bundle")?;
        wait_for_bundle_process_state(paths, true, Duration::from_secs(75))?;
        sleep(Duration::from_secs(8));
        verify_startup(paths)?;
        verify_post_apply_health(paths, Duration::from_secs(30))?
    } else {
        "config state restored while host service remained stopped".to_string()
    };

    Ok(verification_reason)
}

fn restore_latest_config_state(paths: &BundlePaths) -> Result<ConfigStateRestoreResult> {
    let backups = read_config_state_backup_manifests(paths)?;
    let Some(backup) = backups.first() else {
        bail!(
            "no config state backups found in {}",
            paths.config_state_backups_root.display()
        );
    };

    let restored_at_unix_ms = now_unix_ms();
    let verification_reason = restore_config_state_backup(paths, backup)?;
    Ok(ConfigStateRestoreResult {
        backup_id: backup.backup_id.clone(),
        restored_at_unix_ms,
        backup_root: paths
            .config_state_backups_root
            .join(&backup.backup_id)
            .display()
            .to_string(),
        verification_reason,
    })
}

fn copy_optional_file(source: &PathBuf, target: &PathBuf) -> Result<u32> {
    if !source.exists() {
        return Ok(0);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target).with_context(|| {
        format!(
            "failed to snapshot {} -> {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(1)
}

fn copy_optional_top_level_files_by_extension(
    source_dir: &PathBuf,
    target_dir: &PathBuf,
    extensions: &[&str],
) -> Result<u32> {
    if !source_dir.exists() {
        return Ok(0);
    }

    let mut file_count = 0u32;
    for entry in fs::read_dir(source_dir)
        .with_context(|| format!("failed to read directory {}", source_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let source_path = entry.path();
        let Some(extension) = source_path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !extensions
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            continue;
        }

        file_count += copy_optional_file(&source_path, &target_dir.join(entry.file_name()))?;
    }

    Ok(file_count)
}

fn write_json_artifact<T: Serialize>(target: &PathBuf, value: &T) -> Result<u32> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("failed to write {}", target.display()))?;
    Ok(1)
}

fn write_text_artifact(target: &PathBuf, value: &str) -> Result<u32> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, value).with_context(|| format!("failed to write {}", target.display()))?;
    Ok(1)
}

fn copy_optional_directory(source: &PathBuf, target: &PathBuf) -> Result<u32> {
    if !source.exists() {
        return Ok(0);
    }

    if target.exists() {
        fs::remove_dir_all(target)
            .with_context(|| format!("failed to clear snapshot target {}", target.display()))?;
    }
    copy_directory_recursive(source, target)?;
    Ok(1)
}

fn restore_optional_file(source: &PathBuf, target: &PathBuf) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target).with_context(|| {
        format!(
            "failed to restore {} -> {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn restore_optional_directory(source: &PathBuf, target: &PathBuf) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if target.exists() {
        fs::remove_dir_all(target)
            .with_context(|| format!("failed to clear restore target {}", target.display()))?;
    }
    copy_directory_recursive(source, target)
}

fn copy_directory_recursive(source: &PathBuf, target: &PathBuf) -> Result<()> {
    fs::create_dir_all(target)
        .with_context(|| format!("failed to create directory {}", target.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory_recursive(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "failed to copy directory entry {} -> {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn save_config(paths: &BundlePaths, config: &Config) -> Result<()> {
    let pretty = serde_json::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&paths.config_path, pretty)
        .with_context(|| format!("failed to write {}", paths.config_path.display()))
}

fn remediate_config_hygiene(paths: &BundlePaths) -> Result<HygieneRemediationResult> {
    let mut config = load_config(paths)?;
    let mut changed_paths = Vec::new();
    let mut notes = Vec::new();
    let mut config_changed = false;
    let desired_network_types = vec![
        WebRtcNetworkType::Udp4,
        WebRtcNetworkType::Udp6,
        WebRtcNetworkType::Tcp4,
        WebRtcNetworkType::Tcp6,
    ];

    if !config.web_server.session_cookie_secure {
        config.web_server.session_cookie_secure = true;
        config_changed = true;
        notes.push("enabled session_cookie_secure for HTTPS/public bundle access".to_string());
    }

    let network_types_need_repair = config.webrtc.network_types.len()
        != desired_network_types.len()
        || config
            .webrtc
            .network_types
            .iter()
            .map(ToString::to_string)
            .ne(desired_network_types.iter().map(ToString::to_string));
    if network_types_need_repair {
        config.webrtc.network_types = desired_network_types;
        config_changed = true;
        notes.push(
            "restored direct P2P WebRTC network types (udp4/udp6/tcp4/tcp6) for native and browser clients"
                .to_string(),
        );
    }

    if has_default_owner_user(paths, &config)? {
        if config.web_server.first_login_create_admin {
            config.web_server.first_login_create_admin = false;
            config_changed = true;
            notes.push(
                "disabled first-login admin bootstrap because default owner user already exists"
                    .to_string(),
            );
        }
        if config.web_server.first_login_assign_global_hosts {
            config.web_server.first_login_assign_global_hosts = false;
            config_changed = true;
            notes.push(
                "disabled first-login host assignment because default owner user already exists"
                    .to_string(),
            );
        }
    }

    if config_changed {
        save_config(paths, &config)?;
        changed_paths.push(paths.config_path.display().to_string());
    }

    if rewrite_bundle_file(
        &paths.bundle_root.join("SUNSHINE_LOGIN.txt"),
        "Local host runtime credentials are intentionally not written to disk.\r\nUse your secure deployment secret source to manage local runtime admin credentials.\r\n",
    )? {
        changed_paths.push(
            paths
                .bundle_root
                .join("SUNSHINE_LOGIN.txt")
                .display()
                .to_string(),
        );
        notes.push("redacted plaintext host runtime login artifact".to_string());
    }

    if rewrite_lines(&paths.bundle_root.join("SETUP.txt"), |line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- Username:") {
            Some("- Username: intentionally not written to disk".to_string())
        } else if trimmed.starts_with("- Password:") {
            Some(
                "- Secret handling: local runtime admin credentials are intentionally not written to disk"
                    .to_string(),
            )
        } else {
            None
        }
    })? {
        changed_paths.push(paths.bundle_root.join("SETUP.txt").display().to_string());
        notes.push("redacted plaintext setup credentials".to_string());
    }

    if rewrite_lines(&paths.bundle_root.join("README.txt"), |line| {
        if line.contains("Local Sunshine login:") || line.contains("Local runtime admin UI:") {
            Some(
                "- Local runtime admin credentials are intentionally not written to disk."
                    .to_string(),
            )
        } else {
            None
        }
    })? {
        changed_paths.push(paths.bundle_root.join("README.txt").display().to_string());
        notes.push("redacted plaintext readme credentials".to_string());
    }

    if rewrite_lines(&paths.bundle_root.join("start-all.bat"), |line| {
        if line.contains("echo Local Sunshine Login:")
            || line.contains("echo Local runtime admin UI:")
        {
            Some(
                "echo Local runtime admin credentials are intentionally not written to disk."
                    .to_string(),
            )
        } else {
            None
        }
    })? {
        changed_paths.push(
            paths
                .bundle_root
                .join("start-all.bat")
                .display()
                .to_string(),
        );
        notes.push("redacted plaintext start script credentials".to_string());
    }

    Ok(HygieneRemediationResult {
        changed: !changed_paths.is_empty(),
        config_changed,
        changed_paths,
        notes,
    })
}

fn has_default_owner_user(paths: &BundlePaths, config: &Config) -> Result<bool> {
    let Some(default_user_id) = config.web_server.default_user_id else {
        return Ok(false);
    };

    if !paths.data_path.exists() {
        return Ok(false);
    }

    let raw = fs::read_to_string(&paths.data_path)
        .with_context(|| format!("failed to read {}", paths.data_path.display()))?;
    let parsed: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", paths.data_path.display()))?;
    Ok(parsed
        .get("users")
        .and_then(Value::as_object)
        .is_some_and(|users| users.contains_key(&default_user_id.to_string())))
}

fn rewrite_bundle_file(path: &PathBuf, new_content: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let current = fs::read_to_string(path).unwrap_or_default();
    if current == new_content {
        return Ok(false);
    }

    fs::write(path, new_content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn rewrite_lines<F>(path: &PathBuf, mut map_line: F) -> Result<bool>
where
    F: FnMut(&str) -> Option<String>,
{
    if !path.exists() {
        return Ok(false);
    }

    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut changed = false;
    let mut output = Vec::new();
    for line in raw.lines() {
        if let Some(replacement) = map_line(line) {
            output.push(replacement);
            changed = true;
        } else {
            output.push(line.to_string());
        }
    }

    if !changed {
        return Ok(false);
    }

    let mut rewritten = output.join("\r\n");
    if raw.ends_with('\n') {
        rewritten.push_str("\r\n");
    }
    fs::write(path, rewritten).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn default_service_name(paths: &BundlePaths) -> String {
    let bundle_name = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("host");
    format!("CloudgimeHost-{bundle_name}")
}

fn default_user_agent_task_name(paths: &BundlePaths) -> String {
    let bundle_name = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("host");
    format!("CloudgimeHostUser-{bundle_name}")
}

fn default_sunshine_service_name(paths: &BundlePaths) -> String {
    let bundle_name = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("host");
    format!("CloudgimeRuntime-{bundle_name}")
}

fn install_service(paths: &BundlePaths) -> Result<()> {
    ensure_user_agent_task(paths)?;
    ensure_sunshine_runtime_service(paths)?;
    let service_name = default_service_name(paths);
    let display_name = format!(
        "Cloudgime Host {}",
        service_name.trim_start_matches("CloudgimeHost-")
    );
    let bin_path = format!(
        "\"{}\" --bundle-root \"{}\" run-service --service-name \"{}\"",
        paths.supervisor_path.display(),
        paths.bundle_root.display(),
        service_name
    );

    let exists = Command::new("sc.exe")
        .args(["qc", &service_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to query Windows service {}", service_name))?
        .success();

    let mut args = if exists {
        vec![
            "config".to_string(),
            service_name.clone(),
            "type=".to_string(),
            "own".to_string(),
            "start=".to_string(),
            "auto".to_string(),
            "error=".to_string(),
            "normal".to_string(),
            "binPath=".to_string(),
            bin_path.clone(),
            "DisplayName=".to_string(),
            display_name.clone(),
        ]
    } else {
        vec![
            "create".to_string(),
            service_name.clone(),
            "type=".to_string(),
            "own".to_string(),
            "start=".to_string(),
            "auto".to_string(),
            "error=".to_string(),
            "normal".to_string(),
            "binPath=".to_string(),
            bin_path.clone(),
            "DisplayName=".to_string(),
            display_name.clone(),
        ]
    };

    let mut create_command = Command::new("sc.exe");
    create_command.args(args.drain(..));
    let create_output =
        output_hidden(&mut create_command).context("failed to invoke sc.exe create/config")?;
    if !create_output.status.success() {
        let combined = format!(
            "{} {}",
            String::from_utf8_lossy(&create_output.stdout).trim(),
            String::from_utf8_lossy(&create_output.stderr).trim()
        );
        bail!(
            "failed to configure Windows service {}: {}",
            service_name,
            combined.trim()
        );
    }

    let description = format!(
        "Cloudgime Host service supervisor for {}",
        paths.bundle_root.display()
    );
    let _ = Command::new("sc.exe")
        .args(["description", &service_name, &description])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Ok(())
}

fn uninstall_service(paths: &BundlePaths) -> Result<()> {
    let service_name = default_service_name(paths);
    let sunshine_service_name = default_sunshine_service_name(paths);
    let _ = remove_qos(paths);
    let mut stop_service_command = Command::new("sc.exe");
    stop_service_command
        .args(["stop", &service_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = status_hidden(&mut stop_service_command);
    let mut stop_runtime_command = Command::new("sc.exe");
    stop_runtime_command
        .args(["stop", &sunshine_service_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = status_hidden(&mut stop_runtime_command);

    let mut delete_service_command = Command::new("sc.exe");
    delete_service_command.args(["delete", &service_name]);
    let output =
        output_hidden(&mut delete_service_command).context("failed to invoke sc.exe delete")?;
    if output.status.success() {
        remove_user_agent_task(paths)?;
        return Ok(());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    if combined.to_ascii_lowercase().contains("does not exist")
        || combined.to_ascii_lowercase().contains("1060")
    {
        let _ = delete_optional_windows_service(&sunshine_service_name);
        let _ = remove_user_agent_task(paths);
        return Ok(());
    }

    delete_optional_windows_service(&sunshine_service_name)?;
    remove_user_agent_task(paths)?;
    bail!(
        "failed to delete Windows service {}: {}",
        service_name,
        combined.trim()
    )
}

fn ensure_user_agent_task(paths: &BundlePaths) -> Result<()> {
    let task_name = default_user_agent_task_name(paths);
    let task_action = format!(
        "\"{}\" --bundle-root \"{}\" run-daemon",
        paths.supervisor_path.display(),
        paths.bundle_root.display()
    );

    let _ = Command::new("schtasks")
        .args(["/Delete", "/TN", &task_name, "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let mut create_task_command = Command::new("schtasks");
    create_task_command.args([
        "/Create",
        "/TN",
        &task_name,
        "/TR",
        &task_action,
        "/SC",
        "ONLOGON",
        "/RL",
        "HIGHEST",
        "/F",
        "/IT",
    ]);
    let output =
        output_hidden(&mut create_task_command).context("failed to invoke schtasks /Create")?;

    if output.status.success() {
        return Ok(());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    bail!(
        "failed to create user-session supervisor task {}: {}",
        task_name,
        combined.trim()
    )
}

fn ensure_sunshine_runtime_service(paths: &BundlePaths) -> Result<()> {
    let runtime_dir = paths.bundle_root.join(read_selected_runtime_key(paths));
    let service_bin = runtime_dir.join("tools").join("sunshinesvc.exe");
    if !service_bin.exists() {
        return Ok(());
    }

    let service_name = default_sunshine_service_name(paths);
    let display_name = format!(
        "Cloudgime Runtime {}",
        service_name.trim_start_matches("CloudgimeRuntime-")
    );
    let bin_path = format!("\"{}\"", service_bin.display());

    let exists = Command::new("sc.exe")
        .args(["qc", &service_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to query Windows service {}", service_name))?
        .success();

    let mut args = if exists {
        vec![
            "config".to_string(),
            service_name.clone(),
            "type=".to_string(),
            "own".to_string(),
            "start=".to_string(),
            "demand".to_string(),
            "error=".to_string(),
            "normal".to_string(),
            "binPath=".to_string(),
            bin_path.clone(),
        ]
    } else {
        vec![
            "create".to_string(),
            service_name.clone(),
            "type=".to_string(),
            "own".to_string(),
            "start=".to_string(),
            "demand".to_string(),
            "error=".to_string(),
            "normal".to_string(),
            "binPath=".to_string(),
            bin_path.clone(),
            "DisplayName=".to_string(),
            display_name.clone(),
        ]
    };
    if exists {
        args.push("DisplayName=".to_string());
        args.push(display_name.clone());
    }

    let mut config_service_command = Command::new("sc.exe");
    config_service_command.args(args);
    let output = output_hidden(&mut config_service_command)
        .with_context(|| format!("failed to configure Windows service {}", service_name))?;
    if !output.status.success() {
        let combined = format!(
            "{} {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        bail!(
            "failed to configure Windows service {}: {}",
            service_name,
            combined.trim()
        );
    }

    let description = format!(
        "Cloudgime Runtime service for {}",
        paths.bundle_root.display()
    );
    let _ = Command::new("sc.exe")
        .args(["description", &service_name, &description])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(())
}

fn delete_optional_windows_service(service_name: &str) -> Result<()> {
    let mut delete_optional_service_command = Command::new("sc.exe");
    delete_optional_service_command.args(["delete", service_name]);
    let output = output_hidden(&mut delete_optional_service_command)
        .with_context(|| format!("failed to invoke sc.exe delete for {}", service_name))?;

    if output.status.success() {
        return Ok(());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let lowered = combined.to_ascii_lowercase();
    if lowered.contains("does not exist") || lowered.contains("1060") {
        return Ok(());
    }

    bail!(
        "failed to delete Windows service {}: {}",
        service_name,
        combined.trim()
    )
}

fn remove_user_agent_task(paths: &BundlePaths) -> Result<()> {
    let task_name = default_user_agent_task_name(paths);
    let mut delete_task_command = Command::new("schtasks");
    delete_task_command.args(["/Delete", "/TN", &task_name, "/F"]);
    let output =
        output_hidden(&mut delete_task_command).context("failed to invoke schtasks /Delete")?;

    if output.status.success() {
        return Ok(());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let lowered = combined.to_ascii_lowercase();
    if lowered.contains("cannot find the file specified")
        || lowered.contains("does not exist")
        || lowered.contains("error: the system cannot find the file specified")
    {
        return Ok(());
    }

    bail!(
        "failed to delete user-session supervisor task {}: {}",
        task_name,
        combined.trim()
    )
}

fn query_user_agent_task_status(paths: &BundlePaths) -> Result<String> {
    let task_name = default_user_agent_task_name(paths);
    let mut query_task_command = Command::new("schtasks");
    query_task_command.args(["/Query", "/TN", &task_name, "/FO", "LIST", "/V"]);
    let output =
        output_hidden(&mut query_task_command).context("failed to invoke schtasks /Query")?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let lowered = combined.to_ascii_lowercase();
    if lowered.contains("cannot find the file specified")
        || lowered.contains("does not exist")
        || lowered.contains("error: the system cannot find the file specified")
    {
        return Ok(format!("{task_name}: NOT INSTALLED"));
    }

    bail!(
        "failed to query user-session supervisor task {}: {}",
        task_name,
        combined.trim()
    )
}

fn service_control(paths: &BundlePaths, action: &str) -> Result<()> {
    let service_name = default_service_name(paths);
    let mut service_control_command = Command::new("sc.exe");
    service_control_command.args([action, &service_name]);
    let output = output_hidden(&mut service_control_command)
        .with_context(|| format!("failed to invoke sc.exe {action}"))?;
    if output.status.success() {
        return Ok(());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    bail!(
        "failed to {action} Windows service {}: {}",
        service_name,
        combined.trim()
    )
}

fn query_service_status(paths: &BundlePaths) -> Result<String> {
    let service_name = default_service_name(paths);
    let mut query_service_command = Command::new("sc.exe");
    query_service_command.args(["query", &service_name]);
    let output =
        output_hidden(&mut query_service_command).context("failed to invoke sc.exe query")?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let lowered = combined.to_ascii_lowercase();
    if lowered.contains("1060") || lowered.contains("does not exist") {
        return Ok(format!("{service_name}: NOT INSTALLED"));
    }
    bail!(
        "failed to query Windows service {}: {}",
        service_name,
        combined.trim()
    )
}

fn wait_for_bundle_process_state(
    paths: &BundlePaths,
    expected_running: bool,
    timeout: Duration,
) -> Result<()> {
    let started_at = Instant::now();
    loop {
        let status = run_supervisor_status(paths)?;
        let running = has_required_processes(&status)?;
        if running == expected_running {
            return Ok(());
        }

        if started_at.elapsed() >= timeout {
            bail!(
                "bundle for {} did not reach running={} within {}s",
                paths.bundle_root.display(),
                expected_running,
                timeout.as_secs()
            );
        }

        sleep(Duration::from_secs(2));
    }
}

fn configure_firewall(paths: &BundlePaths) -> Result<Vec<String>> {
    let config = load_config(paths)?;
    let host_label = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bundle");
    let port_range = config
        .webrtc
        .port_range
        .clone()
        .ok_or_else(|| anyhow!("missing WebRTC port range in config"))?;
    let rules = vec![
        FirewallRuleSpec {
            name: format!("Cloudgime Host {host_label} WebServer UDP"),
            program: Some(paths.web_server_path.clone()),
            protocol: "UDP",
            local_port: "Any".to_string(),
        },
        FirewallRuleSpec {
            name: format!("Cloudgime Host {host_label} WebServer TCP"),
            program: Some(paths.web_server_path.clone()),
            protocol: "TCP",
            local_port: "Any".to_string(),
        },
        FirewallRuleSpec {
            name: format!("Cloudgime Host {host_label} Streamer UDP"),
            program: Some(paths.streamer_path.clone()),
            protocol: "UDP",
            local_port: "Any".to_string(),
        },
        FirewallRuleSpec {
            name: format!("Cloudgime Host {host_label} Streamer TCP"),
            program: Some(paths.streamer_path.clone()),
            protocol: "TCP",
            local_port: "Any".to_string(),
        },
        FirewallRuleSpec {
            name: format!("Cloudgime Host {host_label} WebRTC UDP Range"),
            program: None,
            protocol: "UDP",
            local_port: format!("{}-{}", port_range.min, port_range.max),
        },
    ];

    let mut warnings = Vec::new();
    for rule in rules {
        delete_firewall_rule(&rule.name);
        if let Err(err) = add_firewall_rule(&rule) {
            let message = format!("{err:#}");
            if looks_like_firewall_permission_error(&message) {
                warnings.push(
                    "[WARN] Firewall rules were not updated because Administrator access was denied. Direct P2P may fail until you run the installer elevated once.".to_string(),
                );
                return Ok(warnings);
            }
            return Err(err);
        }
    }

    Ok(warnings)
}

#[derive(Debug, Clone)]
struct QosPolicySpec {
    name: String,
    app_path: PathBuf,
    source_port_start: u16,
    source_port_end: u16,
    dscp_action: u8,
    precedence: u32,
}

fn configure_qos(paths: &BundlePaths) -> Result<Vec<String>> {
    let config = load_config(paths)?;
    let host_label = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bundle");
    let port_range = config
        .webrtc
        .port_range
        .clone()
        .ok_or_else(|| anyhow!("missing WebRTC port range in config"))?;
    let spec = QosPolicySpec {
        name: format!("Cloudgime Host {host_label} Streamer Media QoS"),
        app_path: paths.streamer_path.clone(),
        source_port_start: port_range.min,
        source_port_end: port_range.max,
        dscp_action: STREAMER_MEDIA_QOS_DSCP,
        precedence: STREAMER_MEDIA_QOS_PRECEDENCE,
    };

    delete_qos_policy(&spec.name);
    let mut warnings = Vec::new();
    if let Err(err) = add_qos_policy(&spec) {
        let message = format!("{err:#}");
        if looks_like_qos_permission_error(&message) {
            warnings.push(
                "[WARN] Stream QoS policy was not updated because Administrator access was denied. Public direct sessions may stay more sensitive to upload/download spikes until you run the installer elevated once.".to_string(),
            );
            return Ok(warnings);
        }

        warnings.push(format!(
            "[WARN] Stream QoS policy was not updated: {}",
            message.trim()
        ));
    }

    Ok(warnings)
}

fn remove_qos(paths: &BundlePaths) -> Result<()> {
    let host_label = paths
        .bundle_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bundle");
    let name = format!("Cloudgime Host {host_label} Streamer Media QoS");
    delete_qos_policy(&name);
    Ok(())
}

fn delete_qos_policy(policy_name: &str) {
    let policy_name = escape_powershell_single_quoted(policy_name);
    let script = format!(
        "Import-Module NetQos -ErrorAction SilentlyContinue; Remove-NetQosPolicy -Name '{policy_name}' -PolicyStore ActiveStore -Confirm:$false -ErrorAction SilentlyContinue | Out-Null"
    );
    let _ = output_hidden(Command::new("powershell.exe").args([
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]));
}

fn add_qos_policy(spec: &QosPolicySpec) -> Result<()> {
    let policy_name = escape_powershell_single_quoted(&spec.name);
    let app_path = escape_powershell_single_quoted(&spec.app_path.to_string_lossy());
    let script = format!(
        r#"
Import-Module NetQos -ErrorAction Stop
Remove-NetQosPolicy -Name '{policy_name}' -PolicyStore ActiveStore -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
New-NetQosPolicy -Name '{policy_name}' -PolicyStore ActiveStore -NetworkProfile All -Precedence {precedence} -AppPathNameMatchCondition '{app_path}' -IPProtocolMatchCondition UDP -IPSrcPortStartMatchCondition {source_port_start} -IPSrcPortEndMatchCondition {source_port_end} -DSCPAction {dscp_action} | Out-Null
"#,
        precedence = spec.precedence,
        source_port_start = spec.source_port_start,
        source_port_end = spec.source_port_end,
        dscp_action = spec.dscp_action,
    );
    let mut qos_command = Command::new("powershell.exe");
    qos_command.args([
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    let output =
        output_hidden(&mut qos_command).context("failed to invoke powershell for QoS policy")?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "failed to add QoS policy {}: {} {}",
        spec.name,
        stdout.trim(),
        stderr.trim()
    )
}

fn looks_like_qos_permission_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("access is denied")
        || lowered.contains("requires elevation")
        || lowered.contains("requested operation requires elevation")
}

fn delete_firewall_rule(rule_name: &str) {
    let _ = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={rule_name}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn add_firewall_rule(rule: &FirewallRuleSpec) -> Result<()> {
    let mut args = vec![
        "advfirewall".to_string(),
        "firewall".to_string(),
        "add".to_string(),
        "rule".to_string(),
        format!("name={}", rule.name),
        "dir=in".to_string(),
        "action=allow".to_string(),
        "profile=private,public".to_string(),
        format!("protocol={}", rule.protocol),
    ];

    if let Some(program) = &rule.program {
        args.push(format!("program={}", program.display()));
    } else {
        args.push("program=any".to_string());
    }

    if rule.local_port.eq_ignore_ascii_case("any") {
        args.push("localport=any".to_string());
    } else {
        args.push(format!("localport={}", rule.local_port));
    }

    let mut firewall_command = Command::new("netsh");
    firewall_command.args(args);
    let output =
        output_hidden(&mut firewall_command).context("failed to invoke netsh for firewall rule")?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "failed to add firewall rule {}: {} {}",
        rule.name,
        stdout.trim(),
        stderr.trim()
    )
}

fn looks_like_firewall_permission_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("access is denied") || lowered.contains("requires elevation")
}

fn run_supervisor_command(paths: &BundlePaths, command: &str) -> Result<()> {
    if !paths.supervisor_path.exists() {
        bail!(
            "missing host supervisor at {}",
            paths.supervisor_path.display()
        );
    }

    let status = Command::new(&paths.supervisor_path)
        .args([
            "--bundle-root",
            &paths.bundle_root.to_string_lossy(),
            command,
        ])
        .current_dir(&paths.bundle_root)
        .status()
        .with_context(|| {
            format!(
                "failed to run host supervisor {}",
                paths.supervisor_path.display()
            )
        })?;
    if !status.success() {
        bail!("host supervisor returned non-zero status for {command}: {status}");
    }
    Ok(())
}

fn run_supervisor_status(paths: &BundlePaths) -> Result<Value> {
    if !paths.supervisor_path.exists() {
        bail!(
            "missing host supervisor at {}",
            paths.supervisor_path.display()
        );
    }

    let mut status_command = Command::new(&paths.supervisor_path);
    status_command
        .args([
            "--bundle-root",
            &paths.bundle_root.to_string_lossy(),
            "status",
        ])
        .current_dir(&paths.bundle_root);
    let output = output_hidden(&mut status_command).with_context(|| {
        format!(
            "failed to run host supervisor status {}",
            paths.supervisor_path.display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "host supervisor status returned non-zero status: {}",
            output.status
        );
    }
    Ok(serde_json::from_slice::<Value>(&output.stdout)
        .context("failed to parse host supervisor status json")?)
}

fn verify_startup(paths: &BundlePaths) -> Result<()> {
    let config = load_config(paths)?;
    let deadline = Instant::now() + Duration::from_secs(20);

    loop {
        let status = run_supervisor_status(paths)?;
        if has_required_processes(&status)? && local_http_ready(&config)? {
            return Ok(());
        }

        if Instant::now() >= deadline {
            bail!(
                "bundle startup incomplete; status={}",
                serde_json::to_string(&status)?
            );
        }
        sleep(Duration::from_millis(500));
    }
}

fn has_required_processes(status: &Value) -> Result<bool> {
    let Some(processes) = status.get("running_processes").and_then(Value::as_array) else {
        bail!("host supervisor status missing running_processes");
    };

    let mut has_sunshine = false;
    let mut has_web_server = false;
    for process in processes {
        let Some(path) = process.get("path").and_then(Value::as_str) else {
            continue;
        };
        let lowered = path.to_ascii_lowercase();
        if lowered.ends_with("\\sunshine.exe") {
            has_sunshine = true;
        } else if lowered.ends_with("\\web-server.exe") {
            has_web_server = true;
        }
    }

    Ok(has_sunshine && has_web_server)
}

fn local_http_ready(config: &Config) -> Result<bool> {
    let address = config.web_server.bind_address;
    let path_prefix = normalize_url_path_prefix(&config.web_server.url_path_prefix);
    let path = format!("{path_prefix}/");
    let mut stream = match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
        Ok(stream) => stream,
        Err(_) => return Ok(false),
    };
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();

    let host_header = match address {
        SocketAddr::V4(value) => format!("{}:{}", value.ip(), value.port()),
        SocketAddr::V6(value) => format!("[{}]:{}", value.ip(), value.port()),
    };

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let Some(first_line) = response.lines().next() else {
        return Ok(false);
    };
    let status_code = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_default();
    Ok((200..500).contains(&status_code))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

fn build_local_url(config: &Config) -> String {
    let address = config.web_server.bind_address;
    let path_prefix = normalize_url_path_prefix(&config.web_server.url_path_prefix);
    format!("http://{}:{}{path_prefix}/", address.ip(), address.port())
}

fn normalize_url_path_prefix(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_trailing = trimmed.trim_end_matches('/');
    if without_trailing.starts_with('/') {
        without_trailing.to_string()
    } else {
        format!("/{without_trailing}")
    }
}

fn read_selected_runtime_key(paths: &BundlePaths) -> String {
    fs::read_to_string(&paths.selected_runtime_path)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "sunshine".to_string())
}

fn canonicalize_without_verbatim_prefix(path: PathBuf) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    let text = canonical.to_string_lossy().replace('/', "\\");
    let normalized = text
        .strip_prefix(r"\\?\")
        .unwrap_or(text.as_str())
        .to_string();
    Ok(PathBuf::from(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_release_info(environment: &str) -> HostReleaseInfo {
        HostReleaseInfo {
            schema_version: 1,
            deployment_environment: Some(environment.to_string()),
            release_channel: Some("mouse-restore-q470".to_string()),
            bundle_version: Some(format!("{environment}.mouse-restore-q470.123456.abc1234")),
            build_id: Some("123456-abc1234".to_string()),
            source_branch: Some("mouse-restore-q470".to_string()),
            source_commit: Some("abc1234deadbeef".to_string()),
            source_commit_short: Some("abc1234".to_string()),
            source_dirty: false,
            build_profile: Some("release".to_string()),
            built_at_unix_ms: Some(123456),
        }
    }

    fn sample_history_entry(
        action: &str,
        status: &str,
        environment: &str,
    ) -> HostReleaseHistoryEntry {
        HostReleaseHistoryEntry {
            schema_version: 1,
            release_id: format!("{environment}.mouse-restore-q470.123456.abc1234"),
            action: action.to_string(),
            status: status.to_string(),
            reason: "ok".to_string(),
            snapshot_id: None,
            started_at_unix_ms: 1,
            completed_at_unix_ms: Some(2),
            bundle_version: Some(format!("{environment}.mouse-restore-q470.123456.abc1234")),
            build_id: Some("123456-abc1234".to_string()),
            source_commit_short: Some("abc1234".to_string()),
            deployment_environment: Some(environment.to_string()),
            release_channel: Some("mouse-restore-q470".to_string()),
        }
    }

    #[test]
    fn next_promotion_target_uses_progressive_ring_policy() {
        assert_eq!(next_promotion_target_for_environment("dev"), Some("canary"));
        assert_eq!(
            next_promotion_target_for_environment("development"),
            Some("canary")
        );
        assert_eq!(
            next_promotion_target_for_environment("canary"),
            Some("staging")
        );
        assert_eq!(
            next_promotion_target_for_environment("staging"),
            Some("production")
        );
        assert_eq!(next_promotion_target_for_environment("production"), None);
    }

    #[test]
    fn required_ready_streak_scales_by_target_environment() {
        assert_eq!(
            required_ready_streak_ms_for_environment("canary"),
            Some(5 * 60 * 1000)
        );
        assert_eq!(
            required_ready_streak_ms_for_environment("staging"),
            Some(10 * 60 * 1000)
        );
        assert_eq!(
            required_ready_streak_ms_for_environment("production"),
            Some(15 * 60 * 1000)
        );
    }

    #[test]
    fn production_release_counts_same_build_across_environment_promotion() {
        let release_info = sample_release_info("production");
        let history = vec![sample_history_entry("apply", "succeeded", "staging")];
        let (stage, _, _) =
            derive_release_promotion_status(Some(&release_info), "passed", &history);

        assert_eq!(stage, "production_applied");
    }
}
