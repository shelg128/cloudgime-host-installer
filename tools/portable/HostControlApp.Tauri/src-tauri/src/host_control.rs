use std::{
    env,
    ffi::{c_void, OsStr},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use base64::{engine::general_purpose::STANDARD as Base64, Engine as _};
use chrono::Utc;
use getrandom::fill as fill_random;
use pbkdf2::pbkdf2_hmac;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, State};
use uuid::Uuid;

const HASH_LENGTH: usize = 32;
const SALT_LENGTH: usize = 16;
const PASSWORD_ITERATIONS: u32 = 120_000;
const DEFAULT_CONTROL_PLANE: &str = "https://cloudgime.my.id";
const DEFAULT_INSTALLED_PRODUCT_NAME: &str = "Cloudgime Host";
const DEFAULT_UNINSTALL_REGISTRY_KEY: &str =
    r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\CloudgimeHostControl";
const HOST_KEEPER_TUNNEL_TASK_NAME: &str = "CloudgimeHostTunnel";
const DEFAULT_HOST_PANEL_LOCAL_URL: &str = "http://127.0.0.1:3000/";
const DEFAULT_HOST_KEEPER_LOCAL_URL: &str = "http://127.0.0.1:18080/stream/";
const STREAM_DISPLAY_PREFERENCES_FILE_NAME: &str = "stream_display_preferences.json";

#[derive(Default)]
pub struct AppSession {
    unlocked: Mutex<bool>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    auth: AuthView,
    bundle_root: String,
    installer_available: bool,
    install: InstallView,
    activation: ActivationView,
    network: NetworkView,
    audio: AudioView,
    display: DisplayView,
    runtime: RuntimeView,
    support: SupportView,
    paths: PathView,
    host_user_daemon_task_health: Value,
    windows_native_diagnostic_reports: Value,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthView {
    password_exists: bool,
    needs_password_setup: bool,
    unlocked: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallView {
    installed_mode: bool,
    install_root: String,
    data_root: String,
    uninstall_registered: bool,
    launch_intent: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivationView {
    host_id: String,
    display_name: String,
    sentinel_pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    token_kind: String,
    instance_type: String,
    phase: String,
    control_plane_url: String,
    activated_at_utc: String,
    redeemed_at_utc: String,
    last_heartbeat_at_utc: String,
    ready_for_stream: bool,
    runtime_token_present: bool,
    activation_record_id_present: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    public_url: String,
    local_url: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
    lifecycle_phase: String,
    health_grade: String,
    audio_status: String,
    service_state: String,
    runtime_label: String,
    runtime_key: String,
    runtime_profile_key: String,
    runtime_version: String,
    encoder: String,
    capture: String,
    capture_reason: String,
    selection_reason: String,
    ffmpeg_source: String,
    fallback_runtime_label: String,
    fallback_runtime_version: String,
    fallback_runtime_reason: String,
    warnings: Vec<String>,
    local_http_ready: bool,
    required_processes_ready: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioView {
    mode: String,
    selected_audio_sink_name: String,
    selected_virtual_sink_name: String,
    selected_microphone_name: String,
    selection_reason: String,
    routing_status: String,
    routing_reason: String,
    available_outputs: Vec<String>,
    available_inputs: Vec<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DisplayView {
    mode: String,
    custom_device_name: String,
    custom_device_id: String,
    custom_label: String,
    effective_label: String,
    updated_at: String,
    dual_stream_enabled: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SupportView {
    support_bundle_count: i32,
    last_support_bundle_id: String,
    last_support_bundle_path: String,
    raw_status_json: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PathView {
    bundle_root: String,
    server_folder_path: String,
    support_folder_path: String,
    runtime_file_path: String,
    release_info_path: String,
    capability_profile_path: String,
    audio_package_path: String,
    audio_inf_path: String,
    display_state_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome {
    message: String,
    state: ShellState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticUploadEnvelope {
    host_id: String,
    pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    display_name: String,
    source: String,
    app_version: String,
    level: String,
    summary: String,
    payload: Value,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "PascalCase")]
struct HostActivationStateRecord {
    schema_version: i32,
    host_id: String,
    machine_identity: String,
    install_instance_id: String,
    activation_state: String,
    setup_token_kind: String,
    instance_type: String,
    control_plane_url: String,
    display_name: String,
    sentinel_pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    runtime_token: String,
    activated_at_utc: String,
    redeemed_at_utc: String,
    activation_record_id: String,
    last_heartbeat_at_utc: String,
    last_ready_for_stream: bool,
    updated_at_utc: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PasswordRecord {
    schema_version: i32,
    iterations: u32,
    salt_base64: String,
    hash_base64: String,
    updated_at_utc: String,
}

#[derive(Default)]
struct BundleNetworkConfig {
    public_url: String,
}

#[derive(Default)]
struct HostStatusData {
    lifecycle_phase: String,
    health_grade: String,
    audio_status: String,
    audio_reason: String,
    local_url: String,
    local_http_ready: bool,
    required_processes_ready: bool,
    runtime_label: String,
    runtime_key: String,
    runtime_profile_key: String,
    runtime_version: String,
    encoder: String,
    capture: String,
    capture_reason: String,
    selection_reason: String,
    ffmpeg_source: String,
    fallback_runtime_label: String,
    fallback_runtime_version: String,
    fallback_runtime_reason: String,
    warnings: Vec<String>,
    support_bundle_count: i32,
    last_support_bundle_id: String,
    audio_package_path: String,
    audio_inf_path: String,
    raw_json: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct AudioPreferenceRecord {
    schema_version: i32,
    mode: String,
    selected_audio_sink_name: String,
    selected_virtual_sink_name: String,
    selected_microphone_name: String,
    updated_at: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct DisplayPreferenceRecord {
    schema_version: i32,
    mode: String,
    custom_device_name: String,
    custom_device_id: String,
    custom_label: String,
    updated_at: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct CapabilityAudioEndpointRecord {
    direction: String,
    device_id: String,
    name: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct CapabilityGpuControllerRecord {
    name: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct CapabilityRuntimeCandidateRecord {
    key: String,
    startup_validation_status: Option<String>,
    startup_validation_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "snake_case")]
struct CapabilityProfileRecord {
    selected_runtime_key: String,
    selected_runtime_display_name: String,
    selected_runtime_version: String,
    selected_encoder: String,
    selected_capture: String,
    selected_capture_reason: Option<String>,
    selected_ffmpeg_source: String,
    selection_reason: String,
    warnings: Vec<String>,
    gpu_controllers: Vec<CapabilityGpuControllerRecord>,
    runtime_candidates: Vec<CapabilityRuntimeCandidateRecord>,
    audio_endpoints: Vec<CapabilityAudioEndpointRecord>,
    selected_audio_sink_name: String,
    selected_virtual_sink_name: String,
    selected_microphone_name: String,
    audio_selection_reason: String,
    audio_selection_mode: String,
}

struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn deserialize_stringish<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::String(value)) => value,
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RedeemPayload {
    ok: bool,
    #[serde(deserialize_with = "deserialize_stringish")]
    host_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    activation_state: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    display_name: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    control_plane_url: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    activation_record_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    runtime_token: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    activated_at_utc: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    redeemed_at_utc: String,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HeartbeatPayload {
    ok: bool,
    last_heartbeat_at_utc: String,
    ready_for_stream: bool,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UninstallPayload {
    ok: bool,
    deleted: bool,
    stale_fallback: bool,
    already_missing: bool,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PresencePayload {
    ok: bool,
    host_id: String,
    display_name: String,
    sentinel_pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    updated_at_utc: String,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase", default)]
struct ClaimSetupTokenPayload {
    ok: bool,
    #[serde(deserialize_with = "deserialize_stringish")]
    token_kind: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    instance_type: String,
    auto_allocated_binding: bool,
    #[serde(deserialize_with = "deserialize_stringish")]
    parent_control_node_token_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    parent_control_node_machine_identity: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    host_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    display_name: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    sentinel_pc_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    sentinel_device_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    keeper_entry_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    device_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    device_token: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    device_token_hint: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    pc_id: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    pc_number: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    slot_label: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    activation_token: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    activation_state: String,
    #[serde(deserialize_with = "deserialize_stringish")]
    control_plane_url: String,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct SharedPcIdentityRecord {
    schema_version: i32,
    host_id: String,
    machine_identity: String,
    sentinel_pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    updated_at_utc: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct PendingUninstallRecord {
    schema_version: i32,
    host_id: String,
    machine_identity: String,
    install_instance_id: String,
    runtime_token: String,
    activation_record_id: String,
    control_plane_url: String,
    recorded_at_utc: String,
}

enum RemoteDeleteOutcome {
    Deleted,
    AlreadyMissing,
    DeferredOffline,
    DeferredStale,
}

enum PersistentServiceState {
    Enabled,
    Deferred,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct InstalledLayoutRecord {
    schema_version: i32,
    install_root: String,
    bundle_root: String,
    product_name: String,
    uninstall_registry_key: String,
    #[serde(default)]
    app_installer_product_code: String,
    #[serde(default)]
    app_installer_registry_path: String,
    #[serde(default)]
    app_executable_name: String,
    updated_at_utc: String,
}

#[tauri::command]
pub async fn get_shell_state(session: State<'_, AppSession>) -> Result<ShellState, String> {
    build_shell_state(
        session
            .unlocked
            .lock()
            .map_err(|_| "Session lock failed.")?
            .to_owned(),
    )
}

#[tauri::command]
pub async fn set_admin_password(
    password: String,
    session: State<'_, AppSession>,
) -> Result<ShellState, String> {
    let bundle_root = resolve_bundle_root()?;
    let store_path = admin_password_store_path(&bundle_root);
    let normalized_password = normalize_admin_password_for_storage(&password);
    if normalized_password.len() < 6 {
        return Err("Password admin minimal 6 karakter.".into());
    }

    write_password_store(&store_path, normalized_password)?;
    if let Ok(mut unlocked) = session.unlocked.lock() {
        *unlocked = true;
    }

    build_shell_state(true)
}

#[tauri::command]
pub async fn unlock_app(
    password: String,
    session: State<'_, AppSession>,
) -> Result<ShellState, String> {
    let bundle_root = resolve_bundle_root()?;
    let store_path = admin_password_store_path(&bundle_root);
    if !store_path.exists() {
        return Err("Admin password has not been created yet.".into());
    }

    if !verify_password_store(&store_path, &password)? {
        return Err("Password admin tidak cocok.".into());
    }

    if let Ok(mut unlocked) = session.unlocked.lock() {
        *unlocked = true;
    }

    build_shell_state(true)
}

#[tauri::command]
pub async fn lock_app(session: State<'_, AppSession>) -> Result<ShellState, String> {
    if let Ok(mut unlocked) = session.unlocked.lock() {
        *unlocked = false;
    }
    build_shell_state(false)
}

#[tauri::command]
pub async fn change_admin_password(
    password: String,
    session: State<'_, AppSession>,
) -> Result<ShellState, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let store_path = admin_password_store_path(&bundle_root);
    let normalized_password = normalize_admin_password_for_storage(&password);
    if normalized_password.len() < 6 {
        return Err("Password admin baru minimal 6 karakter.".into());
    }

    write_password_store(&store_path, normalized_password)?;
    build_shell_state(true)
}

#[tauri::command]
pub async fn run_host_action(
    action: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let activation = load_activation_state(&bundle_root)?;
    let requires_activated = matches!(
        action.as_str(),
        "start_host" | "restart_runtime" | "start_service"
    );

    if requires_activated && !activation_allows_local_runtime(&bundle_root, &activation) {
        return Err(format!(
            "Host is currently {}. Complete setup-token binding or redeem a valid activation token first.",
            normalize_phase(&activation.activation_state)
        ));
    }

    let (installer_command, success_message) = match action.as_str() {
        "setup_host" => (
            "prepare-host",
            "Host set up locally. Next step: open Host Control and generate a token.",
        ),
        "verify_startup" => ("verify-startup", "Startup validation completed."),
        "start_host" => ("start-bundle", "Host runtime started."),
        "restart_runtime" => ("restart-runtime", "Runtime restarted."),
        "stop_host" => ("stop-bundle", "Host runtime stopped."),
        "install_service" => ("install-service", "Service installed."),
        "start_service" => ("start-service", "Service started."),
        "stop_service" => ("stop-service", "Service stopped."),
        "remove_service" => ("uninstall-service", "Service removed."),
        "configure_firewall" => ("configure-firewall", "Firewall rules updated."),
        "collect_support" => ("collect-support-bundle", "Support bundle collected."),
        _ => return Err("Unsupported host action.".into()),
    };

    if matches!(action.as_str(), "start_host" | "restart_runtime" | "start_service") {
        configure_host_keeper_tunnel(&bundle_root, &activation, None)?;
    }

    let result = if installer_command == "restart-runtime" {
        restart_runtime_engine(&bundle_root)?
    } else {
        run_installer_command(&bundle_root, installer_command)?
    };
    if !result.success {
        return Err(join_output(&result));
    }

    if action == "install_service" {
        remove_legacy_runtime_service();
    }

    if action == "setup_host" {
        mark_prepared_locally(&bundle_root)?;
    }

    Ok(ActionOutcome {
        message: success_message.into(),
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn preflight_host(
    fix: bool,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let args = if fix { vec!["--fix"] } else { vec![] };
    let result = run_installer_command_args(
        &bundle_root,
        "preflight-host",
        &args,
        Duration::from_secs(120),
    )?;
    if !result.success {
        return Err(join_output(&result));
    }

    Ok(ActionOutcome {
        message: if fix {
            "Preflight completed with fixes applied.".into()
        } else {
            "Preflight completed.".into()
        },
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn save_preferences(
    display_name: String,
    control_plane_url: String,
    session: State<'_, AppSession>,
) -> Result<ShellState, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let mut activation = load_activation_state(&bundle_root)?;
    activation.display_name = sanitized_display_name(&display_name);
    activation.control_plane_url = normalize_control_plane(&control_plane_url)?;
    save_activation_state(&bundle_root, &activation)?;
    build_shell_state(true)
}

#[tauri::command]
pub async fn save_audio_preferences(
    mode: String,
    selected_audio_sink_name: String,
    selected_virtual_sink_name: String,
    selected_microphone_name: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;

    if read_capability_profile(&bundle_root)
        .audio_endpoints
        .is_empty()
    {
        let refresh = run_installer_command(&bundle_root, "refresh-host-capability")?;
        if !refresh.success {
            return Err(join_output(&refresh));
        }
    }

    let profile = read_capability_profile(&bundle_root);
    let mut available_outputs = collect_audio_endpoint_names(&profile.audio_endpoints, "output");
    let mut available_inputs = collect_audio_endpoint_names(&profile.audio_endpoints, "input");
    extend_audio_choices(
        &mut available_outputs,
        &[
            &profile.selected_audio_sink_name,
            &profile.selected_virtual_sink_name,
        ],
    );
    extend_audio_choices(&mut available_inputs, &[&profile.selected_microphone_name]);
    let normalized_mode = normalize_audio_mode(&mode);

    if normalized_mode == "manual" {
        if available_outputs.is_empty() {
            return Err(
                "No audio output devices were detected yet. Run Setup Host or Refresh Status first."
                    .into(),
            );
        }

        let sink = selected_audio_sink_name.trim();
        let virtual_sink = selected_virtual_sink_name.trim();
        let microphone = selected_microphone_name.trim();

        if sink.is_empty() {
            return Err("Pick an output device first.".into());
        }
        if virtual_sink.is_empty() {
            return Err("Pick a virtual sink first.".into());
        }
        if microphone.is_empty() {
            return Err("Pick a microphone or input device first.".into());
        }
        if !contains_case_insensitive(&available_outputs, sink) {
            return Err(format!(
                "Audio output device \"{sink}\" is no longer available."
            ));
        }
        if !contains_case_insensitive(&available_outputs, virtual_sink) {
            return Err(format!(
                "Virtual sink \"{virtual_sink}\" is no longer available."
            ));
        }
        if !available_inputs.is_empty() && !contains_case_insensitive(&available_inputs, microphone)
        {
            return Err(format!(
                "Microphone/input device \"{microphone}\" is no longer available."
            ));
        }

        let record = AudioPreferenceRecord {
            schema_version: 1,
            mode: "manual".into(),
            selected_audio_sink_name: sink.into(),
            selected_virtual_sink_name: virtual_sink.into(),
            selected_microphone_name: microphone.into(),
            updated_at: Utc::now().to_rfc3339(),
        };
        save_audio_preferences_file(&bundle_root, &record)?;
    } else {
        clear_audio_preferences_file(&bundle_root)?;
    }

    let refresh = run_installer_command(&bundle_root, "refresh-host-capability")?;
    if !refresh.success {
        return Err(join_output(&refresh));
    }

    let activation = load_activation_state(&bundle_root)?;
    let status_before_restart = load_host_status(&bundle_root).unwrap_or_default();
    let mut message = if normalized_mode == "manual" {
        "Audio route saved. The selected devices are now the preferred route.".to_string()
    } else {
        "Automatic audio routing restored.".to_string()
    };

    let should_restart_runtime = activation
        .activation_state
        .eq_ignore_ascii_case("activated")
        && (status_before_restart.required_processes_ready
            || status_before_restart.local_http_ready
            || !status_before_restart.lifecycle_phase.trim().is_empty());

    if should_restart_runtime {
        let restart = restart_runtime_engine(&bundle_root)?;
        if restart.success {
            message = if normalized_mode == "manual" {
                "Audio route saved. Runtime restarted with the selected devices.".to_string()
            } else {
                "Automatic audio routing restored. Runtime restarted.".to_string()
            };
        } else {
            message = format!(
                "{} Restart the runtime once to apply it everywhere: {}",
                message,
                summarize_command_issue(&join_output(&restart))
            );
        }
    }

    Ok(ActionOutcome {
        message,
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn save_display_preferences(
    mode: String,
    custom_device_name: String,
    custom_device_id: String,
    custom_label: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let normalized_mode = normalize_display_mode(&mode);
    let custom_device_name = custom_device_name.trim().to_string();
    let custom_device_id = custom_device_id.trim().to_string();
    let custom_label = custom_label.trim().to_string();

    if normalized_mode == "custom"
        && custom_device_name.is_empty()
        && custom_device_id.is_empty()
        && custom_label.is_empty()
    {
        return Err("Custom display needs a device name, device id, or label match.".into());
    }

    let record = DisplayPreferenceRecord {
        schema_version: 1,
        mode: normalized_mode.clone(),
        custom_device_name,
        custom_device_id,
        custom_label,
        updated_at: Utc::now().to_rfc3339(),
    };
    save_display_preferences_file(&bundle_root, &record)?;

    let activation = load_activation_state(&bundle_root)?;
    let status_before_restart = load_host_status(&bundle_root).unwrap_or_default();
    let mut message = format!(
        "Display target saved: {}.",
        display_mode_label(&normalized_mode)
    );

    let should_restart_runtime = activation
        .activation_state
        .eq_ignore_ascii_case("activated")
        && (status_before_restart.required_processes_ready
            || status_before_restart.local_http_ready
            || !status_before_restart.lifecycle_phase.trim().is_empty());

    if should_restart_runtime {
        let restart = restart_runtime_engine(&bundle_root)?;
        if restart.success {
            message = format!(
                "Display target saved: {}. Runtime restarted.",
                display_mode_label(&normalized_mode)
            );
        } else {
            message = format!(
                "{} Restart the runtime once to apply it everywhere: {}",
                message,
                summarize_command_issue(&join_output(&restart))
            );
        }
    }

    Ok(ActionOutcome {
        message,
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn sync_host_binding(session: State<'_, AppSession>) -> Result<ShellState, String> {
    reconcile_pending_uninstall_marker().await;
    let bundle_root = resolve_bundle_root()?;
    let mut activation = load_activation_state(&bundle_root)?;
    if activation.host_id.trim().is_empty() {
        let unlocked = session
            .unlocked
            .lock()
            .map_err(|_| "Session lock failed.")?
            .to_owned();
        return build_shell_state(unlocked);
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create the control plane client.")?;
    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/presence"))
        .json(&serde_json::json!({
            "hostId": activation.host_id,
            "machineIdentity": empty_to_none(&activation.machine_identity),
            "installInstanceId": empty_to_none(&activation.install_instance_id),
            "displayName": empty_to_none(&activation.display_name),
            "sentinelPcId": empty_to_none(&activation.sentinel_pc_id),
            "sentinelDeviceId": empty_to_none(&activation.sentinel_device_id),
            "keeperEntryId": empty_to_none(&activation.keeper_entry_id),
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the control plane.")?;

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: PresencePayload = serde_json::from_str(&raw).unwrap_or_default();
    if !status.is_success() || !payload.ok {
        return Err(payload
            .error
            .or(payload.message)
            .unwrap_or_else(|| "Could not sync the host binding.".into()));
    }

    if !payload.host_id.trim().is_empty() {
        activation.host_id = payload.host_id.trim().into();
    }
    if !payload.display_name.trim().is_empty() {
        activation.display_name = payload.display_name.trim().into();
    }
    if !payload.sentinel_pc_id.trim().is_empty() {
        activation.sentinel_pc_id = payload.sentinel_pc_id.trim().into();
    }
    if !payload.sentinel_device_id.trim().is_empty() {
        activation.sentinel_device_id = payload.sentinel_device_id.trim().into();
    }
    if !payload.keeper_entry_id.trim().is_empty() {
        activation.keeper_entry_id = payload.keeper_entry_id.trim().into();
    }
    if !payload.updated_at_utc.trim().is_empty() {
        activation.updated_at_utc = payload.updated_at_utc.trim().into();
    }
    save_activation_state(&bundle_root, &activation)?;

    let unlocked = session
        .unlocked
        .lock()
        .map_err(|_| "Session lock failed.")?
        .to_owned();
    build_shell_state(unlocked)
}

#[tauri::command]
pub async fn redeem_activation_token(
    activation_token: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    reconcile_pending_uninstall_marker().await;
    let bundle_root = resolve_bundle_root()?;
    let token = normalize_activation_token(&activation_token);
    if !looks_like_activation_token(&token) {
        return Err("Paste a valid activation token first.".into());
    }

    ensure_host_prepared_automatically(&bundle_root)?;
    let activation = load_activation_state(&bundle_root)?;
    redeem_activation_token_with_state(&bundle_root, activation, token).await
}

#[tauri::command]
pub async fn recover_host_activation(
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    reconcile_pending_uninstall_marker().await;
    let bundle_root = resolve_bundle_root()?;
    let mut activation = load_activation_state(&bundle_root)?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not create the control plane client.")?;
    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/recover"))
        .json(&serde_json::json!({
            "hostId": activation.host_id,
            "machineIdentity": activation.machine_identity,
            "installInstanceId": activation.install_instance_id,
            "displayName": activation.display_name,
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the control plane.")?;

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: RedeemPayload = serde_json::from_str(&raw).unwrap_or_default();
    if !status.is_success() || !payload.ok {
        return Err(build_friendly_redeem_error(status.as_u16(), &payload));
    }

    if !payload.host_id.trim().is_empty() {
        activation.host_id = payload.host_id.trim().into();
    }
    activation.activation_state = if payload.activation_state.trim().is_empty() {
        "activated".into()
    } else {
        payload.activation_state.trim().into()
    };
    if !payload.display_name.trim().is_empty() {
        activation.display_name = payload.display_name.trim().into();
    }
    activation.control_plane_url = if payload.control_plane_url.trim().is_empty() {
        base_url
    } else {
        normalize_control_plane(&payload.control_plane_url)?
    };
    activation.runtime_token = payload.runtime_token.trim().into();
    activation.activation_record_id = payload.activation_record_id.trim().into();
    activation.activated_at_utc = payload.activated_at_utc.trim().into();
    activation.redeemed_at_utc = payload.redeemed_at_utc.trim().into();
    save_activation_state(&bundle_root, &activation)?;
    configure_host_keeper_tunnel(&bundle_root, &activation, None)?;
    let service_state = ensure_persistent_host_service(&bundle_root)?;
    let service_label = match service_state {
        PersistentServiceState::Enabled => "enabled",
        PersistentServiceState::Deferred => "deferred",
    };

    Ok(ActionOutcome {
        message: format!(
            "Aktivasi host dipulihkan dari admin master. Service: {}.",
            service_label
        ),
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn claim_setup_token(
    setup_token: String,
    expected_token_kind: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    reconcile_pending_uninstall_marker().await;
    let bundle_root = resolve_bundle_root()?;
    let mut activation = load_activation_state(&bundle_root)?;
    let normalized_setup_token = normalize_setup_token(&setup_token);
    if normalized_setup_token.len() < 12 {
        return Err("Paste a valid Lisensi Aktivasi first.".into());
    }
    let expected_token_kind = normalize_expected_setup_token_kind(&expected_token_kind)?;
    ensure_host_prepared_automatically(&bundle_root)?;
    prepare_fresh_setup_token_identity(&mut activation);

    let mut actual_setup_token = normalized_setup_token.clone();
    if normalized_setup_token.starts_with("cgpair_") {
        if let Some((parsed_token, parsed_base_url)) = decode_cgpair_token(&normalized_setup_token) {
            actual_setup_token = parsed_token;
            if let Ok(normalized_url) = normalize_control_plane(&parsed_base_url) {
                activation.control_plane_url = normalized_url;
            }
        } else {
            return Err("Invalid pairing token format. Could not decode base64 payload.".into());
        }
    }

    save_activation_state(&bundle_root, &activation)?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not create the control plane client.")?;
    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let machine_name = resolve_local_machine_name();
    let response = client
        .post(format!("{base_url}/api/v1/provisioning/setup-tokens/claim"))
        .json(&serde_json::json!({
            "activationLicense": actual_setup_token.clone(),
            "setupToken": actual_setup_token,
            "componentType": "host_app",
            "machineName": empty_to_none(&machine_name),
            "hostname": empty_to_none(&machine_name),
            "machineIdentity": empty_to_none(&activation.machine_identity),
            "installInstanceId": empty_to_none(&activation.install_instance_id),
            "expectedActivationLicenseKind": empty_to_none(&expected_token_kind),
            "expectedTokenKind": empty_to_none(&expected_token_kind),
            "app": "cloudgime_host_control",
            "version": env!("CARGO_PKG_VERSION"),
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the control plane.")?;

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: ClaimSetupTokenPayload = serde_json::from_str(&raw).unwrap_or_default();
    if !status.is_success() || !payload.ok {
        return Err(build_friendly_claim_error(status.as_u16(), &payload, &raw));
    }
    if payload.token_kind.trim().eq_ignore_ascii_case("control_node") {
        return Err(
            "Lisensi Aktivasi ini adalah Control Node. Aktifkan dari Power Panel/Keeper, lalu gunakan Lisensi Instance Pair atau Always-On Host di Cloudgime Host.".into(),
        );
    }
    if !expected_token_kind.trim().is_empty()
        && !payload
            .token_kind
            .trim()
            .eq_ignore_ascii_case(&expected_token_kind)
    {
        return Err(format!(
            "Lisensi Aktivasi ini adalah {}, bukan {}. Paste lisensi di kolom yang sesuai.",
            setup_token_lane_label(&payload.token_kind),
            setup_token_lane_label(&expected_token_kind),
        ));
    }

    let claimed_host_id = payload.host_id.trim();
    if claimed_host_id.is_empty() {
        return Err("Aktivasi lisensi berhasil, tapi binding host belum lengkap. Update Cloudgime Host dan coba lagi.".into());
    }
    activation.host_id = claimed_host_id.into();
    if !payload.display_name.trim().is_empty() {
        activation.display_name = payload.display_name.trim().into();
    }
    if !payload.sentinel_pc_id.trim().is_empty() {
        activation.sentinel_pc_id = payload.sentinel_pc_id.trim().into();
    }
    if !payload.sentinel_device_id.trim().is_empty() {
        activation.sentinel_device_id = payload.sentinel_device_id.trim().into();
    }
    if !payload.keeper_entry_id.trim().is_empty() {
        activation.keeper_entry_id = payload.keeper_entry_id.trim().into();
    }
    if !payload.control_plane_url.trim().is_empty() {
        activation.control_plane_url = normalize_control_plane(&payload.control_plane_url)?;
    } else {
        activation.control_plane_url = base_url.clone();
    }
    activation.setup_token_kind = payload.token_kind.trim().to_string();
    activation.instance_type = payload.instance_type.trim().to_string();
    if !payload.activation_state.trim().is_empty() {
        activation.activation_state = "activated".into();
    }
    save_activation_state(&bundle_root, &activation)?;
    configure_host_keeper_tunnel(
        &bundle_root,
        &activation,
        Some(payload.device_token.trim()),
    )?;

    let activation_token = normalize_activation_token(&payload.activation_token);
    if false {
        return Err(
            "Aktivasi lisensi berhasil, tapi payload token aktivasi runtime belum lengkap.".into(),
        );
    }

    let lane_label = setup_token_lane_label(&payload.token_kind);
    let binding_label = first_non_empty(&[
        payload.slot_label.clone(),
        payload.sentinel_device_id.clone(),
        payload.pc_number.clone(),
        payload.pc_id.clone(),
        payload.host_id.clone(),
    ]);
    let mut outcome =
        redeem_activation_token_with_state(&bundle_root, activation, activation_token).await?;
    let binding_copy = if binding_label.trim().is_empty() {
        lane_label.clone()
    } else if payload.auto_allocated_binding {
        format!("{lane_label} {binding_label} (auto-assigned)")
    } else {
        format!("{lane_label} {binding_label}")
    };
    outcome.message = format!("{} siap. {}", binding_copy, outcome.message);
    Ok(outcome)
}

#[tauri::command]
pub async fn reset_local_host_identity(
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let current =
        load_activation_state(&bundle_root).unwrap_or_else(|_| default_activation_state());
    let control_plane_url = normalize_control_plane(&current.control_plane_url)
        .unwrap_or_else(|_| DEFAULT_CONTROL_PLANE.into());
    let display_name = if current.display_name.trim().is_empty() {
        default_display_name()
    } else {
        current.display_name.trim().into()
    };
    let machine_identity = random_machine_identity();
    let host_id = random_host_id();
    let now = Utc::now().to_rfc3339();

    let _ = run_installer_command(&bundle_root, "stop-bundle");
    let _ = run_installer_command(&bundle_root, "stop-service");
    let _ = remove_host_keeper_tunnel(&bundle_root);
    let _ = fs::remove_file(pending_uninstall_marker_path());
    let _ = fs::write(bundle_root.join("PUBLIC_URL.txt"), "");

    let shared_identity = SharedPcIdentityRecord {
        schema_version: 1,
        host_id: host_id.clone(),
        machine_identity: machine_identity.clone(),
        sentinel_pc_id: String::new(),
        sentinel_device_id: String::new(),
        keeper_entry_id: String::new(),
        updated_at_utc: now.clone(),
    };
    save_shared_pc_identity(&shared_pc_identity_path(), &shared_identity)?;

    let next = HostActivationStateRecord {
        schema_version: 1,
        host_id,
        machine_identity,
        install_instance_id: random_install_instance_id(),
        activation_state: "prepared_local".into(),
        setup_token_kind: String::new(),
        instance_type: String::new(),
        control_plane_url,
        display_name,
        sentinel_pc_id: String::new(),
        sentinel_device_id: String::new(),
        keeper_entry_id: String::new(),
        runtime_token: String::new(),
        activated_at_utc: String::new(),
        redeemed_at_utc: String::new(),
        activation_record_id: String::new(),
        last_heartbeat_at_utc: String::new(),
        last_ready_for_stream: false,
        updated_at_utc: now,
    };
    save_activation_state(&bundle_root, &next)?;

    Ok(ActionOutcome {
        message: "Local host identity reset. Issue a fresh Lisensi Aktivasi for this PC, then paste it here.".into(),
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn upload_host_diagnostic(
    reason: String,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    let bundle_root = resolve_bundle_root()?;
    let activation = load_activation_state(&bundle_root)?;
    let control_plane = normalize_control_plane(&activation.control_plane_url)
        .unwrap_or_else(|_| DEFAULT_CONTROL_PLANE.to_string());
    let unlocked = session
        .unlocked
        .lock()
        .map_err(|_| "Session lock failed.")?
        .to_owned();
    let shell_state = build_shell_state(unlocked).unwrap_or_default();
    let summary = {
        let trimmed = reason.trim();
        if trimmed.is_empty() {
            "Manual diagnostic from Host App".to_string()
        } else {
            trimmed.chars().take(240).collect()
        }
    };
    let payload = build_host_diagnostic_payload(&bundle_root, &shell_state, &summary);
    let envelope = DiagnosticUploadEnvelope {
        host_id: activation.host_id.clone(),
        pc_id: activation.sentinel_pc_id.clone(),
        sentinel_device_id: activation.sentinel_device_id.clone(),
        keeper_entry_id: activation.keeper_entry_id.clone(),
        display_name: activation.display_name.clone(),
        source: "host_app".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        level: "warning".into(),
        summary: summary.clone(),
        payload,
    };

    let url = format!(
        "{}/api/v1/host-diagnostics",
        control_plane.trim_end_matches('/')
    );
    let response = Client::new()
        .post(&url)
        .json(&envelope)
        .send()
        .await
        .map_err(|error| format!("Gagal mengirim diagnostic ke control plane: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Upload diagnostic gagal ({status}). {}",
            body.chars().take(320).collect::<String>()
        ));
    }
    let diagnostic_id = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("diagnosticId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into());

    Ok(ActionOutcome {
        message: format!("Diagnostic terkirim ke admin master. ID: {diagnostic_id}"),
        state: build_shell_state(unlocked).unwrap_or_default(),
    })
}

async fn redeem_activation_token_with_state(
    bundle_root: &Path,
    mut activation: HostActivationStateRecord,
    token: String,
) -> Result<ActionOutcome, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not create the control plane client.")?;
    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/redeem"))
        .json(&serde_json::json!({
            "hostId": activation.host_id,
            "machineIdentity": activation.machine_identity,
            "installInstanceId": activation.install_instance_id,
            "displayName": activation.display_name,
            "activationToken": token,
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the control plane.")?;

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: RedeemPayload = serde_json::from_str(&raw).unwrap_or_default();
    if !status.is_success() || !payload.ok {
        return Err(build_friendly_redeem_error(status.as_u16(), &payload));
    }

    if !payload.host_id.trim().is_empty() {
        activation.host_id = payload.host_id.trim().into();
    }
    activation.activation_state = if payload.activation_state.trim().is_empty() {
        "activated".into()
    } else {
        payload.activation_state.trim().into()
    };
    activation.display_name = if payload.display_name.trim().is_empty() {
        activation.display_name
    } else {
        payload.display_name.trim().into()
    };
    activation.control_plane_url = if payload.control_plane_url.trim().is_empty() {
        base_url
    } else {
        normalize_control_plane(&payload.control_plane_url)?
    };
    activation.runtime_token = payload.runtime_token.trim().into();
    activation.activation_record_id = payload.activation_record_id.trim().into();
    activation.activated_at_utc = payload.activated_at_utc.trim().into();
    activation.redeemed_at_utc = payload.redeemed_at_utc.trim().into();
    save_activation_state(bundle_root, &activation)?;
    configure_host_keeper_tunnel(bundle_root, &activation, None)?;
    let service_state = ensure_persistent_host_service(bundle_root)?;
    let activation_message = build_activation_success_message(bundle_root, service_state);

    Ok(ActionOutcome {
        message: activation_message,
        state: build_shell_state(true)?,
    })
}

fn reset_stale_local_activation(activation: &mut HostActivationStateRecord) {
    if should_reset_stale_local_activation(activation) {
        activation.activation_state = "prepared_local".into();
    }

    activation.runtime_token.clear();
    activation.activation_record_id.clear();
    activation.activated_at_utc.clear();
    activation.redeemed_at_utc.clear();
    activation.last_heartbeat_at_utc.clear();
    activation.last_ready_for_stream = false;
}

fn should_reset_stale_local_activation(activation: &HostActivationStateRecord) -> bool {
    activation.activation_state.eq_ignore_ascii_case("revoked")
        || activation
            .activation_state
            .eq_ignore_ascii_case("suspended")
        || (activation
            .activation_state
            .eq_ignore_ascii_case("locked_waiting_token")
            && !activation.activation_record_id.trim().is_empty())
}

fn prepare_fresh_setup_token_identity(activation: &mut HostActivationStateRecord) {
    activation.host_id = random_host_id();
    activation.machine_identity = random_machine_identity();
    activation.install_instance_id = random_install_instance_id();
    activation.activation_state = "prepared_local".into();
    activation.setup_token_kind.clear();
    activation.instance_type.clear();
    activation.sentinel_pc_id.clear();
    activation.sentinel_device_id.clear();
    activation.keeper_entry_id.clear();
    reset_stale_local_activation(activation);
}

#[tauri::command]
pub async fn send_heartbeat(session: State<'_, AppSession>) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let mut activation = load_activation_state(&bundle_root)?;
    if !activation
        .activation_state
        .eq_ignore_ascii_case("activated")
    {
        return Err("Redeem a valid activation token first.".into());
    }
    if activation.runtime_token.trim().is_empty() {
        return Err("Runtime token is missing. Reissue and redeem a token first.".into());
    }

    let status = load_host_status(&bundle_root).unwrap_or_default();
    let capability_profile = read_capability_profile(&bundle_root);
    let service_state = load_service_state(&bundle_root).unwrap_or_else(|_| "unknown".into());
    let network = load_network_config(&bundle_root)?;
    let (display_route_ready, display_route_note) =
        evaluate_stream_display_route(&bundle_root, &capability_profile);
    let ready_for_stream = display_route_ready
        && status.local_http_ready
        && status.required_processes_ready
        && status.lifecycle_phase.eq_ignore_ascii_case("ready");
    let stream_note = if ready_for_stream {
        "ready_for_stream".to_string()
    } else {
        build_stream_not_ready_note(
            display_route_note.as_deref(),
            &status.lifecycle_phase,
            status.required_processes_ready,
            status.local_http_ready,
            &network.public_url,
        )
    };

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create the control plane client.")?;
    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/heartbeat"))
        .json(&serde_json::json!({
            "hostId": activation.host_id,
            "machineIdentity": activation.machine_identity,
            "installInstanceId": activation.install_instance_id,
            "runtimeToken": activation.runtime_token,
            "activationRecordId": empty_to_none(&activation.activation_record_id),
            "displayName": empty_to_none(&activation.display_name),
            "lifecyclePhase": empty_to_none(&status.lifecycle_phase),
            "healthGrade": empty_to_none(&status.health_grade),
            "runtimeDisplayName": empty_to_none(&status.runtime_label),
            "publicUrl": empty_to_none(&network.public_url),
            "serviceState": empty_to_none(&service_state),
            "localHttpReady": status.local_http_ready,
            "requiredProcessesReady": status.required_processes_ready,
            "readyForStream": ready_for_stream,
            "note": Some(stream_note),
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the control plane for heartbeat.")?;

    let status_code = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: HeartbeatPayload = serde_json::from_str(&raw).unwrap_or_default();
    if !status_code.is_success() || !payload.ok {
        return Err(payload
            .error
            .or(payload.message)
            .unwrap_or_else(|| "Heartbeat failed.".into()));
    }

    activation.last_heartbeat_at_utc = payload.last_heartbeat_at_utc.trim().into();
    activation.last_ready_for_stream = payload.ready_for_stream;
    save_activation_state(&bundle_root, &activation)?;

    Ok(ActionOutcome {
        message: if payload.ready_for_stream {
            "Heartbeat sent. Host is ready for stream.".into()
        } else {
            "Heartbeat sent. Host runtime is not fully ready yet.".into()
        },
        state: build_shell_state(true)?,
    })
}

#[tauri::command]
pub async fn uninstall_installed_host(
    password: String,
    app: AppHandle,
    session: State<'_, AppSession>,
) -> Result<String, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    if password.trim().is_empty() {
        return Err("Masukkan password admin untuk uninstall.".into());
    }
    ensure_admin_password_matches(&bundle_root, &password)?;
    let install_layout = load_installed_layout();
    if !is_installed_mode(&bundle_root, install_layout.as_ref()) {
        return Err("Installed mode is not active for this host.".into());
    }
    let activation = load_activation_state(&bundle_root)?;
    let delete_outcome = delete_remote_activation_if_present(&activation).await?;
    match delete_outcome {
        RemoteDeleteOutcome::Deleted | RemoteDeleteOutcome::AlreadyMissing => {
            clear_pending_uninstall_marker()?;
        }
        RemoteDeleteOutcome::DeferredOffline | RemoteDeleteOutcome::DeferredStale => {
            save_pending_uninstall_marker(&activation)?;
        }
    }
    let _ = prepare_local_state_for_uninstall(&bundle_root, &activation);

    let install_root = install_layout
        .as_ref()
        .map(|value| PathBuf::from(value.install_root.trim()))
        .filter(|value| value.exists())
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|value| value.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(default_installed_app_root);

    let _ = run_installer_command(&bundle_root, "stop-bundle");
    let _ = run_installer_command(&bundle_root, "stop-service");
    let _ = run_installer_command(&bundle_root, "uninstall-service");

    schedule_installed_uninstall(&bundle_root, &install_root, install_layout.as_ref())?;

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        app.exit(0);
    });

    Ok(match delete_outcome {
        RemoteDeleteOutcome::DeferredOffline =>
            "Cloudgime Host uninstall was scheduled. Remote cleanup is queued and will retry on the next install."
                .into(),
        RemoteDeleteOutcome::DeferredStale =>
            "Cloudgime Host uninstall was scheduled. The stale host record will be retried for cleanup on the next install."
                .into(),
        RemoteDeleteOutcome::AlreadyMissing =>
            "Cloudgime Host uninstall was scheduled. The website record was already clean, and the app will close now."
                .into(),
        RemoteDeleteOutcome::Deleted =>
            "Cloudgime Host uninstall was scheduled. The website record was deleted, and the app will close now."
                .into(),
    })
}

#[tauri::command]
pub async fn launch_emergency_uninstaller(
    app: AppHandle,
    session: State<'_, AppSession>,
) -> Result<String, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let install_layout = load_installed_layout();
    if !is_installed_mode(&bundle_root, install_layout.as_ref()) {
        return Err("Installed mode is not active for this host.".into());
    }

    let activation = load_activation_state(&bundle_root)?;
    if activation.host_id.trim().is_empty() || activation.runtime_token.trim().is_empty() {
        clear_pending_uninstall_marker()?;
    } else {
        save_pending_uninstall_marker(&activation)?;
    }
    let _ = prepare_local_state_for_uninstall(&bundle_root, &activation);

    let install_root = install_layout
        .as_ref()
        .map(|value| PathBuf::from(value.install_root.trim()))
        .filter(|value| value.exists())
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|value| value.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(default_installed_app_root);

    launch_standalone_emergency_uninstaller(&bundle_root, &install_root)?;

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        app.exit(0);
    });

    Ok(
        "Emergency kill was launched. Forced uninstall is running and this app will close now."
            .into(),
    )
}

async fn delete_remote_activation_if_present(
    activation: &HostActivationStateRecord,
) -> Result<RemoteDeleteOutcome, String> {
    if activation.host_id.trim().is_empty() || activation.runtime_token.trim().is_empty() {
        return Ok(RemoteDeleteOutcome::AlreadyMissing);
    }

    let base_url = normalize_control_plane(&activation.control_plane_url)?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not create the control plane client for uninstall.")?;
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/uninstall"))
        .json(&serde_json::json!({
            "hostId": activation.host_id,
            "machineIdentity": activation.machine_identity,
            "installInstanceId": activation.install_instance_id,
            "runtimeToken": activation.runtime_token,
            "activationRecordId": empty_to_none(&activation.activation_record_id),
        }))
        .send()
        .await;

    let response = match response {
        Ok(value) => value,
        Err(err) => {
            if is_transient_delete_error(&err) {
                return Ok(RemoteDeleteOutcome::DeferredOffline);
            }

            return Err(
                "Could not reach the control plane. Uninstall is blocked until the host record can be deleted remotely."
                    .to_string(),
            );
        }
    };

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let payload: UninstallPayload = serde_json::from_str(&raw).unwrap_or_default();
    if status.as_u16() == 404 || payload.already_missing {
        return Ok(RemoteDeleteOutcome::AlreadyMissing);
    }
    if is_transient_delete_status(status.as_u16()) {
        return Ok(RemoteDeleteOutcome::DeferredOffline);
    }
    if payload.stale_fallback {
        return Ok(RemoteDeleteOutcome::Deleted);
    }
    if is_stale_delete_status(status.as_u16(), &payload) {
        return Ok(RemoteDeleteOutcome::DeferredStale);
    }
    if !status.is_success() || !payload.ok {
        return Err(payload.error.or(payload.message).unwrap_or_else(|| {
            "Could not delete the host record from the control plane. Uninstall was cancelled."
                .to_string()
        }));
    }

    if !payload.deleted {
        return Err(
            "The control plane did not confirm host deletion. Uninstall was cancelled.".into(),
        );
    }

    Ok(RemoteDeleteOutcome::Deleted)
}

async fn reconcile_pending_uninstall_marker() {
    let Some(marker) = load_pending_uninstall_marker() else {
        return;
    };
    if marker.host_id.trim().is_empty() || marker.runtime_token.trim().is_empty() {
        let _ = clear_pending_uninstall_marker();
        return;
    }

    let Ok(base_url) = normalize_control_plane(&marker.control_plane_url) else {
        return;
    };
    let Ok(client) = Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
    else {
        return;
    };
    let response = client
        .post(format!("{base_url}/api/v1/host-activation/uninstall"))
        .json(&serde_json::json!({
            "hostId": marker.host_id,
            "machineIdentity": empty_to_none(&marker.machine_identity),
            "installInstanceId": empty_to_none(&marker.install_instance_id),
            "runtimeToken": marker.runtime_token,
            "activationRecordId": empty_to_none(&marker.activation_record_id),
        }))
        .send()
        .await;

    let Ok(response) = response else {
        return;
    };

    let status = response.status().as_u16();
    let raw = response.text().await.unwrap_or_default();
    let payload: UninstallPayload = serde_json::from_str(&raw).unwrap_or_default();
    if status == 404
        || payload.already_missing
        || (status < 500 && status != 429 && payload.ok && payload.deleted)
    {
        let _ = clear_pending_uninstall_marker();
    }
}

fn prepare_local_state_for_uninstall(
    bundle_root: &Path,
    activation: &HostActivationStateRecord,
) -> Result<(), String> {
    let mut next = activation.clone();
    next.activation_state = "prepared_local".into();
    next.runtime_token.clear();
    next.activation_record_id.clear();
    next.activated_at_utc.clear();
    next.redeemed_at_utc.clear();
    next.last_heartbeat_at_utc.clear();
    next.last_ready_for_stream = false;
    save_activation_state(bundle_root, &next)
}

fn load_pending_uninstall_marker() -> Option<PendingUninstallRecord> {
    let path = pending_uninstall_marker_path();
    if !path.exists() {
        return None;
    }

    let raw = fs::read_to_string(path).ok()?;
    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut record: PendingUninstallRecord = serde_json::from_str(clean).ok()?;
    if record.schema_version != 1 {
        return None;
    }
    record.host_id = normalized_host_id(&record.host_id);
    record.machine_identity = normalized_machine_identity(&record.machine_identity);
    record.install_instance_id = normalized_install_instance_id(&record.install_instance_id);
    record.runtime_token = record.runtime_token.trim().to_string();
    record.activation_record_id = record.activation_record_id.trim().to_string();
    record.control_plane_url = record
        .control_plane_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    Some(record)
}

fn save_pending_uninstall_marker(activation: &HostActivationStateRecord) -> Result<(), String> {
    let path = pending_uninstall_marker_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create the pending uninstall marker folder.")?;
    }

    let record = PendingUninstallRecord {
        schema_version: 1,
        host_id: activation.host_id.clone(),
        machine_identity: activation.machine_identity.clone(),
        install_instance_id: activation.install_instance_id.clone(),
        runtime_token: activation.runtime_token.clone(),
        activation_record_id: activation.activation_record_id.clone(),
        control_plane_url: activation.control_plane_url.clone(),
        recorded_at_utc: Utc::now().to_rfc3339(),
    };

    let raw = serde_json::to_string_pretty(&record)
        .map_err(|_| "Could not encode the pending uninstall marker.")?;
    fs::write(path, raw).map_err(|_| "Could not write the pending uninstall marker.")?;
    Ok(())
}

fn clear_pending_uninstall_marker() -> Result<(), String> {
    let path = pending_uninstall_marker_path();
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)
        .map_err(|_| "Could not remove the pending uninstall marker.".to_string())?;
    Ok(())
}

fn is_transient_delete_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

fn is_transient_delete_status(status_code: u16) -> bool {
    status_code == 408
        || status_code == 425
        || status_code == 429
        || (500..=599).contains(&status_code)
}

fn is_stale_delete_status(status_code: u16, payload: &UninstallPayload) -> bool {
    if !matches!(status_code, 401 | 403 | 409) {
        return false;
    }

    let text = payload
        .error
        .as_deref()
        .or(payload.message.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();

    text.contains("invalid runtime token")
        || text.contains("machine identity")
        || text.contains("install instance")
        || text.contains("activation record id mismatch")
        || text.contains("stale")
}

fn build_shell_state(unlocked: bool) -> Result<ShellState, String> {
    let bundle_root = resolve_bundle_root()?;
    let install_layout = load_installed_layout();
    let install_root = install_layout
        .as_ref()
        .map(|value| value.install_root.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_installed_app_root().display().to_string());
    let launch_intent = resolve_launch_intent();
    let installed_mode = is_installed_mode(&bundle_root, install_layout.as_ref());
    let password_exists = admin_password_store_path(&bundle_root).exists();
    let activation = load_activation_state(&bundle_root)?;
    let network = load_network_config(&bundle_root)?;
    let audio = load_audio_view(&bundle_root, &HostStatusData::default());
    let display = load_display_view(&bundle_root);

    if !unlocked {
        return Ok(ShellState {
            auth: AuthView {
                password_exists,
                needs_password_setup: !password_exists,
                unlocked,
            },
            bundle_root: bundle_root.display().to_string(),
            installer_available: host_installer_path(&bundle_root).exists(),
            install: InstallView {
                installed_mode,
                install_root,
                data_root: bundle_root.display().to_string(),
                uninstall_registered: uninstall_registration_exists(install_layout.as_ref()),
                launch_intent,
            },
            activation: ActivationView {
                host_id: activation.host_id.clone(),
                display_name: activation.display_name.clone(),
                sentinel_pc_id: activation.sentinel_pc_id.clone(),
                sentinel_device_id: activation.sentinel_device_id.clone(),
                keeper_entry_id: activation.keeper_entry_id.clone(),
                token_kind: activation.setup_token_kind.clone(),
                instance_type: activation.instance_type.clone(),
                phase: activation.activation_state.clone(),
                control_plane_url: activation.control_plane_url.clone(),
                activated_at_utc: activation.activated_at_utc.clone(),
                redeemed_at_utc: activation.redeemed_at_utc.clone(),
                last_heartbeat_at_utc: activation.last_heartbeat_at_utc.clone(),
                ready_for_stream: activation.last_ready_for_stream,
                runtime_token_present: !activation.runtime_token.is_empty(),
                activation_record_id_present: !activation.activation_record_id.is_empty(),
            },
            network: NetworkView {
                public_url: network.public_url.clone(),
                local_url: String::new(),
            },
            audio,
            display,
            runtime: RuntimeView::default(),
            support: SupportView::default(),
            paths: build_paths(&bundle_root, &HostStatusData::default()),
            host_user_daemon_task_health: Value::Null,
            windows_native_diagnostic_reports: Value::Null,
        });
    }

    let status = load_host_status(&bundle_root)?;
    let service_state = load_service_state(&bundle_root)?;
    let paths = build_paths(&bundle_root, &status);
    let audio = load_audio_view(&bundle_root, &status);
    let display = load_display_view(&bundle_root);

    Ok(ShellState {
        auth: AuthView {
            password_exists,
            needs_password_setup: !password_exists,
            unlocked,
        },
        bundle_root: bundle_root.display().to_string(),
        installer_available: host_installer_path(&bundle_root).exists(),
        install: InstallView {
            installed_mode,
            install_root,
            data_root: bundle_root.display().to_string(),
            uninstall_registered: uninstall_registration_exists(install_layout.as_ref()),
            launch_intent,
        },
        activation: ActivationView {
            host_id: activation.host_id.clone(),
            display_name: activation.display_name.clone(),
            sentinel_pc_id: activation.sentinel_pc_id.clone(),
            sentinel_device_id: activation.sentinel_device_id.clone(),
            keeper_entry_id: activation.keeper_entry_id.clone(),
            token_kind: activation.setup_token_kind.clone(),
            instance_type: activation.instance_type.clone(),
            phase: activation.activation_state.clone(),
            control_plane_url: activation.control_plane_url.clone(),
            activated_at_utc: activation.activated_at_utc.clone(),
            redeemed_at_utc: activation.redeemed_at_utc.clone(),
            last_heartbeat_at_utc: activation.last_heartbeat_at_utc.clone(),
            ready_for_stream: activation.last_ready_for_stream,
            runtime_token_present: !activation.runtime_token.is_empty(),
            activation_record_id_present: !activation.activation_record_id.is_empty(),
        },
        network: NetworkView {
            public_url: network.public_url.clone(),
            local_url: status.local_url.clone(),
        },
        audio,
        display,
        runtime: RuntimeView {
            lifecycle_phase: non_empty_or(&status.lifecycle_phase, "unknown"),
            health_grade: non_empty_or(&status.health_grade, "unknown"),
            audio_status: non_empty_or(&status.audio_status, "unknown"),
            service_state,
            runtime_label: status.runtime_label.clone(),
            runtime_key: status.runtime_key.clone(),
            runtime_profile_key: status.runtime_profile_key.clone(),
            runtime_version: status.runtime_version.clone(),
            encoder: status.encoder.clone(),
            capture: status.capture.clone(),
            capture_reason: status.capture_reason.clone(),
            selection_reason: status.selection_reason.clone(),
            ffmpeg_source: status.ffmpeg_source.clone(),
            fallback_runtime_label: status.fallback_runtime_label.clone(),
            fallback_runtime_version: status.fallback_runtime_version.clone(),
            fallback_runtime_reason: status.fallback_runtime_reason.clone(),
            warnings: status.warnings.clone(),
            local_http_ready: status.local_http_ready,
            required_processes_ready: status.required_processes_ready,
        },
        support: SupportView {
            support_bundle_count: status.support_bundle_count,
            last_support_bundle_id: status.last_support_bundle_id.clone(),
            last_support_bundle_path: last_support_bundle_path(
                &bundle_root,
                &status.last_support_bundle_id,
            ),
            raw_status_json: status.raw_json.clone(),
        },
        paths,
        host_user_daemon_task_health: read_json_file_value(
            &bundle_root
                .join("moonlight")
                .join("server")
                .join("host_user_daemon_task_health.json"),
        ),
        windows_native_diagnostic_reports: read_json_file_value(
            &bundle_root
                .join("moonlight")
                .join("server")
                .join("windows_native_diagnostic_reports.json"),
        ),
    })
}

fn ensure_unlocked(session: &State<'_, AppSession>) -> Result<(), String> {
    let unlocked = session
        .unlocked
        .lock()
        .map_err(|_| "Session lock failed.")?
        .to_owned();
    let bundle_root = resolve_bundle_root()?;
    if admin_password_store_path(&bundle_root).exists() && !unlocked {
        return Err("Unlock Cloudgime Host Control first.".into());
    }
    Ok(())
}

fn ensure_admin_password_matches(bundle_root: &Path, password: &str) -> Result<(), String> {
    let store_path = admin_password_store_path(bundle_root);
    if !store_path.exists() {
        return Err("Admin password has not been created yet.".into());
    }

    if !verify_password_store(&store_path, password)? {
        return Err("Password admin tidak cocok.".into());
    }

    Ok(())
}

fn resolve_launch_intent() -> String {
    let mut args = env::args();
    while let Some(arg) = args.next() {
        if arg.eq_ignore_ascii_case("--intent") {
            if let Some(value) = args.next() {
                return value.trim().to_ascii_lowercase();
            }
        }
    }

    String::new()
}

fn program_data_root() -> PathBuf {
    env::var("ProgramData")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

fn program_files_root() -> PathBuf {
    env::var("ProgramFiles")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

fn default_installed_bundle_root() -> PathBuf {
    program_data_root().join("Cloudgime").join("Host")
}

fn default_installed_app_root() -> PathBuf {
    program_files_root().join(DEFAULT_INSTALLED_PRODUCT_NAME)
}

fn installed_layout_path_for_install_root(install_root: &Path) -> PathBuf {
    install_root.join("install-layout.json")
}

fn load_installed_layout_from_path(path: &Path) -> Option<InstalledLayoutRecord> {
    if !path.exists() {
        return None;
    }

    let raw = fs::read_to_string(path).ok()?;
    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut layout: InstalledLayoutRecord = serde_json::from_str(clean).ok()?;
    if layout.schema_version != 1 {
        return None;
    }

    if layout.install_root.trim().is_empty() {
        layout.install_root = default_installed_app_root().display().to_string();
    }
    if layout.bundle_root.trim().is_empty() {
        layout.bundle_root = default_installed_bundle_root().display().to_string();
    }
    if layout.product_name.trim().is_empty() {
        layout.product_name = DEFAULT_INSTALLED_PRODUCT_NAME.into();
    }
    if layout.uninstall_registry_key.trim().is_empty() {
        layout.uninstall_registry_key = DEFAULT_UNINSTALL_REGISTRY_KEY.into();
    }
    if layout.app_executable_name.trim().is_empty() {
        layout.app_executable_name = "hostcontrolapptauri.exe".into();
    }

    Some(layout)
}

fn load_installed_layout() -> Option<InstalledLayoutRecord> {
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(layout) =
                load_installed_layout_from_path(&installed_layout_path_for_install_root(parent))
            {
                return Some(layout);
            }
        }
    }

    load_installed_layout_from_path(&installed_layout_path_for_install_root(
        &default_installed_app_root(),
    ))
}

fn uninstall_registry_key(layout: Option<&InstalledLayoutRecord>) -> String {
    layout
        .map(|value| value.uninstall_registry_key.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_UNINSTALL_REGISTRY_KEY.into())
}

fn uninstall_registration_exists(layout: Option<&InstalledLayoutRecord>) -> bool {
    let key = uninstall_registry_key(layout);
    let mut command = Command::new("reg");
    command
        .args(["query", &key])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000);
    }

    let status = command.status();

    matches!(status, Ok(value) if value.success())
}

fn is_installed_mode(bundle_root: &Path, layout: Option<&InstalledLayoutRecord>) -> bool {
    (default_installed_bundle_root() == bundle_root && default_installed_app_root().exists())
        || layout
            .map(|value| {
                PathBuf::from(value.bundle_root.trim()) == bundle_root
                    && PathBuf::from(value.install_root.trim()).exists()
            })
            .unwrap_or(false)
}

fn schedule_installed_uninstall(
    bundle_root: &Path,
    install_root: &Path,
    layout: Option<&InstalledLayoutRecord>,
) -> Result<(), String> {
    let bootstrap_path = host_bootstrap_path(install_root);
    if !bootstrap_path.exists() {
        return Err("Could not find the installed bootstrap helper.".into());
    }

    let temp_bootstrap = env::temp_dir().join(format!(
        "cloudgime-host-uninstall-{}.exe",
        Utc::now().timestamp_millis()
    ));
    fs::copy(&bootstrap_path, &temp_bootstrap)
        .map_err(|_| "Could not stage the uninstall helper executable.")?;

    let current_pid = std::process::id();
    let uninstall_key = uninstall_registry_key(layout);
    let app_installer_product_code = layout
        .map(|value| value.app_installer_product_code.trim().to_string())
        .unwrap_or_default();
    let app_installer_registry_path = layout
        .map(|value| value.app_installer_registry_path.trim().to_string())
        .unwrap_or_default();
    let desktop_shortcut = program_data_root()
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(format!("{DEFAULT_INSTALLED_PRODUCT_NAME}.lnk"));
    let start_menu_folder = program_data_root()
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(DEFAULT_INSTALLED_PRODUCT_NAME);
    let public_desktop_shortcut = env::var("PUBLIC")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
        .join("Desktop")
        .join(format!("{DEFAULT_INSTALLED_PRODUCT_NAME}.lnk"));
    let mut arguments = vec![
        "run-installed-uninstall".to_string(),
        "--bundle-root".to_string(),
        bundle_root.display().to_string(),
        "--install-root".to_string(),
        install_root.display().to_string(),
        "--target-pid".to_string(),
        current_pid.to_string(),
        "--uninstall-registry-key".to_string(),
        uninstall_key,
        "--start-menu-shortcut".to_string(),
        desktop_shortcut.display().to_string(),
        "--start-menu-folder".to_string(),
        start_menu_folder.display().to_string(),
        "--public-desktop-shortcut".to_string(),
        public_desktop_shortcut.display().to_string(),
    ];

    if !app_installer_product_code.is_empty() {
        arguments.push("--app-installer-product-code".to_string());
        arguments.push(app_installer_product_code);
    }

    if !app_installer_registry_path.is_empty() {
        arguments.push("--app-installer-registry-path".to_string());
        arguments.push(app_installer_registry_path);
    }

    shell_execute_runas_hidden(&temp_bootstrap, &arguments)
        .map_err(|_| "Could not start the uninstall helper.")?;

    Ok(())
}

fn launch_standalone_emergency_uninstaller(
    bundle_root: &Path,
    install_root: &Path,
) -> Result<(), String> {
    let uninstaller_path = emergency_uninstaller_path(install_root);
    if !uninstaller_path.exists() {
        return Err("Emergency uninstaller was not found in the installed app folder.".into());
    }

    let temp_uninstaller = env::temp_dir().join(format!(
        "cloudgime-host-emergency-{}.exe",
        Utc::now().timestamp_millis()
    ));
    fs::copy(&uninstaller_path, &temp_uninstaller)
        .map_err(|_| "Could not stage the emergency uninstaller executable.")?;

    let arguments = vec![
        "uninstall".to_string(),
        "--install-root".to_string(),
        install_root.display().to_string(),
        "--bundle-root".to_string(),
        bundle_root.display().to_string(),
        "--no-confirm".to_string(),
        "--silent".to_string(),
        "--keep-shared-state".to_string(),
    ];

    shell_execute_runas_hidden(&temp_uninstaller, &arguments)
        .map_err(|_| "Could not start the emergency uninstaller.")?;

    Ok(())
}

fn resolve_bundle_root() -> Result<PathBuf, String> {
    let mut args = env::args();
    while let Some(arg) = args.next() {
        if arg.eq_ignore_ascii_case("--bundle-root") {
            if let Some(value) = args.next() {
                let candidate = PathBuf::from(value);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }

    if let Ok(value) = env::var("HOST_CONTROL_BUNDLE_ROOT") {
        let candidate = PathBuf::from(value);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Some(layout) = load_installed_layout() {
        let candidate = PathBuf::from(layout.bundle_root.trim());
        if host_installer_path(&candidate).exists() {
            return Ok(candidate);
        }
    }

    let installed_default = default_installed_bundle_root();
    if host_installer_path(&installed_default).exists() {
        return Ok(installed_default);
    }

    let mut roots = Vec::new();
    if let Ok(current) = env::current_dir() {
        roots.push(current);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }

    for root in roots {
        for ancestor in root.ancestors() {
            let direct = ancestor.to_path_buf();
            if host_installer_path(&direct).exists() {
                return Ok(direct);
            }

            let export_mon = ancestor.join("export").join("mon1");
            if host_installer_path(&export_mon).exists() {
                return Ok(export_mon);
            }
        }
    }

    Err("Could not resolve a valid host bundle root.".into())
}

fn host_installer_path(bundle_root: &Path) -> PathBuf {
    bundle_root.join("host-installer.exe")
}

fn host_bootstrap_path(install_root: &Path) -> PathBuf {
    install_root.join("cloudgime-host-bootstrap.exe")
}

fn emergency_uninstaller_path(install_root: &Path) -> PathBuf {
    install_root.join("uninstaller-cloudgime.exe")
}

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: *mut c_void,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show_cmd: i32,
    ) -> isize;
}

#[cfg(target_os = "windows")]
fn shell_execute_runas_hidden(executable: &Path, arguments: &[String]) -> Result<(), String> {
    const SW_HIDE: i32 = 0;

    let operation = wide_null(OsStr::new("runas"));
    let file = wide_null(executable.as_os_str());
    let parameters = wide_null(OsStr::new(&join_windows_arguments(arguments)));
    let directory = executable
        .parent()
        .map(|value| wide_null(value.as_os_str()))
        .unwrap_or_else(|| wide_null(OsStr::new("")));

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            directory.as_ptr(),
            SW_HIDE,
        )
    };

    if result <= 32 {
        return Err(format!("ShellExecuteW failed with code {result}."));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn join_windows_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|value| quote_windows_argument(value))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "windows")]
fn quote_windows_argument(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }

    if !value
        .bytes()
        .any(|byte| matches!(byte, b' ' | b'\t' | b'"'))
    {
        return value.to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
                quoted.push(ch);
            }
        }
    }

    if backslashes > 0 {
        quoted.push_str(&"\\".repeat(backslashes * 2));
    }

    quoted.push('"');
    quoted
}

#[cfg(target_os = "windows")]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(not(target_os = "windows"))]
fn shell_execute_runas_hidden(_executable: &Path, _arguments: &[String]) -> Result<(), String> {
    Err("Elevated uninstall helper is only supported on Windows.".into())
}

fn admin_password_store_path(bundle_root: &Path) -> PathBuf {
    bundle_root
        .join("moonlight")
        .join("server")
        .join("host_control_admin.json")
}

fn activation_state_path(bundle_root: &Path) -> PathBuf {
    bundle_root
        .join("moonlight")
        .join("server")
        .join("host_activation_state.json")
}

fn capability_profile_path(bundle_root: &Path) -> PathBuf {
    bundle_root
        .join("moonlight")
        .join("server")
        .join("host_capability_profile.json")
}

fn audio_preferences_path(bundle_root: &Path) -> PathBuf {
    bundle_root
        .join("moonlight")
        .join("server")
        .join("host_audio_preferences.json")
}

fn display_preferences_path(bundle_root: &Path) -> PathBuf {
    bundle_root
        .join("moonlight")
        .join("server")
        .join(STREAM_DISPLAY_PREFERENCES_FILE_NAME)
}

fn shared_pc_identity_path() -> PathBuf {
    program_data_root()
        .join("Cloudgime")
        .join("pc_identity.json")
}

fn pending_uninstall_marker_path() -> PathBuf {
    program_data_root()
        .join("Cloudgime")
        .join("pending_uninstall.json")
}

fn host_keeper_root(bundle_root: &Path) -> PathBuf {
    bundle_root.join("keeper-tunnel")
}

fn host_keeper_agent_path(bundle_root: &Path) -> PathBuf {
    host_keeper_root(bundle_root).join("cloudgimehosttunnel.exe")
}

fn host_keeper_data_root(bundle_root: &Path) -> PathBuf {
    host_keeper_root(bundle_root).join("data")
}

fn host_keeper_env_path(bundle_root: &Path) -> PathBuf {
    host_keeper_data_root(bundle_root).join("cloudrental.env")
}

fn host_keeper_status_path(bundle_root: &Path) -> PathBuf {
    host_keeper_data_root(bundle_root).join("keeper-tunnel-status.json")
}

fn legacy_keeper_data_root() -> PathBuf {
    program_data_root().join("Cloudgime").join("Keeper").join("data")
}

fn legacy_keeper_env_path() -> PathBuf {
    legacy_keeper_data_root().join("cloudrental.env")
}

fn normalize_pc_id_value(value: &str) -> String {
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return String::new();
    }
    let normalized = digits.trim_start_matches('0').to_string();
    let candidate = if normalized.is_empty() { "0" } else { normalized.as_str() };
    match candidate.parse::<u32>() {
        Ok(parsed) if parsed > 0 => parsed.to_string(),
        _ => String::new(),
    }
}

fn normalize_pc_number_value(value: &str) -> String {
    let pc_id = normalize_pc_id_value(value);
    if pc_id.is_empty() {
        return String::new();
    }
    match pc_id.parse::<u32>() {
        Ok(parsed) if parsed > 0 => format!("{parsed:02}"),
        _ => String::new(),
    }
}

fn normalize_keeper_entry_id_value(value: &str, pc_number: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        return trimmed.to_ascii_lowercase();
    }
    if pc_number.is_empty() {
        return String::new();
    }
    format!("inst{pc_number}").to_ascii_lowercase()
}

fn normalize_local_keeper_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return DEFAULT_HOST_KEEPER_LOCAL_URL.into();
    }
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

fn resolve_host_keeper_local_url(bundle_root: &Path) -> String {
    load_host_status(bundle_root)
        .ok()
        .map(|status| normalize_local_keeper_url(&status.local_url))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_HOST_KEEPER_LOCAL_URL.into())
}

fn read_simple_env_value(path: &Path, key: &str) -> String {
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        if raw_key.trim().eq_ignore_ascii_case(key) {
            return raw_value.trim().to_string();
        }
    }

    String::new()
}

fn write_host_keeper_env(
    bundle_root: &Path,
    activation: &HostActivationStateRecord,
    device_token: &str,
) -> Result<(), String> {
    let agent_path = host_keeper_agent_path(bundle_root);
    if !agent_path.exists() {
        return Err("cloudgimehosttunnel.exe belum ikut di bundle host ini. Build Host Setup terbaru dulu.".into());
    }

    let device_id = activation.sentinel_device_id.trim().to_string();
    let pc_id = normalize_pc_id_value(&activation.sentinel_pc_id);
    let pc_number = normalize_pc_number_value(&activation.sentinel_pc_id);
    let keeper_entry_id = normalize_keeper_entry_id_value(&activation.keeper_entry_id, &pc_number);
    let slot_label = if !device_id.is_empty() {
        device_id.clone()
    } else if !pc_number.is_empty() {
        format!("PC-{pc_number}")
    } else {
        "HOST-STREAM".into()
    };

    if device_id.is_empty() || device_token.trim().is_empty() || pc_id.is_empty() {
        return Err("Binding Lisensi Aktivasi belum lengkap. deviceId, deviceToken, dan pcId wajib terisi.".into());
    }

    let env_lines = [
        format!("CLOUDRENTAL_API_BASE={}", activation.control_plane_url.trim()),
        format!("CLOUDRENTAL_DEVICE_ID_DEFAULT={device_id}"),
        format!("CLOUDRENTAL_DEVICE_TOKEN={}", device_token.trim()),
        format!("CLOUDRENTAL_PC_ID_DEFAULT={pc_id}"),
        format!("CLOUDRENTAL_PC_NUMBER_DEFAULT={pc_number}"),
        format!("CLOUDRENTAL_SLOT_LABEL_DEFAULT={slot_label}"),
        format!("CLOUDRENTAL_KEEPER_ENTRY_ID={keeper_entry_id}"),
        "CLOUDRENTAL_TUNNEL_ROUTES=moonlight".into(),
    ];

    if let Some(parent) = host_keeper_env_path(bundle_root).parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create the host keeper data folder.")?;
    }

    write_text_file_with_retry(
        &host_keeper_env_path(bundle_root),
        &env_lines.join("\r\n"),
        "Could not write the host keeper env file.",
    )
}

fn read_host_keeper_device_token(bundle_root: &Path) -> String {
    let current = read_simple_env_value(&host_keeper_env_path(bundle_root), "CLOUDRENTAL_DEVICE_TOKEN");
    if !current.trim().is_empty() {
        return current;
    }

    read_simple_env_value(&legacy_keeper_env_path(), "CLOUDRENTAL_DEVICE_TOKEN")
}

fn read_host_keeper_device_id(bundle_root: &Path) -> String {
    let current = read_simple_env_value(
        &host_keeper_env_path(bundle_root),
        "CLOUDRENTAL_DEVICE_ID_DEFAULT",
    );
    if !current.trim().is_empty() {
        return current.trim().to_string();
    }

    read_simple_env_value(
        &legacy_keeper_env_path(),
        "CLOUDRENTAL_DEVICE_ID_DEFAULT",
    )
    .trim()
    .to_string()
}

fn read_host_keeper_pc_id(bundle_root: &Path) -> String {
    let current = normalize_pc_id_value(&read_simple_env_value(
        &host_keeper_env_path(bundle_root),
        "CLOUDRENTAL_PC_ID_DEFAULT",
    ));
    if !current.trim().is_empty() {
        return current;
    }

    normalize_pc_id_value(&read_simple_env_value(
        &legacy_keeper_env_path(),
        "CLOUDRENTAL_PC_ID_DEFAULT",
    ))
}

fn read_host_keeper_entry_id(bundle_root: &Path, pc_number: &str) -> String {
    let current = normalize_keeper_entry_id_value(
        &read_simple_env_value(
            &host_keeper_env_path(bundle_root),
            "CLOUDRENTAL_KEEPER_ENTRY_ID",
        ),
        pc_number,
    );
    if !current.trim().is_empty() {
        return current;
    }

    normalize_keeper_entry_id_value(
        &read_simple_env_value(
            &legacy_keeper_env_path(),
            "CLOUDRENTAL_KEEPER_ENTRY_ID",
        ),
        pc_number,
    )
}

fn activation_allows_local_runtime(
    bundle_root: &Path,
    activation: &HostActivationStateRecord,
) -> bool {
    if activation.activation_state.eq_ignore_ascii_case("activated")
        && !activation.host_id.trim().is_empty()
    {
        return true;
    }

    let phase_ready = activation.activation_state.eq_ignore_ascii_case("prepared_local")
        || activation
            .activation_state
            .eq_ignore_ascii_case("locked_waiting_token");
    if !phase_ready {
        return false;
    }

    let sentinel_pc_id = if activation.sentinel_pc_id.trim().is_empty() {
        read_host_keeper_pc_id(bundle_root)
    } else {
        normalize_pc_id_value(&activation.sentinel_pc_id)
    };
    let sentinel_device_id = if activation.sentinel_device_id.trim().is_empty() {
        read_host_keeper_device_id(bundle_root)
    } else {
        activation.sentinel_device_id.trim().to_string()
    };

    !activation.host_id.trim().is_empty()
        && !sentinel_pc_id.trim().is_empty()
        && !sentinel_device_id.trim().is_empty()
        && !read_host_keeper_device_token(bundle_root).trim().is_empty()
}

fn write_host_keeper_status(bundle_root: &Path) -> Result<(), String> {
    let local_url = resolve_host_keeper_local_url(bundle_root);
    let status = serde_json::json!({
        "routes": [
            {
                "name": "panel",
                "localUrl": DEFAULT_HOST_PANEL_LOCAL_URL,
                "panelLocalUrl": DEFAULT_HOST_PANEL_LOCAL_URL,
            },
            {
                "name": "moonlight",
                "localUrl": local_url,
                "moonlightLocalUrl": local_url,
            }
        ]
    });
    let raw = serde_json::to_string_pretty(&status)
        .map_err(|_| "Could not encode the host keeper tunnel status.")?;
    if let Some(parent) = host_keeper_status_path(bundle_root).parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create the host keeper status folder.")?;
    }
    write_text_file_with_retry(
        &host_keeper_status_path(bundle_root),
        &raw,
        "Could not write the host keeper tunnel status.",
    )
}

fn run_hidden_command_output(
    executable: &str,
    args: &[&str],
    working_directory: &Path,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(working_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start {executable}: {error}"))?;

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started_at.elapsed() >= timeout => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|_| format!("Could not collect output from {executable}."))?;
                return Ok(CommandOutput {
                    success: false,
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: format!(
                        "{}{}{} timed out after {} seconds.",
                        String::from_utf8_lossy(&output.stderr).trim(),
                        if output.stderr.is_empty() { "" } else { "\n" },
                        executable,
                        timeout.as_secs()
                    ),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(_) => return Err(format!("Could not monitor {executable}.")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|_| format!("Could not collect output from {executable}."))?;

    Ok(CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn escape_powershell_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn run_hidden_powershell_script(
    script: &str,
    working_directory: &Path,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    run_hidden_command_output(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        working_directory,
        timeout,
    )
}

fn register_host_keeper_tunnel_task(bundle_root: &Path) -> Result<(), String> {
    let task_name = escape_powershell_literal(HOST_KEEPER_TUNNEL_TASK_NAME);
    let exe_path = escape_powershell_literal(
        &host_keeper_agent_path(bundle_root).to_string_lossy(),
    );
    let working_dir = escape_powershell_literal(
        &host_keeper_root(bundle_root).to_string_lossy(),
    );
    let script_template = concat!(
        "$ErrorActionPreference = 'Stop';",
        "$taskName = '__TASK_NAME__';",
        "$exePath = '__EXE_PATH__';",
        "$workingDir = '__WORKING_DIR__';",
        "$action = New-ScheduledTaskAction -Execute $exePath -WorkingDirectory $workingDir;",
        "$startupTrigger = New-ScheduledTaskTrigger -AtStartup;",
        "$logonTrigger = New-ScheduledTaskTrigger -AtLogOn;",
        "$settings = New-ScheduledTaskSettingsSet ",
        "-AllowStartIfOnBatteries ",
        "-DontStopIfGoingOnBatteries ",
        "-StartWhenAvailable ",
        "-MultipleInstances IgnoreNew ",
        "-Hidden ",
        "-RestartCount 999 ",
        "-RestartInterval (New-TimeSpan -Minutes 1) ",
        "-ExecutionTimeLimit (New-TimeSpan -Seconds 0);",
        "$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest;",
        "Register-ScheduledTask -TaskName $taskName -Action $action -Trigger @($startupTrigger,$logonTrigger) -Settings $settings -Principal $principal -Force | Out-Null;",
        "$taskXmlRaw = Export-ScheduledTask -TaskName $taskName;",
        "[xml]$taskXml = $taskXmlRaw;",
        "$ns = New-Object System.Xml.XmlNamespaceManager($taskXml.NameTable);",
        "$ns.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task');",
        "$settingsNode = $taskXml.SelectSingleNode('/t:Task/t:Settings', $ns);",
        "if ($null -eq $settingsNode) { throw 'Settings node missing in keeper tunnel task XML.'; }",
        "$taskXml.DocumentElement.SetAttribute('version', '1.3');",
        "while ($settingsNode.HasChildNodes) { [void]$settingsNode.RemoveChild($settingsNode.FirstChild) };",
        "function New-TaskNode([string]$name, [string]$value) {",
        "  $node = $taskXml.CreateElement($name, $taskXml.DocumentElement.NamespaceURI);",
        "  $node.InnerText = $value;",
        "  return $node;",
        "};",
        "function New-RestartOnFailureNode([string]$count, [string]$interval) {",
        "  $node = $taskXml.CreateElement('RestartOnFailure', $taskXml.DocumentElement.NamespaceURI);",
        "  $countNode = $taskXml.CreateElement('Count', $taskXml.DocumentElement.NamespaceURI);",
        "  $countNode.InnerText = $count;",
        "  [void]$node.AppendChild($countNode);",
        "  $intervalNode = $taskXml.CreateElement('Interval', $taskXml.DocumentElement.NamespaceURI);",
        "  $intervalNode.InnerText = $interval;",
        "  [void]$node.AppendChild($intervalNode);",
        "  return $node;",
        "};",
        "function New-IdleSettingsNode() {",
        "  $node = $taskXml.CreateElement('IdleSettings', $taskXml.DocumentElement.NamespaceURI);",
        "  $durationNode = $taskXml.CreateElement('Duration', $taskXml.DocumentElement.NamespaceURI);",
        "  $durationNode.InnerText = 'PT10M';",
        "  [void]$node.AppendChild($durationNode);",
        "  $waitNode = $taskXml.CreateElement('WaitTimeout', $taskXml.DocumentElement.NamespaceURI);",
        "  $waitNode.InnerText = 'PT1H';",
        "  [void]$node.AppendChild($waitNode);",
        "  $stopNode = $taskXml.CreateElement('StopOnIdleEnd', $taskXml.DocumentElement.NamespaceURI);",
        "  $stopNode.InnerText = 'false';",
        "  [void]$node.AppendChild($stopNode);",
        "  $restartNode = $taskXml.CreateElement('RestartOnIdle', $taskXml.DocumentElement.NamespaceURI);",
        "  $restartNode.InnerText = 'true';",
        "  [void]$node.AppendChild($restartNode);",
        "  return $node;",
        "};",
        "[void]$settingsNode.AppendChild((New-TaskNode 'DisallowStartIfOnBatteries' 'false'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'StopIfGoingOnBatteries' 'false'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'ExecutionTimeLimit' 'PT0S'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'Hidden' 'true'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'MultipleInstancesPolicy' 'IgnoreNew'));",
        "[void]$settingsNode.AppendChild((New-RestartOnFailureNode '999' 'PT1M'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'StartWhenAvailable' 'true'));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'RunOnlyIfIdle' 'false'));",
        "[void]$settingsNode.AppendChild((New-IdleSettingsNode));",
        "[void]$settingsNode.AppendChild((New-TaskNode 'UseUnifiedSchedulingEngine' 'true'));",
        "$tempXml = Join-Path $env:TEMP ('cloudgime-keeper-task-' + [guid]::NewGuid().ToString('N') + '.xml');",
        "try {",
        "  $taskXml.Save($tempXml);",
        "  schtasks.exe /Create /TN $taskName /XML $tempXml /F | Out-Null;",
        "} finally {",
        "  Remove-Item $tempXml -Force -ErrorAction SilentlyContinue;",
        "};",
        "Start-ScheduledTask -TaskName $taskName;"
    );
    let script = script_template
        .replace("__TASK_NAME__", &task_name)
        .replace("__EXE_PATH__", &exe_path)
        .replace("__WORKING_DIR__", &working_dir);

    run_hidden_powershell_script(
        &script,
        &host_keeper_root(bundle_root),
        Duration::from_secs(45),
    )
    .map(|_| ())
}

fn unregister_host_keeper_tunnel_task(bundle_root: &Path) {
    let task_name = escape_powershell_literal(HOST_KEEPER_TUNNEL_TASK_NAME);
    let stop_script = format!(
        concat!(
            "$ErrorActionPreference = 'SilentlyContinue';",
            "$taskName = '{task_name}';",
            "Stop-ScheduledTask -TaskName $taskName;",
            "exit 0;"
        ),
        task_name = task_name,
    );
    let delete_script = format!(
        concat!(
            "$ErrorActionPreference = 'SilentlyContinue';",
            "$taskName = '{task_name}';",
            "Unregister-ScheduledTask -TaskName $taskName -Confirm:$false;",
            "exit 0;"
        ),
        task_name = task_name,
    );
    let working_dir = host_keeper_root(bundle_root);
    let _ = run_hidden_powershell_script(&stop_script, &working_dir, Duration::from_secs(20));
    let _ = run_hidden_powershell_script(&delete_script, &working_dir, Duration::from_secs(20));
}

fn spawn_host_keeper_agent_hidden(bundle_root: &Path) -> Result<(), String> {
    let agent_path = host_keeper_agent_path(bundle_root);
    if !agent_path.exists() {
        return Err("cloudgimehosttunnel.exe belum ditemukan di bundle host.".into());
    }

    let mut command = Command::new(&agent_path);
    command
        .current_dir(host_keeper_root(bundle_root))
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000);
    }

    command
        .spawn()
        .map_err(|error| format!("Could not start the host keeper tunnel agent: {error}"))?;
    Ok(())
}

fn stop_keeper_tunnel_processes(bundle_root: &Path) {
    let keeper_root = host_keeper_root(bundle_root);
    let mut keeper_root_text = keeper_root.to_string_lossy().replace('/', "\\");
    if !keeper_root_text.ends_with('\\') {
        keeper_root_text.push('\\');
    }
    let keeper_root = escape_powershell_literal(&keeper_root_text);
    let script = format!(
        concat!(
            "$ErrorActionPreference = 'SilentlyContinue';",
            "$root = '{keeper_root}';",
            "Get-CimInstance Win32_Process -Filter \"Name = 'cloudgimehosttunnel.exe'\" | ",
            "Where-Object {{ $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase) }} | ",
            "ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }};",
            "exit 0;"
        ),
        keeper_root = keeper_root,
    );
    let _ = run_hidden_powershell_script(
        &script,
        &host_keeper_root(bundle_root),
        Duration::from_secs(20),
    );
    thread::sleep(Duration::from_millis(350));
}

fn configure_host_keeper_tunnel(
    bundle_root: &Path,
    activation: &HostActivationStateRecord,
    device_token_override: Option<&str>,
) -> Result<(), String> {
    let explicit_request = device_token_override.is_some();
    let device_token = device_token_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| read_host_keeper_device_token(bundle_root));
    if device_token.trim().is_empty() {
        if explicit_request {
            return Err("Aktivasi Lisensi berhasil, tapi device token keeper belum ikut dikirim backend.".into());
        }
        return Ok(());
    }

    write_host_keeper_env(bundle_root, activation, &device_token)?; 
    write_host_keeper_status(bundle_root)?; 
    stop_keeper_tunnel_processes(bundle_root);
    unregister_host_keeper_tunnel_task(bundle_root);

    let task_result = register_host_keeper_tunnel_task(bundle_root);
    let spawn_result = spawn_host_keeper_agent_hidden(bundle_root);
    if let Err(task_error) = task_result {
        spawn_result.map_err(|spawn_error| {
            format!("{task_error} Fallback start also failed: {spawn_error}")
        })?;
    }

    Ok(())
}

fn remove_host_keeper_tunnel(bundle_root: &Path) -> Result<(), String> {
    stop_keeper_tunnel_processes(bundle_root);
    unregister_host_keeper_tunnel_task(bundle_root);
    let _ = fs::remove_file(host_keeper_env_path(bundle_root));
    let _ = fs::remove_file(host_keeper_status_path(bundle_root));
    Ok(())
}

fn write_text_file_with_retry(path: &Path, raw: &str, write_error: &str) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..5 {
        match fs::write(path, raw) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt < 4 {
                    thread::sleep(Duration::from_millis(150));
                }
            }
        }
    }

    let detail = last_error
        .as_ref()
        .map(|error| error.to_string())
        .unwrap_or_else(|| "unknown write error".into());
    Err(format!("{write_error} ({detail})"))
}

fn write_password_store(path: &Path, password: &str) -> Result<(), String> {
    let normalized_password = normalize_admin_password_for_storage(password);
    let mut salt = [0u8; SALT_LENGTH];
    fill_random(&mut salt).map_err(|_| "Could not generate a secure password salt.")?;

    let mut hash = [0u8; HASH_LENGTH];
    pbkdf2_hmac::<Sha256>(
        normalized_password.as_bytes(),
        &salt,
        PASSWORD_ITERATIONS,
        &mut hash,
    );

    let record = PasswordRecord {
        schema_version: 1,
        iterations: PASSWORD_ITERATIONS,
        salt_base64: Base64.encode(salt),
        hash_base64: Base64.encode(hash),
        updated_at_utc: Utc::now().to_rfc3339(),
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not create the admin password folder.")?;
    }

    let raw = serde_json::to_string_pretty(&record)
        .map_err(|_| "Could not encode the admin password store.")?;
    write_text_file_with_retry(path, &raw, "Could not write the admin password store.")?;
    Ok(())
}

fn verify_password_store(path: &Path, password: &str) -> Result<bool, String> {
    let raw = fs::read_to_string(path).map_err(|_| "Could not read the admin password store.")?;
    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let record: PasswordRecord =
        serde_json::from_str(clean).map_err(|_| "The admin password store is invalid.")?;

    if password_matches_record(&record, password)? {
        return Ok(true);
    }

    let normalized_password = normalize_admin_password_for_storage(password);
    if normalized_password == password {
        return Ok(false);
    }

    password_matches_record(&record, normalized_password)
}

fn normalize_admin_password_for_storage(password: &str) -> &str {
    password.trim()
}

fn password_matches_record(record: &PasswordRecord, password: &str) -> Result<bool, String> {
    let salt = Base64
        .decode(record.salt_base64.as_bytes())
        .map_err(|_| "The admin password salt is invalid.")?;
    let expected = Base64
        .decode(record.hash_base64.as_bytes())
        .map_err(|_| "The admin password hash is invalid.")?;

    let mut actual = [0u8; HASH_LENGTH];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, record.iterations, &mut actual);

    Ok(expected.as_slice().ct_eq(&actual).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_password_store_accepts_legacy_password_with_surrounding_spaces() {
        let test_root = env::temp_dir().join(format!("host-control-password-{}", Uuid::new_v4()));
        fs::create_dir_all(&test_root).unwrap();
        let store_path = test_root.join("host_control_admin.json");

        write_legacy_password_store(&store_path, "  rahasia  ").unwrap();

        assert!(verify_password_store(&store_path, "  rahasia  ").unwrap());
        assert!(!verify_password_store(&store_path, "rahasia").unwrap());

        fs::remove_dir_all(&test_root).unwrap();
    }

    #[test]
    fn verify_password_store_falls_back_to_trimmed_password_for_current_store() {
        let test_root = env::temp_dir().join(format!("host-control-password-{}", Uuid::new_v4()));
        fs::create_dir_all(&test_root).unwrap();
        let store_path = test_root.join("host_control_admin.json");

        write_password_store(&store_path, "  rahasia  ").unwrap();

        assert!(verify_password_store(&store_path, "rahasia").unwrap());
        assert!(verify_password_store(&store_path, "  rahasia  ").unwrap());

        fs::remove_dir_all(&test_root).unwrap();
    }

    fn write_legacy_password_store(path: &Path, password: &str) -> Result<(), String> {
        let salt = [7u8; SALT_LENGTH];
        let mut hash = [0u8; HASH_LENGTH];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PASSWORD_ITERATIONS, &mut hash);

        let record = PasswordRecord {
            schema_version: 1,
            iterations: PASSWORD_ITERATIONS,
            salt_base64: Base64.encode(salt),
            hash_base64: Base64.encode(hash),
            updated_at_utc: Utc::now().to_rfc3339(),
        };

        let raw = serde_json::to_string_pretty(&record)
            .map_err(|_| "Could not encode the admin password store.")?;
        write_text_file_with_retry(path, &raw, "Could not write the admin password store.")
    }
}

fn load_activation_state(bundle_root: &Path) -> Result<HostActivationStateRecord, String> {
    let path = activation_state_path(bundle_root);
    let state_file_existed = path.exists();
    let mut needs_persist = !state_file_existed;
    let mut state = if !state_file_existed {
        default_activation_state()
    } else {
        let raw = fs::read_to_string(&path).map_err(|_| "Could not read the activation state.")?;
        let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
        let mut parsed: HostActivationStateRecord =
            serde_json::from_str(clean).map_err(|_| "The activation state file is invalid.")?;
        if parsed.schema_version != 1 {
            parsed = default_activation_state();
            needs_persist = true;
        }
        parsed
    };

    state.machine_identity = if state.machine_identity.trim().is_empty() {
        needs_persist = true;
        ensure_shared_machine_identity(None)
    } else {
        ensure_shared_machine_identity(Some(&state.machine_identity))
    };

    state.host_id = if state.host_id.trim().is_empty() {
        needs_persist = true;
        ensure_shared_host_id(None, &state.machine_identity)
    } else {
        ensure_shared_host_id(Some(&state.host_id), &state.machine_identity)
    };

    state.install_instance_id = normalized_install_instance_id(&state.install_instance_id);
    if state.install_instance_id.trim().is_empty() {
        needs_persist = true;
        state.install_instance_id = random_install_instance_id();
    }

    if state.display_name.trim().is_empty() {
        needs_persist = true;
        state.display_name = default_display_name();
    }

    if state.activation_state.trim().is_empty() {
        needs_persist = true;
        state.activation_state = "installed_unprepared".into();
    } else if state_file_existed
        && state
            .activation_state
            .eq_ignore_ascii_case("installed_unprepared")
    {
        needs_persist = true;
        state.activation_state = "prepared_local".into();
    }

    if should_reset_stale_local_activation(&state) {
        needs_persist = true;
        reset_stale_local_activation(&mut state);
    }

    if state.control_plane_url.trim().is_empty() {
        needs_persist = true;
        state.control_plane_url = DEFAULT_CONTROL_PLANE.into();
    }

    if repair_setup_token_metadata(&mut state) {
        needs_persist = true;
    }

    if let Some(shared) = load_shared_pc_identity() {
        if state.machine_identity.trim().is_empty() && !shared.machine_identity.trim().is_empty() {
            state.machine_identity = shared.machine_identity;
        }
        if state.host_id.trim().is_empty() && !shared.host_id.trim().is_empty() {
            state.host_id = shared.host_id;
        }
        if state.sentinel_pc_id.trim().is_empty() {
            state.sentinel_pc_id = shared.sentinel_pc_id;
        }
        if state.sentinel_device_id.trim().is_empty() {
            state.sentinel_device_id = shared.sentinel_device_id;
        }
        if state.keeper_entry_id.trim().is_empty() {
            state.keeper_entry_id = shared.keeper_entry_id;
        }
    }

    if state.sentinel_pc_id.trim().is_empty() {
        let pc_id = read_host_keeper_pc_id(bundle_root);
        if !pc_id.trim().is_empty() {
            state.sentinel_pc_id = pc_id;
            needs_persist = true;
        }
    }
    if state.sentinel_device_id.trim().is_empty() {
        let device_id = read_host_keeper_device_id(bundle_root);
        if !device_id.trim().is_empty() {
            state.sentinel_device_id = device_id;
            needs_persist = true;
        }
    }
    if state.keeper_entry_id.trim().is_empty() {
        let pc_number = normalize_pc_number_value(&state.sentinel_pc_id);
        let keeper_entry_id = read_host_keeper_entry_id(bundle_root, &pc_number);
        if !keeper_entry_id.trim().is_empty() {
            state.keeper_entry_id = keeper_entry_id;
            needs_persist = true;
        }
    }

    sync_shared_identity_from_activation(&state);
    if needs_persist {
        save_activation_state(bundle_root, &state)?;
    }

    Ok(state)
}

fn save_activation_state(
    bundle_root: &Path,
    state: &HostActivationStateRecord,
) -> Result<(), String> {
    let path = activation_state_path(bundle_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not create the activation state folder.")?;
    }

    let mut next = state.clone();
    next.schema_version = 1;
    next.updated_at_utc = Utc::now().to_rfc3339();
    next.machine_identity = if next.machine_identity.trim().is_empty() {
        ensure_shared_machine_identity(None)
    } else {
        ensure_shared_machine_identity(Some(&next.machine_identity))
    };
    next.host_id = if next.host_id.trim().is_empty() {
        ensure_shared_host_id(None, &next.machine_identity)
    } else {
        ensure_shared_host_id(Some(&next.host_id), &next.machine_identity)
    };
    next.install_instance_id = normalized_install_instance_id(&next.install_instance_id);
    if next.install_instance_id.trim().is_empty() {
        next.install_instance_id = random_install_instance_id();
    }
    if next.display_name.trim().is_empty() {
        next.display_name = default_display_name();
    }
    if next.activation_state.trim().is_empty() {
        next.activation_state = "installed_unprepared".into();
    }
    next.setup_token_kind = next.setup_token_kind.trim().to_string();
    next.instance_type = next.instance_type.trim().to_string();
    let _ = repair_setup_token_metadata(&mut next);
    if next.control_plane_url.trim().is_empty() {
        next.control_plane_url = DEFAULT_CONTROL_PLANE.into();
    }

    let raw = serde_json::to_string_pretty(&next)
        .map_err(|_| "Could not encode the activation state file.")?;
    write_text_file_with_retry(&path, &raw, "Could not write the activation state file.")?;
    sync_shared_identity_from_activation(&next);
    Ok(())
}

fn default_activation_state() -> HostActivationStateRecord {
    let machine_identity = ensure_shared_machine_identity(None);
    HostActivationStateRecord {
        schema_version: 1,
        host_id: ensure_shared_host_id(None, &machine_identity),
        machine_identity,
        install_instance_id: random_install_instance_id(),
        activation_state: "installed_unprepared".into(),
        setup_token_kind: String::new(),
        instance_type: String::new(),
        control_plane_url: DEFAULT_CONTROL_PLANE.into(),
        display_name: default_display_name(),
        sentinel_pc_id: String::new(),
        sentinel_device_id: String::new(),
        keeper_entry_id: String::new(),
        runtime_token: String::new(),
        activated_at_utc: String::new(),
        redeemed_at_utc: String::new(),
        activation_record_id: String::new(),
        last_heartbeat_at_utc: String::new(),
        last_ready_for_stream: false,
        updated_at_utc: String::new(),
    }
}

fn repair_setup_token_metadata(state: &mut HostActivationStateRecord) -> bool {
    if !looks_like_always_on_host_state(state) {
        return false;
    }

    let mut changed = false;
    if state.setup_token_kind.trim().is_empty() {
        state.setup_token_kind = "always_on_host".into();
        changed = true;
    }
    if state.instance_type.trim().is_empty() {
        state.instance_type = "always-on".into();
        changed = true;
    }
    changed
}

fn looks_like_always_on_host_state(state: &HostActivationStateRecord) -> bool {
    let host_id = state.host_id.trim();
    let activation_record_id = state.activation_record_id.trim();
    host_id
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cgslot-"))
        || activation_record_id
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cgslot-"))
        || state
            .setup_token_kind
            .trim()
            .eq_ignore_ascii_case("always_on_host")
        || state
            .instance_type
            .trim()
            .eq_ignore_ascii_case("always-on")
}

fn load_shared_pc_identity() -> Option<SharedPcIdentityRecord> {
    let path = shared_pc_identity_path();
    if !path.exists() {
        return None;
    }

    let raw = fs::read_to_string(path).ok()?;
    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut record: SharedPcIdentityRecord = serde_json::from_str(clean).ok()?;
    if record.schema_version != 1 {
        return None;
    }
    record.host_id = normalized_host_id(&record.host_id);
    record.machine_identity = normalized_machine_identity(&record.machine_identity);
    record.sentinel_pc_id = record.sentinel_pc_id.trim().to_string();
    record.sentinel_device_id = record.sentinel_device_id.trim().to_string();
    record.keeper_entry_id = record.keeper_entry_id.trim().to_string();
    if record.host_id.is_empty()
        && record.machine_identity.is_empty()
        && record.sentinel_pc_id.is_empty()
        && record.sentinel_device_id.is_empty()
    {
        return None;
    }
    Some(record)
}

fn save_shared_pc_identity(path: &Path, record: &SharedPcIdentityRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create the shared PC identity folder.")?;
    }

    let raw = serde_json::to_string_pretty(record)
        .map_err(|_| "Could not encode the shared PC identity.")?;
    write_text_file_with_retry(path, &raw, "Could not write the shared PC identity.")?;
    Ok(())
}

fn ensure_shared_machine_identity(preferred_machine_identity: Option<&str>) -> String {
    let path = shared_pc_identity_path();
    let mut record = load_shared_pc_identity().unwrap_or_default();
    let existing_shared_machine_identity = normalized_machine_identity(&record.machine_identity);
    let preferred = preferred_machine_identity
        .map(normalized_machine_identity)
        .unwrap_or_default();

    let mut machine_identity = if !preferred.is_empty() {
        preferred
    } else if !existing_shared_machine_identity.is_empty() {
        existing_shared_machine_identity
    } else {
        normalized_machine_identity(&record.machine_identity)
    };

    if machine_identity.is_empty() {
        let seed = [
            record.sentinel_device_id.trim(),
            record.sentinel_pc_id.trim(),
            record.keeper_entry_id.trim(),
        ]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("|");

        machine_identity = if seed.is_empty() {
            random_machine_identity()
        } else {
            stable_machine_identity_from_seed(&seed)
        };
    }

    let mut changed = false;
    if record.schema_version != 1 {
        record.schema_version = 1;
        changed = true;
    }
    if record.machine_identity != machine_identity {
        record.machine_identity = machine_identity.clone();
        changed = true;
    }
    if record.updated_at_utc.trim().is_empty() || changed {
        record.updated_at_utc = Utc::now().to_rfc3339();
        changed = true;
    }

    if changed || !path.exists() {
        let _ = save_shared_pc_identity(&path, &record);
    }

    machine_identity
}

fn ensure_shared_host_id(preferred_host_id: Option<&str>, machine_identity: &str) -> String {
    let path = shared_pc_identity_path();
    let mut record = load_shared_pc_identity().unwrap_or_default();
    let existing_shared_host_id = normalized_host_id(&record.host_id);
    let preferred = preferred_host_id
        .map(normalized_host_id)
        .unwrap_or_default();

    let mut host_id = if !preferred.is_empty() {
        preferred
    } else if !existing_shared_host_id.is_empty() {
        existing_shared_host_id
    } else {
        normalized_host_id(&record.host_id)
    };

    if host_id.is_empty() {
        let seed = if !machine_identity.trim().is_empty() {
            machine_identity.trim().to_string()
        } else {
            [
                record.sentinel_device_id.trim(),
                record.sentinel_pc_id.trim(),
                record.keeper_entry_id.trim(),
            ]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("|")
        };

        host_id = if seed.is_empty() {
            random_host_id()
        } else {
            stable_host_id_from_seed(&seed)
        };
    }

    let mut changed = false;
    if record.schema_version != 1 {
        record.schema_version = 1;
        changed = true;
    }
    if !machine_identity.trim().is_empty()
        && record.machine_identity != normalized_machine_identity(machine_identity)
    {
        record.machine_identity = normalized_machine_identity(machine_identity);
        changed = true;
    }
    if record.host_id != host_id {
        record.host_id = host_id.clone();
        changed = true;
    }
    if record.updated_at_utc.trim().is_empty() || changed {
        record.updated_at_utc = Utc::now().to_rfc3339();
        changed = true;
    }

    if changed || !path.exists() {
        let _ = save_shared_pc_identity(&path, &record);
    }

    host_id
}

fn sync_shared_identity_from_activation(state: &HostActivationStateRecord) {
    let path = shared_pc_identity_path();
    let mut record = load_shared_pc_identity().unwrap_or_default();
    let mut changed = false;

    if record.schema_version != 1 {
        record.schema_version = 1;
        changed = true;
    }
    let machine_identity = normalized_machine_identity(&state.machine_identity);
    if !machine_identity.is_empty() && record.machine_identity != machine_identity {
        record.machine_identity = machine_identity;
        changed = true;
    }
    let host_id = normalized_host_id(&state.host_id);
    if !host_id.is_empty() && record.host_id != host_id {
        record.host_id = host_id;
        changed = true;
    }
    let sentinel_pc_id = state.sentinel_pc_id.trim();
    if !sentinel_pc_id.is_empty() && record.sentinel_pc_id != sentinel_pc_id {
        record.sentinel_pc_id = sentinel_pc_id.to_string();
        changed = true;
    }
    let sentinel_device_id = state.sentinel_device_id.trim();
    if !sentinel_device_id.is_empty() && record.sentinel_device_id != sentinel_device_id {
        record.sentinel_device_id = sentinel_device_id.to_string();
        changed = true;
    }
    let keeper_entry_id = state.keeper_entry_id.trim();
    if !keeper_entry_id.is_empty() && record.keeper_entry_id != keeper_entry_id {
        record.keeper_entry_id = keeper_entry_id.to_string();
        changed = true;
    }
    if record.updated_at_utc.trim().is_empty() || changed {
        record.updated_at_utc = Utc::now().to_rfc3339();
        changed = true;
    }

    if changed || !path.exists() {
        let _ = save_shared_pc_identity(&path, &record);
    }
}

fn stable_machine_identity_from_seed(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("cgm-{}", &hex[..32])
}

fn stable_host_id_from_seed(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("cg-{}", &hex[..16])
}

fn random_machine_identity() -> String {
    format!("cgm-{}", Uuid::new_v4().simple())
}

fn random_host_id() -> String {
    format!("cg-{}", &Uuid::new_v4().simple().to_string()[..16])
}

fn random_install_instance_id() -> String {
    format!("cgi-{}", &Uuid::new_v4().simple().to_string()[..16])
}

fn normalized_machine_identity(value: &str) -> String {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.len() < 12 || trimmed.len() > 96 {
        return String::new();
    }
    if !trimmed.starts_with("cgm-") {
        return String::new();
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'))
    {
        return String::new();
    }
    trimmed
}

fn normalized_host_id(value: &str) -> String {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.len() < 6 || trimmed.len() > 64 {
        return String::new();
    }
    let Some(first) = trimmed.chars().next() else {
        return String::new();
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return String::new();
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'))
    {
        return String::new();
    }
    trimmed
}

fn normalized_install_instance_id(value: &str) -> String {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.len() < 8 || trimmed.len() > 64 {
        return String::new();
    }
    if !trimmed.starts_with("cgi-") {
        return String::new();
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'))
    {
        return String::new();
    }
    trimmed
}

fn default_display_name() -> String {
    env::var("COMPUTERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Cloudgime Host".into())
}

fn load_network_config(bundle_root: &Path) -> Result<BundleNetworkConfig, String> {
    let public_url_path = bundle_root.join("PUBLIC_URL.txt");
    let public_url = fs::read_to_string(&public_url_path)
        .map(|raw| raw.trim().to_string())
        .unwrap_or_default();

    Ok(BundleNetworkConfig { public_url })
}

fn load_host_status(bundle_root: &Path) -> Result<HostStatusData, String> {
    let result = run_installer_command(bundle_root, "status")?;
    if !result.success {
        return Ok(HostStatusData::default());
    }

    let value: Value = serde_json::from_str(&result.stdout).unwrap_or(Value::Null);
    let profile = read_capability_profile(bundle_root);
    let runtime_label = first_non_empty(&[
        json_string(&value, "selected_runtime_display_name"),
        profile.selected_runtime_display_name.clone(),
    ]);
    let runtime_version = first_non_empty(&[
        json_string(&value, "selected_runtime_version"),
        profile.selected_runtime_version.clone(),
    ]);
    let encoder = first_non_empty(&[
        json_string(&value, "selected_encoder"),
        profile.selected_encoder.clone(),
    ]);
    let capture = first_non_empty(&[
        json_string(&value, "selected_capture"),
        profile.selected_capture.clone(),
    ]);
    let capture_reason = first_non_empty(&[
        json_string(&value, "selected_capture_reason"),
        profile.selected_capture_reason.clone().unwrap_or_default(),
    ]);
    let selection_reason = first_non_empty(&[
        json_string(&value, "capability_selection_reason"),
        json_string(&value, "selection_reason"),
        profile.selection_reason.clone(),
    ]);
    let ffmpeg_source = first_non_empty(&[
        json_string(&value, "selected_ffmpeg_source"),
        profile.selected_ffmpeg_source.clone(),
    ]);
    let warnings = first_non_empty_vec(&[
        json_string_vec(&value, "config_hygiene_warnings"),
        profile.warnings.clone(),
    ]);

    Ok(HostStatusData {
        lifecycle_phase: json_string(&value, "lifecycle_phase"),
        health_grade: json_string(&value, "health_grade"),
        audio_status: json_string(&value, "audio_routing_status"),
        audio_reason: json_string(&value, "audio_routing_reason"),
        local_url: json_string(&value, "local_url"),
        local_http_ready: json_bool(&value, "local_http_ready"),
        required_processes_ready: json_bool(&value, "required_processes_ready"),
        runtime_label,
        runtime_key: json_string(&value, "selected_runtime_key"),
        runtime_profile_key: profile.selected_runtime_key.clone(),
        runtime_version,
        encoder,
        capture,
        capture_reason,
        selection_reason,
        ffmpeg_source,
        fallback_runtime_label: json_string(&value, "recommended_runtime_display_name"),
        fallback_runtime_version: json_string(&value, "recommended_runtime_version"),
        fallback_runtime_reason: json_string(&value, "recommended_runtime_reason"),
        warnings,
        support_bundle_count: json_i32(&value, "support_bundle_count"),
        last_support_bundle_id: json_string(&value, "last_support_bundle_id"),
        audio_package_path: json_string(&value, "audio_fallback_installer_path"),
        audio_inf_path: json_string(&value, "audio_dependency_package_inf_path"),
        raw_json: result.stdout.clone(),
    })
}

fn load_audio_view(bundle_root: &Path, status: &HostStatusData) -> AudioView {
    let profile = read_capability_profile(bundle_root);
    let preferences = read_audio_preferences(bundle_root);
    let mut available_outputs = collect_audio_endpoint_names(&profile.audio_endpoints, "output");
    let mut available_inputs = collect_audio_endpoint_names(&profile.audio_endpoints, "input");
    extend_audio_choices(
        &mut available_outputs,
        &[
            &profile.selected_audio_sink_name,
            &profile.selected_virtual_sink_name,
            &preferences.selected_audio_sink_name,
            &preferences.selected_virtual_sink_name,
        ],
    );
    extend_audio_choices(
        &mut available_inputs,
        &[
            &profile.selected_microphone_name,
            &preferences.selected_microphone_name,
        ],
    );
    let mode = if normalize_audio_mode(&preferences.mode) == "manual" {
        "manual".to_string()
    } else if !profile.audio_selection_mode.trim().is_empty() {
        normalize_audio_mode(&profile.audio_selection_mode)
    } else {
        "auto".to_string()
    };

    let selected_audio_sink_name = preferred_audio_value(
        if mode == "manual" {
            &preferences.selected_audio_sink_name
        } else {
            &profile.selected_audio_sink_name
        },
        if mode == "manual" {
            &profile.selected_audio_sink_name
        } else {
            &preferences.selected_audio_sink_name
        },
    );
    let selected_virtual_sink_name = preferred_audio_value(
        if mode == "manual" {
            &preferences.selected_virtual_sink_name
        } else {
            &profile.selected_virtual_sink_name
        },
        if mode == "manual" {
            &profile.selected_virtual_sink_name
        } else {
            &preferences.selected_virtual_sink_name
        },
    );
    let selected_microphone_name = preferred_audio_value(
        if mode == "manual" {
            &preferences.selected_microphone_name
        } else {
            &profile.selected_microphone_name
        },
        if mode == "manual" {
            &profile.selected_microphone_name
        } else {
            &preferences.selected_microphone_name
        },
    );
    let selection_reason = preferred_audio_value(
        &profile.audio_selection_reason,
        if mode == "manual" {
            "manual_override_pending_refresh"
        } else {
            ""
        },
    );
    let routing_status = if status.audio_status.trim().is_empty() {
        if mode == "manual" {
            "manual".to_string()
        } else {
            "unknown".to_string()
        }
    } else {
        status.audio_status.trim().to_string()
    };
    let routing_reason = preferred_audio_value(&status.audio_reason, &selection_reason);

    AudioView {
        mode,
        selected_audio_sink_name,
        selected_virtual_sink_name,
        selected_microphone_name,
        selection_reason,
        routing_status,
        routing_reason,
        available_outputs,
        available_inputs,
    }
}

fn load_display_view(bundle_root: &Path) -> DisplayView {
    let preferences = read_display_preferences(bundle_root);
    let force_wgc_path = bundle_root.join("moonlight").join("server").join("force-wgc.txt");
    let dual_stream_enabled = force_wgc_path.exists();
    DisplayView {
        mode: preferences.mode.clone(),
        custom_device_name: preferences.custom_device_name.clone(),
        custom_device_id: preferences.custom_device_id.clone(),
        custom_label: preferences.custom_label.clone(),
        effective_label: display_mode_label(&preferences.mode),
        updated_at: preferences.updated_at.clone(),
        dual_stream_enabled,
    }
}

fn load_service_state(bundle_root: &Path) -> Result<String, String> {
    let result = run_installer_command(bundle_root, "service-status")?;
    if !result.success {
        return Ok("unknown".into());
    }

    let upper = result.stdout.to_uppercase();
    if upper.contains("RUNNING") {
        return Ok("running".into());
    }
    if upper.contains("STOPPED") {
        return Ok("stopped".into());
    }
    if upper.contains("NOT INSTALLED") {
        return Ok("not installed".into());
    }
    if upper.contains("START_PENDING") {
        return Ok("start pending".into());
    }
    if upper.contains("STOP_PENDING") {
        return Ok("stop pending".into());
    }
    Ok("unknown".into())
}

fn read_capability_profile(bundle_root: &Path) -> CapabilityProfileRecord {
    let path = capability_profile_path(bundle_root);
    if !path.exists() {
        return CapabilityProfileRecord::default();
    }

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return CapabilityProfileRecord::default(),
    };
    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut parsed = serde_json::from_str::<CapabilityProfileRecord>(clean).unwrap_or_default();
    parsed.selected_runtime_key = parsed.selected_runtime_key.trim().to_string();
    parsed.selected_runtime_display_name = parsed
        .selected_runtime_display_name
        .trim()
        .to_string();
    parsed.selected_runtime_version = parsed.selected_runtime_version.trim().to_string();
    parsed.selected_encoder = parsed.selected_encoder.trim().to_string();
    parsed.selected_capture = parsed.selected_capture.trim().to_string();
    parsed.selected_capture_reason = parsed
        .selected_capture_reason
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    parsed.selected_ffmpeg_source = parsed.selected_ffmpeg_source.trim().to_string();
    parsed.selection_reason = parsed.selection_reason.trim().to_string();
    parsed.warnings = parsed
        .warnings
        .into_iter()
        .map(|warning| warning.trim().to_string())
        .filter(|warning| !warning.is_empty())
        .collect();
    parsed.selected_audio_sink_name = parsed.selected_audio_sink_name.trim().to_string();
    parsed.selected_virtual_sink_name = parsed.selected_virtual_sink_name.trim().to_string();
    parsed.selected_microphone_name = parsed.selected_microphone_name.trim().to_string();
    parsed.audio_selection_reason = parsed.audio_selection_reason.trim().to_string();
    parsed.audio_selection_mode = normalize_audio_mode(&parsed.audio_selection_mode);
    parsed.gpu_controllers = parsed
        .gpu_controllers
        .into_iter()
        .filter_map(|gpu| {
            let name = gpu.name.trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(CapabilityGpuControllerRecord { name })
            }
        })
        .collect();
    parsed.runtime_candidates = parsed
        .runtime_candidates
        .into_iter()
        .filter_map(|candidate| {
            let key = candidate.key.trim().to_string();
            if key.is_empty() {
                return None;
            }

            Some(CapabilityRuntimeCandidateRecord {
                key,
                startup_validation_status: candidate
                    .startup_validation_status
                    .as_ref()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                startup_validation_reason: candidate
                    .startup_validation_reason
                    .as_ref()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
            })
        })
        .collect();
    parsed.audio_endpoints = parsed
        .audio_endpoints
        .into_iter()
        .filter_map(|endpoint| {
            let direction = endpoint.direction.trim().to_string();
            let name = endpoint.name.trim().to_string();
            if direction.is_empty() || name.is_empty() {
                None
            } else {
                Some(CapabilityAudioEndpointRecord {
                    direction,
                    device_id: endpoint.device_id.trim().to_string(),
                    name,
                })
            }
        })
        .collect();
    parsed
}

fn selected_runtime_candidate<'a>(
    profile: &'a CapabilityProfileRecord,
) -> Option<&'a CapabilityRuntimeCandidateRecord> {
    if profile.selected_runtime_key.trim().is_empty() {
        return None;
    }

    profile.runtime_candidates.iter().find(|candidate| {
        candidate
            .key
            .eq_ignore_ascii_case(profile.selected_runtime_key.trim())
    })
}

fn capability_profile_has_virtual_display_driver(profile: &CapabilityProfileRecord) -> bool {
    profile
        .selected_capture_reason
        .as_deref()
        .is_some_and(|reason| reason.eq_ignore_ascii_case("virtual_display_driver_present"))
        || profile.gpu_controllers.iter().any(|gpu| {
            gpu.name
                .to_ascii_lowercase()
                .contains("virtual display driver")
        })
}

fn selected_capture_supports_active_display_fallback(profile: &CapabilityProfileRecord) -> bool {
    profile
        .selected_capture_reason
        .as_deref()
        .is_some_and(|reason| reason.eq_ignore_ascii_case("rdp_remote_display_active"))
        || profile.selected_capture.eq_ignore_ascii_case("wgc")
}

fn evaluate_stream_display_route(
    bundle_root: &Path,
    profile: &CapabilityProfileRecord,
) -> (bool, Option<String>) {
    if profile.selected_runtime_key.trim().is_empty()
        && profile.selected_capture.trim().is_empty()
        && profile.runtime_candidates.is_empty()
        && profile.gpu_controllers.is_empty()
    {
        return (
            false,
            Some("Display capability profile is missing. Refresh Host Control first.".to_string()),
        );
    }

    if let Some(candidate) = selected_runtime_candidate(profile) {
        if let Some(status) = candidate
            .startup_validation_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !status.eq_ignore_ascii_case("passed") {
                let reason_suffix = candidate
                    .startup_validation_reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                return (
                    false,
                    Some(format!(
                        "Selected runtime startup validation is {status}{reason_suffix}."
                    )),
                );
            }
        }
    }

    if selected_capture_supports_active_display_fallback(profile) {
        return (true, None);
    }

    if display_preference_allows_non_vdd(bundle_root) {
        return (true, None);
    }

    if !capability_profile_has_virtual_display_driver(profile) {
        let capture = if profile.selected_capture.trim().is_empty() {
            "unknown".to_string()
        } else {
            profile.selected_capture.trim().to_string()
        };
        let reason = profile
            .selected_capture_reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("virtual display driver not detected");
        return (
            false,
            Some(format!(
                "Virtual Display Driver is not active for stream. Current capture route: {capture} ({reason})."
            )),
        );
    }

    (true, None)
}

fn build_stream_not_ready_note(
    display_route_note: Option<&str>,
    lifecycle_phase: &str,
    required_processes_ready: bool,
    local_http_ready: bool,
    public_url: &str,
) -> String {
    if let Some(note) = display_route_note
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return note.to_string();
    }
    if !local_http_ready {
        return "Local host HTTP endpoint is not ready.".to_string();
    }
    if !required_processes_ready {
        return "Required host processes are not ready.".to_string();
    }
    if !lifecycle_phase.eq_ignore_ascii_case("ready") {
        return format!(
            "Host lifecycle is {}.",
            if lifecycle_phase.trim().is_empty() {
                "not ready"
            } else {
                lifecycle_phase.trim()
            }
        );
    }
    if public_url.trim().is_empty() {
        return "Public stream route is not ready yet.".to_string();
    }

    "Host is still preparing the stream route.".to_string()
}

fn read_audio_preferences(bundle_root: &Path) -> AudioPreferenceRecord {
    let path = audio_preferences_path(bundle_root);
    if !path.exists() {
        return AudioPreferenceRecord {
            mode: "auto".into(),
            ..AudioPreferenceRecord::default()
        };
    }

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => {
            return AudioPreferenceRecord {
                mode: "auto".into(),
                ..AudioPreferenceRecord::default()
            }
        }
    };

    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut parsed = serde_json::from_str::<AudioPreferenceRecord>(clean).unwrap_or_default();
    if parsed.schema_version != 1 {
        return AudioPreferenceRecord {
            mode: "auto".into(),
            ..AudioPreferenceRecord::default()
        };
    }

    parsed.mode = normalize_audio_mode(&parsed.mode);
    parsed.selected_audio_sink_name = parsed.selected_audio_sink_name.trim().to_string();
    parsed.selected_virtual_sink_name = parsed.selected_virtual_sink_name.trim().to_string();
    parsed.selected_microphone_name = parsed.selected_microphone_name.trim().to_string();
    parsed
}

fn save_audio_preferences_file(
    bundle_root: &Path,
    record: &AudioPreferenceRecord,
) -> Result<(), String> {
    let path = audio_preferences_path(bundle_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not create the audio settings folder.")?;
    }

    let raw = serde_json::to_string_pretty(record)
        .map_err(|_| "Could not encode the audio settings file.")?;
    write_text_file_with_retry(&path, &raw, "Could not write the audio settings file.")
}

fn clear_audio_preferences_file(bundle_root: &Path) -> Result<(), String> {
    let path = audio_preferences_path(bundle_root);
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path).map_err(|_| "Could not clear the audio settings file.")?;
    Ok(())
}

fn read_display_preferences(bundle_root: &Path) -> DisplayPreferenceRecord {
    let path = display_preferences_path(bundle_root);
    if !path.exists() {
        return DisplayPreferenceRecord {
            schema_version: 1,
            mode: "mtt_vdd".into(),
            ..DisplayPreferenceRecord::default()
        };
    }

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => {
            return DisplayPreferenceRecord {
                schema_version: 1,
                mode: "mtt_vdd".into(),
                ..DisplayPreferenceRecord::default()
            }
        }
    };

    let clean = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut parsed = serde_json::from_str::<DisplayPreferenceRecord>(clean).unwrap_or_default();
    if parsed.schema_version != 1 {
        return DisplayPreferenceRecord {
            schema_version: 1,
            mode: "mtt_vdd".into(),
            ..DisplayPreferenceRecord::default()
        };
    }

    parsed.mode = normalize_display_mode(&parsed.mode);
    parsed.custom_device_name = parsed.custom_device_name.trim().to_string();
    parsed.custom_device_id = parsed.custom_device_id.trim().to_string();
    parsed.custom_label = parsed.custom_label.trim().to_string();
    parsed
}

fn save_display_preferences_file(
    bundle_root: &Path,
    record: &DisplayPreferenceRecord,
) -> Result<(), String> {
    let path = display_preferences_path(bundle_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not create the display settings folder.")?;
    }

    let raw = serde_json::to_string_pretty(record)
        .map_err(|_| "Could not encode the display settings file.")?;
    write_text_file_with_retry(&path, &raw, "Could not write the display settings file.")
}

fn normalize_display_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "auto" => "auto".into(),
        "mtt" | "mtt_vdd" | "vdd" | "virtual_display" => "mtt_vdd".into(),
        "qemu" | "virtio" | "qemu_virtio" => "qemu_virtio".into(),
        // Parsec may stay installed for other apps, but Cloudgime must always route stream
        // sessions back to MTT VDD.
        "parsec" | "parsec_vda" => "mtt_vdd".into(),
        "primary" | "current_primary" => "primary".into(),
        "custom" | "device" => "custom".into(),
        _ => "mtt_vdd".into(),
    }
}

fn display_mode_label(mode: &str) -> String {
    match normalize_display_mode(mode).as_str() {
        "auto" => "Auto (prefer MTT VDD, fallback active display)".into(),
        "mtt_vdd" => "MTT VDD".into(),
        "qemu_virtio" => "QEMU / VirtIO display".into(),
        "parsec_vda" => "Parsec Virtual Display Adapter".into(),
        "primary" => "Current primary display".into(),
        "custom" => "Custom display match".into(),
        _ => "MTT VDD".into(),
    }
}

fn display_preference_allows_non_vdd(bundle_root: &Path) -> bool {
    matches!(
        normalize_display_mode(&read_display_preferences(bundle_root).mode).as_str(),
        "qemu_virtio" | "parsec_vda" | "primary" | "custom"
    )
}

fn collect_audio_endpoint_names(
    endpoints: &[CapabilityAudioEndpointRecord],
    direction: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    for endpoint in endpoints {
        if !endpoint.direction.eq_ignore_ascii_case(direction) {
            continue;
        }

        let name = endpoint.name.trim();
        if name.is_empty() || contains_case_insensitive(&names, name) {
            continue;
        }

        names.push(name.to_string());
    }

    names
}

fn extend_audio_choices(values: &mut Vec<String>, extras: &[&str]) {
    for extra in extras {
        let trimmed = extra.trim();
        if trimmed.is_empty() || contains_case_insensitive(values, trimmed) {
            continue;
        }
        values.push(trimmed.to_string());
    }
}

fn preferred_audio_value(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if !primary.is_empty() {
        return primary.to_string();
    }

    fallback.trim().to_string()
}

fn contains_case_insensitive(values: &[String], needle: &str) -> bool {
    values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(needle.trim()))
}

fn normalize_audio_mode(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("manual") {
        "manual".to_string()
    } else {
        "auto".to_string()
    }
}

fn build_host_diagnostic_payload(bundle_root: &Path, shell: &ShellState, summary: &str) -> Value {
    let server_root = bundle_root.join("moonlight").join("server");
    let install_root = PathBuf::from(shell.install.install_root.trim());
    let activation = serde_json::to_value(&shell.activation).unwrap_or(Value::Null);
    let runtime = serde_json::to_value(&shell.runtime).unwrap_or(Value::Null);
    let network = serde_json::to_value(&shell.network).unwrap_or(Value::Null);
    let paths = serde_json::to_value(&shell.paths).unwrap_or(Value::Null);
    let support = serde_json::to_value(&shell.support).unwrap_or(Value::Null);

    serde_json::json!({
        "schemaVersion": 1,
        "createdAtUtc": Utc::now().to_rfc3339(),
        "reason": summary,
        "machine": {
            "computerName": env::var("COMPUTERNAME").unwrap_or_default(),
            "username": env::var("USERNAME").unwrap_or_default(),
        },
        "activation": activation,
        "runtime": runtime,
        "network": network,
        "support": support,
        "paths": paths,
        "install": {
            "installedMode": shell.install.installed_mode,
            "installRoot": shell.install.install_root.clone(),
            "dataRoot": shell.install.data_root.clone(),
            "uninstallRegistered": shell.install.uninstall_registered,
            "launchIntent": shell.install.launch_intent.clone(),
        },
        "files": {
            "appExeProgramFiles": file_metadata_value(&install_root.join("cloudgime-host-control.exe")),
            "bootstrapProgramFiles": file_metadata_value(&install_root.join("cloudgime-host-bootstrap.exe")),
            "uninstallerProgramFiles": file_metadata_value(&install_root.join("uninstaller-cloudgime.exe")),
            "appExeBundle": file_metadata_value(&bundle_root.join("cloudgime-host-control.exe")),
            "bootstrapBundle": file_metadata_value(&bundle_root.join("cloudgime-host-bootstrap.exe")),
            "hostInstaller": file_metadata_value(&host_installer_path(bundle_root)),
            "runtimeAgent": file_metadata_value(&bundle_root.join("moonlight").join("system").join("cloudgime-runtime-agent.exe")),
            "activationState": file_metadata_value(&activation_state_path(bundle_root)),
            "supervisorState": file_metadata_value(&server_root.join("host_supervisor_state.json")),
            "hostUserDaemonTaskHealth": file_metadata_value(&server_root.join("host_user_daemon_task_health.json")),
            "supervisorLog": file_metadata_value(&server_root.join("host_supervisor.log")),
            "publicUrl": file_metadata_value(&bundle_root.join("PUBLIC_URL.txt")),
        },
        "supervisorState": read_json_file_value(&server_root.join("host_supervisor_state.json")),
        "hostUserDaemonTaskHealth": read_json_file_value(&server_root.join("host_user_daemon_task_health.json")),
        "capabilityProfile": read_json_file_value(&capability_profile_path(bundle_root)),
        "publicUrlText": read_text_file_tail(&bundle_root.join("PUBLIC_URL.txt"), 20, 4_000),
        "logs": {
            "hostSupervisorTail": read_text_file_tail(&server_root.join("host_supervisor.log"), 240, 120_000),
            "sunshineTail": read_text_file_tail(&bundle_root.join("sunshine").join("config").join("sunshine.log"), 160, 80_000),
            "sunshineLegacyTail": read_text_file_tail(&bundle_root.join("sunshine-legacy").join("config").join("sunshine.log"), 160, 80_000),
        }
    })
}

fn file_metadata_value(path: &Path) -> Value {
    match fs::metadata(path) {
        Ok(metadata) => {
            let modified_unix = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs())
                .unwrap_or_default();
            serde_json::json!({
                "path": path.display().to_string(),
                "exists": true,
                "length": metadata.len(),
                "modifiedUnix": modified_unix,
            })
        }
        Err(_) => serde_json::json!({
            "path": path.display().to_string(),
            "exists": false,
        }),
    }
}

fn read_json_file_value(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or(Value::Null)
}

fn read_text_file_tail(path: &Path, max_lines: usize, max_chars: usize) -> String {
    let bytes = match fs::read(path) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };
    let start = bytes.len().saturating_sub(max_chars.saturating_mul(2));
    let text = String::from_utf8_lossy(&bytes[start..]).to_string();
    let lines: Vec<&str> = text.lines().collect();
    let tail = if lines.len() > max_lines {
        lines[lines.len() - max_lines..].join("\n")
    } else {
        text
    };
    last_chars(&tail, max_chars)
}

fn last_chars(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

fn build_paths(bundle_root: &Path, status: &HostStatusData) -> PathView {
    let server_root = bundle_root.join("moonlight").join("server");
    PathView {
        bundle_root: bundle_root.display().to_string(),
        server_folder_path: server_root.display().to_string(),
        support_folder_path: server_root.join("support-bundles").display().to_string(),
        runtime_file_path: server_root
            .join("selected_sunshine_runtime.txt")
            .display()
            .to_string(),
        release_info_path: server_root.join("release_info.json").display().to_string(),
        capability_profile_path: server_root
            .join("host_capability_profile.json")
            .display()
            .to_string(),
        audio_package_path: status.audio_package_path.clone(),
        audio_inf_path: status.audio_inf_path.clone(),
        display_state_path: server_root
            .join("dynamic_display_state.json")
            .display()
            .to_string(),
    }
}

fn restart_runtime_engine(bundle_root: &Path) -> Result<CommandOutput, String> {
    let force_wgc_path = bundle_root.join("moonlight").join("server").join("force-wgc.txt");
    if force_wgc_path.exists() {
        let script = r#"
Stop-Process -Name sunshine -Force -ErrorAction SilentlyContinue
Start-ScheduledTask -TaskName "CloudgimeUserSunshine" -ErrorAction SilentlyContinue
        "#;
        let mut cmd = Command::new("powershell");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(script);
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }
        let _ = cmd.output();

        // Jalankan preflight refresh
        let helper = bundle_root.join("moonlight").join("server").join("display-prepare-helper.exe");
        if helper.exists() {
            let mut helper_cmd = Command::new(helper);
            helper_cmd.arg("preflight")
                .arg("--bundle-root")
                .arg(bundle_root)
                .arg("--refresh")
                .current_dir(bundle_root);
            #[cfg(target_os = "windows")]
            {
                helper_cmd.creation_flags(0x08000000);
            }
            let _ = helper_cmd.output();
        }

        Ok(CommandOutput {
            success: true,
            stdout: "User session Sunshine restarted.".to_string(),
            stderr: "".to_string(),
        })
    } else {
        run_installer_command(bundle_root, "restart-runtime")
    }
}

fn run_installer_command(bundle_root: &Path, command: &str) -> Result<CommandOutput, String> {
    run_installer_command_args(bundle_root, command, &[], Duration::from_secs(90))
}

fn run_installer_command_args(
    bundle_root: &Path,
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let installer = host_installer_path(bundle_root);
    if !installer.exists() {
        return Err("host-installer.exe was not found for this bundle.".into());
    }

    let mut cmd = Command::new(installer);
    cmd.arg("--bundle-root")
        .arg(bundle_root)
        .arg(command)
        .args(args)
        .current_dir(bundle_root);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Could not run host-installer.exe.")?;

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started_at.elapsed() >= timeout => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|_| "Could not collect host-installer.exe output.")?;
                return Ok(CommandOutput {
                    success: false,
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: format!(
                        "{}{}host-installer.exe timed out after {} seconds while running {}.",
                        String::from_utf8_lossy(&output.stderr).trim(),
                        if output.stderr.is_empty() { "" } else { "\n" },
                        timeout.as_secs(),
                        command
                    ),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(250)),
            Err(_) => return Err("Could not monitor host-installer.exe.".into()),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|_| "Could not collect host-installer.exe output.")?;

    Ok(CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn mark_prepared_locally(bundle_root: &Path) -> Result<(), String> {
    let mut state = load_activation_state(bundle_root)?;
    if matches!(
        state.activation_state.as_str(),
        "activated" | "suspended" | "revoked"
    ) {
        return Ok(());
    }

    state.activation_state = "prepared_local".into();
    save_activation_state(bundle_root, &state)
}

fn ensure_host_prepared_automatically(bundle_root: &Path) -> Result<(), String> {
    let prepare_result =
        run_installer_command_args(bundle_root, "prepare-host", &[], Duration::from_secs(180))?;
    if !prepare_result.success {
        let issue = join_output(&prepare_result);
        if is_nonfatal_prepare_startup_issue(&issue) {
            mark_prepared_locally(bundle_root)?;
            return Ok(());
        }

        return Err(format!(
            "Setup host otomatis gagal: {}",
            summarize_command_issue(&issue)
        ));
    }

    mark_prepared_locally(bundle_root)
}

fn ensure_persistent_host_service(bundle_root: &Path) -> Result<PersistentServiceState, String> {
    let install_result = run_installer_command(bundle_root, "install-service")?;
    if !install_result.success {
        let issue = join_output(&install_result);
        if is_service_permission_issue(&issue) {
            return Ok(PersistentServiceState::Deferred);
        }
        return Err(issue);
    }
    remove_legacy_runtime_service();

    let service_state = load_service_state(bundle_root)?;
    if service_state.eq_ignore_ascii_case("running")
        || service_state.eq_ignore_ascii_case("start pending")
    {
        return Ok(PersistentServiceState::Enabled);
    }

    let start_result = run_installer_command(bundle_root, "start-service")?;
    if start_result.success {
        return Ok(PersistentServiceState::Enabled);
    }

    let combined = join_output(&start_result);
    let lowered = combined.to_ascii_lowercase();
    if lowered.contains("already running") || lowered.contains("1056") {
        return Ok(PersistentServiceState::Enabled);
    }
    if is_service_permission_issue(&combined) {
        return Ok(PersistentServiceState::Deferred);
    }

    Err(combined)
}

fn remove_legacy_runtime_service() {
    #[cfg(target_os = "windows")]
    {
        let system_dir = Path::new("C:\\Windows\\System32");
        let _ = run_hidden_command_output(
            "sc.exe",
            &["stop", "CloudgimeRuntime-Host"],
            system_dir,
            Duration::from_secs(15),
        );
        let _ = run_hidden_command_output(
            "sc.exe",
            &["delete", "CloudgimeRuntime-Host"],
            system_dir,
            Duration::from_secs(15),
        );
    }
}

fn build_activation_success_message(
    bundle_root: &Path,
    service_state: PersistentServiceState,
) -> String {
    let base_message = match service_state {
        PersistentServiceState::Enabled =>
            "Host activation redeemed. Persistent host service is enabled and will recover after reboot.",
        PersistentServiceState::Deferred =>
            "Host activation redeemed. Runtime access is ready now; persistent service setup will be finalized by the elevated installer lane.",
    };

    let start_result = match run_installer_command(bundle_root, "start-bundle") {
        Ok(result) => result,
        Err(_) => {
            return format!(
                "{base_message} Automatic runtime start could not be requested from this app. Host Control will keep retrying from the automatic flow."
            )
        }
    };

    if !start_result.success {
        return format!(
            "{base_message} Automatic runtime start is still warming up: {}",
            summarize_command_issue(&join_output(&start_result))
        );
    }

    match run_installer_command(bundle_root, "verify-startup") {
        Ok(result) if result.success => format!(
            "{base_message} Runtime startup was requested automatically and startup checks already passed."
        ),
        _ => format!(
            "{base_message} Runtime startup was requested automatically and is warming up now."
        ),
    }
}

fn is_service_permission_issue(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("access is denied")
        || lowered.contains("openscmanager failed 5")
        || lowered.contains("error 5")
        || lowered.contains("failed to configure windows service")
}

fn is_nonfatal_prepare_startup_issue(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("start-bundle")
        && (lowered.contains("bundle startup incomplete")
            || lowered.contains("timed out waiting")
            || lowered.contains("49000")
            || lowered.contains("connection timed out")
            || lowered.contains("connection refused")
            || lowered.contains("local_http_ready"))
}

fn last_support_bundle_path(bundle_root: &Path, bundle_id: &str) -> String {
    if bundle_id.trim().is_empty() {
        return String::new();
    }

    bundle_root
        .join("moonlight")
        .join("server")
        .join("support-bundles")
        .join(bundle_id)
        .display()
        .to_string()
}

fn sanitized_display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        default_display_name()
    } else {
        trimmed.into()
    }
}

fn normalize_control_plane(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let candidate = if trimmed.is_empty() {
        DEFAULT_CONTROL_PLANE
    } else {
        trimmed
    };

    let parsed = reqwest::Url::parse(candidate).map_err(|_| "Control plane URL is invalid.")?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.as_str().trim_end_matches('/').to_string()),
        _ => Err("Control plane URL must use http or https.".into()),
    }
}

fn normalize_activation_token(raw: &str) -> String {
    raw.replace([' ', '\r', '\n', '\t'], "").trim().to_string()
}

fn looks_like_activation_token(token: &str) -> bool {
    token.to_ascii_lowercase().starts_with("cgha_") && token.len() >= 20
}

fn setup_token_lane_label(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "instance_pair" => "Power-Managed PC ID".into(),
        "always_on_host" => "Always-On Host".into(),
        "control_node" => "Control Node".into(),
        "legacy_slot" => "Legacy Slot".into(),
        _ => "Lisensi Aktivasi".into(),
    }
}

fn normalize_setup_token(raw: &str) -> String {
    raw.replace([' ', '\r', '\n', '\t'], "").trim().to_string()
}

fn decode_cgpair_token(input: &str) -> Option<(String, String)> {
    if !input.starts_with("cgpair_") {
        return None;
    }
    let encoded = &input["cgpair_".len()..];
    let decoded_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded.as_bytes()))
        .ok()?;
    let decoded_str = String::from_utf8(decoded_bytes).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&decoded_str).ok()?;
    let setup_token = parsed.get("setupToken")?.as_str()?.to_string();
    let base_url = parsed.get("baseUrl")?.as_str()?.to_string();
    Some((setup_token, base_url))
}

fn normalize_expected_setup_token_kind(raw: &str) -> Result<String, String> {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "" => Ok(String::new()),
        "instance_pair" | "pair" | "power_managed" => Ok("instance_pair".into()),
        "always_on_host" | "always_on" | "always" => Ok("always_on_host".into()),
        "control_node" => Ok("control_node".into()),
        _ => Err("Unknown activation license mode.".into()),
    }
}

fn resolve_local_machine_name() -> String {
    env::var("COMPUTERNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_display_name)
}

fn build_friendly_claim_error(
    status_code: u16,
    payload: &ClaimSetupTokenPayload,
    raw_body: &str,
) -> String {
    let backend_message = payload
        .error
        .as_ref()
        .or(payload.message.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    match status_code {
        0 => "Could not reach the control plane.".into(),
        400 => backend_message
            .unwrap_or_else(|| "Activation license payload was rejected by the control plane.".into()),
        404 => backend_message
            .unwrap_or_else(|| "Lisensi Aktivasi tidak ditemukan atau sudah tidak tersedia.".into()),
        409 => backend_message.unwrap_or_else(|| {
            "Activation license conflict detected. Release the existing activation or issue a fresh license."
                .into()
        }),
        410 => backend_message
            .unwrap_or_else(|| "Lisensi Aktivasi sudah kedaluwarsa. Terbitkan lisensi baru dari Host Control.".into()),
        _ => backend_message
            .or_else(|| {
                let trimmed = raw_body.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.chars().take(240).collect())
                }
            })
            .unwrap_or_else(|| {
                format!("Could not activate the license. HTTP status {status_code}.")
            }),
    }
}

fn build_friendly_redeem_error(status_code: u16, payload: &RedeemPayload) -> String {
    match status_code {
        0 => "Could not reach the control plane.".into(),
        401 => "This activation token is invalid. Reissue the token in Host Control and paste it again.".into(),
        403 => {
            if payload.activation_state.trim().is_empty() {
                "This host is blocked by the control plane.".into()
            } else {
                format!(
                    "This host is currently {} in the control plane.",
                    normalize_phase(&payload.activation_state)
                )
            }
        }
        404 => "This host ID has no activation record yet. Issue a token first in Host Control.".into(),
        409 => {
            if payload.activation_state.eq_ignore_ascii_case("activated") {
                "This host is already activated. The token has already been used. Refresh status, then start the host runtime if it is not running yet.".into()
            } else {
                payload
                    .error
                    .clone()
                    .or(payload.message.clone())
                    .unwrap_or_else(|| "Activation conflict detected. Generate a fresh token and try again.".into())
            }
        }
        _ => payload
            .error
            .clone()
            .or(payload.message.clone())
            .unwrap_or_else(|| "Activation failed.".into()),
    }
}

fn normalize_phase(raw: &str) -> String {
    raw.replace('_', " ")
}

fn join_output(result: &CommandOutput) -> String {
    match (result.stdout.trim(), result.stderr.trim()) {
        ("", "") => "The host action failed.".into(),
        ("", stderr) => stderr.into(),
        (stdout, "") => stdout.into(),
        (stdout, stderr) => format!("{stdout}\n\n{stderr}"),
    }
}

fn summarize_command_issue(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= 180 {
        compact
    } else {
        format!("{}...", &compact[..177])
    }
}

fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.into())
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.into()
    } else {
        trimmed.into()
    }
}

fn json_string(root: &Value, key: &str) -> String {
    root.get(key)
        .and_then(|value| match value {
            Value::String(inner) => Some(inner.clone()),
            Value::Number(inner) => Some(inner.to_string()),
            Value::Bool(inner) => Some(inner.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn json_string_vec(root: &Value, key: &str) -> Vec<String> {
    root.get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(inner) => Some(inner.trim().to_string()),
                    Value::Number(inner) => Some(inner.to_string()),
                    Value::Bool(inner) => Some(inner.to_string()),
                    _ => None,
                })
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn json_bool(root: &Value, key: &str) -> bool {
    root.get(key)
        .and_then(|value| match value {
            Value::Bool(inner) => Some(*inner),
            Value::String(inner) => inner.parse::<bool>().ok(),
            _ => None,
        })
        .unwrap_or(false)
}

fn json_i32(root: &Value, key: &str) -> i32 {
    root.get(key)
        .and_then(|value| match value {
            Value::Number(inner) => inner.as_i64().map(|parsed| parsed as i32),
            Value::String(inner) => inner.parse::<i32>().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn first_non_empty_vec(values: &[Vec<String>]) -> Vec<String> {
    values
        .iter()
        .find(|items| !items.is_empty())
        .cloned()
        .unwrap_or_default()
}

#[tauri::command]
pub async fn toggle_dual_stream(
    enabled: bool,
    session: State<'_, AppSession>,
) -> Result<ActionOutcome, String> {
    ensure_unlocked(&session)?;
    let bundle_root = resolve_bundle_root()?;
    let bundle_root_str = bundle_root.to_string_lossy().replace('\\', "\\\\");

    let script = if enabled {
        format!(
            r#"
$ErrorActionPreference = 'Stop'
$paths = @("C:\VirtualDisplayDriver\vdd_settings.xml", "D:\binary\display\vdd_settings.xml")
foreach ($path in $paths) {{
    if (Test-Path $path) {{
        $content = Get-Content $path -Raw
        $content = $content -replace '<count>[0-9]+</count>', '<count>2</count>'
        $content | Set-Content $path -Force
    }}
}}
$device = Get-PnpDevice -FriendlyName "*Virtual Display Driver*" -ErrorAction SilentlyContinue
if ($device) {{
    Disable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false
    Start-Sleep -Seconds 1
    Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false
}}
$serverDir = "{}\moonlight\server"
if (Test-Path $serverDir) {{
    "forced" | Set-Content (Join-Path $serverDir "force-wgc.txt") -Force
}}
Stop-Service -Name CloudgimeRuntime-Host -Force -ErrorAction SilentlyContinue
Set-Service -Name CloudgimeRuntime-Host -StartupType Disabled -ErrorAction SilentlyContinue
Stop-Process -Name sunshine -Force -ErrorAction SilentlyContinue
$currentUser = (Get-CimInstance Win32_ComputerSystem).UserName
if ($currentUser) {{ $currentUser = $currentUser.Split('\')[-1] }} else {{ $currentUser = [Environment]::UserName }}
$action = New-ScheduledTaskAction -Execute "{}\sunshine\sunshine.exe" -WorkingDirectory "{}\sunshine"
$trigger = New-ScheduledTaskTrigger -AtLogOn
Unregister-ScheduledTask -TaskName "CloudgimeUserSunshine" -Confirm:$false -ErrorAction SilentlyContinue
Register-ScheduledTask -TaskName "CloudgimeUserSunshine" -Action $action -Trigger $trigger -User $currentUser -ErrorAction SilentlyContinue
Start-ScheduledTask -TaskName "CloudgimeUserSunshine" -ErrorAction SilentlyContinue
$helper = "{}\moonlight\server\display-prepare-helper.exe"
if (Test-Path $helper) {{
    & $helper preflight --bundle-root "{}" --refresh
}}
"#,
            bundle_root_str,
            bundle_root_str,
            bundle_root_str,
            bundle_root_str,
            bundle_root_str
        )
    } else {
        format!(
            r#"
$ErrorActionPreference = 'Stop'
$paths = @("C:\VirtualDisplayDriver\vdd_settings.xml", "D:\binary\display\vdd_settings.xml")
foreach ($path in $paths) {{
    if (Test-Path $path) {{
        $content = Get-Content $path -Raw
        $content = $content -replace '<count>[0-9]+</count>', '<count>1</count>'
        $content | Set-Content $path -Force
    }}
}}
$device = Get-PnpDevice -FriendlyName "*Virtual Display Driver*" -ErrorAction SilentlyContinue
if ($device) {{
    Disable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false
    Start-Sleep -Seconds 1
    Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false
}}
$wgcFile = "{}\moonlight\server\force-wgc.txt"
if (Test-Path $wgcFile) {{
    Remove-Item $wgcFile -Force
}}
Unregister-ScheduledTask -TaskName "CloudgimeUserSunshine" -Confirm:$false -ErrorAction SilentlyContinue
Stop-Process -Name sunshine -Force -ErrorAction SilentlyContinue
Set-Service -Name CloudgimeRuntime-Host -StartupType Automatic -ErrorAction SilentlyContinue
Start-Service -Name CloudgimeRuntime-Host -ErrorAction SilentlyContinue
$helper = "{}\moonlight\server\display-prepare-helper.exe"
if (Test-Path $helper) {{
    & $helper preflight --bundle-root "{}" --refresh
}}
"#,
            bundle_root_str,
            bundle_root_str,
            bundle_root_str
        )
    };

    let mut cmd = Command::new("powershell");
    cmd.arg("-NoProfile")
       .arg("-NonInteractive")
       .arg("-Command")
       .arg(script);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = cmd.output().map_err(|e| format!("Failed to run PowerShell script: {}", e))?;
    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("PowerShell script failed: {}", err_msg));
    }

    let message = if enabled {
        "Mode Sewa PC (Dual-Streaming & Anti-Flicker) berhasil diaktifkan. Driver VDD di-reload ke 2 display, WGC dipaksa aktif, dan Sunshine berjalan di session user secara otomatis.".to_string()
    } else {
        "Mode Sewa PC berhasil dinonaktifkan. VDD kembali ke 1 display, WGC force dihapus, dan service runtime diaktifkan kembali secara otomatis.".to_string()
    };

    Ok(ActionOutcome {
        message,
        state: build_shell_state(true)?,
    })
}
