use std::{
    collections::HashSet,
    fs,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    sync::{Arc, LazyLock},
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

use actix_web::{
    Error, HttpRequest, HttpResponse, get, post, rt as actix_rt,
    web::{Data, Json, Payload},
};
use actix_ws::{Closed, Message, Session};
use common::{
    api_bindings::{
        DetailedHost, DisplayModePhase, LogMessageType, MicSidecarClientMessage,
        MicSidecarServerMessage, PostCancelRequest, PostCancelResponse, StreamClientMessage,
        StreamServerMessage, StreamSignalingMessage, TransportChannelId,
    },
    ipc::{
        IpcSender, MicSidecarIpcMessage, MicSidecarServerIpcMessage, ServerIpcMessage,
        StreamerConfig, StreamerIpcMessage, create_child_ipc,
    },
    serialize_json,
};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpStream,
    process::{Child, Command},
    spawn,
    sync::{Mutex, mpsc::unbounded_channel},
    task::spawn_blocking,
    time::{sleep, timeout},
};
use tracing::{Level, instrument, span};
use uuid::Uuid;

use crate::app::{
    App, AppError,
    auth::UserAuth,
    host::{AppId, HostId},
    user::AuthenticatedUser,
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        DuplicateTokenEx, SecurityImpersonation, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
    },
    System::RemoteDesktop::{
        WTS_SESSION_INFOW, WTSActive, WTSConnected, WTSEnumerateSessionsW, WTSFreeMemory,
        WTSGetActiveConsoleSessionId, WTSQueryUserToken,
    },
    System::Threading::{
        CREATE_NO_WINDOW, CreateProcessAsUserW, GetExitCodeProcess, NORMAL_PRIORITY_CLASS,
        OpenProcess, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW, SetPriorityClass,
        TerminateProcess, WaitForSingleObject,
    },
    UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION, SW_HIDE},
};

static ACTIVE_STREAM_CHILD: LazyLock<Mutex<Option<Arc<Mutex<Child>>>>> =
    LazyLock::new(|| Mutex::new(None));
static ACTIVE_MIC_SIDECAR_CHILD: LazyLock<Mutex<Option<Arc<Mutex<Child>>>>> =
    LazyLock::new(|| Mutex::new(None));
static ACTIVE_WINDOW_WATCH_CHILD: LazyLock<Mutex<Option<Arc<Mutex<Child>>>>> =
    LazyLock::new(|| Mutex::new(None));
static ACTIVE_SHARED_PLAYER2_BRIDGE: LazyLock<Mutex<Option<ActiveSharedPlayer2Bridge>>> =
    LazyLock::new(|| Mutex::new(None));
static ACTIVE_SHARED_PLAYER2_JOINER: LazyLock<Mutex<Option<ActiveSharedPlayer2Joiner>>> =
    LazyLock::new(|| Mutex::new(None));
static PREWARMED_DISPLAY_SESSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LEGACY_RUNTIME_AUTO_FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);
static HOST_FAILURE_RECOVERY_ACTIVE: AtomicBool = AtomicBool::new(false);
static RECENT_CAPTURE_INIT_FAILURE_AT_MS: AtomicU64 = AtomicU64::new(0);
static CAPTURE_PRESTART_REFRESH_ACTIVE: AtomicBool = AtomicBool::new(false);
static LEGACY_RUNTIME_STARTUP_FAILURE_STATE: LazyLock<Mutex<(u32, u64)>> =
    LazyLock::new(|| Mutex::new((0, 0)));
static RECENT_STREAM_CLOSE_CONTEXT: LazyLock<Mutex<(String, u64, bool)>> =
    LazyLock::new(|| Mutex::new((String::new(), 0, false)));
const STREAM_CONTROL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const GOLDEN_PATH_CONTROL_PROBE_TIMEOUT_MS: u64 = 50;
const DISPLAY_PREPARE_HELPER_TIMEOUT: Duration = Duration::from_secs(25);
const TASKKILL_TIMEOUT: Duration = Duration::from_secs(5);
const CHILD_WAIT_AFTER_KILL_TIMEOUT: Duration = Duration::from_secs(3);
const CAPTURE_INIT_PRESTART_REFRESH_WINDOW: Duration = Duration::from_secs(30 * 60);
const SHARED_PLAYER2_CONTROLLER_ID: u8 = 1;

#[derive(Clone)]
struct ActiveSharedPlayer2Bridge {
    runtime_session_token: String,
    owner_session_id: String,
    host_id: HostId,
    app_id: AppId,
    ipc_sender: IpcSender<ServerIpcMessage>,
}

#[derive(Clone)]
struct ActiveSharedPlayer2Joiner {
    owner_session_id: String,
    connection_id: String,
}

#[derive(Deserialize)]
struct SharedPlayer2InviteQuery {
    #[serde(rename = "inviteToken")]
    invite_token_camel: Option<String>,
    invite_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SharedPlayer2ClientMessage {
    ControllerConnected {
        controller_type: u8,
        supported_buttons: u32,
        capabilities: u16,
    },
    ControllerState {
        button_flags: u32,
        left_trigger: u8,
        right_trigger: u8,
        left_stick_x: i16,
        left_stick_y: i16,
        right_stick_x: i16,
        right_stick_y: i16,
    },
    ControllerDisconnected,
    Heartbeat,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SharedPlayer2ServerMessage {
    Ready {
        owner_session_id: String,
        host_id: u32,
        app_id: u32,
        connection_id: String,
    },
    Status {
        message: String,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostCapabilityGpuControllerSnapshot {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostCapabilityRuntimeCandidateSnapshot {
    key: String,
    healthy_encoders: Vec<String>,
    runtime_status: Option<String>,
    runtime_status_reason: Option<String>,
    startup_validation_status: Option<String>,
    startup_validation_reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostCapabilityEncoderProbeSnapshot {
    runtime_key: String,
    encoder_key: String,
    available: bool,
    ok: bool,
    detail: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostCapabilityProfileSnapshot {
    selected_runtime_key: String,
    selected_encoder: String,
    force_nvenc_enabled: bool,
    selected_capture: String,
    selected_capture_reason: Option<String>,
    gpu_controllers: Vec<HostCapabilityGpuControllerSnapshot>,
    runtime_candidates: Vec<HostCapabilityRuntimeCandidateSnapshot>,
    encoder_probes: Vec<HostCapabilityEncoderProbeSnapshot>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StreamDisplayPreferenceSnapshot {
    mode: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HostSupervisorStateSnapshot {
    lifecycle_phase: String,
    lifecycle_reason: String,
    last_failure_recovery_reason: Option<String>,
    updated_at_unix_ms: Option<u64>,
}

fn current_runtime_dir() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    current_exe.parent().map(Path::to_path_buf)
}

#[cfg(windows)]
fn apply_background_spawn_flags(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_background_spawn_flags(_command: &mut Command) {}

fn current_bundle_root() -> Option<PathBuf> {
    current_runtime_dir()?.parent().map(Path::to_path_buf)
}

fn current_host_supervisor_state_path() -> Option<PathBuf> {
    Some(
        current_runtime_dir()?
            .join("server")
            .join("host_supervisor_state.json"),
    )
}

fn note_host_lifecycle_phase(phase: &str, reason: &str) {
    let Some(state_path) = current_host_supervisor_state_path() else {
        return;
    };

    let now = now_unix_millis();
    let raw = fs::read_to_string(&state_path).unwrap_or_else(|_| "{}".to_string());
    let mut value =
        serde_json::from_str::<serde_json::Value>(&raw).unwrap_or_else(|_| serde_json::json!({}));
    let Some(object) = value.as_object_mut() else {
        return;
    };

    object.insert(
        "lifecycle_phase".to_string(),
        serde_json::Value::String(phase.to_string()),
    );
    object.insert(
        "lifecycle_reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );
    object.insert(
        "lifecycle_updated_at_unix_ms".to_string(),
        serde_json::Value::Number(now.into()),
    );
    object.insert(
        "updated_at_unix_ms".to_string(),
        serde_json::Value::Number(now.into()),
    );

    match serde_json::to_string_pretty(&value) {
        Ok(serialized) => {
            if let Err(err) = fs::write(&state_path, format!("{serialized}\n")) {
                debug!(
                    "[Stream]: failed to write host lifecycle state {}: {err}",
                    state_path.display()
                );
            }
        }
        Err(err) => {
            debug!(
                "[Stream]: failed to serialize host lifecycle state {}: {err}",
                state_path.display()
            );
        }
    }
}

fn lifecycle_recovery_reason_from_close_reason(reason_text: &str) -> Option<&'static str> {
    ReconnectRecoveryPolicy::from_reason_text(reason_text).map(|policy| policy.lifecycle_reason())
}

fn truthy_flag_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }

    !matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

fn extract_android_native_stream_ticket(request: &HttpRequest) -> Option<String> {
    let header = request.headers().get("Authorization")?;
    let bearer = header.to_str().ok()?;
    let token = bearer.strip_prefix("Bearer")?.trim();
    if token.starts_with("mlnatstream_") {
        Some(token.to_string())
    } else {
        None
    }
}

fn extract_android_native_stream_ticket_query(request: &HttpRequest) -> Option<String> {
    request
        .query_string()
        .split('&')
        .filter_map(|entry| entry.split_once('='))
        .find_map(|(key, value)| {
            if key == "streamTicket" || key == "stream_ticket" {
                let token = value.trim();
                if token.starts_with("mlnatstream_") {
                    Some(token.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
}

fn extract_shared_player2_invite_token(request: &HttpRequest) -> Option<String> {
    request
        .query_string()
        .split('&')
        .filter_map(|entry| entry.split_once('='))
        .find_map(|(key, value)| {
            if key == "inviteToken" || key == "invite_token" {
                let token = value.trim();
                (!token.is_empty()).then(|| token.to_string())
            } else {
                None
            }
        })
}

fn build_shared_player2_transport_bytes(channel_id: u8, payload: Vec<u8>) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 1);
    packet.push(channel_id);
    packet.extend_from_slice(&payload);
    packet
}

fn build_shared_player2_controller_connected_packet(
    controller_type: u8,
    supported_buttons: u32,
    capabilities: u16,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(9);
    payload.push(0);
    payload.push(SHARED_PLAYER2_CONTROLLER_ID);
    payload.push(controller_type.min(3));
    payload.extend_from_slice(&supported_buttons.to_be_bytes());
    payload.extend_from_slice(&capabilities.to_be_bytes());
    build_shared_player2_transport_bytes(TransportChannelId::CONTROLLERS, payload)
}

fn build_shared_player2_controller_disconnected_packet() -> Vec<u8> {
    build_shared_player2_transport_bytes(
        TransportChannelId::CONTROLLERS,
        vec![1, SHARED_PLAYER2_CONTROLLER_ID],
    )
}

fn build_shared_player2_controller_state_packet(
    button_flags: u32,
    left_trigger: u8,
    right_trigger: u8,
    left_stick_x: i16,
    left_stick_y: i16,
    right_stick_x: i16,
    right_stick_y: i16,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(15);
    payload.push(0);
    payload.extend_from_slice(&button_flags.to_be_bytes());
    payload.push(left_trigger);
    payload.push(right_trigger);
    payload.extend_from_slice(&left_stick_x.to_be_bytes());
    payload.extend_from_slice(&left_stick_y.to_be_bytes());
    payload.extend_from_slice(&right_stick_x.to_be_bytes());
    payload.extend_from_slice(&right_stick_y.to_be_bytes());
    build_shared_player2_transport_bytes(TransportChannelId::CONTROLLER1, payload)
}

async fn register_shared_player2_bridge(
    owner_session_id: &str,
    runtime_session_token: &str,
    host_id: HostId,
    app_id: AppId,
    ipc_sender: &IpcSender<ServerIpcMessage>,
) {
    let mut slot = ACTIVE_SHARED_PLAYER2_BRIDGE.lock().await;
    *slot = Some(ActiveSharedPlayer2Bridge {
        runtime_session_token: runtime_session_token.to_string(),
        owner_session_id: owner_session_id.to_string(),
        host_id,
        app_id,
        ipc_sender: ipc_sender.clone(),
    });
    append_host_stream_trace(&format!(
        "SHARED_PLAYER2_BRIDGE_READY token={} owner_session_id={} host_id={} app_id={}",
        sanitize_trace_value(runtime_session_token),
        sanitize_trace_value(owner_session_id),
        host_id.0,
        app_id.0
    ));
}

async fn clear_shared_player2_bridge(owner_session_id: &str) {
    let mut slot = ACTIVE_SHARED_PLAYER2_BRIDGE.lock().await;
    let should_clear = slot
        .as_ref()
        .is_some_and(|active| active.owner_session_id == owner_session_id);
    if should_clear {
        let runtime_session_token = slot
            .as_ref()
            .map(|active| active.runtime_session_token.clone())
            .unwrap_or_default();
        append_host_stream_trace(&format!(
            "SHARED_PLAYER2_BRIDGE_CLEARED token={} owner_session_id={}",
            sanitize_trace_value(&runtime_session_token),
            sanitize_trace_value(owner_session_id)
        ));
        *slot = None;
    }
}

async fn get_shared_player2_bridge(
    owner_session_id: &str,
    host_id: HostId,
    app_id: AppId,
) -> Option<ActiveSharedPlayer2Bridge> {
    let slot = ACTIVE_SHARED_PLAYER2_BRIDGE.lock().await;
    slot.as_ref().and_then(|active| {
        (active.owner_session_id == owner_session_id
            && active.host_id == host_id
            && active.app_id == app_id)
            .then(|| active.clone())
    })
}

pub(crate) async fn shared_player2_attach_available_now(
    owner_session_id: &str,
    host_id: HostId,
    app_id: AppId,
) -> bool {
    get_shared_player2_bridge(owner_session_id, host_id, app_id)
        .await
        .is_some()
}

async fn maybe_register_shared_player2_bridge_for_message(
    owner_session_id: Option<&str>,
    runtime_session_token: &str,
    host_id: HostId,
    app_id: AppId,
    ipc_sender: &IpcSender<ServerIpcMessage>,
    message: &StreamServerMessage,
) {
    let Some(owner_session_id) = owner_session_id else {
        return;
    };

    if matches!(
        message,
        StreamServerMessage::ConnectionComplete { .. } | StreamServerMessage::VideoFlowReady { .. }
    ) {
        register_shared_player2_bridge(
            owner_session_id,
            runtime_session_token,
            host_id,
            app_id,
            ipc_sender,
        )
        .await;
    }
}

async fn claim_shared_player2_joiner(owner_session_id: &str, connection_id: &str) {
    let mut slot = ACTIVE_SHARED_PLAYER2_JOINER.lock().await;
    *slot = Some(ActiveSharedPlayer2Joiner {
        owner_session_id: owner_session_id.to_string(),
        connection_id: connection_id.to_string(),
    });
}

async fn clear_shared_player2_joiner(owner_session_id: &str, connection_id: Option<&str>) {
    let mut slot = ACTIVE_SHARED_PLAYER2_JOINER.lock().await;
    let should_clear = slot.as_ref().is_some_and(|active| {
        active.owner_session_id == owner_session_id
            && connection_id.is_none_or(|value| value == active.connection_id)
    });
    if should_clear {
        *slot = None;
    }
}

async fn shared_player2_joiner_is_current(owner_session_id: &str, connection_id: &str) -> bool {
    let slot = ACTIVE_SHARED_PLAYER2_JOINER.lock().await;
    slot.as_ref().is_some_and(|active| {
        active.owner_session_id == owner_session_id && active.connection_id == connection_id
    })
}

async fn send_shared_player2_transport_bytes(bridge: &ActiveSharedPlayer2Bridge, bytes: Vec<u8>) {
    let mut ipc_sender = bridge.ipc_sender.clone();
    ipc_sender
        .send(ServerIpcMessage::WebSocketTransport(bytes.into()))
        .await;
}

async fn send_shared_player2_disconnect_if_ready(
    owner_session_id: &str,
    host_id: HostId,
    app_id: AppId,
) {
    if let Some(bridge) = get_shared_player2_bridge(owner_session_id, host_id, app_id).await {
        send_shared_player2_transport_bytes(
            &bridge,
            build_shared_player2_controller_disconnected_packet(),
        )
        .await;
    }
}

fn force_legacy_nvenc_enabled() -> bool {
    let Some(bundle_root) = current_bundle_root() else {
        return false;
    };
    let flag_path = bundle_root
        .join("moonlight")
        .join("server")
        .join("force_legacy_nvenc.txt");
    let Ok(raw) = fs::read_to_string(flag_path) else {
        return false;
    };

    truthy_flag_value(&raw)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyRuntimeHardResetMode {
    Auto,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReconnectRecoveryPolicy {
    DisplayTransition,
    DeviceMatch,
    VideoStall,
    RouteFailure,
    StartupRecovery,
    ResumeRecovery,
    Settings,
    Manual,
    Generic,
}

impl ReconnectRecoveryPolicy {
    fn from_reason_text(reason_text: &str) -> Option<Self> {
        let lower = reason_text.to_ascii_lowercase();

        if lower.contains("reconnect policy:display_transition")
            || lower.contains("reconnect display transition:")
        {
            return Some(Self::DisplayTransition);
        }
        if lower.contains("reconnect policy:device_match")
            || lower.contains("reconnect device match orientation:")
            || lower.contains("reconnect device match fullscreen:")
            || lower.contains("reconnect silent device match reconnect:")
        {
            return Some(Self::DeviceMatch);
        }
        if lower.contains("reconnect policy:video_stall")
            || lower.contains("reconnect video stall watchdog")
        {
            return Some(Self::VideoStall);
        }
        if lower.contains("reconnect policy:route_failure")
            || lower.contains("reconnect route failure auto reconnect")
        {
            return Some(Self::RouteFailure);
        }
        if lower.contains("reconnect policy:startup_recovery")
            || lower.contains("reconnect startup timeout recovery")
            || lower.contains("reconnect startup interrupted:")
        {
            return Some(Self::StartupRecovery);
        }
        if lower.contains("reconnect policy:resume_recovery")
            || lower.contains("reconnect resume recovery:")
        {
            return Some(Self::ResumeRecovery);
        }
        if lower.contains("reconnect policy:settings")
            || lower.contains("reconnect settings change requires reconnect")
            || lower.contains("reconnect quick session quality preset")
        {
            return Some(Self::Settings);
        }
        if lower.contains("reconnect policy:manual")
            || lower.contains("reconnect manual reconnect:")
        {
            return Some(Self::Manual);
        }
        if lower.contains("reconnect policy:generic") {
            return Some(Self::Generic);
        }

        None
    }

    fn lifecycle_reason(self) -> &'static str {
        match self {
            Self::DisplayTransition => "display_transition_reconnect",
            Self::DeviceMatch => "device_match_reconnect",
            Self::VideoStall => "video_stall_reconnect",
            Self::RouteFailure => "route_failure_reconnect",
            Self::StartupRecovery => "startup_recovery_reconnect",
            Self::ResumeRecovery => "resume_recovery_reconnect",
            Self::Settings => "settings_reconnect",
            Self::Manual => "manual_reconnect",
            Self::Generic => "generic_reconnect",
        }
    }

    fn policy_code(self) -> &'static str {
        match self {
            Self::DisplayTransition => "display_transition",
            Self::DeviceMatch => "device_match",
            Self::VideoStall => "video_stall",
            Self::RouteFailure => "route_failure",
            Self::StartupRecovery => "startup_recovery",
            Self::ResumeRecovery => "resume_recovery",
            Self::Settings => "settings",
            Self::Manual => "manual",
            Self::Generic => "generic",
        }
    }

    fn allows_soft_reuse(self) -> bool {
        matches!(
            self,
            Self::DisplayTransition | Self::DeviceMatch | Self::Settings
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureRecoveryStrategy {
    RestartRuntime,
    RestartBundle,
    None,
}

impl FailureRecoveryStrategy {
    fn strategy_code(self) -> &'static str {
        match self {
            Self::RestartRuntime => "restart_runtime",
            Self::RestartBundle => "restart_bundle",
            Self::None => "none",
        }
    }
}

fn legacy_runtime_hard_reset_mode() -> LegacyRuntimeHardResetMode {
    let Some(bundle_root) = current_bundle_root() else {
        return LegacyRuntimeHardResetMode::Auto;
    };
    let flag_path = bundle_root
        .join("moonlight")
        .join("server")
        .join("hard_reset_mode.txt");
    let Ok(raw) = fs::read_to_string(flag_path) else {
        return LegacyRuntimeHardResetMode::Auto;
    };

    match raw.trim().to_ascii_lowercase().as_str() {
        "always" | "hard" | "full" => LegacyRuntimeHardResetMode::Always,
        _ => LegacyRuntimeHardResetMode::Auto,
    }
}

fn resolve_runtime_binary_path(configured_path: &str) -> Result<PathBuf, String> {
    let configured = PathBuf::from(configured_path);
    if configured.is_absolute() {
        return Ok(configured);
    }

    let Some(runtime_dir) = current_runtime_dir() else {
        return Err("failed to determine runtime directory".to_string());
    };

    Ok(runtime_dir.join(configured))
}

fn is_loopback_host_address(address: &str) -> bool {
    matches!(address.trim(), "127.0.0.1" | "localhost" | "::1")
}

fn discover_streamer_host_address() -> Option<String> {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(value) => value,
        Err(_) => return None,
    };

    if socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).is_err() {
        return None;
    }

    let Ok(local_addr) = socket.local_addr() else {
        return None;
    };

    match local_addr.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => Some(ip.to_string()),
        IpAddr::V6(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip.to_string()),
        _ => None,
    }
}

fn resolve_streamer_host_address(app: &crate::app::App, detailed: &DetailedHost) -> String {
    if !is_loopback_host_address(&detailed.address) {
        return detailed.address.clone();
    }

    if detailed.local_ip != "Offline" && !is_loopback_host_address(&detailed.local_ip) {
        return detailed.local_ip.clone();
    }

    if let Some(public_ip) = app
        .config()
        .webrtc
        .nat_1to1
        .as_ref()
        .and_then(|mapping| mapping.ips.iter().find(|ip| !is_loopback_host_address(ip)))
    {
        return public_ip.clone();
    }

    if let Some(discovered_ip) = discover_streamer_host_address() {
        return discovered_ip;
    }

    detailed.address.clone()
}

fn stage_child_binary(source_path: &Path, session_token: &str) -> Result<PathBuf, String> {
    let Some(runtime_dir) = current_runtime_dir() else {
        return Err("failed to determine runtime directory".to_string());
    };

    let file_stem = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid child binary path: {}", source_path.display()))?;

    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");

    let staged_dir = runtime_dir.join("staged-runtime");
    fs::create_dir_all(&staged_dir).map_err(|err| {
        format!(
            "failed to create staged runtime dir {}: {err}",
            staged_dir.display()
        )
    })?;

    let staged_name = if extension.is_empty() {
        format!("{file_stem}-{session_token}")
    } else {
        format!("{file_stem}-{session_token}.{extension}")
    };
    let staged_path = staged_dir.join(staged_name);
    fs::copy(source_path, &staged_path).map_err(|err| {
        format!(
            "failed to stage child binary {} -> {}: {err}",
            source_path.display(),
            staged_path.display()
        )
    })?;

    Ok(staged_path)
}

fn cleanup_staged_child_binary(path: &Path) {
    if let Err(err) = fs::remove_file(path) {
        debug!(
            "[Stream]: failed to remove staged child binary {}: {err}",
            path.display()
        );
    }
}

pub(super) fn append_host_stream_trace(line: &str) {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    let Some(runtime_dir) = current_exe.parent() else {
        return;
    };
    let trace_path = runtime_dir.join("server").join("host-stream-live.log");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("{}.{}", duration.as_secs(), duration.subsec_millis()))
        .unwrap_or_else(|_| "time_error".to_string());
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(trace_path)
    {
        let _ = writeln!(file, "[{timestamp}] {line}");
    }
}

fn sanitize_trace_preview(value: &str, max_len: usize) -> String {
    let sanitized = sanitize_trace_value(value);
    if sanitized.len() <= max_len {
        sanitized
    } else {
        format!("{}...", &sanitized[..max_len])
    }
}

fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn record_recent_capture_init_failure() {
    RECENT_CAPTURE_INIT_FAILURE_AT_MS.store(now_unix_millis(), Ordering::Release);
}

fn pending_capture_prestart_refresh_reason(max_age: Duration) -> Option<String> {
    let now = now_unix_millis();
    let max_age_ms = max_age.as_millis() as u64;
    let recent_failure_at = RECENT_CAPTURE_INIT_FAILURE_AT_MS.load(Ordering::Acquire);
    if recent_failure_at > 0 && now.saturating_sub(recent_failure_at) <= max_age_ms {
        return Some("streamer_capture_init_failed".to_string());
    }

    if recent_failure_at > 0 {
        let _ = RECENT_CAPTURE_INIT_FAILURE_AT_MS.compare_exchange(
            recent_failure_at,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    let state_path = current_host_supervisor_state_path()?;
    let raw = fs::read_to_string(state_path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let reason = value
        .get("last_failure_recovery_reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut recovery_source = "supervisor_sunshine_capture_init_failed";
    let completed_at = if reason == "sunshine_capture_init_failed" {
        value
            .get("last_failure_recovery_completed_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .or_else(|| {
                value
                    .get("last_command_finished_at_unix_ms")
                    .and_then(serde_json::Value::as_u64)
            })
            .unwrap_or(0)
    } else {
        recovery_source = "supervisor_recent_incident_capture_init_failed";
        value
            .get("recent_incidents")
            .and_then(serde_json::Value::as_array)
            .and_then(|incidents| {
                incidents
                    .iter()
                    .filter_map(|incident| {
                        let kind = incident
                            .get("kind")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        let reason = incident
                            .get("reason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        if kind == "failure_recovery" && reason == "sunshine_capture_init_failed" {
                            incident
                                .get("at_unix_ms")
                                .and_then(serde_json::Value::as_u64)
                        } else {
                            None
                        }
                    })
                    .max()
            })
            .unwrap_or(0)
    };
    if completed_at == 0 || now.saturating_sub(completed_at) > max_age_ms {
        return None;
    }

    let consumed_at = value
        .get("capture_prestart_refresh_after_failure_at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if consumed_at >= completed_at {
        let consumed_reason = value
            .get("capture_prestart_refresh_after_failure_reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if consumed_reason.starts_with("service_only:") {
            return None;
        }
        return Some("retry_after_unsafe_prestart_refresh".to_string());
    }

    Some(recovery_source.to_string())
}

fn mark_capture_prestart_refresh_consumed(reason: &str) {
    let Some(state_path) = current_host_supervisor_state_path() else {
        return;
    };
    let raw = fs::read_to_string(&state_path).unwrap_or_else(|_| "{}".to_string());
    let mut value =
        serde_json::from_str::<serde_json::Value>(&raw).unwrap_or_else(|_| serde_json::json!({}));
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let now = now_unix_millis();
    object.insert(
        "capture_prestart_refresh_after_failure_at_unix_ms".to_string(),
        serde_json::Value::Number(now.into()),
    );
    object.insert(
        "capture_prestart_refresh_after_failure_reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );
    object.insert(
        "updated_at_unix_ms".to_string(),
        serde_json::Value::Number(now.into()),
    );

    match serde_json::to_string_pretty(&value) {
        Ok(serialized) => {
            if let Err(err) = fs::write(&state_path, format!("{serialized}\n")) {
                debug!(
                    "[Stream]: failed to persist capture prestart refresh state {}: {err}",
                    state_path.display()
                );
            }
        }
        Err(err) => {
            debug!(
                "[Stream]: failed to serialize capture prestart refresh state {}: {err}",
                state_path.display()
            );
        }
    }
}

async fn remember_recent_stream_close(reason: &str, connection_established: bool) {
    let mut state = RECENT_STREAM_CLOSE_CONTEXT.lock().await;
    *state = (
        reason.to_string(),
        now_unix_millis(),
        connection_established,
    );
}

async fn take_recent_connected_reconnect_reason(max_age: Duration) -> Option<String> {
    let now = now_unix_millis();
    let max_age_ms = max_age.as_millis() as u64;
    let mut state = RECENT_STREAM_CLOSE_CONTEXT.lock().await;
    let matches = state.2
        && now.saturating_sub(state.1) <= max_age_ms
        && state.0.to_ascii_lowercase().contains("reconnect");

    if matches {
        let reason = state.0.clone();
        *state = (String::new(), 0, false);
        return Some(reason);
    }

    None
}

fn read_host_capability_profile_snapshot() -> Option<HostCapabilityProfileSnapshot> {
    let runtime_dir = current_runtime_dir()?;
    let profile_path = runtime_dir
        .join("server")
        .join("host_capability_profile.json");
    let raw = fs::read(profile_path).ok()?;
    serde_json::from_slice::<HostCapabilityProfileSnapshot>(&raw).ok()
}

fn read_stream_display_preference_snapshot() -> Option<StreamDisplayPreferenceSnapshot> {
    let runtime_dir = current_runtime_dir()?;
    let preference_path = runtime_dir
        .join("server")
        .join("stream_display_preferences.json");
    let raw = fs::read(preference_path).ok()?;
    serde_json::from_slice::<StreamDisplayPreferenceSnapshot>(&raw).ok()
}

fn read_host_supervisor_state_snapshot() -> Option<HostSupervisorStateSnapshot> {
    let state_path = current_host_supervisor_state_path()?;
    let raw = fs::read(state_path).ok()?;
    serde_json::from_slice::<HostSupervisorStateSnapshot>(&raw).ok()
}

fn recent_host_recovery_gate_reason(
    max_age: Duration,
    include_active_flags: bool,
) -> Option<String> {
    if include_active_flags {
        if HOST_FAILURE_RECOVERY_ACTIVE.load(Ordering::Acquire) {
            return Some("host_failure_recovery_active".to_string());
        }

        if CAPTURE_PRESTART_REFRESH_ACTIVE.load(Ordering::Acquire) {
            return Some("capture_prestart_refresh_active".to_string());
        }
    }

    let snapshot = read_host_supervisor_state_snapshot()?;
    let phase = snapshot.lifecycle_phase.trim();
    if phase.is_empty() {
        return None;
    }

    let updated_at = snapshot.updated_at_unix_ms.unwrap_or_default();
    if updated_at > 0 {
        let now = now_unix_millis();
        let max_age_ms = max_age.as_millis() as u64;
        if now.saturating_sub(updated_at) > max_age_ms {
            return None;
        }
    }

    if phase.eq_ignore_ascii_case("recovering") || phase.eq_ignore_ascii_case("failed") {
        let reason = snapshot.lifecycle_reason.trim().to_string();
        let reason = if reason.is_empty() {
            snapshot
                .last_failure_recovery_reason
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            reason
        };

        if reason.is_empty() {
            return Some(format!("host_lifecycle_{phase}"));
        }

        return Some(format!(
            "host_lifecycle_{phase}:{}",
            sanitize_trace_value(&reason)
        ));
    }

    None
}

fn normalize_stream_display_mode(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        // Embedded invariant for Cloudgime stream: the host display lane must
        // resolve back to the MTT VDD path, even if stale preferences mention
        // primary/custom/QEMU/Parsec displays.
        "mtt" | "mtt_vdd" | "vdd" | "virtual_display" => "mtt_vdd".to_string(),
        _ => "mtt_vdd".to_string(),
    }
}

fn stream_display_preference_allows_non_vdd() -> bool {
    read_stream_display_preference_snapshot()
        .map(|preference| {
            matches!(
                normalize_stream_display_mode(&preference.mode).as_str(),
                "qemu_virtio" | "parsec_vda" | "primary" | "custom"
            )
        })
        .unwrap_or(false)
}

fn selected_runtime_candidate<'a>(
    profile: &'a HostCapabilityProfileSnapshot,
) -> Option<&'a HostCapabilityRuntimeCandidateSnapshot> {
    if profile.selected_runtime_key.trim().is_empty() {
        return None;
    }

    profile.runtime_candidates.iter().find(|candidate| {
        candidate
            .key
            .eq_ignore_ascii_case(profile.selected_runtime_key.trim())
    })
}

fn selected_runtime_encoder_marked_healthy(
    profile: &HostCapabilityProfileSnapshot,
    candidate: &HostCapabilityRuntimeCandidateSnapshot,
) -> bool {
    let runtime_key = profile.selected_runtime_key.trim();
    let encoder_key = profile.selected_encoder.trim();
    if runtime_key.is_empty() || encoder_key.is_empty() {
        return false;
    }

    candidate
        .healthy_encoders
        .iter()
        .any(|value| value.eq_ignore_ascii_case(encoder_key))
        || profile.encoder_probes.iter().any(|probe| {
            probe.runtime_key.eq_ignore_ascii_case(runtime_key)
                && probe.encoder_key.eq_ignore_ascii_case(encoder_key)
                && probe.available
                && probe.ok
        })
}

fn stale_runtime_encoder_startup_issue(
    profile: &HostCapabilityProfileSnapshot,
    candidate: &HostCapabilityRuntimeCandidateSnapshot,
    status: &str,
    reason: Option<&str>,
) -> bool {
    if !selected_runtime_encoder_marked_healthy(profile, candidate) {
        return false;
    }

    let startup_reason_matches = reason.is_some_and(|value| {
        value.eq_ignore_ascii_case("runtime_no_working_encoder")
            || value.eq_ignore_ascii_case("runtime_start_validation_failed")
    });
    let runtime_reason_matches = candidate
        .runtime_status_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("runtime_no_working_encoder")
                || value.eq_ignore_ascii_case("runtime_start_validation_failed")
        });
    let status_matches = status.eq_ignore_ascii_case("failed")
        || status.eq_ignore_ascii_case("validation_failed")
        || status.eq_ignore_ascii_case("pending");

    status_matches && (startup_reason_matches || runtime_reason_matches)
}

fn capability_profile_has_virtual_display_driver(profile: &HostCapabilityProfileSnapshot) -> bool {
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

fn selected_capture_supports_active_display_fallback(
    profile: &HostCapabilityProfileSnapshot,
) -> bool {
    profile
        .selected_capture_reason
        .as_deref()
        .is_some_and(|reason| reason.eq_ignore_ascii_case("rdp_remote_display_active"))
        || profile.selected_capture.eq_ignore_ascii_case("wgc")
}

#[cfg(windows)]
fn current_session_is_remote_desktop() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

#[cfg(not(windows))]
fn current_session_is_remote_desktop() -> bool {
    false
}

fn capability_profile_has_remote_display_adapter(profile: &HostCapabilityProfileSnapshot) -> bool {
    profile.gpu_controllers.iter().any(|gpu| {
        let lowered = gpu.name.to_ascii_lowercase();
        lowered.contains("remote display adapter") || lowered.contains("rdp")
    })
}

fn should_release_rdp_session_before_stream(
    profile: Option<&HostCapabilityProfileSnapshot>,
) -> bool {
    if current_session_is_remote_desktop() {
        return true;
    }

    let Some(profile) = profile else {
        return false;
    };

    profile
        .selected_capture_reason
        .as_deref()
        .is_some_and(|reason| reason.eq_ignore_ascii_case("rdp_remote_display_active"))
        || capability_profile_has_remote_display_adapter(profile)
}

#[cfg(windows)]
async fn release_active_rdp_session_for_stream() -> Result<bool, String> {
    if !should_release_rdp_session_before_stream(read_host_capability_profile_snapshot().as_ref()) {
        return Ok(false);
    }

    let script = concat!(
        "$ErrorActionPreference='Stop';",
        "$sid=(Get-Process -Id $PID).SessionId;",
        "if($sid -lt 1){ throw 'invalid_session_id'; }",
        "Start-Process ",
        "-FilePath \"$env:SystemRoot\\System32\\tscon.exe\" ",
        "-ArgumentList @($sid,'/dest:console') ",
        "-WindowStyle Hidden | Out-Null"
    );

    let mut command = Command::new("powershell.exe");
    apply_background_spawn_flags(&mut command);
    let output = command
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| format!("failed to spawn tscon helper: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit_status={}", output.status)
        };
        return Err(format!(
            "failed to transfer RDP session to console: {detail}"
        ));
    }

    sleep(Duration::from_millis(1800)).await;
    Ok(true)
}

#[cfg(not(windows))]
async fn release_active_rdp_session_for_stream() -> Result<bool, String> {
    Ok(false)
}

fn detect_display_route_issue(profile: Option<&HostCapabilityProfileSnapshot>) -> Option<String> {
    let profile = profile?;

    if selected_capture_supports_active_display_fallback(profile) {
        return None;
    }

    if stream_display_preference_allows_non_vdd() {
        return None;
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
        return Some(format!(
            "Virtual Display Driver is not active. Current capture route: {capture} ({reason})."
        ));
    }

    None
}

fn build_display_prepare_failure_message(err: &str, display_route_issue: Option<&str>) -> String {
    if let Some(issue) = display_route_issue
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!(
            "Display host belum siap untuk stream. {issue} Atur target display dari panel Display di Cloudgime Stream atau repair display di Host Control, lalu coba lagi."
        );
    }

    let detail = err.trim();
    let lowered = detail.to_ascii_lowercase();
    if lowered.contains("virtual display")
        || lowered.contains("vdd")
        || lowered.contains("display prepare")
    {
        return format!(
            "Display host belum siap. Cloudgime stream wajib memakai display target yang berhasil diberi authority. Atur target display dari panel Display di Cloudgime Stream atau perbaiki display driver di Host Control, lalu coba lagi. Detail: {detail}"
        );
    }

    format!("Display host belum siap untuk stream. {detail}")
}

fn should_fallback_after_display_prepare_failure(
    _err: &str,
    _display_route_issue: Option<&str>,
) -> bool {
    // Memperbolehkan fallback ke layar aktif host jika VDD gagal dipersiapkan,
    // agar sesi streaming direct/P2P tetap berjalan stabil dan tidak mati total.
    true
}

fn should_retry_display_prepare_failure(err: &str, display_route_issue: Option<&str>) -> bool {
    let lowered = err.trim().to_ascii_lowercase();
    if lowered.contains("virtual display driver is not active")
        || lowered.contains("current capture route")
        || lowered.contains("safe_default")
        || lowered.contains("disp_change code -1")
        || lowered.contains("vdd display did not come online after duplicate recovery")
        || lowered.contains("stream display disappeared")
        || lowered.contains("authority was not applied")
    {
        return true;
    }

    display_route_issue
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .is_some_and(|issue| {
            issue.contains("virtual display driver is not active")
                || issue.contains("current capture route")
                || issue.contains("safe_default")
        })
}

fn failure_recovery_strategy_for_reason(
    failure_reason: &str,
) -> Option<(FailureRecoveryStrategy, &'static str)> {
    if failure_reason.starts_with("connection_terminated:-100") {
        return Some((
            FailureRecoveryStrategy::RestartRuntime,
            "connection_terminated_no_video_traffic",
        ));
    }
    if failure_reason == "host_api_list_apps_failed" {
        return Some((
            FailureRecoveryStrategy::RestartBundle,
            "host_api_list_apps_failed",
        ));
    }
    if failure_reason == "sunshine_capture_init_failed" {
        return Some((
            FailureRecoveryStrategy::RestartRuntime,
            "sunshine_capture_init_failed",
        ));
    }

    None
}

fn is_sunshine_capture_init_failure_line(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    lowered.contains("failed to initialize video capture")
        || lowered.contains("failed to start moonlight stream")
            && lowered.contains("failed to initialize video capture")
}

fn is_noisy_streamer_stderr_line(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    lowered.contains("pingallcandidates called with no candidate pairs")
        || lowered.contains("failed to resolve stun host")
        || lowered.contains("no available ipv6 ip")
        || lowered.contains("could not listen udp ::")
        || lowered.contains("discard message from")
        || lowered.contains("incoming unhandled rtcp ssrc")
        || lowered.contains("failed to accept rtcp sessionsrtp")
        || lowered.contains("failed to accept rtp sessionsrtp")
        || lowered.contains("closing connection")
        || lowered.contains("closing server connection")
}

fn should_apply_start_stream_settle_delay() -> bool {
    let Some(profile) = read_host_capability_profile_snapshot() else {
        return false;
    };

    profile.selected_runtime_key.eq_ignore_ascii_case("legacy")
        || profile.selected_encoder.eq_ignore_ascii_case("nvenc")
}

fn software_runtime_selected(profile: Option<&HostCapabilityProfileSnapshot>) -> bool {
    profile.is_some_and(|profile| profile.selected_encoder.eq_ignore_ascii_case("software"))
}

fn blocking_runtime_startup_issue(
    profile: Option<&HostCapabilityProfileSnapshot>,
) -> Option<String> {
    let profile = profile?;

    if profile.force_nvenc_enabled && !profile.selected_encoder.eq_ignore_ascii_case("nvenc") {
        return Some(format!(
            "Force NVENC aktif, tetapi runtime memilih encoder {}.",
            profile.selected_encoder
        ));
    }

    if let Some(candidate) = selected_runtime_candidate(profile)
        && let Some(status) = candidate
            .startup_validation_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        let reason = candidate
            .startup_validation_reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if status.eq_ignore_ascii_case("passed") || status.eq_ignore_ascii_case("forced") {
            return None;
        }

        if status.eq_ignore_ascii_case("pending")
            && reason.is_some_and(|value| {
                value.eq_ignore_ascii_case("runtime_start_validation_required")
            })
        {
            return None;
        }

        if stale_runtime_encoder_startup_issue(profile, candidate, status, reason) {
            return None;
        }

        let reason_suffix = candidate
            .startup_validation_reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!(" ({value})"))
            .unwrap_or_default();
        return Some(format!(
            "Runtime host belum sehat untuk stream: status {status}{reason_suffix}."
        ));
    }

    None
}

fn should_retry_runtime_preflight_before_session(
    profile: Option<&HostCapabilityProfileSnapshot>,
    runtime_issue: Option<&str>,
) -> bool {
    let Some(profile) = profile else {
        return false;
    };

    if legacy_runtime_selected_in_snapshot(Some(profile)) {
        return false;
    }

    if !profile.force_nvenc_enabled && !profile.selected_encoder.eq_ignore_ascii_case("nvenc") {
        return false;
    }

    let Some(runtime_issue) = runtime_issue else {
        return false;
    };

    let lowered = runtime_issue.trim().to_ascii_lowercase();
    lowered.contains("runtime_no_working_encoder")
        || lowered.contains("runtime_start_validation_required")
        || lowered.contains("runtime_start_validation_failed")
}

fn apply_software_stream_limits(
    width: &mut u32,
    height: &mut u32,
    fps: &mut u32,
    bitrate: &mut u32,
    packet_size: &mut u32,
) -> bool {
    let mut changed = false;

    let max_fps = if (*width >= 1200 || *height >= 1200) && (*width >= 700 || *height >= 700) {
        30
    } else {
        45
    };
    if *fps > max_fps {
        *fps = max_fps;
        changed = true;
    }

    let max_bitrate = if *fps > 30 { 6500 } else { 5000 };
    if *bitrate > max_bitrate {
        *bitrate = max_bitrate;
        changed = true;
    }

    if *packet_size > 1000 {
        *packet_size = 1000;
        changed = true;
    }

    changed
}

fn apply_legacy_nvenc_stream_limits(
    width: &mut u32,
    height: &mut u32,
    fps: &mut u32,
    bitrate: &mut u32,
    packet_size: &mut u32,
    preserve_surface: bool,
) -> bool {
    let mut changed = false;

    if !preserve_surface && clamp_legacy_nvenc_surface(width, height, *fps) {
        changed = true;
    }

    let max_fps = get_legacy_nvenc_surface_max_fps(*width, *height);
    if *fps == 0 || *fps > max_fps {
        *fps = max_fps;
        changed = true;
    }

    let max_bitrate = get_legacy_nvenc_surface_max_bitrate(*width, *height, *fps);
    if *bitrate == 0 || *bitrate > max_bitrate {
        *bitrate = max_bitrate;
        changed = true;
    }

    let max_packet_size = 960;
    if *packet_size == 0 || *packet_size > max_packet_size {
        *packet_size = max_packet_size;
        changed = true;
    }

    changed
}

fn clamp_legacy_nvenc_surface(width: &mut u32, height: &mut u32, _requested_fps: u32) -> bool {
    let original_width = *width;
    let original_height = *height;

    if original_width == 0 || original_height == 0 {
        return false;
    }

    let portrait = original_height > original_width;
    let long_edge_u32 = original_width.max(original_height);
    let short_edge_u32 = original_width.min(original_height);

    let legacy_large_long_edge_limit: u32 = 2160;
    let legacy_large_short_edge_limit: u32 = 1440;

    let quantize_dimension = |value: f32, min_value: u32| -> u32 {
        let rounded_down = (value.max(min_value as f32) / 16.0).floor() as u32 * 16;
        rounded_down.max(min_value)
    };

    if portrait && long_edge_u32 >= 1440 && short_edge_u32 >= 900 {
        if long_edge_u32 <= legacy_large_long_edge_limit
            && short_edge_u32 <= legacy_large_short_edge_limit
        {
            return false;
        }

        let scale = f32::min(
            legacy_large_long_edge_limit as f32 / long_edge_u32 as f32,
            legacy_large_short_edge_limit as f32 / short_edge_u32 as f32,
        );
        let next_width = quantize_dimension((original_width as f32) * scale, 320);
        let next_height = quantize_dimension((original_height as f32) * scale, 180);
        if next_width != original_width || next_height != original_height {
            *width = next_width;
            *height = next_height;
            return true;
        }

        return false;
    }

    if !portrait && long_edge_u32 >= 1440 && short_edge_u32 >= 884 {
        if long_edge_u32 <= legacy_large_long_edge_limit
            && short_edge_u32 <= legacy_large_short_edge_limit
        {
            return false;
        }

        let scale = f32::min(
            legacy_large_long_edge_limit as f32 / long_edge_u32 as f32,
            legacy_large_short_edge_limit as f32 / short_edge_u32 as f32,
        );
        let next_width = quantize_dimension((original_width as f32) * scale, 320);
        let next_height = quantize_dimension((original_height as f32) * scale, 180);
        if next_width != original_width || next_height != original_height {
            *width = next_width;
            *height = next_height;
            return true;
        }

        return false;
    }

    let width_f = original_width as f32;
    let height_f = original_height as f32;
    let long_edge = width_f.max(height_f);
    let short_edge = width_f.min(height_f).max(1.0);

    // The legacy NVENC runtime on this host becomes flaky when mobile clients
    // request arbitrary portrait surfaces. Large portrait requests can be
    // snapped above to a canonical 900x1440 mode that this host repeatedly
    // negotiates successfully, but smaller requests must never be upscaled
    // into a heavier surface. Those stay in a conservative aligned envelope
    // for older encoder/decoder paths.
    let scale = f32::min(1.0, f32::min(1280.0 / long_edge, 720.0 / short_edge));
    if scale >= 0.999 {
        return false;
    }

    let scaled_width = (width_f * scale).max(320.0);
    let scaled_height = (height_f * scale).max(180.0);

    let mut next_width = quantize_dimension(scaled_width, 320);
    let mut next_height = quantize_dimension(scaled_height, 180);

    if next_width == original_width && next_height == original_height {
        return false;
    }

    let next_long_edge = next_width.max(next_height);
    let next_short_edge = next_width.min(next_height);
    if next_long_edge > 1280 || next_short_edge > 720 {
        let correction = f32::min(
            1280.0 / (next_long_edge as f32),
            720.0 / (next_short_edge as f32).max(1.0),
        );
        next_width = quantize_dimension((next_width as f32) * correction, 320);
        next_height = quantize_dimension((next_height as f32) * correction, 180);
    }

    *width = next_width;
    *height = next_height;
    true
}

fn get_legacy_nvenc_surface_max_fps(width: u32, height: u32) -> u32 {
    let long_edge = width.max(height);
    let short_edge = width.min(height);

    if long_edge >= 2160 && short_edge >= 1400 {
        60
    } else if long_edge >= 1400 && short_edge >= 900 {
        60
    } else if long_edge >= 1280 && short_edge >= 720 {
        45
    } else {
        60
    }
}

fn get_legacy_nvenc_surface_max_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let long_edge = width.max(height);
    let short_edge = width.min(height);
    if long_edge >= 2160 && short_edge >= 1400 {
        if fps > 45 { 12_000 } else { 9000 }
    } else if long_edge >= 1400 && short_edge >= 900 {
        if fps > 45 { 9000 } else { 7000 }
    } else if fps > 45 {
        6500
    } else {
        5000
    }
}

fn apply_software_resize_limits(width: u32, height: u32, fps: &mut u32) -> bool {
    let max_fps = if (width >= 1200 || height >= 1200) && (width >= 700 || height >= 700) {
        30
    } else {
        45
    };
    if *fps > max_fps {
        *fps = max_fps;
        return true;
    }
    false
}

fn apply_legacy_nvenc_resize_limits(
    width: &mut u32,
    height: &mut u32,
    fps: &mut u32,
    preserve_surface: bool,
) -> bool {
    let mut changed = false;

    if !preserve_surface && clamp_legacy_nvenc_surface(width, height, *fps) {
        changed = true;
    }

    let max_fps = get_legacy_nvenc_surface_max_fps(*width, *height);
    if *fps == 0 || *fps > max_fps {
        *fps = max_fps;
        changed = true;
    }

    changed
}

fn should_preserve_legacy_nvenc_stream_surface(
    requested_width: u32,
    requested_height: u32,
    applied_width: u32,
    applied_height: u32,
) -> bool {
    requested_width > 0
        && requested_height > 0
        && same_stream_orientation(
            requested_width,
            requested_height,
            applied_width,
            applied_height,
        )
        && applied_width >= requested_width
        && applied_height >= requested_height
        && (applied_width != requested_width || applied_height != requested_height)
}

fn same_stream_orientation(
    requested_width: u32,
    requested_height: u32,
    applied_width: u32,
    applied_height: u32,
) -> bool {
    (requested_width >= requested_height) == (applied_width >= applied_height)
}

fn should_preserve_requested_stream_surface_after_display_fallback(
    _profile: Option<&HostCapabilityProfileSnapshot>,
    requested_width: u32,
    requested_height: u32,
    applied_width: u32,
    applied_height: u32,
    reason: &str,
) -> bool {
    let fallback_reason = reason.trim().to_ascii_lowercase();
    reason_indicates_stream_surface_fallback(&fallback_reason)
        && requested_width > 0
        && requested_height > 0
        && same_stream_orientation(
            requested_width,
            requested_height,
            applied_width,
            applied_height,
        )
        && applied_width >= requested_width
        && applied_height >= requested_height
        && (applied_width != requested_width || applied_height != requested_height)
}

fn reason_indicates_stream_surface_fallback(reason: &str) -> bool {
    reason.contains("surface_fallback")
        || reason.ends_with("_fallback")
            && (reason.contains("stream_display_session") || reason.contains("vdd_only"))
}

fn cap_stream_fps_to_applied_refresh(requested_fps: &mut u32, applied_refresh: u32) {
    if applied_refresh == 0 {
        return;
    }

    if *requested_fps == 0 {
        *requested_fps = applied_refresh;
    } else {
        *requested_fps = (*requested_fps).min(applied_refresh);
    }
}

fn legacy_runtime_selected_in_snapshot(profile: Option<&HostCapabilityProfileSnapshot>) -> bool {
    profile.is_some_and(|profile| profile.selected_runtime_key.eq_ignore_ascii_case("legacy"))
}

fn should_auto_fallback_legacy_after_host_api_error(err_text: &str) -> bool {
    let err_text = err_text.to_ascii_lowercase();
    err_text.contains("49000")
        || err_text.contains("connection refused")
        || err_text.contains("actively refused")
        || err_text.contains("failed to connect")
        || err_text.contains("connection reset")
}

fn disable_legacy_runtime_auto_select(reason: &str) -> Result<bool, String> {
    let Some(bundle_root) = current_bundle_root() else {
        return Err("failed to determine bundle root".to_string());
    };
    let metadata_path = bundle_root
        .join("sunshine-legacy")
        .join("sunshine_runtime_info.json");
    let raw = fs::read_to_string(&metadata_path).map_err(|err| {
        format!(
            "failed to read legacy runtime metadata {}: {err}",
            metadata_path.display()
        )
    })?;
    let mut value = serde_json::from_str::<serde_json::Value>(&raw).map_err(|err| {
        format!(
            "failed to parse legacy runtime metadata {}: {err}",
            metadata_path.display()
        )
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        format!(
            "legacy runtime metadata is not a JSON object: {}",
            metadata_path.display()
        )
    })?;

    let already_disabled = object.get("auto_select").and_then(|value| value.as_bool())
        == Some(false)
        && object
            .get("startup_validation_reason")
            .and_then(|value| value.as_str())
            == Some(reason);

    object.insert("auto_select".to_string(), serde_json::Value::Bool(false));
    object.insert(
        "startup_validation_status".to_string(),
        serde_json::Value::String("failed".to_string()),
    );
    object.insert(
        "startup_validation_reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );

    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|err| format!("failed to serialize legacy runtime metadata: {err}"))?;
    fs::write(&metadata_path, format!("{serialized}\n")).map_err(|err| {
        format!(
            "failed to write legacy runtime metadata {}: {err}",
            metadata_path.display()
        )
    })?;

    Ok(!already_disabled)
}

async fn refresh_bundle_host_preflight() -> Result<(), String> {
    let Some(bundle_root) = current_bundle_root() else {
        return Err("failed to determine bundle root".to_string());
    };
    let bundle_root = bundle_root.to_string_lossy().to_string();
    run_display_prepare_helper(&[
        "preflight".to_string(),
        "--refresh".to_string(),
        "--bundle-root".to_string(),
        bundle_root,
    ])
    .await?;
    Ok(())
}

async fn wait_for_host_tcp_ready(
    address: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut last_error = String::new();

    while started.elapsed() < timeout {
        match TcpStream::connect((address, port)).await {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(err) => {
                last_error = err.to_string();
                sleep(Duration::from_millis(250)).await;
            }
        }
    }

    Err(if last_error.is_empty() {
        format!(
            "timed out waiting for host tcp {}:{} to accept connections",
            address, port
        )
    } else {
        format!(
            "timed out waiting for host tcp {}:{} to accept connections: {}",
            address, port, last_error
        )
    })
}

async fn wait_for_host_tcp_closed(
    address: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let started = std::time::Instant::now();

    while started.elapsed() < timeout {
        match TcpStream::connect((address, port)).await {
            Ok(stream) => {
                drop(stream);
                sleep(Duration::from_millis(200)).await;
            }
            Err(_) => {
                return Ok(());
            }
        }
    }

    Err(format!(
        "timed out waiting for host tcp {}:{} to stop accepting connections",
        address, port
    ))
}

async fn wait_for_host_runtime_ready(
    address: &str,
    http_port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let control_address = legacy_runtime_control_address(address);
    let started = std::time::Instant::now();
    let mut last_detail = String::new();

    while started.elapsed() < timeout {
        if let Err(err) =
            wait_for_host_tcp_ready(control_address, http_port, Duration::from_millis(900)).await
        {
            last_detail = err;
            sleep(Duration::from_millis(350)).await;
            continue;
        }

        if let Some(gate_reason) = recent_host_recovery_gate_reason(Duration::from_secs(180), false)
        {
            last_detail = format!("host recovery still active ({gate_reason})");
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        let capability = read_host_capability_profile_snapshot();
        if let Some(runtime_issue) = blocking_runtime_startup_issue(capability.as_ref()) {
            last_detail = runtime_issue;
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        return Ok(());
    }

    Err(if last_detail.is_empty() {
        format!(
            "host runtime did not become ready in time for {}:{}",
            sanitize_trace_value(control_address),
            http_port
        )
    } else {
        format!(
            "host runtime did not become ready in time for {}:{} ({})",
            sanitize_trace_value(control_address),
            http_port,
            sanitize_trace_value(&last_detail)
        )
    })
}

async fn run_blocking_host_failure_recovery(
    session_token: &str,
    strategy: FailureRecoveryStrategy,
    failure_reason: &str,
    address: &str,
    http_port: u16,
) -> Result<(), String> {
    if !HOST_FAILURE_RECOVERY_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        append_host_stream_trace(&format!(
            "HOST_FAILURE_RECOVERY stage=wait_existing token={} strategy={} reason={}",
            session_token,
            strategy.strategy_code(),
            sanitize_trace_value(failure_reason)
        ));
        return wait_for_host_runtime_ready(address, http_port, Duration::from_secs(70)).await;
    }

    let result = async {
        append_host_stream_trace(&format!(
            "HOST_FAILURE_RECOVERY stage=blocking_begin token={} strategy={} reason={}",
            session_token,
            strategy.strategy_code(),
            sanitize_trace_value(failure_reason)
        ));
        note_host_lifecycle_phase("recovering", failure_reason);

        let bundle_root =
            current_bundle_root().ok_or_else(|| "failed to determine bundle root".to_string())?;
        let supervisor_path = get_host_supervisor_path()?;
        if !supervisor_path.exists() {
            return Err(format!(
                "missing host supervisor at {}",
                supervisor_path.display()
            ));
        }

        let status = Command::new(&supervisor_path)
            .arg("--bundle-root")
            .arg(&*bundle_root.to_string_lossy())
            .arg("recover-failure")
            .arg("--strategy")
            .arg(strategy.strategy_code())
            .arg("--reason")
            .arg(failure_reason)
            .current_dir(&bundle_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|err| {
                format!(
                    "failed to run blocking host recovery via supervisor {}: {err}",
                    supervisor_path.display()
                )
            })?;

        if !status.success() {
            return Err(format!(
                "host supervisor returned non-zero status while recovering failure: {status}"
            ));
        }

        refresh_bundle_host_preflight().await?;
        wait_for_host_runtime_ready(address, http_port, Duration::from_secs(70)).await?;

        append_host_stream_trace(&format!(
            "HOST_FAILURE_RECOVERY stage=blocking_completed token={} strategy={} reason={}",
            session_token,
            strategy.strategy_code(),
            sanitize_trace_value(failure_reason)
        ));

        Ok::<(), String>(())
    }
    .await;

    HOST_FAILURE_RECOVERY_ACTIVE.store(false, Ordering::Release);
    result
}

#[cfg(windows)]
async fn taskkill_image(image_name: &str) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/IM", image_name, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|err| format!("failed to stop {image_name}: {err}"))?;

    if !status.success() {
        return Err(format!(
            "taskkill returned non-zero status for {image_name}: {status}"
        ));
    }

    Ok(())
}

#[cfg(windows)]
async fn control_windows_service(action: &str, service_name: &str) -> Result<(), String> {
    let output = Command::new("sc.exe")
        .args([action, service_name])
        .output()
        .await
        .map_err(|err| format!("failed to run sc.exe {action} {service_name}: {err}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout} {stderr}").to_ascii_lowercase();
    let acceptable = match action.to_ascii_lowercase().as_str() {
        "stop" => {
            combined.contains("1062")
                || combined.contains("service has not been started")
                || combined.contains("not been started")
        }
        "start" => combined.contains("1056") || combined.contains("already running"),
        _ => false,
    };

    if acceptable {
        return Ok(());
    }

    Err(format!(
        "sc.exe {action} {service_name} returned {}: {} {}",
        output.status,
        sanitize_trace_value(&stdout),
        sanitize_trace_value(&stderr)
    ))
}

#[cfg(windows)]
fn powershell_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(windows)]
async fn taskkill_pid(pid: u32) -> Result<(), String> {
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = match timeout(TASKKILL_TIMEOUT, command.status()).await {
        Ok(result) => result.map_err(|err| format!("failed to stop pid {pid}: {err}"))?,
        Err(_) => return Err(format!("timed out stopping pid {pid}")),
    };

    if !status.success() {
        return Err(format!(
            "taskkill returned non-zero status for pid {pid}: {status}"
        ));
    }

    Ok(())
}

#[cfg(windows)]
async fn list_bundle_staged_runtime_processes(
    prefixes: &[&str],
) -> Result<Vec<(u32, PathBuf)>, String> {
    let Some(bundle_root) = current_bundle_root() else {
        return Err("failed to determine bundle root".to_string());
    };
    let staged_dir = bundle_root.join("moonlight").join("staged-runtime");
    let staged_root = format!("{}\\", staged_dir.to_string_lossy());
    let staged_root = powershell_single_quote(&staged_root);
    let name_checks = prefixes
        .iter()
        .map(|prefix| {
            format!(
                "([System.IO.Path]::GetFileName($_.ExecutablePath) -like '{}*.exe')",
                powershell_single_quote(prefix)
            )
        })
        .collect::<Vec<_>>()
        .join(" -or ");
    let script = format!(
        "$root='{staged_root}'; \
         Get-CimInstance Win32_Process | Where-Object {{ \
           $_.ExecutablePath -and \
           $_.ExecutablePath.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase) -and \
           ({name_checks}) \
         }} | ForEach-Object {{ '{{0}}|{{1}}' -f $_.ProcessId, $_.ExecutablePath }}"
    );
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    apply_background_spawn_flags(&mut command);
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|err| format!("failed to query staged runtime processes: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "powershell query returned non-zero status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((pid_text, path_text)) = line.split_once('|') else {
            continue;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            continue;
        };
        processes.push((pid, PathBuf::from(path_text.trim())));
    }

    Ok(processes)
}

#[cfg(windows)]
async fn list_bundle_runtime_processes() -> Result<Vec<(u32, PathBuf)>, String> {
    let Some(bundle_root) = current_bundle_root() else {
        return Err("failed to determine bundle root".to_string());
    };
    let bundle_root = format!("{}\\", bundle_root.to_string_lossy());
    let bundle_root = powershell_single_quote(&bundle_root);
    let script = format!(
        "$root='{bundle_root}'; \
         Get-CimInstance Win32_Process | Where-Object {{ \
           $_.ExecutablePath -and \
           $_.ExecutablePath.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase) -and \
           ( \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -ieq 'sunshine.exe') -or \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -ieq 'sunshinesvc.exe') -or \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -ieq 'streamer.exe') -or \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -ieq 'mic_sidecar.exe') -or \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -like 'streamer-*.exe') -or \
             ([System.IO.Path]::GetFileName($_.ExecutablePath) -like 'mic_sidecar-*.exe') \
           ) \
         }} | ForEach-Object {{ '{{0}}|{{1}}' -f $_.ProcessId, $_.ExecutablePath }}"
    );
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    apply_background_spawn_flags(&mut command);
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|err| format!("failed to query bundle runtime processes: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "powershell runtime query returned non-zero status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((pid_text, path_text)) = line.split_once('|') else {
            continue;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            continue;
        };
        processes.push((pid, PathBuf::from(path_text.trim())));
    }

    Ok(processes)
}

#[cfg(windows)]
async fn wait_for_bundle_runtime_processes_closed(timeout: Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut last_remaining = String::new();

    while started.elapsed() < timeout {
        match list_bundle_runtime_processes().await {
            Ok(processes) if processes.is_empty() => return Ok(()),
            Ok(processes) => {
                last_remaining = processes
                    .into_iter()
                    .map(|(pid, path)| format!("{pid}:{}", path.display()))
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            Err(err) => {
                last_remaining = err;
            }
        }

        sleep(Duration::from_millis(250)).await;
    }

    Err(if last_remaining.is_empty() {
        "timed out waiting for bundle runtime processes to stop".to_string()
    } else {
        format!("timed out waiting for bundle runtime processes to stop: {last_remaining}")
    })
}

#[cfg(windows)]
fn cleanup_staged_runtime_files(session_token: &str) {
    let Some(bundle_root) = current_bundle_root() else {
        return;
    };
    let staged_dir = bundle_root.join("moonlight").join("staged-runtime");
    let entries = match fs::read_dir(&staged_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = file_name.to_ascii_lowercase();
        let is_staged_child = (lower.starts_with("streamer-") || lower.starts_with("mic_sidecar-"))
            && lower.ends_with(".exe");
        if !is_staged_child {
            continue;
        }

        if let Err(err) = fs::remove_file(&path) {
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_staged_file_failed path={} error={}",
                session_token,
                sanitize_trace_value(&path.display().to_string()),
                sanitize_trace_value(&err.to_string())
            ));
        }
    }
}

#[cfg(windows)]
async fn cleanup_bundle_stream_processes(session_token: &str) {
    let mut killed_any = false;

    if let Some(previous_stream) = take_active_child(&ACTIVE_STREAM_CHILD).await {
        killed_any = true;
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_active_stream_child",
            session_token
        ));
        force_kill_child(&previous_stream, "Stream").await;
    }

    if let Some(previous_window_watch) = take_active_child(&ACTIVE_WINDOW_WATCH_CHILD).await {
        killed_any = true;
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_active_window_watch_child",
            session_token
        ));
        force_kill_child(&previous_window_watch, "Window Watch").await;
    }

    if let Some(previous_mic_sidecar) = take_active_child(&ACTIVE_MIC_SIDECAR_CHILD).await {
        killed_any = true;
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_active_mic_sidecar_child",
            session_token
        ));
        force_kill_child(&previous_mic_sidecar, "Mic Sidecar").await;
    }

    match list_bundle_staged_runtime_processes(&["streamer-", "mic_sidecar-"]).await {
        Ok(processes) => {
            for (pid, path) in processes {
                match taskkill_pid(pid).await {
                    Ok(()) => {
                        killed_any = true;
                        append_host_stream_trace(&format!(
                            "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_staged_process_killed pid={} path={}",
                            session_token,
                            pid,
                            sanitize_trace_value(&path.display().to_string())
                        ));
                    }
                    Err(err) => {
                        append_host_stream_trace(&format!(
                            "LEGACY_RUNTIME_REFRESH token={} stage=cleanup_staged_process_failed pid={} path={} error={}",
                            session_token,
                            pid,
                            sanitize_trace_value(&path.display().to_string()),
                            sanitize_trace_value(&err)
                        ));
                    }
                }
            }
        }
        Err(err) => {
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_REFRESH token={} stage=query_staged_processes_failed error={}",
                session_token,
                sanitize_trace_value(&err)
            ));
        }
    }

    if killed_any {
        sleep(Duration::from_millis(250)).await;
    }

    cleanup_staged_runtime_files(session_token);
}

#[cfg(windows)]
pub(crate) async fn cleanup_android_native_stream_session(session_token: &str, reason: &str) {
    if session_token.trim().is_empty() {
        return;
    }

    append_host_stream_trace(&format!(
        "ANDROID_NATIVE_SESSION_CLEANUP_REQUEST token={} reason={}",
        session_token,
        sanitize_trace_value(reason)
    ));
    note_host_lifecycle_phase("recovering", "android_native_session_cleanup");
    cleanup_bundle_stream_processes(session_token).await;

    if should_skip_prepared_stream_restore_after_session_cleanup() {
        append_host_stream_trace(&format!(
            "ANDROID_NATIVE_SESSION_CLEANUP_DISPLAY_RESTORE_SKIPPED token={} reason=mtt_vdd_cleanup_invariant",
            session_token
        ));
    } else {
        match restore_prepared_stream_display(session_token).await {
            Ok(result) => {
                append_host_stream_trace(&format!(
                    "ANDROID_NATIVE_SESSION_CLEANUP_DISPLAY_RESTORE token={} restored={} skipped={} reason={}",
                    session_token,
                    result.restored,
                    result.skipped,
                    sanitize_trace_value(&result.reason)
                ));
            }
            Err(err) => {
                warn!("[Stream]: android native cleanup display restore failed: {err}");
                append_host_stream_trace(&format!(
                    "ANDROID_NATIVE_SESSION_CLEANUP_DISPLAY_RESTORE_FAILED token={} error={}",
                    session_token,
                    sanitize_trace_value(&err)
                ));
            }
        }
    }

    note_host_lifecycle_phase("ready", "android_native_session_cleanup_completed");
    append_host_stream_trace(&format!(
        "ANDROID_NATIVE_SESSION_CLEANUP_COMPLETED token={} reason={}",
        session_token,
        sanitize_trace_value(reason)
    ));
}

#[cfg(not(windows))]
pub(crate) async fn cleanup_android_native_stream_session(session_token: &str, reason: &str) {
    append_host_stream_trace(&format!(
        "ANDROID_NATIVE_SESSION_CLEANUP_REQUEST token={} reason={} platform=non_windows",
        session_token,
        sanitize_trace_value(reason)
    ));
}

#[cfg(windows)]
async fn restart_bundle_sunshine(address: &str, http_port: u16) -> Result<(), String> {
    restart_bundle_sunshine_with_policy(address, http_port, None).await
}

#[cfg(windows)]
fn legacy_runtime_control_address(_address: &str) -> &'static str {
    "127.0.0.1"
}

#[cfg(windows)]
async fn restart_bundle_sunshine_with_policy(
    address: &str,
    http_port: u16,
    policy: Option<ReconnectRecoveryPolicy>,
) -> Result<(), String> {
    let control_address = legacy_runtime_control_address(address);
    let Some(bundle_root) = current_bundle_root() else {
        return Err("failed to determine bundle root".to_string());
    };

    if let Ok(supervisor_path) = get_host_supervisor_path()
        && supervisor_path.exists()
    {
        let mut command = Command::new(&supervisor_path);
        command
            .arg("--bundle-root")
            .arg(&*bundle_root.to_string_lossy());
        if let Some(policy) = policy {
            command
                .arg("recover-runtime")
                .arg("--policy")
                .arg(policy.policy_code());
        } else {
            command.arg("restart-runtime");
        }

        let status = command
            .current_dir(&bundle_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|err| {
                format!(
                    "failed to restart sunshine via host supervisor {}: {err}",
                    supervisor_path.display()
                )
            })?;

        if !status.success() {
            return Err(format!(
                "host supervisor returned non-zero status while restarting sunshine: {status}"
            ));
        }

        return Ok(());
    }

    let start_script = bundle_root.join("start-sunshine.bat");
    if !start_script.exists() {
        return Err(format!(
            "failed to find start-sunshine.bat at {}",
            start_script.display()
        ));
    }

    cleanup_bundle_stream_processes("bundle_restart").await;

    for pass in 0..3 {
        for image_name in [
            "sunshinesvc.exe",
            "sunshine.exe",
            "streamer.exe",
            "mic_sidecar.exe",
        ] {
            if let Err(err) = taskkill_image(image_name).await {
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_REFRESH stage=restart_sunshine_cleanup pass={} image={} error={}",
                    pass + 1,
                    sanitize_trace_value(image_name),
                    sanitize_trace_value(&err)
                ));
            }
        }

        cleanup_bundle_stream_processes("bundle_restart").await;
        sleep(Duration::from_millis(500)).await;
    }

    if let Err(err) =
        wait_for_host_tcp_closed(control_address, http_port, Duration::from_secs(12)).await
    {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH stage=wait_closed_before_restart address={} control_address={} port={} error={}",
            sanitize_trace_value(address),
            sanitize_trace_value(control_address),
            http_port,
            sanitize_trace_value(&err)
        ));
    }

    if let Err(err) = wait_for_bundle_runtime_processes_closed(Duration::from_secs(12)).await {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH stage=wait_runtime_processes_closed error={}",
            sanitize_trace_value(&err)
        ));
    }

    sleep(Duration::from_millis(1800)).await;

    Command::new("cmd")
        .arg("/C")
        .arg(&start_script)
        .current_dir(&bundle_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            format!(
                "failed to restart sunshine with {}: {err}",
                start_script.display()
            )
        })?;

    Ok(())
}

#[cfg(windows)]
async fn start_bundle_runtime_detached(bundle_root: &Path) -> Result<(), String> {
    if let Ok(supervisor_path) = get_host_supervisor_path()
        && supervisor_path.exists()
    {
        let mut command = Command::new(&supervisor_path);
        command
            .arg("--bundle-root")
            .arg(&*bundle_root.to_string_lossy())
            .arg("start-bundle")
            .current_dir(bundle_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_background_spawn_flags(&mut command);
        command.spawn().map_err(|err| {
            format!(
                "failed to start bundle runtime via host supervisor {}: {err}",
                supervisor_path.display()
            )
        })?;
        return Ok(());
    }

    let start_script = bundle_root.join("start-bundle.bat");
    if !start_script.exists() {
        return Err(format!(
            "failed to find start-bundle.bat at {}",
            start_script.display()
        ));
    }

    let mut command = Command::new("cmd");
    command
        .arg("/C")
        .arg(&start_script)
        .current_dir(bundle_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_background_spawn_flags(&mut command);
    command.spawn().map_err(|err| {
        format!(
            "failed to start bundle runtime with {}: {err}",
            start_script.display()
        )
    })?;

    Ok(())
}

#[cfg(windows)]
async fn refresh_bundle_capture_runtime_with_prepared_display(
    session_token: &str,
    requested_mode: Option<(u32, u32, u32)>,
    address: &str,
    http_port: u16,
) -> Result<DynamicDisplayResult, String> {
    let control_address = legacy_runtime_control_address(address);
    let bundle_root =
        current_bundle_root().ok_or_else(|| "failed to determine bundle root".to_string())?;

    for pass in 0..3 {
        for image_name in ["sunshinesvc.exe", "sunshine.exe"] {
            if let Err(err) = taskkill_image(image_name).await {
                append_host_stream_trace(&format!(
                    "SUNSHINE_CAPTURE_RUNTIME_STOP token={} pass={} image={} error={}",
                    session_token,
                    pass + 1,
                    sanitize_trace_value(image_name),
                    sanitize_trace_value(&err)
                ));
            }
        }

        if wait_for_host_tcp_closed(control_address, http_port, Duration::from_millis(1200))
            .await
            .is_ok()
        {
            break;
        }

        sleep(Duration::from_millis(400)).await;
    }

    wait_for_host_tcp_closed(control_address, http_port, Duration::from_secs(12))
        .await
        .map_err(|err| format!("capture runtime did not stop cleanly: {err}"))?;

    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_RUNTIME_STOPPED token={} address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    let prepared_result = match prepare_stream_display(session_token, requested_mode).await {
        Ok(result) => result,
        Err(err) => {
            let _ = start_bundle_runtime_detached(&bundle_root).await;
            let _ =
                wait_for_host_tcp_ready(control_address, http_port, Duration::from_secs(15)).await;
            return Err(format!(
                "failed to prepare stream display while capture runtime was stopped: {err}"
            ));
        }
    };

    if let Some(applied) = prepared_result.applied.as_ref() {
        append_host_stream_trace(&format!(
            "SUNSHINE_CAPTURE_RUNTIME_PRESTART_PREPARE token={} applied={}x{}@{} changed={} skipped={} reason={}",
            session_token,
            applied.width,
            applied.height,
            applied.frequency,
            prepared_result.changed,
            prepared_result.skipped,
            sanitize_trace_value(&prepared_result.reason)
        ));
    } else {
        append_host_stream_trace(&format!(
            "SUNSHINE_CAPTURE_RUNTIME_PRESTART_PREPARE token={} changed={} skipped={} reason={}",
            session_token,
            prepared_result.changed,
            prepared_result.skipped,
            sanitize_trace_value(&prepared_result.reason)
        ));
    }

    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_RUNTIME_STARTING token={} address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    match start_bundle_runtime_detached(&bundle_root).await {
        Ok(()) => {}
        Err(err) => {
            restart_bundle_sunshine(address, http_port)
                .await
                .map_err(|restart_err| {
                    format!(
                        "failed to start bundle runtime after prepared display refresh: {err}; fallback restart failed: {restart_err}"
                    )
                })?;
        }
    }

    wait_for_host_tcp_ready(control_address, http_port, Duration::from_secs(20))
        .await
        .map_err(|err| {
            format!("capture runtime did not become ready after prepared display refresh: {err}")
        })?;

    Ok(prepared_result)
}

#[cfg(windows)]
async fn restart_sunshine_capture_service_only(
    session_token: &str,
    address: &str,
    http_port: u16,
) -> Result<(), String> {
    let control_address = legacy_runtime_control_address(address);
    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_SERVICE_ONLY_REFRESH token={} stage=begin address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    if let Err(err) = control_windows_service("stop", "CloudgimeRuntime-Host").await {
        append_host_stream_trace(&format!(
            "SUNSHINE_CAPTURE_SERVICE_ONLY_REFRESH token={} stage=service_stop_warning error={}",
            session_token,
            sanitize_trace_value(&err)
        ));
    }

    for pass in 0..3 {
        for image_name in ["sunshinesvc.exe", "sunshine.exe"] {
            if let Err(err) = taskkill_image(image_name).await {
                append_host_stream_trace(&format!(
                    "SUNSHINE_CAPTURE_SERVICE_ONLY_REFRESH token={} stage=taskkill_warning pass={} image={} error={}",
                    session_token,
                    pass + 1,
                    sanitize_trace_value(image_name),
                    sanitize_trace_value(&err)
                ));
            }
        }

        if wait_for_host_tcp_closed(control_address, http_port, Duration::from_millis(900))
            .await
            .is_ok()
        {
            break;
        }

        sleep(Duration::from_millis(350)).await;
    }

    wait_for_host_tcp_closed(control_address, http_port, Duration::from_secs(10))
        .await
        .map_err(|err| format!("Sunshine capture service did not stop cleanly: {err}"))?;

    control_windows_service("start", "CloudgimeRuntime-Host").await?;
    wait_for_host_tcp_ready(control_address, http_port, Duration::from_secs(35))
        .await
        .map_err(|err| {
            format!(
                "Sunshine capture service did not become ready after service-only refresh: {err}"
            )
        })?;

    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_SERVICE_ONLY_REFRESH token={} stage=completed address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    Ok(())
}

#[cfg(windows)]
async fn restart_capture_runtime_for_output_change(
    session_token: &str,
    address: &str,
    http_port: u16,
) -> Result<(), String> {
    let control_address = legacy_runtime_control_address(address);
    let bundle_root =
        current_bundle_root().ok_or_else(|| "failed to determine bundle root".to_string())?;

    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_OUTPUT_REFRESH token={} stage=begin address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    for pass in 0..3 {
        for image_name in ["sunshinesvc.exe", "sunshine.exe"] {
            if let Err(err) = taskkill_image(image_name).await {
                append_host_stream_trace(&format!(
                    "SUNSHINE_CAPTURE_OUTPUT_REFRESH_STOP token={} pass={} image={} error={}",
                    session_token,
                    pass + 1,
                    sanitize_trace_value(image_name),
                    sanitize_trace_value(&err)
                ));
            }
        }

        if wait_for_host_tcp_closed(control_address, http_port, Duration::from_millis(900))
            .await
            .is_ok()
        {
            break;
        }

        sleep(Duration::from_millis(350)).await;
    }

    if let Ok(supervisor_path) = get_host_supervisor_path()
        && supervisor_path.exists()
    {
        append_host_stream_trace(&format!(
            "SUNSHINE_CAPTURE_OUTPUT_REFRESH token={} stage=managed_restart supervisor={}",
            session_token,
            sanitize_trace_value(&supervisor_path.display().to_string())
        ));

        let status = Command::new(&supervisor_path)
            .arg("--bundle-root")
            .arg(&*bundle_root.to_string_lossy())
            .arg("restart-runtime")
            .current_dir(&bundle_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|err| {
                format!(
                    "failed to restart capture runtime via host supervisor {}: {err}",
                    supervisor_path.display()
                )
            })?;

        if !status.success() {
            return Err(format!(
                "host supervisor returned non-zero status while refreshing capture runtime: {status}"
            ));
        }
    } else {
        let runtime_restart_script = bundle_root.join("start-host-runtime.bat");
        let legacy_start_script = bundle_root.join("start-sunshine.bat");

        let start_script = if runtime_restart_script.exists() {
            runtime_restart_script
        } else if legacy_start_script.exists() {
            legacy_start_script
        } else {
            return Err(format!(
                "failed to find start-host-runtime.bat or start-sunshine.bat under {}",
                bundle_root.display()
            ));
        };

        append_host_stream_trace(&format!(
            "SUNSHINE_CAPTURE_OUTPUT_REFRESH token={} stage=script_restart script={}",
            session_token,
            sanitize_trace_value(&start_script.display().to_string())
        ));

        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg(&start_script)
            .current_dir(&bundle_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_background_spawn_flags(&mut command);
        command.spawn().map_err(|err| {
            format!(
                "failed to start capture runtime after output change with {}: {err}",
                start_script.display()
            )
        })?;
    }

    wait_for_host_tcp_ready(control_address, http_port, Duration::from_secs(35))
        .await
        .map_err(|err| {
            format!("Sunshine did not become ready after capture output change: {err}")
        })?;

    append_host_stream_trace(&format!(
        "SUNSHINE_CAPTURE_OUTPUT_REFRESH token={} stage=completed address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    Ok(())
}

#[cfg(windows)]
async fn restart_sunshine_capture_with_fallback(
    session_token: &str,
    address: &str,
    http_port: u16,
) -> Result<&'static str, String> {
    match restart_sunshine_capture_service_only(session_token, address, http_port).await {
        Ok(()) => Ok("service_only"),
        Err(service_only_error) => {
            append_host_stream_trace(&format!(
                "SUNSHINE_CAPTURE_REFRESH_FALLBACK token={} stage=service_only_failed error={}",
                session_token,
                sanitize_trace_value(&service_only_error)
            ));

            restart_capture_runtime_for_output_change(session_token, address, http_port)
                .await
                .map(|_| "full_restart")
                .map_err(|full_restart_error| {
                    format!(
                        "service-only refresh failed: {}; full runtime restart failed: {}",
                        service_only_error, full_restart_error
                    )
                })
        }
    }
}

#[cfg(not(windows))]
async fn restart_bundle_sunshine(_address: &str, _http_port: u16) -> Result<(), String> {
    Err("sunshine restart is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn restart_bundle_sunshine_with_policy(
    _address: &str,
    _http_port: u16,
    _policy: Option<ReconnectRecoveryPolicy>,
) -> Result<(), String> {
    Err("sunshine restart is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn start_bundle_runtime_detached(_bundle_root: &Path) -> Result<(), String> {
    Err("bundle runtime start is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn refresh_bundle_capture_runtime_with_prepared_display(
    _session_token: &str,
    _requested_mode: Option<(u32, u32, u32)>,
    _address: &str,
    _http_port: u16,
) -> Result<DynamicDisplayResult, String> {
    Err("capture runtime refresh is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn restart_sunshine_capture_service_only(
    _session_token: &str,
    _address: &str,
    _http_port: u16,
) -> Result<(), String> {
    Err("Sunshine service-only refresh is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn restart_capture_runtime_for_output_change(
    _session_token: &str,
    _address: &str,
    _http_port: u16,
) -> Result<(), String> {
    Err("capture runtime output refresh is only supported on Windows hosts".to_string())
}

#[cfg(not(windows))]
async fn restart_sunshine_capture_with_fallback(
    _session_token: &str,
    _address: &str,
    _http_port: u16,
) -> Result<&'static str, String> {
    Err("Sunshine refresh fallback is only supported on Windows hosts".to_string())
}

async fn refresh_legacy_runtime_before_session(
    session_token: &str,
    address: &str,
    http_port: u16,
) -> Result<(), String> {
    if std::env::var("CLOUDGIME_FAST_CONNECT").unwrap_or_default() == "1" {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH_SKIPPED token={} reason=fast_connect",
            session_token
        ));
        return Ok(());
    }

    let control_address = legacy_runtime_control_address(address);
    cleanup_bundle_stream_processes(session_token).await;
    note_host_lifecycle_phase("recovering", "legacy_runtime_refresh");

    let hard_reset_mode = legacy_runtime_hard_reset_mode();

    let recent_connected_reconnect_reason =
        take_recent_connected_reconnect_reason(Duration::from_secs(8)).await;
    let recent_connected_reconnect_policy = recent_connected_reconnect_reason
        .as_deref()
        .and_then(ReconnectRecoveryPolicy::from_reason_text);
    let supervisor_handles_recovery_policy = get_host_supervisor_path()
        .ok()
        .is_some_and(|path| path.exists());

    if supervisor_handles_recovery_policy && let Some(policy) = recent_connected_reconnect_policy {
        note_host_lifecycle_phase("recovering", policy.lifecycle_reason());
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=delegate_recovery_policy_to_supervisor mode={:?} policy={} address={} port={}",
            session_token,
            hard_reset_mode,
            policy.policy_code(),
            sanitize_trace_value(control_address),
            http_port
        ));

        restart_bundle_sunshine_with_policy(control_address, http_port, Some(policy)).await?;
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=supervisor_recovery_policy_completed mode={:?} policy={} address={} port={}",
            session_token,
            hard_reset_mode,
            policy.policy_code(),
            sanitize_trace_value(control_address),
            http_port
        ));
        return Ok(());
    }

    if let Some(recent_reason) = recent_connected_reconnect_reason.as_deref() {
        if let Some(policy) = recent_connected_reconnect_policy {
            note_host_lifecycle_phase("recovering", policy.lifecycle_reason());

            if policy.allows_soft_reuse() {
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_REFRESH token={} stage=prefer_soft_reuse_after_internal_reconnect mode={:?} policy={} reason={} address={} port={}",
                    session_token,
                    hard_reset_mode,
                    policy.policy_code(),
                    sanitize_trace_value(recent_reason),
                    sanitize_trace_value(address),
                    http_port
                ));

                if wait_for_host_tcp_ready(control_address, http_port, Duration::from_millis(2500))
                    .await
                    .is_ok()
                {
                    sleep(Duration::from_millis(1200)).await;
                    append_host_stream_trace(&format!(
                        "LEGACY_RUNTIME_REFRESH token={} stage=reuse_recent_connected_runtime mode={:?} policy={} reason={} address={} port={}",
                        session_token,
                        hard_reset_mode,
                        policy.policy_code(),
                        sanitize_trace_value(recent_reason),
                        sanitize_trace_value(control_address),
                        http_port
                    ));
                    return Ok(());
                }

                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_REFRESH token={} stage=recent_connected_runtime_not_ready mode={:?} policy={} reason={} address={} port={}",
                    session_token,
                    hard_reset_mode,
                    policy.policy_code(),
                    sanitize_trace_value(recent_reason),
                    sanitize_trace_value(control_address),
                    http_port
                ));
            } else {
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_REFRESH token={} stage=policy_requires_hard_restart mode={:?} policy={} reason={} address={} port={}",
                    session_token,
                    hard_reset_mode,
                    policy.policy_code(),
                    sanitize_trace_value(recent_reason),
                    sanitize_trace_value(control_address),
                    http_port
                ));
            }
        } else {
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_REFRESH token={} stage=unknown_recent_reconnect_policy action=hard_restart mode={:?} reason={} address={} port={}",
                session_token,
                hard_reset_mode,
                sanitize_trace_value(recent_reason),
                sanitize_trace_value(control_address),
                http_port
            ));
            note_host_lifecycle_phase("recovering", "unknown_recent_reconnect");
        }
    }

    let failure_count = current_legacy_runtime_startup_failure_count().await;
    if hard_reset_mode != LegacyRuntimeHardResetMode::Always
        && failure_count == 0
        && recent_connected_reconnect_policy.is_none_or(|policy| policy.allows_soft_reuse())
        && wait_for_host_tcp_ready(control_address, http_port, Duration::from_millis(1200))
            .await
            .is_ok()
    {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=reuse_ready_runtime mode={:?} address={} port={}",
            session_token,
            hard_reset_mode,
            sanitize_trace_value(control_address),
            http_port
        ));
        return Ok(());
    }

    if hard_reset_mode == LegacyRuntimeHardResetMode::Always {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=force_hard_restart_always mode={:?} address={} port={}",
            session_token,
            hard_reset_mode,
            sanitize_trace_value(control_address),
            http_port
        ));
    } else if failure_count > 0 {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_REFRESH token={} stage=force_hard_restart_after_failures mode={:?} failure_count={} address={} port={}",
            session_token,
            hard_reset_mode,
            failure_count,
            sanitize_trace_value(control_address),
            http_port
        ));
    }

    append_host_stream_trace(&format!(
        "LEGACY_RUNTIME_REFRESH token={} stage=restart_before_session mode={:?} address={} port={}",
        session_token,
        hard_reset_mode,
        sanitize_trace_value(control_address),
        http_port
    ));
    note_host_lifecycle_phase("recovering", "legacy_runtime_restart_before_session");

    restart_bundle_sunshine(control_address, http_port).await?;
    wait_for_host_tcp_ready(control_address, http_port, Duration::from_secs(15)).await?;
    sleep(Duration::from_millis(1000)).await;

    append_host_stream_trace(&format!(
        "LEGACY_RUNTIME_REFRESH token={} stage=ready_after_restart address={} port={}",
        session_token,
        sanitize_trace_value(control_address),
        http_port
    ));

    Ok(())
}

fn maybe_schedule_legacy_runtime_auto_fallback(session_token: &str, reason: &str) {
    if force_legacy_nvenc_enabled() {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=skipped_force_legacy_nvenc reason={}",
            session_token,
            sanitize_trace_value(reason)
        ));
        return;
    }

    if !LEGACY_RUNTIME_AUTO_FALLBACK_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=skip_already_running reason={}",
            session_token,
            sanitize_trace_value(reason)
        ));
        return;
    }

    let session_token = session_token.to_string();
    let reason = reason.to_string();
    spawn(async move {
        append_host_stream_trace(&format!(
            "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=begin reason={}",
            session_token,
            sanitize_trace_value(&reason)
        ));

        let result = async {
            let metadata_changed = disable_legacy_runtime_auto_select(&reason)?;
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=demote_metadata changed={} reason={}",
                session_token,
                metadata_changed,
                sanitize_trace_value(&reason)
            ));

            refresh_bundle_host_preflight().await?;
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=preflight_refreshed",
                session_token
            ));

            restart_bundle_sunshine("127.0.0.1", 49000).await?;
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=sunshine_restarted",
                session_token
            ));

            Ok::<(), String>(())
        }
        .await;

        if let Err(err) = result {
            append_host_stream_trace(&format!(
                "LEGACY_RUNTIME_AUTO_FALLBACK token={} stage=failed error={}",
                session_token,
                sanitize_trace_value(&err)
            ));
            warn!("[Stream]: failed to auto-fallback legacy runtime: {err}");
        }

        LEGACY_RUNTIME_AUTO_FALLBACK_ACTIVE.store(false, Ordering::Release);
    });
}

fn maybe_schedule_host_failure_recovery(failure_reason: &str) {
    let Some((strategy, normalized_reason)) = failure_recovery_strategy_for_reason(failure_reason)
    else {
        return;
    };

    if strategy == FailureRecoveryStrategy::None {
        return;
    }

    if !HOST_FAILURE_RECOVERY_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        append_host_stream_trace(&format!(
            "HOST_FAILURE_RECOVERY stage=skip_already_running strategy={} reason={}",
            strategy.strategy_code(),
            sanitize_trace_value(normalized_reason)
        ));
        return;
    }

    let failure_reason = normalized_reason.to_string();
    spawn(async move {
        append_host_stream_trace(&format!(
            "HOST_FAILURE_RECOVERY stage=begin strategy={} reason={}",
            strategy.strategy_code(),
            sanitize_trace_value(&failure_reason)
        ));

        let result = async {
            let bundle_root = current_bundle_root()
                .ok_or_else(|| "failed to determine bundle root".to_string())?;
            let supervisor_path = get_host_supervisor_path()?;
            if !supervisor_path.exists() {
                return Err(format!(
                    "missing host supervisor at {}",
                    supervisor_path.display()
                ));
            }

            let status = Command::new(&supervisor_path)
                .arg("--bundle-root")
                .arg(&*bundle_root.to_string_lossy())
                .arg("recover-failure")
                .arg("--strategy")
                .arg(strategy.strategy_code())
                .arg("--reason")
                .arg(&failure_reason)
                .current_dir(&bundle_root)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|err| {
                    format!(
                        "failed to run host failure recovery via supervisor {}: {err}",
                        supervisor_path.display()
                    )
                })?;

            if !status.success() {
                return Err(format!(
                    "host supervisor returned non-zero status while recovering failure: {status}"
                ));
            }

            Ok::<(), String>(())
        }
        .await;

        match result {
            Ok(()) => append_host_stream_trace(&format!(
                "HOST_FAILURE_RECOVERY stage=completed strategy={} reason={}",
                strategy.strategy_code(),
                sanitize_trace_value(&failure_reason)
            )),
            Err(err) => append_host_stream_trace(&format!(
                "HOST_FAILURE_RECOVERY stage=failed strategy={} reason={} error={}",
                strategy.strategy_code(),
                sanitize_trace_value(&failure_reason),
                sanitize_trace_value(&err)
            )),
        }

        HOST_FAILURE_RECOVERY_ACTIVE.store(false, Ordering::Release);
    });
}

fn sanitize_trace_value(text: &str) -> String {
    let sanitized = text
        .chars()
        .map(|char| match char {
            '\r' | '\n' | '\t' => ' ',
            _ => char,
        })
        .collect::<String>();

    let sanitized = sanitized.trim();
    if sanitized.len() <= 160 {
        sanitized.to_string()
    } else {
        format!("{}...", &sanitized[..160])
    }
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

async fn note_legacy_runtime_startup_failure() -> u32 {
    const FAILURE_WINDOW_SECS: u64 = 180;
    let mut state = LEGACY_RUNTIME_STARTUP_FAILURE_STATE.lock().await;
    let now = unix_timestamp_seconds();
    if now.saturating_sub(state.1) > FAILURE_WINDOW_SECS {
        state.0 = 0;
    }
    state.0 = state.0.saturating_add(1);
    state.1 = now;
    state.0
}

async fn reset_legacy_runtime_startup_failure_state() {
    let mut state = LEGACY_RUNTIME_STARTUP_FAILURE_STATE.lock().await;
    state.0 = 0;
    state.1 = unix_timestamp_seconds();
}

async fn current_legacy_runtime_startup_failure_count() -> u32 {
    const FAILURE_WINDOW_SECS: u64 = 180;
    let mut state = LEGACY_RUNTIME_STARTUP_FAILURE_STATE.lock().await;
    let now = unix_timestamp_seconds();
    if now.saturating_sub(state.1) > FAILURE_WINDOW_SECS {
        state.0 = 0;
    }
    state.0
}

#[cfg(windows)]
fn boost_child_process_priority(pid: u32, label: &str, session_token: &str) {
    unsafe {
        let handle = OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if handle.is_null() {
            append_host_stream_trace(&format!(
                "PROCESS_PRIORITY_FAILED token={} label={} pid={} error=open_process_failed",
                session_token,
                sanitize_trace_value(label),
                pid
            ));
            return;
        }

        let ok = SetPriorityClass(handle, NORMAL_PRIORITY_CLASS);
        let _ = CloseHandle(handle);

        if ok == 0 {
            append_host_stream_trace(&format!(
                "PROCESS_PRIORITY_FAILED token={} label={} pid={} error=set_priority_failed",
                session_token,
                sanitize_trace_value(label),
                pid
            ));
            return;
        }

        append_host_stream_trace(&format!(
            "PROCESS_PRIORITY_NORMALIZED token={} label={} pid={} priority=Normal",
            session_token,
            sanitize_trace_value(label),
            pid
        ));
    }
}

fn describe_stream_client_message(message: &StreamClientMessage) -> String {
    match message {
        StreamClientMessage::Heartbeat { ts_ms } => {
            format!("type=Heartbeat ts_ms={ts_ms}")
        }
        StreamClientMessage::RouteTelemetry { route, detail } => format!(
            "type=RouteTelemetry route={} detail={}",
            sanitize_trace_value(route),
            sanitize_trace_value(detail)
        ),
        StreamClientMessage::ProjectDisplay { mode } => {
            format!("type=ProjectDisplay mode={}", sanitize_trace_value(mode))
        }
        StreamClientMessage::SetTransport(transport) => {
            format!("type=SetTransport transport={transport:?}")
        }
        StreamClientMessage::StartStream {
            bitrate,
            packet_size,
            fps,
            width,
            height,
            host_mouse_emulation,
            play_audio_local,
            hdr,
            ..
        } => format!(
            "type=StartStream size={}x{} fps={} bitrate={} packet_size={} host_mouse_emulation={host_mouse_emulation:?} play_audio_local={} hdr={}",
            width, height, fps, bitrate, packet_size, play_audio_local, hdr
        ),
        StreamClientMessage::ResizeStream { width, height, fps } => {
            format!("type=ResizeStream size={}x{} fps={}", width, height, fps)
        }
        StreamClientMessage::SetHostMouseEmulation {
            host_mouse_emulation,
        } => {
            format!("type=SetHostMouseEmulation host_mouse_emulation={host_mouse_emulation:?}")
        }
        StreamClientMessage::WebRtc(StreamSignalingMessage::Description(description)) => {
            format!("type=WebRtcDescription sdp={:?}", description.ty)
        }
        StreamClientMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(candidate)) => {
            format!(
                "type=WebRtcIceCandidate mid={} mline={:?}",
                sanitize_trace_value(candidate.sdp_mid.as_deref().unwrap_or("none")),
                candidate.sdp_mline_index
            )
        }
        _ => format!("type={message:?}"),
    }
}

fn describe_mic_sidecar_client_message(message: &MicSidecarClientMessage) -> String {
    match message {
        MicSidecarClientMessage::Init { host_id } => {
            format!("type=Init host_id={host_id}")
        }
        MicSidecarClientMessage::Heartbeat { ts_ms } => {
            format!("type=Heartbeat ts_ms={ts_ms}")
        }
        MicSidecarClientMessage::WebRtc(StreamSignalingMessage::Description(description)) => {
            format!("type=WebRtcDescription sdp={:?}", description.ty)
        }
        MicSidecarClientMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(candidate)) => {
            format!(
                "type=WebRtcIceCandidate mid={} mline={:?}",
                sanitize_trace_value(candidate.sdp_mid.as_deref().unwrap_or("none")),
                candidate.sdp_mline_index
            )
        }
        MicSidecarClientMessage::Stop => "type=Stop".to_owned(),
    }
}

fn describe_mic_sidecar_server_message(message: &MicSidecarServerMessage) -> String {
    match message {
        MicSidecarServerMessage::Setup { ice_servers } => {
            format!("type=Setup ice_servers={}", ice_servers.len())
        }
        MicSidecarServerMessage::DebugLog { message, ty } => format!(
            "type=DebugLog ty={:?} message={}",
            ty,
            sanitize_trace_value(message)
        ),
        MicSidecarServerMessage::WebRtc(StreamSignalingMessage::Description(description)) => {
            format!("type=WebRtcDescription sdp={:?}", description.ty)
        }
        MicSidecarServerMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(candidate)) => {
            format!(
                "type=WebRtcIceCandidate mid={} mline={:?}",
                sanitize_trace_value(candidate.sdp_mid.as_deref().unwrap_or("none")),
                candidate.sdp_mline_index
            )
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DynamicDisplayAppliedMode {
    width: u32,
    height: u32,
    frequency: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DynamicDisplayResult {
    ok: bool,
    changed: bool,
    restored: bool,
    skipped: bool,
    reason: String,
    applied: Option<DynamicDisplayAppliedMode>,
    sunshine_capture_changed: bool,
    sunshine_capture_target_changed: bool,
    sunshine_capture_display: Option<String>,
    sunshine_capture_config_path: Option<String>,
}

async fn force_kill_child(child: &Arc<Mutex<Child>>, label: &str) {
    let mut child = child.lock().await;
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            let mut killed = false;

            #[cfg(windows)]
            if let Some(pid) = child.id() {
                let mut command = Command::new("taskkill");
                command
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                match timeout(TASKKILL_TIMEOUT, command.status()).await {
                    Ok(Ok(status)) if status.success() => {
                        killed = true;
                        debug!("[{label}]: taskkill terminated child tree pid={pid}");
                    }
                    Ok(Ok(status)) => {
                        warn!(
                            "[{label}]: taskkill returned non-zero status for pid={pid}: {status}"
                        );
                    }
                    Ok(Err(err)) => {
                        warn!("[{label}]: failed to run taskkill for pid={pid}: {err}");
                    }
                    Err(_) => {
                        warn!("[{label}]: taskkill timed out for pid={pid}");
                    }
                }
            }

            if !killed && let Err(err) = child.kill().await {
                warn!("[{label}]: failed to kill child: {err}");
            }

            match timeout(CHILD_WAIT_AFTER_KILL_TIMEOUT, child.wait()).await {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    warn!("[{label}]: failed to wait for child after kill: {err}");
                }
                Err(_) => {
                    warn!("[{label}]: timed out waiting for child after kill");
                }
            }
        }
        Err(err) => {
            warn!("[{label}]: failed to inspect child status: {err}");
        }
    }
}

async fn replace_active_child(
    slot: &LazyLock<Mutex<Option<Arc<Mutex<Child>>>>>,
    child: Arc<Mutex<Child>>,
    label: &str,
) {
    let previous = {
        let mut slot = slot.lock().await;
        slot.replace(child)
    };

    if let Some(previous) = previous {
        force_kill_child(&previous, label).await;
    }
}

async fn take_active_child(
    slot: &LazyLock<Mutex<Option<Arc<Mutex<Child>>>>>,
) -> Option<Arc<Mutex<Child>>> {
    let mut slot = slot.lock().await;
    slot.take()
}

async fn stop_active_window_watch(session_token: &str, reason: &str) {
    if let Some(window_watch_child) = take_active_child(&ACTIVE_WINDOW_WATCH_CHILD).await {
        append_host_stream_trace(&format!(
            "WINDOW_WATCH_STOP token={} reason={}",
            session_token, reason
        ));
        force_kill_child(&window_watch_child, "Window Watch").await;
    }
}

async fn preempt_active_stream_runtime(session_token: &str) {
    if let Some(previous_stream) = take_active_child(&ACTIVE_STREAM_CHILD).await {
        append_host_stream_trace(&format!(
            "PREEMPT_ACTIVE_STREAM token={} reason=new_session_start",
            session_token
        ));
        force_kill_child(&previous_stream, "Stream").await;
    }

    if let Some(previous_window_watch) = take_active_child(&ACTIVE_WINDOW_WATCH_CHILD).await {
        append_host_stream_trace(&format!(
            "PREEMPT_ACTIVE_WINDOW_WATCH token={} reason=new_session_start",
            session_token
        ));
        force_kill_child(&previous_window_watch, "Window Watch").await;
    }
}

fn get_display_prepare_helper_path() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|err| {
        format!("failed to resolve current executable for display prepare helper: {err}")
    })?;
    let Some(runtime_dir) = current_exe.parent() else {
        return Err("failed to resolve runtime directory for display prepare helper".to_string());
    };

    Ok(runtime_dir
        .join("server")
        .join("display-prepare-helper.exe"))
}

fn get_host_supervisor_path() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|err| {
        format!("failed to resolve current executable for host supervisor: {err}")
    })?;
    let Some(runtime_dir) = current_exe.parent() else {
        return Err("failed to resolve runtime directory for host supervisor".to_string());
    };

    let internal_path = runtime_dir
        .join("system")
        .join("cloudgime-runtime-agent.exe");
    if internal_path.exists() {
        Ok(internal_path)
    } else {
        Ok(runtime_dir.join("host-supervisor.exe"))
    }
}

#[cfg(windows)]
pub(super) struct HelperCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[cfg(windows)]
fn helper_stdout_capture_dir() -> PathBuf {
    std::env::temp_dir().join("cloudgime-helper-captures")
}

#[cfg(windows)]
fn helper_should_prefer_active_user_session() -> bool {
    std::env::var("USERNAME")
        .map(|value| value.eq_ignore_ascii_case("SYSTEM"))
        .unwrap_or(false)
        || std::env::var("SESSIONNAME")
            .map(|value| value.eq_ignore_ascii_case("Services"))
            .unwrap_or(false)
}

#[cfg(windows)]
fn quote_batch_argument(value: &str) -> String {
    if value.is_empty()
        || value.chars().any(|candidate| {
            candidate.is_whitespace()
                || matches!(candidate, '"' | '&' | '|' | '<' | '>' | '^' | '(' | ')')
        })
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn enumerate_interactive_user_session_candidates() -> Vec<u32> {
    let console_session_id = unsafe { WTSGetActiveConsoleSessionId() };
    let mut sessions_ptr: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut session_count = 0u32;
    let mut ranked_sessions: Vec<(u8, u32)> = Vec::new();

    let enumerate_ok = unsafe {
        WTSEnumerateSessionsW(
            std::ptr::null_mut(),
            0,
            1,
            &mut sessions_ptr,
            &mut session_count,
        )
    } != 0;
    if enumerate_ok && !sessions_ptr.is_null() && session_count > 0 {
        let sessions = unsafe { std::slice::from_raw_parts(sessions_ptr, session_count as usize) };
        for session in sessions {
            let session_id = session.SessionId;
            let is_console = console_session_id != u32::MAX && session_id == console_session_id;
            let rank = match (session.State, is_console) {
                (WTSActive, false) => Some(0),
                (WTSConnected, false) => Some(1),
                (WTSActive, true) => Some(2),
                (WTSConnected, true) => Some(3),
                _ => None,
            };
            if let Some(rank) = rank {
                ranked_sessions.push((rank, session_id));
            }
        }
        unsafe {
            WTSFreeMemory(sessions_ptr.cast());
        }
    }

    ranked_sessions.sort_unstable_by_key(|(rank, session_id)| (*rank, *session_id));

    let mut session_ids = Vec::new();
    for (_, session_id) in ranked_sessions {
        if !session_ids.contains(&session_id) {
            session_ids.push(session_id);
        }
    }

    if session_ids.is_empty() && console_session_id != u32::MAX {
        session_ids.push(console_session_id);
    }

    session_ids
}

#[cfg(windows)]
fn run_helper_in_active_user_session_blocking(
    helper_path: PathBuf,
    helper_arguments: Vec<String>,
    timeout_duration: Duration,
) -> Result<HelperCommandOutput, String> {
    let session_candidates = enumerate_interactive_user_session_candidates();
    if session_candidates.is_empty() {
        return Err("no interactive user session is available for display helper".to_string());
    }

    let capture_dir = helper_stdout_capture_dir();
    fs::create_dir_all(&capture_dir)
        .map_err(|err| format!("failed to create helper capture dir: {err}"))?;
    let mut failures = Vec::new();

    for active_session_id in session_candidates {
        let mut user_token: HANDLE = std::ptr::null_mut();
        if unsafe { WTSQueryUserToken(active_session_id, &mut user_token) } == 0 {
            failures.push(format!(
                "WTSQueryUserToken(session={active_session_id}) failed with win32={}",
                unsafe { GetLastError() }
            ));
            continue;
        }

        let mut primary_token: HANDLE = std::ptr::null_mut();
        let duplicate_ok = unsafe {
            DuplicateTokenEx(
                user_token,
                TOKEN_ASSIGN_PRIMARY
                    | TOKEN_DUPLICATE
                    | TOKEN_QUERY
                    | TOKEN_ADJUST_DEFAULT
                    | TOKEN_ADJUST_SESSIONID,
                std::ptr::null(),
                SecurityImpersonation,
                TokenPrimary,
                &mut primary_token,
            )
        } != 0;
        unsafe {
            CloseHandle(user_token);
        }
        if !duplicate_ok {
            failures.push(format!(
                "DuplicateTokenEx(session={active_session_id}) failed with win32={}",
                unsafe { GetLastError() }
            ));
            continue;
        }

        let capture_id = Uuid::new_v4().to_string();
        let stdout_path = capture_dir.join(format!("{capture_id}.stdout"));
        let stderr_path = capture_dir.join(format!("{capture_id}.stderr"));
        let script_path = capture_dir.join(format!("{capture_id}.cmd"));

        let helper_command = std::iter::once(quote_batch_argument(&helper_path.to_string_lossy()))
            .chain(
                helper_arguments
                    .iter()
                    .map(|argument| quote_batch_argument(argument)),
            )
            .collect::<Vec<_>>()
            .join(" ");
        let script_contents = format!(
            "@echo off\r\n{} 1>{} 2>{}\r\nexit /b %errorlevel%\r\n",
            helper_command,
            quote_batch_argument(&stdout_path.to_string_lossy()),
            quote_batch_argument(&stderr_path.to_string_lossy())
        );
        fs::write(&script_path, script_contents)
            .map_err(|err| format!("failed to write helper relay script: {err}"))?;

        let cmd_path = std::env::var("ComSpec")
            .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string());
        let mut command_line = wide_null(&format!(
            "cmd.exe /d /c {}",
            quote_batch_argument(&script_path.to_string_lossy())
        ));
        let application_name = wide_null(&cmd_path);
        let current_directory = wide_null(
            helper_path
                .parent()
                .unwrap_or_else(|| Path::new("C:\\"))
                .to_string_lossy()
                .as_ref(),
        );
        let mut desktop_name = wide_null("winsta0\\default");
        let startup_info = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            lpDesktop: desktop_name.as_mut_ptr(),
            dwFlags: STARTF_USESHOWWINDOW,
            wShowWindow: SW_HIDE as u16,
            ..unsafe { std::mem::zeroed() }
        };
        let mut process_information = PROCESS_INFORMATION {
            ..unsafe { std::mem::zeroed() }
        };

        let created = unsafe {
            CreateProcessAsUserW(
                primary_token,
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS,
                std::ptr::null(),
                current_directory.as_ptr(),
                &startup_info,
                &mut process_information,
            )
        } != 0;

        unsafe {
            CloseHandle(primary_token);
        }

        if !created {
            let _ = fs::remove_file(&script_path);
            failures.push(format!(
                "CreateProcessAsUserW(session={active_session_id}) failed with win32={}",
                unsafe { GetLastError() }
            ));
            continue;
        }

        let wait_millis = timeout_duration.as_millis().min(u32::MAX as u128) as u32;
        let wait_result = unsafe { WaitForSingleObject(process_information.hProcess, wait_millis) };
        if wait_result == WAIT_TIMEOUT {
            unsafe {
                let _ = TerminateProcess(process_information.hProcess, 1);
                CloseHandle(process_information.hThread);
                CloseHandle(process_information.hProcess);
            }
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            let _ = fs::remove_file(&script_path);
            failures.push(format!(
                "display prepare helper timed out after {}s in interactive session {}",
                timeout_duration.as_secs(),
                active_session_id
            ));
            continue;
        }
        if wait_result != WAIT_OBJECT_0 {
            unsafe {
                CloseHandle(process_information.hThread);
                CloseHandle(process_information.hProcess);
            }
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            let _ = fs::remove_file(&script_path);
            failures.push(format!(
                "WaitForSingleObject(session={active_session_id}) failed with status={wait_result}"
            ));
            continue;
        }

        let mut exit_code = 1u32;
        if unsafe { GetExitCodeProcess(process_information.hProcess, &mut exit_code) } == 0 {
            exit_code = 1;
        }

        unsafe {
            CloseHandle(process_information.hThread);
            CloseHandle(process_information.hProcess);
        }

        let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        let _ = fs::remove_file(&script_path);

        return Ok(HelperCommandOutput {
            stdout: stdout.trim().to_string(),
            stderr: stderr.trim().to_string(),
            exit_code: exit_code as i32,
        });
    }

    Err(failures.join(" | "))
}

#[cfg(windows)]
pub(super) async fn run_display_helper_command(
    helper_path: &Path,
    helper_arguments: &[String],
    timeout_duration: Duration,
    helper_action: Option<&str>,
) -> Result<HelperCommandOutput, String> {
    if helper_should_prefer_active_user_session() {
        let helper_path = helper_path.to_path_buf();
        let helper_arguments = helper_arguments.to_vec();
        match spawn_blocking(move || {
            run_helper_in_active_user_session_blocking(
                helper_path,
                helper_arguments,
                timeout_duration,
            )
        })
        .await
        {
            Ok(Ok(output)) => return Ok(output),
            Ok(Err(error)) => {
                warn!(
                    "interactive display helper relay failed; falling back to service session: {error}"
                );
            }
            Err(error) => {
                warn!(
                    "interactive display helper relay task failed; falling back to service session: {error}"
                );
            }
        }
    }

    let mut command = Command::new(helper_path);
    command
        .args(helper_arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.kill_on_drop(true);
    apply_background_spawn_flags(&mut command);
    let output = match timeout(timeout_duration, command.output()).await {
        Ok(result) => {
            result.map_err(|err| format!("failed to run display prepare helper: {err}"))?
        }
        Err(_) => {
            if let Some(helper_action) = helper_action {
                append_host_stream_trace(&format!(
                    "DISPLAY_HELPER_TIMEOUT action={} timeout_secs={}",
                    sanitize_trace_value(helper_action),
                    timeout_duration.as_secs()
                ));
            }
            return Err(format!(
                "display prepare helper timed out after {}s while running {}",
                timeout_duration.as_secs(),
                helper_action.unwrap_or("command")
            ));
        }
    };

    Ok(HelperCommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code().unwrap_or(1),
    })
}

#[cfg(windows)]
async fn spawn_window_primary_watch(session_token: &str) -> Result<Arc<Mutex<Child>>, String> {
    let helper_path = get_display_prepare_helper_path()?;
    if !helper_path.exists() {
        return Err(format!(
            "window watch helper was not found at {}",
            helper_path.display()
        ));
    }

    let mut command = Command::new(&helper_path);
    command
        .arg("watch-window-primary")
        .args(["--session-token", session_token])
        .stdin(Stdio::null());
    apply_background_spawn_flags(&mut command);
    let child = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to spawn window watch helper: {err}"))?;

    Ok(Arc::new(Mutex::new(child)))
}

#[cfg(windows)]
async fn run_display_prepare_helper(arguments: &[String]) -> Result<DynamicDisplayResult, String> {
    let helper_path = get_display_prepare_helper_path()?;
    if !helper_path.exists() {
        return Err(format!(
            "display prepare helper was not found at {}",
            helper_path.display()
        ));
    }

    let mut helper_arguments = arguments.to_vec();
    if !helper_arguments
        .iter()
        .any(|argument| argument == "--bundle-root")
        && let Some(bundle_root) = current_bundle_root()
    {
        helper_arguments.push("--bundle-root".to_string());
        helper_arguments.push(bundle_root.to_string_lossy().to_string());
    }

    let helper_action = helper_arguments
        .first()
        .map(String::as_str)
        .unwrap_or("unknown")
        .to_string();
    let output = run_display_helper_command(
        &helper_path,
        &helper_arguments,
        DISPLAY_PREPARE_HELPER_TIMEOUT,
        Some(&helper_action),
    )
    .await?;

    let stdout = output.stdout;
    let stderr = output.stderr;

    if !stdout.is_empty()
        && let Ok(result) = serde_json::from_str::<DynamicDisplayResult>(&stdout)
    {
        if result.ok {
            return Ok(result);
        }

        return Err(if !result.reason.trim().is_empty() {
            result.reason.clone()
        } else if !stderr.is_empty() {
            stderr
        } else {
            format!(
                "display prepare helper exited with status {}",
                output.exit_code
            )
        });
    }

    if output.exit_code != 0 {
        return Err(if stderr.is_empty() {
            format!(
                "display prepare helper exited with status {}",
                output.exit_code
            )
        } else {
            format!(
                "display prepare helper exited with status {}: {stderr}",
                output.exit_code
            )
        });
    }

    let result = serde_json::from_str::<DynamicDisplayResult>(&stdout).map_err(|err| {
        format!(
            "failed to parse display prepare helper output: {err}; stdout={stdout}; stderr={stderr}"
        )
    })?;

    if !result.ok {
        return Err(if result.reason.is_empty() {
            "display prepare helper reported failure".to_string()
        } else {
            result.reason.clone()
        });
    }

    Ok(result)
}

#[cfg(not(windows))]
async fn run_display_prepare_helper(_arguments: &[String]) -> Result<DynamicDisplayResult, String> {
    Err("display prepare helper is only supported on Windows hosts".to_string())
}

async fn apply_dynamic_display_match(
    session_token: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<DynamicDisplayResult, String> {
    run_display_prepare_helper(&[
        "resize".to_string(),
        "--session-token".to_string(),
        session_token.to_string(),
        "--width".to_string(),
        width.to_string(),
        "--height".to_string(),
        height.to_string(),
        "--fps".to_string(),
        fps.to_string(),
    ])
    .await
}

async fn apply_project_display_mode(
    session_token: &str,
    mode: &str,
) -> Result<DynamicDisplayResult, String> {
    run_display_prepare_helper(&[
        "project-display".to_string(),
        "--session-token".to_string(),
        session_token.to_string(),
        "--mode".to_string(),
        mode.to_string(),
    ])
    .await
}

pub(super) async fn remember_prewarmed_display_session(session_token: &str) {
    PREWARMED_DISPLAY_SESSIONS
        .lock()
        .await
        .insert(session_token.to_string());
}

async fn prepare_stream_display(
    session_token: &str,
    requested_mode: Option<(u32, u32, u32)>,
) -> Result<DynamicDisplayResult, String> {
    let reusing_prewarmed_session = PREWARMED_DISPLAY_SESSIONS
        .lock()
        .await
        .remove(session_token);
    if reusing_prewarmed_session {
        append_host_stream_trace(&format!(
            "DISPLAY_PREPARE_REUSE token={} requested_mode={}",
            session_token,
            sanitize_trace_value(
                &requested_mode
                    .map(|(width, height, fps)| format!("{width}x{height}@{fps}"))
                    .unwrap_or_else(|| "none".to_string()),
            )
        ));
        if let Some((width, height, fps)) = requested_mode {
            match apply_dynamic_display_match(session_token, width, height, fps).await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "DISPLAY_PREPARE_REUSE_RESIZE_FAILED token={} error={}",
                        session_token,
                        sanitize_trace_value(&err)
                    ));
                }
            }
        } else {
            return Ok(DynamicDisplayResult {
                ok: true,
                skipped: true,
                reason: "reused prewarmed display authority".to_string(),
                ..Default::default()
            });
        }
    }

    let mut arguments = vec![
        "prepare".to_string(),
        "--session-token".to_string(),
        session_token.to_string(),
    ];

    if let Some((width, height, fps)) = requested_mode {
        arguments.extend([
            "--width".to_string(),
            width.to_string(),
            "--height".to_string(),
            height.to_string(),
            "--fps".to_string(),
            fps.to_string(),
        ]);
    }

    run_display_prepare_helper(&arguments).await
}

async fn restore_prepared_stream_display(
    session_token: &str,
) -> Result<DynamicDisplayResult, String> {
    run_display_prepare_helper(&[
        "restore".to_string(),
        "--session-token".to_string(),
        session_token.to_string(),
    ])
    .await
}

fn should_skip_prepared_stream_restore_after_session_cleanup() -> bool {
    // Cloudgime invariant: stream sessions always own the dedicated MTT VDD
    // lane. A slow restore after close must not block the next session.
    true
}

async fn restore_runtime_dynamic_display_if_active(active: &Arc<AtomicBool>, _session_token: &str) {
    let _ = active.swap(false, Ordering::AcqRel);
}

async fn restore_prepared_stream_display_if_active(active: &Arc<AtomicBool>, session_token: &str) {
    if !active.swap(false, Ordering::AcqRel) {
        return;
    }

    match restore_prepared_stream_display(session_token).await {
        Ok(result) => {
            if result.restored {
                info!("[Stream]: restored prepared display layout");
            } else if result.skipped {
                info!(
                    "[Stream]: prepared display restore skipped ({})",
                    if result.reason.is_empty() {
                        "no saved prepare state"
                    } else {
                        &result.reason
                    }
                );
            }
        }
        Err(err) => {
            warn!("[Stream]: failed to restore prepared display layout: {err}");
        }
    }
}

async fn restore_prepared_stream_display_best_effort(session_token: &str, stage: &str) {
    match restore_prepared_stream_display(session_token).await {
        Ok(result) => {
            append_host_stream_trace(&format!(
                "DISPLAY_RESTORE_BEST_EFFORT token={} stage={} restored={} skipped={} reason={}",
                session_token,
                sanitize_trace_value(stage),
                result.restored,
                result.skipped,
                sanitize_trace_value(&result.reason)
            ));
        }
        Err(err) => {
            warn!("[Stream]: best-effort display restore failed: {err}");
            append_host_stream_trace(&format!(
                "DISPLAY_RESTORE_BEST_EFFORT_FAILED token={} stage={} error={}",
                session_token,
                sanitize_trace_value(stage),
                sanitize_trace_value(&err)
            ));
        }
    }
}

async fn forward_stream_server_message(
    session: &mut Session,
    ipc_sender: &mut common::ipc::IpcSender<ServerIpcMessage>,
    message: StreamServerMessage,
    dynamic_display_session_token: &str,
    legacy_runtime_selected_for_session: bool,
    stream_connection_established: &AtomicBool,
    video_flow_ready_observed: &AtomicBool,
    start_stream_received: &AtomicBool,
    warned_closed: &mut bool,
) {
    match &message {
        StreamServerMessage::ConnectionComplete {
            width, height, fps, ..
        } => {
            append_host_stream_trace(&format!(
                "STREAM_CONNECTION_COMPLETE token={} negotiated={}x{}@{}",
                dynamic_display_session_token, width, height, fps
            ));
            append_host_stream_trace(&format!(
                "GOLDEN_PATH_CONNECTED token={} negotiated={}x{}@{}",
                dynamic_display_session_token, width, height, fps
            ));
        }
        StreamServerMessage::ConnectionTerminated { error_code } => {
            let failure_reason = format!("connection_terminated:{error_code}");
            append_host_stream_trace(&format!(
                "STREAM_CONNECTION_TERMINATED token={} error_code={}",
                dynamic_display_session_token, error_code
            ));
            note_host_lifecycle_phase("failed", &failure_reason);
            maybe_schedule_host_failure_recovery(&failure_reason);
            if legacy_runtime_selected_for_session && *error_code == -100 {
                let failure_count = note_legacy_runtime_startup_failure().await;
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_STARTUP_FAILURE token={} stage=connection_terminated failure_count={} reason=legacy_no_video_traffic video_flow_ready={}",
                    dynamic_display_session_token,
                    failure_count,
                    video_flow_ready_observed.load(Ordering::Acquire)
                ));
                if failure_count >= 2 {
                    maybe_schedule_legacy_runtime_auto_fallback(
                        dynamic_display_session_token,
                        "legacy_no_video_traffic",
                    );
                }
            }
        }
        StreamServerMessage::DebugLog { message, ty } => {
            let ty_text = ty
                .as_ref()
                .map(|ty| format!("{ty:?}"))
                .unwrap_or_else(|| "None".to_string());
            let should_trace_startup_debug = message.starts_with("Preparing paired host session")
                || message == "Applying pairing identity"
                || message == "Pairing identity ready"
                || message == "Preparing stream connection"
                || message == "Transport negotiation ready"
                || message.starts_with("Timed out while setting pairing info")
                || message.starts_with("Timed out while preparing stream connection")
                || message.starts_with("Failed to set pairing info")
                || message.starts_with("Failed to create connection");
            if message.starts_with("MOUSE_") || should_trace_startup_debug {
                append_host_stream_trace(&format!(
                    "STREAM_DEBUG token={} ty={} message={}",
                    dynamic_display_session_token,
                    sanitize_trace_value(&ty_text),
                    sanitize_trace_value(message)
                ));
            } else if matches!(
                ty,
                Some(
                    LogMessageType::Fatal
                        | LogMessageType::FatalDescription
                        | LogMessageType::IfErrorDescription
                        | LogMessageType::InformError
                )
            ) {
                append_host_stream_trace(&format!(
                    "STREAM_DEBUG token={} ty={} message={}",
                    dynamic_display_session_token,
                    sanitize_trace_value(&ty_text),
                    sanitize_trace_value(message)
                ));
            }
        }
        StreamServerMessage::DisplayModeApplied {
            phase,
            width,
            height,
            fps,
            changed,
            skipped,
        } => {
            append_host_stream_trace(&format!(
                "DISPLAY_MODE_APPLIED token={} phase={phase:?} applied={}x{}@{} changed={} skipped={}",
                dynamic_display_session_token, width, height, fps, changed, skipped
            ));
        }
        StreamServerMessage::VideoFlowReady {
            phase,
            width,
            height,
            fps,
        } => {
            append_host_stream_trace(&format!(
                "VIDEO_FLOW_READY token={} phase={phase:?} {}x{}@{}",
                dynamic_display_session_token, width, height, fps
            ));
            append_host_stream_trace(&format!(
                "GOLDEN_PATH_VIDEO_READY token={} phase={phase:?} {}x{}@{}",
                dynamic_display_session_token, width, height, fps
            ));
            video_flow_ready_observed.store(true, Ordering::Release);
        }
        _ => {}
    }

    if matches!(message, StreamServerMessage::ConnectionComplete { .. }) {
        stream_connection_established.store(true, Ordering::Release);
        note_host_lifecycle_phase("ready", "connection_complete");
        if legacy_runtime_selected_for_session {
            reset_legacy_runtime_startup_failure_state().await;
        }
        append_host_stream_trace(&format!(
            "CONNECTION_COMPLETE token={}",
            dynamic_display_session_token
        ));
    }

    if let Err(Closed) = send_ws_message(session, message).await
        && !*warned_closed
    {
        warn!("[Ipc]: Tried to send a ws message (text) but the socket is already closed");
        if !stream_connection_established.load(Ordering::Acquire) {
            warn!(
                "[Stream]: websocket closed before connection was stable (start_stream_received={})",
                start_stream_received.load(Ordering::Acquire)
            );
            ipc_sender.send(ServerIpcMessage::Stop).await;
        }
        *warned_closed = true;
    }
}

#[get("/host/stream")]
#[instrument(name = "start_host", skip(web_app, user, payload), fields(user_id = %user.id()))]
pub async fn start_host(
    web_app: Data<App>,
    mut user: AuthenticatedUser,
    request: HttpRequest,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut stream) = actix_ws::handle(&request, payload)?;
    let ws_request_path = request.uri().to_string();
    let ws_stream_ticket_present = extract_android_native_stream_ticket(&request).is_some();

    let client_unique_id = user.host_unique_id().await?;

    let web_app = web_app.clone();
    actix_rt::spawn(async move {
        // -- Init and Configure
        let message;
        loop {
            let raw_message = match stream.recv().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Binary(_))) => {
                    append_host_stream_trace(&format!(
                        "WS_INIT_BINARY path={} stream_ticket_present={}",
                        sanitize_trace_value(&ws_request_path),
                        ws_stream_ticket_present
                    ));
                    return;
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) => {
                    append_host_stream_trace(&format!(
                        "WS_INIT_READ_FAILED path={} stream_ticket_present={}",
                        sanitize_trace_value(&ws_request_path),
                        ws_stream_ticket_present
                    ));
                    return;
                }
                None => {
                    append_host_stream_trace(&format!(
                        "WS_INIT_MISSING path={} stream_ticket_present={}",
                        sanitize_trace_value(&ws_request_path),
                        ws_stream_ticket_present
                    ));
                    return;
                }
            };

            append_host_stream_trace(&format!(
                "WS_INIT_RAW path={} stream_ticket_present={} bytes={} preview={}",
                sanitize_trace_value(&ws_request_path),
                ws_stream_ticket_present,
                raw_message.len(),
                sanitize_trace_preview(&raw_message, 220)
            ));

            let parsed_message = match serde_json::from_str::<StreamClientMessage>(&raw_message) {
                Ok(value) => value,
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "WS_INIT_PARSE_FAILED path={} stream_ticket_present={} error={}",
                        sanitize_trace_value(&ws_request_path),
                        ws_stream_ticket_present,
                        sanitize_trace_value(&err.to_string())
                    ));
                    return;
                }
            };

            if matches!(parsed_message, StreamClientMessage::Heartbeat { .. }) {
                append_host_stream_trace(&format!(
                    "WS_INIT_DEFER_HEARTBEAT path={} stream_ticket_present={}",
                    sanitize_trace_value(&ws_request_path),
                    ws_stream_ticket_present
                ));
                continue;
            }

            message = parsed_message;
            break;
        }

        let StreamClientMessage::Init {
            host_id,
            app_id,
            video_frame_queue_size,
            audio_sample_queue_size,
            client_build,
            dynamic_display_match,
        } = message
        else {
            let _ = session.close(None).await;
            append_host_stream_trace(&format!(
                "WS_INIT_WRONG_MESSAGE path={} stream_ticket_present={}",
                sanitize_trace_value(&ws_request_path),
                ws_stream_ticket_present
            ));

            warn!("WebSocket didn't send init as first message, closing it");
            return;
        };

        let host_id = HostId(host_id);
        let app_id = AppId(app_id);
        let android_native_stream_ticket_record = if let Some(stream_ticket) =
            extract_android_native_stream_ticket(&request)
        {
            match web_app
                .authorize_android_native_stream_ticket_for_stream(&stream_ticket, host_id, app_id)
                .await
            {
                Ok(record) => Some(record),
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "ANDROID_NATIVE_STREAM_TICKET_REJECTED host_id={} app_id={} reason={}",
                        host_id.0,
                        app_id.0,
                        sanitize_trace_value(&err.to_string())
                    ));
                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::DebugLog {
                            message: "Failed to authorize Android native stream ticket".to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    return;
                }
            }
        } else {
            None
        };
        let dynamic_display_session_token = android_native_stream_ticket_record
            .as_ref()
            .map(|record| record.session_id.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let android_native_owner_session_id = android_native_stream_ticket_record
            .as_ref()
            .map(|record| record.session_id.clone());
        if let Some(record) = android_native_stream_ticket_record.as_ref() {
            append_host_stream_trace(&format!(
                "ANDROID_NATIVE_SESSION_BOUND host_id={} app_id={} token={} session_id={} token_id={}",
                host_id.0,
                app_id.0,
                dynamic_display_session_token,
                sanitize_trace_value(&record.session_id),
                sanitize_trace_value(&record.token_id)
            ));
        }
        let mut runtime_profile_snapshot = read_host_capability_profile_snapshot();
        let mut runtime_startup_issue_at_init =
            blocking_runtime_startup_issue(runtime_profile_snapshot.as_ref());
        let mut legacy_runtime_selected_for_session =
            legacy_runtime_selected_in_snapshot(runtime_profile_snapshot.as_ref());
        let mut software_runtime_selected_for_session =
            software_runtime_selected(runtime_profile_snapshot.as_ref());
        let mut preserve_legacy_vdd_surface_for_session = dynamic_display_match
            && runtime_profile_snapshot
                .as_ref()
                .is_some_and(capability_profile_has_virtual_display_driver);
        append_host_stream_trace(&format!(
            "INIT host_id={} app_id={} token={} client_build={} dynamic_display_match={}",
            host_id.0, app_id.0, dynamic_display_session_token, client_build, dynamic_display_match
        ));

        preempt_active_stream_runtime(&dynamic_display_session_token).await;
        if let Some(profile) = runtime_profile_snapshot.as_ref() {
            append_host_stream_trace(&format!(
                "RUNTIME_PROFILE token={} runtime={} encoder={}",
                dynamic_display_session_token,
                sanitize_trace_value(&profile.selected_runtime_key),
                sanitize_trace_value(&profile.selected_encoder)
            ));
        }

        if let Some(runtime_issue) = runtime_startup_issue_at_init.as_deref() {
            append_host_stream_trace(&format!(
                "RUNTIME_PROFILE_ISSUE_INIT token={} detail={}",
                dynamic_display_session_token,
                sanitize_trace_value(runtime_issue)
            ));
        }

        if should_retry_runtime_preflight_before_session(
            runtime_profile_snapshot.as_ref(),
            runtime_startup_issue_at_init.as_deref(),
        ) {
            append_host_stream_trace(&format!(
                "RUNTIME_PROFILE_RECOVERY token={} stage=preflight_refresh_before_session",
                dynamic_display_session_token
            ));

            match refresh_bundle_host_preflight().await {
                Ok(()) => {
                    runtime_profile_snapshot = read_host_capability_profile_snapshot();
                    runtime_startup_issue_at_init =
                        blocking_runtime_startup_issue(runtime_profile_snapshot.as_ref());
                    legacy_runtime_selected_for_session =
                        legacy_runtime_selected_in_snapshot(runtime_profile_snapshot.as_ref());
                    software_runtime_selected_for_session =
                        software_runtime_selected(runtime_profile_snapshot.as_ref());
                    preserve_legacy_vdd_surface_for_session = dynamic_display_match
                        && runtime_profile_snapshot
                            .as_ref()
                            .is_some_and(capability_profile_has_virtual_display_driver);

                    let runtime_key = runtime_profile_snapshot
                        .as_ref()
                        .map(|profile| sanitize_trace_value(&profile.selected_runtime_key))
                        .unwrap_or_else(|| "(missing)".to_string());
                    let encoder_key = runtime_profile_snapshot
                        .as_ref()
                        .map(|profile| sanitize_trace_value(&profile.selected_encoder))
                        .unwrap_or_else(|| "(missing)".to_string());
                    let runtime_issue = runtime_startup_issue_at_init
                        .as_deref()
                        .map(sanitize_trace_value)
                        .unwrap_or_else(|| "(none)".to_string());

                    append_host_stream_trace(&format!(
                        "RUNTIME_PROFILE_RECOVERY token={} stage=preflight_refresh_complete runtime={} encoder={} issue={}",
                        dynamic_display_session_token, runtime_key, encoder_key, runtime_issue
                    ));
                }
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "RUNTIME_PROFILE_RECOVERY token={} stage=preflight_refresh_failed error={}",
                        dynamic_display_session_token,
                        sanitize_trace_value(&err)
                    ));
                    warn!("[Stream]: failed to refresh host preflight before session: {err}");
                }
            }
        }

        if client_build.trim().is_empty() || client_build == "unknown" {
            let _ = send_ws_message(
                &mut session,
                StreamServerMessage::DebugLog {
                    message: "Client build is outdated. Reload the stream page and try again."
                        .to_string(),
                    ty: Some(LogMessageType::FatalDescription),
                },
            )
            .await;
            let _ = session.close(None).await;
            append_host_stream_trace(&format!(
                "REJECT_OUTDATED_CLIENT token={} host_id={} app_id={}",
                dynamic_display_session_token, host_id.0, app_id.0
            ));
            return;
        }

        // -- Collect host data
        let mut host = match user.host(host_id).await {
            Ok(host) => host,
            Err(AppError::HostNotFound) => {
                append_host_stream_trace(&format!(
                    "HOST_LOOKUP_FAILED token={} host_id={} reason=host_not_found",
                    dynamic_display_session_token, host_id.0
                ));
                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because the host was not found"
                            .to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
            Err(err) => {
                warn!("failed to start stream for host {host_id:?} (at host): {err}");
                append_host_stream_trace(&format!(
                    "HOST_LOOKUP_FAILED token={} host_id={} reason={}",
                    dynamic_display_session_token,
                    host_id.0,
                    sanitize_trace_value(&err.to_string())
                ));

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because of a server error".to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        let detailed_host = match host.detailed_host(&mut user).await {
            Ok(detailed) => Some(detailed),
            Err(err) => {
                warn!("failed to resolve detailed host for {host_id:?}: {err}");
                append_host_stream_trace(&format!(
                    "HOST_DETAILED_RESOLVE_FAILED token={} host_id={} reason={}",
                    dynamic_display_session_token,
                    host_id.0,
                    sanitize_trace_value(&err.to_string())
                ));
                None
            }
        };

        let (address, http_port) = if let Some(detailed_host) = detailed_host.as_ref() {
            (
                resolve_streamer_host_address(web_app.get_ref(), detailed_host),
                detailed_host.http_port,
            )
        } else {
            match host.address_port(&mut user).await {
                Ok(address_port) => address_port,
                Err(err) => {
                    warn!(
                        "failed to start stream for host {host_id:?} (at get address_port): {err}"
                    );
                    append_host_stream_trace(&format!(
                        "HOST_ADDRESS_FAILED token={} host_id={} reason={}",
                        dynamic_display_session_token,
                        host_id.0,
                        sanitize_trace_value(&err.to_string())
                    ));

                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::DebugLog {
                            message: "Failed to start stream because of a server error".to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    return;
                }
            }
        };
        append_host_stream_trace(&format!(
            "HOST_STREAM_ADDRESS token={} host_id={} address={} http_port={}",
            dynamic_display_session_token,
            host_id.0,
            sanitize_trace_value(&address),
            http_port
        ));

        let recovery_gate_reason = recent_host_recovery_gate_reason(Duration::from_secs(180), true);
        if let Some(recovery_gate_reason) = recovery_gate_reason.as_deref() {
            append_host_stream_trace(&format!(
                "HOST_STARTUP_RECOVERY token={} stage=wait_existing reason={}",
                dynamic_display_session_token,
                sanitize_trace_value(recovery_gate_reason)
            ));
            let _ = send_ws_message(
                &mut session,
                StreamServerMessage::DebugLog {
                    message: "Host masih menyelesaikan recovery sebelumnya. Menunggu runtime capture stabil.".to_string(),
                    ty: Some(LogMessageType::Recover),
                },
            )
            .await;

            if let Err(err) =
                wait_for_host_runtime_ready(&address, http_port, Duration::from_secs(70)).await
            {
                append_host_stream_trace(&format!(
                    "HOST_STARTUP_RECOVERY token={} stage=wait_existing_failed reason={} error={}",
                    dynamic_display_session_token,
                    sanitize_trace_value(recovery_gate_reason),
                    sanitize_trace_value(&err)
                ));
                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Host belum selesai recovery runtime capture. Coba buka stream lagi beberapa detik lagi.".to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        }

        let startup_recovery = runtime_startup_issue_at_init
            .as_deref()
            .map(|runtime_issue| {
                (
                    format!("runtime_issue_before_session:{runtime_issue}"),
                    FailureRecoveryStrategy::RestartRuntime,
                )
            })
            .or_else(|| {
                pending_capture_prestart_refresh_reason(CAPTURE_INIT_PRESTART_REFRESH_WINDOW)
                    .map(|reason| (reason, FailureRecoveryStrategy::RestartRuntime))
            });

        if let Some((startup_recovery_reason, startup_recovery_strategy)) = startup_recovery {
            append_host_stream_trace(&format!(
                "HOST_STARTUP_RECOVERY token={} strategy={} reason={}",
                dynamic_display_session_token,
                startup_recovery_strategy.strategy_code(),
                sanitize_trace_value(&startup_recovery_reason)
            ));
            let _ = send_ws_message(
                &mut session,
                StreamServerMessage::DebugLog {
                    message: "Host sedang recovery. Menyiapkan ulang runtime capture sebelum stream dimulai.".to_string(),
                    ty: Some(LogMessageType::Recover),
                },
            )
            .await;

            match run_blocking_host_failure_recovery(
                &dynamic_display_session_token,
                startup_recovery_strategy,
                &startup_recovery_reason,
                &address,
                http_port,
            )
            .await
            {
                Ok(()) => {
                    runtime_profile_snapshot = read_host_capability_profile_snapshot();
                    runtime_startup_issue_at_init =
                        blocking_runtime_startup_issue(runtime_profile_snapshot.as_ref());
                    legacy_runtime_selected_for_session =
                        legacy_runtime_selected_in_snapshot(runtime_profile_snapshot.as_ref());
                    software_runtime_selected_for_session =
                        software_runtime_selected(runtime_profile_snapshot.as_ref());
                    preserve_legacy_vdd_surface_for_session = dynamic_display_match
                        && runtime_profile_snapshot
                            .as_ref()
                            .is_some_and(capability_profile_has_virtual_display_driver);
                    RECENT_CAPTURE_INIT_FAILURE_AT_MS.store(0, Ordering::Release);
                    mark_capture_prestart_refresh_consumed(&format!(
                        "blocking_recovery:{}",
                        sanitize_trace_value(&startup_recovery_reason)
                    ));
                    append_host_stream_trace(&format!(
                        "HOST_STARTUP_RECOVERY token={} stage=completed runtime={} encoder={} issue={}",
                        dynamic_display_session_token,
                        runtime_profile_snapshot
                            .as_ref()
                            .map(|profile| sanitize_trace_value(&profile.selected_runtime_key))
                            .unwrap_or_else(|| "(missing)".to_string()),
                        runtime_profile_snapshot
                            .as_ref()
                            .map(|profile| sanitize_trace_value(&profile.selected_encoder))
                            .unwrap_or_else(|| "(missing)".to_string()),
                        runtime_startup_issue_at_init
                            .as_deref()
                            .map(sanitize_trace_value)
                            .unwrap_or_else(|| "(none)".to_string())
                    ));
                }
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "HOST_STARTUP_RECOVERY token={} stage=failed strategy={} reason={} error={}",
                        dynamic_display_session_token,
                        startup_recovery_strategy.strategy_code(),
                        sanitize_trace_value(&startup_recovery_reason),
                        sanitize_trace_value(&err)
                    ));
                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::DebugLog {
                            message: "Host belum sehat setelah recovery runtime capture. Stream dibatalkan agar tidak macet.".to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    return;
                }
            }
        }

        if let Some(runtime_issue) = runtime_startup_issue_at_init.as_deref() {
            append_host_stream_trace(&format!(
                "RUNTIME_PROFILE_ISSUE_BLOCKING token={} detail={}",
                dynamic_display_session_token,
                sanitize_trace_value(runtime_issue)
            ));
            let _ = send_ws_message(
                &mut session,
                StreamServerMessage::DebugLog {
                    message: format!("Runtime host belum sehat untuk stream. {runtime_issue}"),
                    ty: Some(LogMessageType::FatalDescription),
                },
            )
            .await;
            let _ = session.close(None).await;
            return;
        }

        let pair_info = match host.pair_info(&mut user).await {
            Ok(pair_info) => pair_info,
            Err(err) => {
                warn!("failed to start stream for host {host_id:?} (at get pair_info): {err}");
                append_host_stream_trace(&format!(
                    "HOST_PAIR_INFO_FAILED token={} host_id={} reason={}",
                    dynamic_display_session_token,
                    host_id.0,
                    sanitize_trace_value(&err.to_string())
                ));

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because the host is not paired"
                            .to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        if legacy_runtime_selected_for_session {
            let _ = send_ws_message(
                &mut session,
                StreamServerMessage::DebugLog {
                    message: "Refreshing NVENC runtime before session start".to_string(),
                    ty: None,
                },
            )
            .await;

            if let Err(err) = refresh_legacy_runtime_before_session(
                &dynamic_display_session_token,
                &address,
                http_port,
            )
            .await
            {
                warn!("failed to refresh legacy runtime before stream start: {err}");
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_REFRESH_FAILED token={} error={}",
                    dynamic_display_session_token,
                    sanitize_trace_value(&err)
                ));

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to prepare the NVENC runtime before stream start"
                            .to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        }

        let mut apps = match host.list_apps(&mut user).await {
            Ok(apps) => apps,
            Err(err) => {
                let err_text = err.to_string();
                warn!("failed to start stream for host {host_id:?} (at list_apps): {err}");
                append_host_stream_trace(&format!(
                    "HOST_APPS_FAILED token={} host_id={} reason={}",
                    dynamic_display_session_token,
                    host_id.0,
                    sanitize_trace_value(&err_text)
                ));
                note_host_lifecycle_phase("failed", "host_api_list_apps_failed");
                maybe_schedule_host_failure_recovery("host_api_list_apps_failed");

                if legacy_runtime_selected_for_session
                    && should_auto_fallback_legacy_after_host_api_error(&err_text)
                {
                    let failure_count = note_legacy_runtime_startup_failure().await;
                    append_host_stream_trace(&format!(
                        "LEGACY_RUNTIME_STARTUP_FAILURE token={} stage=host_api failure_count={} reason=legacy_host_api_unreachable_after_selection",
                        dynamic_display_session_token, failure_count
                    ));
                    if failure_count >= 2 {
                        maybe_schedule_legacy_runtime_auto_fallback(
                            &dynamic_display_session_token,
                            "legacy_host_api_unreachable_after_selection",
                        );
                    }
                }

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because of a server error".to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        let requested_app_index = apps.iter().position(|app| app.id == app_id);
        let fallback_app_index = if requested_app_index.is_none() {
            apps.iter()
                .position(|app| app.title.eq_ignore_ascii_case("desktop"))
                .or_else(|| (apps.len() == 1).then_some(0))
        } else {
            None
        };

        let app = match requested_app_index.or(fallback_app_index) {
            Some(index) => {
                let selected_app = apps.swap_remove(index);
                if selected_app.id != app_id {
                    let fallback_reason = if selected_app.title.eq_ignore_ascii_case("desktop") {
                        "stale_app_id_desktop_fallback"
                    } else {
                        "stale_app_id_single_app_fallback"
                    };
                    append_host_stream_trace(&format!(
                        "APP_LOOKUP_FALLBACK token={} host_id={} requested_app_id={} selected_app_id={} selected_title={} reason={}",
                        dynamic_display_session_token,
                        host_id.0,
                        app_id.0,
                        selected_app.id.0,
                        sanitize_trace_value(&selected_app.title),
                        fallback_reason
                    ));
                    info!(
                        "app id {:?} not found for host {:?}, falling back to {:?} ({})",
                        app_id, host_id, selected_app.id, selected_app.title
                    );
                }
                selected_app
            }
            None => {
                warn!(
                    "failed to start stream for host {host_id:?} because the app couldn't be found!"
                );
                append_host_stream_trace(&format!(
                    "APP_LOOKUP_FAILED token={} host_id={} app_id={} reason=app_not_found",
                    dynamic_display_session_token, host_id.0, app_id.0
                ));

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because the app was not found".to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };
        let effective_app_id = app.id;

        // -- Send App info
        let _ = send_ws_message(
            &mut session,
            StreamServerMessage::UpdateApp { app: app.into() },
        )
        .await;

        // -- Starting stage: launch streamer
        let _ = send_ws_message(
            &mut session,
            StreamServerMessage::DebugLog {
                message: "Launching streamer".to_string(),
                ty: None,
            },
        )
        .await;

        let staged_streamer_path =
            match resolve_runtime_binary_path(&web_app.config().streamer_path)
                .and_then(|path| stage_child_binary(&path, &dynamic_display_session_token))
            {
                Ok(path) => path,
                Err(err) => {
                    error!("[Stream]: failed to stage streamer process: {err}");
                    append_host_stream_trace(&format!(
                        "STAGE_STREAMER_FAILED token={} error={}",
                        dynamic_display_session_token,
                        sanitize_trace_value(&err)
                    ));

                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::DebugLog {
                            message: "Failed to start stream because of a server error".to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    return;
                }
            };
        append_host_stream_trace(&format!(
            "STREAMER_STAGED token={} path={}",
            dynamic_display_session_token,
            sanitize_trace_value(&staged_streamer_path.display().to_string())
        ));

        // Spawn child
        let mut staged_streamer_command = Command::new(&staged_streamer_path);
        staged_streamer_command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if legacy_runtime_selected_for_session {
            staged_streamer_command.env("ML_LEGACY_NVENC_COMPAT", "1");
            append_host_stream_trace(&format!(
                "STREAMER_ENV token={} legacy_nvenc_compat=1",
                dynamic_display_session_token
            ));
        }

        let (mut child, stdin, stdout) = match staged_streamer_command.spawn() {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.take()
                    && let Some(stdout) = child.stdout.take()
                {
                    (child, stdin, stdout)
                } else {
                    error!("[Stream]: streamer process didn't include a stdin or stdout");
                    append_host_stream_trace(&format!(
                        "SPAWN_STREAMER_FAILED token={} error=missing_stdio",
                        dynamic_display_session_token
                    ));

                    let _ = send_ws_message(
                        &mut session,
                        StreamServerMessage::DebugLog {
                            message: "Failed to start stream because of a server error".to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;

                    if let Err(err) = child.kill().await {
                        warn!("[Stream]: failed to kill child: {err}");
                    }
                    cleanup_staged_child_binary(&staged_streamer_path);

                    return;
                }
            }
            Err(err) => {
                error!("[Stream]: failed to spawn streamer process: {err}");
                append_host_stream_trace(&format!(
                    "SPAWN_STREAMER_FAILED token={} error={err}",
                    dynamic_display_session_token
                ));

                let _ = send_ws_message(
                    &mut session,
                    StreamServerMessage::DebugLog {
                        message: "Failed to start stream because of a server error".to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                return;
            }
        };

        #[cfg(windows)]
        if let Some(pid) = child.id() {
            boost_child_process_priority(pid, "streamer", &dynamic_display_session_token);
        }

        if let Some(stderr) = child.stderr.take() {
            let dynamic_display_session_token = dynamic_display_session_token.clone();
            spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                let mut suppressed_noisy_lines: u32 = 0;
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    let noisy_line = is_noisy_streamer_stderr_line(line);
                    if noisy_line {
                        suppressed_noisy_lines = suppressed_noisy_lines.saturating_add(1);
                        if suppressed_noisy_lines == 1 || suppressed_noisy_lines % 100 == 0 {
                            append_host_stream_trace(&format!(
                                "STREAMER_STDERR_SUPPRESSED token={} noisy_count={}",
                                dynamic_display_session_token, suppressed_noisy_lines
                            ));
                        }
                    } else {
                        append_host_stream_trace(&format!(
                            "STREAMER_STDERR token={} line={}",
                            dynamic_display_session_token,
                            sanitize_trace_value(line)
                        ));
                    }

                    if is_sunshine_capture_init_failure_line(line) {
                        record_recent_capture_init_failure();
                        append_host_stream_trace(&format!(
                            "GOLDEN_PATH_CAPTURE_INIT_FAILED token={} reason=sunshine_capture_init_failed line={}",
                            dynamic_display_session_token,
                            sanitize_trace_value(line)
                        ));
                        note_host_lifecycle_phase("recovering", "sunshine_capture_init_failed");
                        maybe_schedule_host_failure_recovery("sunshine_capture_init_failed");
                    }
                }
            });
        }

        // Create ipc
        static CHILD_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = CHILD_COUNTER.fetch_add(1, Ordering::Relaxed);
        let span = span!(Level::INFO, "ipc", child_id = id);

        let (mut ipc_sender, mut ipc_receiver) =
            create_child_ipc::<ServerIpcMessage, StreamerIpcMessage>(span, stdin, stdout, None)
                .await;
        let child = Arc::new(Mutex::new(child));
        replace_active_child(&ACTIVE_STREAM_CHILD, child.clone(), "Stream").await;
        let mut current_window_watch_child: Option<Arc<Mutex<Child>>> = None;
        match spawn_window_primary_watch(&dynamic_display_session_token).await {
            Ok(window_watch_child) => {
                current_window_watch_child = Some(window_watch_child.clone());
                replace_active_child(
                    &ACTIVE_WINDOW_WATCH_CHILD,
                    window_watch_child,
                    "Window Watch",
                )
                .await;
                append_host_stream_trace(&format!(
                    "WINDOW_WATCH_STARTED token={}",
                    dynamic_display_session_token
                ));
            }
            Err(err) => {
                warn!("[Stream]: failed to start window watch helper: {err}");
                append_host_stream_trace(&format!(
                    "WINDOW_WATCH_FAILED token={} error={}",
                    dynamic_display_session_token,
                    sanitize_trace_value(&err)
                ));
            }
        }
        append_host_stream_trace(&format!(
            "STREAMER_SPAWNED token={} host_id={} app_id={}",
            dynamic_display_session_token, host_id.0, effective_app_id.0
        ));

        let stream_connection_established = Arc::new(AtomicBool::new(false));
        let video_flow_ready_observed = Arc::new(AtomicBool::new(false));
        let start_stream_received = Arc::new(AtomicBool::new(false));
        let dynamic_display_match_active = Arc::new(AtomicBool::new(false));
        let prepared_display_active = Arc::new(AtomicBool::new(false));
        let (server_ws_sender, mut server_ws_receiver) = unbounded_channel::<StreamServerMessage>();

        // Redirect ipc message into ws
        let mut write_session = session.clone();
        spawn({
            let mut ipc_sender = ipc_sender.clone();
            let child = child.clone();
            let staged_streamer_path = staged_streamer_path.clone();
            let stream_connection_established = stream_connection_established.clone();
            let video_flow_ready_observed = video_flow_ready_observed.clone();
            let start_stream_received = start_stream_received.clone();
            let dynamic_display_match_active = dynamic_display_match_active.clone();
            let prepared_display_active = prepared_display_active.clone();
            let dynamic_display_session_token = dynamic_display_session_token.clone();
            let android_native_owner_session_id = android_native_owner_session_id.clone();
            async move {
                let mut warned_closed = false;
                let mut ipc_receiver_closed = false;
                let mut server_ws_receiver_closed = false;
                loop {
                    tokio::select! {
                        maybe_message = ipc_receiver.recv(), if !ipc_receiver_closed => {
                            match maybe_message {
                                Some(StreamerIpcMessage::WebSocket(message)) => {
                                    maybe_register_shared_player2_bridge_for_message(
                                        android_native_owner_session_id.as_deref(),
                                        &dynamic_display_session_token,
                                        host_id,
                                        effective_app_id,
                                        &ipc_sender,
                                        &message,
                                    ).await;
                                    forward_stream_server_message(
                                    &mut write_session,
                                    &mut ipc_sender,
                                    message,
                                     &dynamic_display_session_token,
                                     legacy_runtime_selected_for_session,
                                     &stream_connection_established,
                                     &video_flow_ready_observed,
                                     &start_stream_received,
                                     &mut warned_closed,
                                 ).await;
                                }
                                Some(StreamerIpcMessage::WebSocketTransport(data)) => {
                                    if let Err(Closed) = write_session.binary(data).await
                                        && !warned_closed
                                    {
                                        warn!(
                                            "[Ipc]: Tried to send a ws message (binary) but the socket is already closed"
                                        );
                                        if !stream_connection_established.load(Ordering::Acquire) {
                                            warn!(
                                                "[Stream]: websocket binary path closed before connection was stable (start_stream_received={})",
                                                start_stream_received.load(Ordering::Acquire)
                                            );
                                            ipc_sender.send(ServerIpcMessage::Stop).await;
                                        }
                                        warned_closed = true;
                                    }
                                }
                                Some(StreamerIpcMessage::Stop) => {
                                    debug!("[Ipc]: ipc receiver stopped by streamer");
                                    ipc_receiver_closed = true;
                                }
                                None => {
                                    ipc_receiver_closed = true;
                                }
                            }
                        }
                        maybe_message = server_ws_receiver.recv(), if !server_ws_receiver_closed => {
                            match maybe_message {
                                Some(message) => {
                                    maybe_register_shared_player2_bridge_for_message(
                                        android_native_owner_session_id.as_deref(),
                                        &dynamic_display_session_token,
                                        host_id,
                                        effective_app_id,
                                        &ipc_sender,
                                        &message,
                                    ).await;
                                    forward_stream_server_message(
                                    &mut write_session,
                                    &mut ipc_sender,
                                    message,
                                     &dynamic_display_session_token,
                                     legacy_runtime_selected_for_session,
                                     &stream_connection_established,
                                     &video_flow_ready_observed,
                                     &start_stream_received,
                                     &mut warned_closed,
                                 ).await;
                                }
                                None => {
                                    server_ws_receiver_closed = true;
                                }
                            }
                        }
                    }

                    if ipc_receiver_closed && server_ws_receiver_closed {
                        break;
                    }
                }
                info!("[Ipc]: ipc receiver is closed");

                if let Some(owner_session_id) = android_native_owner_session_id.as_deref() {
                    clear_shared_player2_joiner(owner_session_id, None).await;
                    clear_shared_player2_bridge(owner_session_id).await;
                }

                stop_active_window_watch(
                    &dynamic_display_session_token,
                    "ipc_receiver_closed_before_display_restore",
                )
                .await;
                restore_runtime_dynamic_display_if_active(
                    &dynamic_display_match_active,
                    &dynamic_display_session_token,
                )
                .await;
                restore_prepared_stream_display_if_active(
                    &prepared_display_active,
                    &dynamic_display_session_token,
                )
                .await;

                let _ = ipc_sender.send(ServerIpcMessage::Stop).await;

                // Give the staged streamer a brief chance to exit cleanly before we force-kill it.
                sleep(Duration::from_millis(1200)).await;

                // close the websocket when the streamer crashed / disconnected / whatever
                if let Err(err) = write_session.close(None).await {
                    warn!("failed to close streamer web socket: {err}");
                }

                // kill the streamer
                force_kill_child(&child, "Stream").await;
                cleanup_staged_child_binary(&staged_streamer_path);
                if let Some(window_watch_child) = {
                    let mut slot = ACTIVE_WINDOW_WATCH_CHILD.lock().await;
                    slot.take()
                } {
                    force_kill_child(&window_watch_child, "Window Watch").await;
                }
            }
        });

        let streamer_host_control_address = legacy_runtime_control_address(&address).to_string();
        append_host_stream_trace(&format!(
            "STREAMER_CONTROL_ADDRESS token={} host_id={} public_address={} control_address={} http_port={}",
            dynamic_display_session_token,
            host_id.0,
            sanitize_trace_value(&address),
            sanitize_trace_value(&streamer_host_control_address),
            http_port
        ));

        // Send init into ipc
        ipc_sender
            .send(ServerIpcMessage::Init {
                config: StreamerConfig {
                    webrtc: web_app.config().webrtc.clone(),
                    log_level: web_app.config().log.level_filter,
                },
                host_address: streamer_host_control_address,
                host_http_port: http_port,
                client_unique_id: Some(client_unique_id),
                client_private_key: pair_info.client_private_key,
                client_certificate: pair_info.client_certificate,
                server_certificate: pair_info.server_certificate,
                app_id: effective_app_id.0,
                video_frame_queue_size,
                audio_sample_queue_size,
            })
            .await;

        let mut dynamic_display_match_requested = dynamic_display_match;
        let mut stream_display_prepare_requested = true;
        let mut ws_close_logged = false;
        let mut pre_connect_stop_reason: Option<String> = None;
        let mut pending_control_message: Option<Message> = None;

        // Redirect ws message into ipc
        loop {
            let message = if let Some(message) = pending_control_message.take() {
                message
            } else {
                let next_message = match timeout(STREAM_CONTROL_IDLE_TIMEOUT, stream.recv()).await {
                    Ok(message) => message,
                    Err(_) => {
                        let connection_established =
                            stream_connection_established.load(Ordering::Acquire);
                        let timeout_reason = if connection_established {
                            "control_channel_heartbeat_timeout_after_connect"
                        } else {
                            "control_channel_heartbeat_timeout_before_connect"
                        };
                        append_host_stream_trace(&format!(
                            "CONTROL_CHANNEL_IDLE_TIMEOUT token={} start_stream_received={} connection_established={} reason={}",
                            dynamic_display_session_token,
                            start_stream_received.load(Ordering::Acquire),
                            connection_established,
                            timeout_reason
                        ));
                        if connection_established {
                            note_host_lifecycle_phase("recovering", timeout_reason);
                        } else {
                            pre_connect_stop_reason = Some(timeout_reason.to_string());
                        }
                        warn!(
                            "[Stream]: control channel heartbeat timed out (start_stream_received={}, connection_established={})",
                            start_stream_received.load(Ordering::Acquire),
                            connection_established
                        );
                        break;
                    }
                };

                let Some(message) = next_message else {
                    break;
                };
                match message {
                    Ok(message) => message,
                    Err(err) => {
                        warn!("[Stream]: websocket receive failed: {err}");
                        append_host_stream_trace(&format!(
                            "WS_RECV_FAILED token={} error={}",
                            dynamic_display_session_token,
                            sanitize_trace_value(&err.to_string())
                        ));
                        if !stream_connection_established.load(Ordering::Acquire) {
                            pre_connect_stop_reason =
                                Some("control_channel_receive_failed".to_string());
                        }
                        break;
                    }
                }
            };

            match message {
                Message::Text(text) => {
                    let Ok(mut message) = serde_json::from_str::<StreamClientMessage>(&text) else {
                        warn!("[Stream]: failed to deserialize from json");
                        break;
                    };

                    if !matches!(message, StreamClientMessage::Heartbeat { .. }) {
                        append_host_stream_trace(&format!(
                            "WS_RX token={} {}",
                            dynamic_display_session_token,
                            describe_stream_client_message(&message)
                        ));
                    }

                    if matches!(message, StreamClientMessage::Heartbeat { .. }) {
                        continue;
                    }

                    if let StreamClientMessage::RouteTelemetry { route, detail } = &message {
                        append_host_stream_trace(&format!(
                            "ROUTE token={} route={} detail={}",
                            dynamic_display_session_token,
                            sanitize_trace_value(route),
                            sanitize_trace_value(detail)
                        ));
                        continue;
                    }

                    if let StreamClientMessage::ProjectDisplay { mode } = &message {
                        let requested_mode = mode.trim().to_string();
                        let normalized_mode = requested_mode.to_ascii_lowercase().replace('-', "_");
                        let label = match normalized_mode.as_str() {
                            "extend" | "perluas" => "Perluas",
                            "duplicate" | "clone" | "mirror" | "double" | "doble" | "duplikat" => {
                                "Duplikat"
                            }
                            "primary"
                            | "utama"
                            | "make_primary"
                            | "stream_primary"
                            | "second_screen_only"
                            | "second_screen"
                            | "stream_only"
                            | "stream_display_only"
                            | "layar_stream"
                            | "layar_stream_saja" => "Layar stream saja",
                            _ => "Mode layar",
                        };
                        append_host_stream_trace(&format!(
                            "DISPLAY_MODE_REQUEST token={} mode={}",
                            dynamic_display_session_token,
                            sanitize_trace_value(&requested_mode)
                        ));
                        match apply_project_display_mode(
                            &dynamic_display_session_token,
                            &requested_mode,
                        )
                        .await
                        {
                            Ok(result) => {
                                append_host_stream_trace(&format!(
                                    "DISPLAY_MODE_APPLIED token={} mode={} changed={} skipped={} reason={}",
                                    dynamic_display_session_token,
                                    sanitize_trace_value(&requested_mode),
                                    result.changed,
                                    result.skipped,
                                    sanitize_trace_value(if result.reason.is_empty() {
                                        "ok"
                                    } else {
                                        &result.reason
                                    })
                                ));
                                let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                    message: format!("{label} diterapkan."),
                                    ty: Some(LogMessageType::Recover),
                                });
                            }
                            Err(err) => {
                                warn!("[Stream]: display mode command failed: {err}");
                                append_host_stream_trace(&format!(
                                    "DISPLAY_MODE_FAILED token={} mode={} error={}",
                                    dynamic_display_session_token,
                                    sanitize_trace_value(&requested_mode),
                                    sanitize_trace_value(&err)
                                ));
                                let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                    message: format!("{label} gagal diterapkan."),
                                    ty: Some(LogMessageType::Recover),
                                });
                            }
                        }
                        continue;
                    }

                    if let StreamClientMessage::StartStream {
                        width,
                        height,
                        fps,
                        bitrate,
                        packet_size,
                        ..
                    } = &mut message
                    {
                        if legacy_runtime_selected_for_session {
                            if preserve_legacy_vdd_surface_for_session {
                                append_host_stream_trace(&format!(
                                    "LEGACY_NVENC_VDD_SURFACE_PRESERVED token={} phase=start width={} height={} fps={}",
                                    dynamic_display_session_token, *width, *height, *fps
                                ));
                            }
                            if apply_legacy_nvenc_stream_limits(
                                width,
                                height,
                                fps,
                                bitrate,
                                packet_size,
                                preserve_legacy_vdd_surface_for_session,
                            ) {
                                append_host_stream_trace(&format!(
                                    "LEGACY_NVENC_STREAM_LIMITS token={} width={} height={} fps={} bitrate={} packet_size={}",
                                    dynamic_display_session_token,
                                    *width,
                                    *height,
                                    *fps,
                                    *bitrate,
                                    *packet_size
                                ));
                            }
                        } else if software_runtime_selected_for_session
                            && apply_software_stream_limits(
                                width,
                                height,
                                fps,
                                bitrate,
                                packet_size,
                            )
                        {
                            append_host_stream_trace(&format!(
                                "SOFTWARE_STREAM_LIMITS token={} width={} height={} fps={} bitrate={} packet_size={}",
                                dynamic_display_session_token,
                                *width,
                                *height,
                                *fps,
                                *bitrate,
                                *packet_size
                            ));
                        }
                    } else if let StreamClientMessage::ResizeStream { width, height, fps } =
                        &mut message
                    {
                        if legacy_runtime_selected_for_session {
                            if preserve_legacy_vdd_surface_for_session {
                                append_host_stream_trace(&format!(
                                    "LEGACY_NVENC_VDD_SURFACE_PRESERVED token={} phase=resize width={} height={} fps={}",
                                    dynamic_display_session_token, *width, *height, *fps
                                ));
                            }
                            if apply_legacy_nvenc_resize_limits(
                                width,
                                height,
                                fps,
                                preserve_legacy_vdd_surface_for_session,
                            ) {
                                append_host_stream_trace(&format!(
                                    "LEGACY_NVENC_RESIZE_LIMITS token={} width={} height={} fps={}",
                                    dynamic_display_session_token, *width, *height, *fps
                                ));
                            }
                        } else if software_runtime_selected_for_session
                            && apply_software_resize_limits(*width, *height, fps)
                        {
                            append_host_stream_trace(&format!(
                                "SOFTWARE_RESIZE_LIMITS token={} width={} height={} fps={}",
                                dynamic_display_session_token, *width, *height, *fps
                            ));
                        }
                    }

                    if let StreamClientMessage::StartStream {
                        width,
                        height,
                        fps,
                        host_mouse_emulation,
                        ..
                    } = &message
                    {
                        start_stream_received.store(true, Ordering::Release);
                        note_host_lifecycle_phase("starting", "stream_start_requested");
                        info!("[Stream]: received StartStream from client");
                        append_host_stream_trace(&format!(
                            "START_STREAM token={} host_mouse_emulation={host_mouse_emulation:?}",
                            dynamic_display_session_token,
                        ));
                        append_host_stream_trace(&format!(
                            "GOLDEN_PATH_START_REQUEST token={} requested={}x{}@{} policy=exact_vdd_no_runtime_refresh",
                            dynamic_display_session_token, width, height, fps
                        ));
                    }

                    if stream_display_prepare_requested
                        && let StreamClientMessage::StartStream {
                            width, height, fps, ..
                        } = &mut message
                    {
                        stream_display_prepare_requested = false;
                        let mut start_stream_settle_delay_ms = None;
                        let requested_width = *width;
                        let requested_height = *height;
                        let requested_fps = *fps;
                        append_host_stream_trace(&format!(
                            "GOLDEN_PATH_PREPARE_BEGIN token={} requested={}x{}@{}",
                            dynamic_display_session_token,
                            requested_width,
                            requested_height,
                            requested_fps
                        ));

                        if should_release_rdp_session_before_stream(
                            runtime_profile_snapshot.as_ref(),
                        ) {
                            append_host_stream_trace(&format!(
                                "RDP_SESSION_RELEASE_REQUEST token={}",
                                dynamic_display_session_token
                            ));
                            let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                message: "Desktop remote aktif. Menyiapkan desktop lokal agar stream bisa mulai.".to_string(),
                                ty: Some(LogMessageType::Recover),
                            });

                            match release_active_rdp_session_for_stream().await {
                                Ok(true) => {
                                    append_host_stream_trace(&format!(
                                        "RDP_SESSION_RELEASED token={}",
                                        dynamic_display_session_token
                                    ));
                                    let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                        message: "Desktop lokal siap. Memulai stream.".to_string(),
                                        ty: Some(LogMessageType::Recover),
                                    });
                                }
                                Ok(false) => {}
                                Err(err) => {
                                    warn!("[Stream]: failed to release active RDP session: {err}");
                                    append_host_stream_trace(&format!(
                                        "RDP_SESSION_RELEASE_FAILED token={} error={}",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(&err)
                                    ));
                                }
                            }
                        }

                        let mut prepare_result = None;
                        let max_prepare_attempts = 3usize;
                        for attempt in 1..=max_prepare_attempts {
                            match prepare_stream_display(
                                &dynamic_display_session_token,
                                Some((requested_width, requested_height, requested_fps)),
                            )
                            .await
                            {
                                Ok(result) => {
                                    if attempt > 1 {
                                        append_host_stream_trace(&format!(
                                            "DISPLAY_PREPARE_RETRY_OK token={} attempt={}/{} changed={} skipped={} reason={}",
                                            dynamic_display_session_token,
                                            attempt,
                                            max_prepare_attempts,
                                            result.changed,
                                            result.skipped,
                                            sanitize_trace_value(if result.reason.is_empty() {
                                                "ok"
                                            } else {
                                                &result.reason
                                            })
                                        ));
                                    }
                                    prepare_result = Some(Ok(result));
                                    break;
                                }
                                Err(err) => {
                                    let display_route_issue = detect_display_route_issue(
                                        runtime_profile_snapshot.as_ref(),
                                    );
                                    let should_retry = attempt < max_prepare_attempts
                                        && should_retry_display_prepare_failure(
                                            &err,
                                            display_route_issue.as_deref(),
                                        );
                                    if should_retry {
                                        append_host_stream_trace(&format!(
                                            "DISPLAY_PREPARE_RETRY token={} attempt={}/{} error={} route_issue={}",
                                            dynamic_display_session_token,
                                            attempt,
                                            max_prepare_attempts,
                                            sanitize_trace_value(&err),
                                            sanitize_trace_value(
                                                display_route_issue.as_deref().unwrap_or("none"),
                                            )
                                        ));
                                        let retry_ordinal = attempt + 1;
                                        let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                            message: format!(
                                                "Display stream belum siap. Mencoba mengaktifkan ulang Virtual Display Driver ({retry_ordinal}/{max_prepare_attempts})."
                                            ),
                                            ty: Some(LogMessageType::Recover),
                                        });
                                        let retry_delay_ms = 900 + (((attempt - 1) as u64) * 700);
                                        sleep(Duration::from_millis(retry_delay_ms)).await;
                                        continue;
                                    }

                                    if attempt > 1 {
                                        append_host_stream_trace(&format!(
                                            "DISPLAY_PREPARE_RETRY_FAILED token={} attempt={}/{} error={} route_issue={}",
                                            dynamic_display_session_token,
                                            attempt,
                                            max_prepare_attempts,
                                            sanitize_trace_value(&err),
                                            sanitize_trace_value(
                                                display_route_issue.as_deref().unwrap_or("none"),
                                            )
                                        ));
                                    }
                                    prepare_result = Some(Err((err, display_route_issue)));
                                    break;
                                }
                            }
                        }

                        let prepare_result = prepare_result.unwrap_or_else(|| {
                            Err((
                                "display prepare ended without a result".to_string(),
                                detect_display_route_issue(runtime_profile_snapshot.as_ref()),
                            ))
                        });

                        match prepare_result {
                            Ok(result) => {
                                dynamic_display_match_requested = false;
                                if result.changed {
                                    prepared_display_active.store(true, Ordering::Release);
                                }

                                if let Some(applied) = result.applied.as_ref() {
                                    if should_preserve_requested_stream_surface_after_display_fallback(
                                        runtime_profile_snapshot.as_ref(),
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                        &result.reason,
                                    ) {
                                        append_host_stream_trace(&format!(
                                            "NVENC_KEEP_START_STREAM_SURFACE token={} requested={}x{} applied={}x{} reason={}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                            sanitize_trace_value(&result.reason)
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else if legacy_runtime_selected_for_session
                                        && should_preserve_legacy_nvenc_stream_surface(
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                        )
                                    {
                                        append_host_stream_trace(&format!(
                                            "LEGACY_NVENC_KEEP_STREAM_SURFACE token={} requested={}x{} applied={}x{}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else {
                                        *width = applied.width.max(1);
                                        *height = applied.height.max(1);
                                    }
                                    cap_stream_fps_to_applied_refresh(fps, applied.frequency);
                                }

                                if let Some(applied) = result.applied.as_ref() {
                                    append_host_stream_trace(&format!(
                                        "DISPLAY_PREPARE token={} applied={}x{}@{} changed={} skipped={} reason={}",
                                        dynamic_display_session_token,
                                        applied.width,
                                        applied.height,
                                        applied.frequency,
                                        result.changed,
                                        result.skipped,
                                        sanitize_trace_value(&result.reason)
                                    ));
                                    append_host_stream_trace(&format!(
                                        "GOLDEN_PATH_PREPARED token={} requested={}x{}@{} applied={}x{}@{} stream={}x{}@{} exact_match={} reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        applied.width,
                                        applied.height,
                                        applied.frequency,
                                        *width,
                                        *height,
                                        *fps,
                                        applied.width == requested_width
                                            && applied.height == requested_height
                                            && (*width == requested_width.max(1))
                                            && (*height == requested_height.max(1)),
                                        sanitize_trace_value(&result.reason)
                                    ));
                                } else {
                                    append_host_stream_trace(&format!(
                                        "DISPLAY_PREPARE token={} changed={} skipped={} reason={}",
                                        dynamic_display_session_token,
                                        result.changed,
                                        result.skipped,
                                        sanitize_trace_value(&result.reason)
                                    ));
                                }

                                if result.sunshine_capture_target_changed {
                                    let capture_display = result
                                        .sunshine_capture_display
                                        .as_deref()
                                        .unwrap_or("unknown");
                                    let capture_config_path = result
                                        .sunshine_capture_config_path
                                        .as_deref()
                                        .unwrap_or("unknown");
                                    append_host_stream_trace(&format!(
                                        "SUNSHINE_CAPTURE_CONFIG_CHANGED token={} display={} config={}",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(capture_display),
                                        sanitize_trace_value(capture_config_path)
                                    ));
                                    append_host_stream_trace(&format!(
                                        "SUNSHINE_CAPTURE_RUNTIME_REFRESH_REQUIRED token={} display={} config={} reason=capture_display_changed",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(capture_display),
                                        sanitize_trace_value(capture_config_path)
                                    ));
                                    note_host_lifecycle_phase(
                                        "recovering",
                                        "capture_display_changed_refresh",
                                    );
                                    let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                        message: "Layar stream sudah dipilih. Menyegarkan capture host sebelum video dimulai.".to_string(),
                                        ty: Some(LogMessageType::Recover),
                                    });

                                    let refresh_result = restart_sunshine_capture_with_fallback(
                                        &dynamic_display_session_token,
                                        &address,
                                        http_port,
                                    )
                                    .await;

                                    match refresh_result {
                                        Ok(refresh_strategy) => {
                                            append_host_stream_trace(&format!(
                                                "SUNSHINE_CAPTURE_RUNTIME_REFRESH_COMPLETED token={} display={} strategy={}",
                                                dynamic_display_session_token,
                                                sanitize_trace_value(capture_display),
                                                sanitize_trace_value(refresh_strategy)
                                            ));
                                            note_host_lifecycle_phase(
                                                "starting",
                                                "capture_display_changed_ready",
                                            );
                                            let settle_delay_ms =
                                                if refresh_strategy == "full_restart" {
                                                    2500
                                                } else {
                                                    1200
                                                };
                                            start_stream_settle_delay_ms = Some(
                                                start_stream_settle_delay_ms
                                                    .unwrap_or(0)
                                                    .max(settle_delay_ms),
                                            );
                                        }
                                        Err(err) => {
                                            record_recent_capture_init_failure();
                                            append_host_stream_trace(&format!(
                                                "SUNSHINE_CAPTURE_RUNTIME_REFRESH_FAILED token={} display={} error={}",
                                                dynamic_display_session_token,
                                                sanitize_trace_value(capture_display),
                                                sanitize_trace_value(&err)
                                            ));
                                            warn!(
                                                "[Stream]: failed to refresh Sunshine after capture display changed: {err}"
                                            );
                                            let _ = server_ws_sender.send(
                                                StreamServerMessage::DebugLog {
                                                    message: "Capture host belum bisa disegarkan. Menjalankan recovery penuh sebelum stream dilanjutkan.".to_string(),
                                                    ty: Some(LogMessageType::Recover),
                                                },
                                            );

                                            match run_blocking_host_failure_recovery(
                                                &dynamic_display_session_token,
                                                FailureRecoveryStrategy::RestartRuntime,
                                                "capture_display_refresh_failed",
                                                &address,
                                                http_port,
                                            )
                                            .await
                                            {
                                                Ok(()) => {
                                                    runtime_profile_snapshot =
                                                        read_host_capability_profile_snapshot();
                                                    runtime_startup_issue_at_init =
                                                        blocking_runtime_startup_issue(
                                                            runtime_profile_snapshot.as_ref(),
                                                        );
                                                    legacy_runtime_selected_for_session =
                                                        legacy_runtime_selected_in_snapshot(
                                                            runtime_profile_snapshot.as_ref(),
                                                        );
                                                    software_runtime_selected_for_session =
                                                        software_runtime_selected(
                                                            runtime_profile_snapshot.as_ref(),
                                                        );
                                                    preserve_legacy_vdd_surface_for_session =
                                                        dynamic_display_match
                                                            && runtime_profile_snapshot
                                                                .as_ref()
                                                                .is_some_and(
                                                                    capability_profile_has_virtual_display_driver,
                                                                );
                                                    RECENT_CAPTURE_INIT_FAILURE_AT_MS
                                                        .store(0, Ordering::Release);
                                                    note_host_lifecycle_phase(
                                                        "starting",
                                                        "capture_display_changed_ready_after_blocking_recovery",
                                                    );
                                                    start_stream_settle_delay_ms = Some(
                                                        start_stream_settle_delay_ms
                                                            .unwrap_or(0)
                                                            .max(3200),
                                                    );
                                                }
                                                Err(recovery_err) => {
                                                    let _ = server_ws_sender.send(
                                                        StreamServerMessage::DebugLog {
                                                            message: "Capture host belum bisa disegarkan. Stream dibatalkan agar PC tidak macet.".to_string(),
                                                            ty: Some(LogMessageType::FatalDescription),
                                                        },
                                                    );
                                                    pre_connect_stop_reason = Some(format!(
                                                        "capture_display_refresh_failed:{}",
                                                        sanitize_trace_value(&recovery_err)
                                                    ));
                                                    note_host_lifecycle_phase(
                                                        "failed",
                                                        pre_connect_stop_reason
                                                            .as_deref()
                                                            .unwrap_or(
                                                                "capture_display_refresh_failed",
                                                            ),
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                } else if result.sunshine_capture_changed {
                                    let capture_display = result
                                        .sunshine_capture_display
                                        .as_deref()
                                        .unwrap_or("unknown");
                                    let capture_config_path = result
                                        .sunshine_capture_config_path
                                        .as_deref()
                                        .unwrap_or("unknown");
                                    append_host_stream_trace(&format!(
                                        "SUNSHINE_CAPTURE_CONFIG_UPDATED_NO_REFRESH token={} display={} config={} reason=config_text_only",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(capture_display),
                                        sanitize_trace_value(capture_config_path)
                                    ));
                                } else if let Some(runtime_issue) =
                                    runtime_startup_issue_at_init.as_deref()
                                {
                                    append_host_stream_trace(&format!(
                                        "RUNTIME_PROFILE_ISSUE_DEFERRED token={} detail={}",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(runtime_issue)
                                    ));
                                }

                                let exact_prepared_surface =
                                    result.applied.as_ref().is_some_and(|applied| {
                                        applied.width == requested_width
                                            && applied.height == requested_height
                                    });
                                let ddx_nvenc_session =
                                    runtime_profile_snapshot.as_ref().is_none_or(|profile| {
                                        profile.selected_encoder.eq_ignore_ascii_case("nvenc")
                                            && profile.selected_capture.eq_ignore_ascii_case("ddx")
                                    });
                                let should_proactively_refresh_exact_vdd_capture = !result
                                    .sunshine_capture_target_changed
                                    && exact_prepared_surface
                                    && ddx_nvenc_session
                                    && result.sunshine_capture_changed
                                    && std::env::var("CLOUDGIME_FAST_CONNECT").unwrap_or_default()
                                        != "1";
                                let mut proactive_capture_refresh_completed = false;

                                if should_proactively_refresh_exact_vdd_capture {
                                    if CAPTURE_PRESTART_REFRESH_ACTIVE
                                        .compare_exchange(
                                            false,
                                            true,
                                            Ordering::AcqRel,
                                            Ordering::Acquire,
                                        )
                                        .is_ok()
                                    {
                                        append_host_stream_trace(&format!(
                                            "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=begin reason=exact_vdd_mode_changed prepared={}x{}@{} capture_changed={} display_changed={}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            requested_fps,
                                            result.sunshine_capture_changed,
                                            result.changed
                                        ));
                                        note_host_lifecycle_phase(
                                            "recovering",
                                            "capture_prestart_runtime_refresh",
                                        );
                                        let _ = server_ws_sender.send(
                                            StreamServerMessage::DebugLog {
                                                message: "Capture Sunshine disegarkan ulang setelah Virtual Display exact siap, sebelum video dimulai.".to_string(),
                                                ty: Some(LogMessageType::Recover),
                                            },
                                        );

                                        match restart_sunshine_capture_with_fallback(
                                            &dynamic_display_session_token,
                                            &address,
                                            http_port,
                                        )
                                        .await
                                        {
                                            Ok(refresh_strategy) => {
                                                RECENT_CAPTURE_INIT_FAILURE_AT_MS
                                                    .store(0, Ordering::Release);
                                                mark_capture_prestart_refresh_consumed(&format!(
                                                    "proactive_exact_vdd_refresh:{refresh_strategy}"
                                                ));
                                                append_host_stream_trace(&format!(
                                                    "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=completed reason=exact_vdd_mode_changed strategy={} prepared={}x{}@{}",
                                                    dynamic_display_session_token,
                                                    sanitize_trace_value(refresh_strategy),
                                                    requested_width,
                                                    requested_height,
                                                    requested_fps
                                                ));
                                                note_host_lifecycle_phase(
                                                    "starting",
                                                    "capture_prestart_runtime_ready",
                                                );
                                                let settle_delay_ms =
                                                    if refresh_strategy == "full_restart" {
                                                        1800
                                                    } else {
                                                        900
                                                    };
                                                start_stream_settle_delay_ms = Some(
                                                    start_stream_settle_delay_ms
                                                        .unwrap_or(0)
                                                        .max(settle_delay_ms),
                                                );
                                                proactive_capture_refresh_completed = true;
                                            }
                                            Err(err) => {
                                                record_recent_capture_init_failure();
                                                append_host_stream_trace(&format!(
                                                    "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=failed reason=exact_vdd_mode_changed error={}",
                                                    dynamic_display_session_token,
                                                    sanitize_trace_value(&err)
                                                ));
                                                warn!(
                                                    "[Stream]: failed to proactively refresh Sunshine after exact VDD prepare: {err}"
                                                );
                                                let _ = server_ws_sender.send(
                                                    StreamServerMessage::DebugLog {
                                                        message: "Recovery capture sebelum start belum berhasil. Menjalankan recovery penuh host sebelum stream dilanjutkan.".to_string(),
                                                        ty: Some(LogMessageType::Recover),
                                                    },
                                                );

                                                match run_blocking_host_failure_recovery(
                                                    &dynamic_display_session_token,
                                                    FailureRecoveryStrategy::RestartRuntime,
                                                    "sunshine_capture_init_failed",
                                                    &address,
                                                    http_port,
                                                )
                                                .await
                                                {
                                                    Ok(()) => {
                                                        runtime_profile_snapshot =
                                                            read_host_capability_profile_snapshot();
                                                        runtime_startup_issue_at_init =
                                                            blocking_runtime_startup_issue(
                                                                runtime_profile_snapshot.as_ref(),
                                                            );
                                                        legacy_runtime_selected_for_session =
                                                            legacy_runtime_selected_in_snapshot(
                                                                runtime_profile_snapshot.as_ref(),
                                                            );
                                                        software_runtime_selected_for_session =
                                                            software_runtime_selected(
                                                                runtime_profile_snapshot.as_ref(),
                                                            );
                                                        preserve_legacy_vdd_surface_for_session =
                                                            dynamic_display_match
                                                                && runtime_profile_snapshot
                                                                    .as_ref()
                                                                    .is_some_and(
                                                                        capability_profile_has_virtual_display_driver,
                                                                    );
                                                        RECENT_CAPTURE_INIT_FAILURE_AT_MS
                                                            .store(0, Ordering::Release);
                                                        mark_capture_prestart_refresh_consumed(
                                                            "proactive_exact_vdd_refresh:blocking_recovery",
                                                        );
                                                        note_host_lifecycle_phase(
                                                            "starting",
                                                            "capture_prestart_runtime_ready_after_blocking_recovery",
                                                        );
                                                        start_stream_settle_delay_ms = Some(
                                                            start_stream_settle_delay_ms
                                                                .unwrap_or(0)
                                                                .max(3200),
                                                        );
                                                        proactive_capture_refresh_completed = true;
                                                    }
                                                    Err(recovery_err) => {
                                                        let _ = server_ws_sender.send(
                                                            StreamServerMessage::DebugLog {
                                                                message: "Capture host belum sehat setelah recovery penuh. Stream dibatalkan agar tidak macet.".to_string(),
                                                                ty: Some(LogMessageType::FatalDescription),
                                                            },
                                                        );
                                                        pre_connect_stop_reason = Some(format!(
                                                            "capture_prestart_runtime_refresh_failed:{}",
                                                            sanitize_trace_value(&recovery_err)
                                                        ));
                                                        note_host_lifecycle_phase(
                                                            "failed",
                                                            pre_connect_stop_reason
                                                                .as_deref()
                                                                .unwrap_or("capture_prestart_runtime_refresh_failed"),
                                                        );
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        CAPTURE_PRESTART_REFRESH_ACTIVE
                                            .store(false, Ordering::Release);
                                    }
                                }

                                if !proactive_capture_refresh_completed
                                    && let Some(recovery_reason) =
                                        pending_capture_prestart_refresh_reason(
                                            CAPTURE_INIT_PRESTART_REFRESH_WINDOW,
                                        )
                                {
                                    if exact_prepared_surface && ddx_nvenc_session {
                                        if CAPTURE_PRESTART_REFRESH_ACTIVE
                                            .compare_exchange(
                                                false,
                                                true,
                                                Ordering::AcqRel,
                                                Ordering::Acquire,
                                            )
                                            .is_ok()
                                        {
                                            append_host_stream_trace(&format!(
                                                "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=begin reason={} prepared={}x{}@{}",
                                                dynamic_display_session_token,
                                                sanitize_trace_value(&recovery_reason),
                                                requested_width,
                                                requested_height,
                                                requested_fps
                                            ));
                                            note_host_lifecycle_phase(
                                                "recovering",
                                                "capture_prestart_runtime_refresh",
                                            );
                                            let _ = server_ws_sender.send(
                                                StreamServerMessage::DebugLog {
                                                    message: "Capture Sunshine perlu disegarkan di Virtual Display yang sudah pas. Menyiapkan ulang sebelum stream dimulai.".to_string(),
                                                    ty: Some(LogMessageType::Recover),
                                                },
                                            );

                                            match restart_sunshine_capture_with_fallback(
                                                &dynamic_display_session_token,
                                                &address,
                                                http_port,
                                            )
                                            .await
                                            {
                                                Ok(refresh_strategy) => {
                                                    RECENT_CAPTURE_INIT_FAILURE_AT_MS
                                                        .store(0, Ordering::Release);
                                                    mark_capture_prestart_refresh_consumed(
                                                        &format!(
                                                            "{refresh_strategy}:{recovery_reason}"
                                                        ),
                                                    );
                                                    append_host_stream_trace(&format!(
                                                        "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=completed reason={} strategy={} prepared={}x{}@{}",
                                                        dynamic_display_session_token,
                                                        sanitize_trace_value(&recovery_reason),
                                                        sanitize_trace_value(refresh_strategy),
                                                        requested_width,
                                                        requested_height,
                                                        requested_fps
                                                    ));
                                                    note_host_lifecycle_phase(
                                                        "starting",
                                                        "capture_prestart_runtime_ready",
                                                    );
                                                    let settle_delay_ms =
                                                        if refresh_strategy == "full_restart" {
                                                            1800
                                                        } else {
                                                            900
                                                        };
                                                    start_stream_settle_delay_ms = Some(
                                                        start_stream_settle_delay_ms
                                                            .unwrap_or(0)
                                                            .max(settle_delay_ms),
                                                    );
                                                }
                                                Err(err) => {
                                                    record_recent_capture_init_failure();
                                                    append_host_stream_trace(&format!(
                                                        "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=failed reason={} error={}",
                                                        dynamic_display_session_token,
                                                        sanitize_trace_value(&recovery_reason),
                                                        sanitize_trace_value(&err)
                                                    ));
                                                    warn!(
                                                        "[Stream]: failed to prestart refresh Sunshine after capture init failure: {err}"
                                                    );
                                                    let _ = server_ws_sender.send(
                                                        StreamServerMessage::DebugLog {
                                                            message: "Recovery capture belum berhasil. Menjalankan recovery penuh host sebelum stream dilanjutkan.".to_string(),
                                                            ty: Some(LogMessageType::Recover),
                                                        },
                                                    );

                                                    match run_blocking_host_failure_recovery(
                                                        &dynamic_display_session_token,
                                                        FailureRecoveryStrategy::RestartRuntime,
                                                        "sunshine_capture_init_failed",
                                                        &address,
                                                        http_port,
                                                    )
                                                    .await
                                                    {
                                                        Ok(()) => {
                                                            runtime_profile_snapshot =
                                                                read_host_capability_profile_snapshot();
                                                            runtime_startup_issue_at_init =
                                                                blocking_runtime_startup_issue(
                                                                    runtime_profile_snapshot
                                                                        .as_ref(),
                                                                );
                                                            legacy_runtime_selected_for_session =
                                                                legacy_runtime_selected_in_snapshot(
                                                                    runtime_profile_snapshot
                                                                        .as_ref(),
                                                                );
                                                            software_runtime_selected_for_session =
                                                                software_runtime_selected(
                                                                    runtime_profile_snapshot
                                                                        .as_ref(),
                                                                );
                                                            preserve_legacy_vdd_surface_for_session =
                                                                dynamic_display_match
                                                                    && runtime_profile_snapshot
                                                                        .as_ref()
                                                                        .is_some_and(
                                                                            capability_profile_has_virtual_display_driver,
                                                                        );
                                                            RECENT_CAPTURE_INIT_FAILURE_AT_MS
                                                                .store(0, Ordering::Release);
                                                            mark_capture_prestart_refresh_consumed(
                                                                &format!(
                                                                    "blocking_recovery:{recovery_reason}"
                                                                ),
                                                            );
                                                            note_host_lifecycle_phase(
                                                                "starting",
                                                                "capture_prestart_runtime_ready_after_blocking_recovery",
                                                            );
                                                            start_stream_settle_delay_ms = Some(
                                                                start_stream_settle_delay_ms
                                                                    .unwrap_or(0)
                                                                    .max(3200),
                                                            );
                                                        }
                                                        Err(recovery_err) => {
                                                            let _ = server_ws_sender.send(
                                                                StreamServerMessage::DebugLog {
                                                                    message: "Recovery capture belum berhasil. Stream dibatalkan agar host bisa pulih dulu.".to_string(),
                                                                    ty: Some(LogMessageType::FatalDescription),
                                                                },
                                                            );
                                                            pre_connect_stop_reason = Some(
                                                                format!(
                                                                    "capture_prestart_refresh_failed:{}",
                                                                    sanitize_trace_value(
                                                                        &recovery_err
                                                                    )
                                                                ),
                                                            );
                                                            note_host_lifecycle_phase(
                                                                "failed",
                                                                pre_connect_stop_reason
                                                                    .as_deref()
                                                                    .unwrap_or("capture_prestart_refresh_failed"),
                                                            );
                                                            CAPTURE_PRESTART_REFRESH_ACTIVE
                                                                .store(false, Ordering::Release);
                                                            break;
                                                        }
                                                    }
                                                }
                                            }

                                            CAPTURE_PRESTART_REFRESH_ACTIVE
                                                .store(false, Ordering::Release);
                                        } else {
                                            append_host_stream_trace(&format!(
                                                "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=skip_already_running reason={}",
                                                dynamic_display_session_token,
                                                sanitize_trace_value(&recovery_reason)
                                            ));
                                        }
                                    } else {
                                        append_host_stream_trace(&format!(
                                            "GOLDEN_PATH_PRESTART_CAPTURE_REFRESH token={} stage=skip reason={} exact_prepared_surface={} ddx_nvenc_session={}",
                                            dynamic_display_session_token,
                                            sanitize_trace_value(&recovery_reason),
                                            exact_prepared_surface,
                                            ddx_nvenc_session
                                        ));
                                    }
                                }

                                let _ = server_ws_sender.send(
                                    StreamServerMessage::DisplayModeApplied {
                                        phase: DisplayModePhase::Prepare,
                                        width: (*width).max(1),
                                        height: (*height).max(1),
                                        fps: (*fps).max(1),
                                        changed: result.changed,
                                        skipped: result.skipped,
                                    },
                                );

                                if should_apply_start_stream_settle_delay() {
                                    start_stream_settle_delay_ms =
                                        Some(if legacy_runtime_selected_for_session {
                                            if result.changed {
                                                let applied_height = result
                                                    .applied
                                                    .as_ref()
                                                    .map(|mode| mode.height)
                                                    .unwrap_or(*height);
                                                let applied_width = result
                                                    .applied
                                                    .as_ref()
                                                    .map(|mode| mode.width)
                                                    .unwrap_or(*width);
                                                if applied_height >= 1200 || applied_width >= 1200 {
                                                    1600
                                                } else {
                                                    1200
                                                }
                                            } else {
                                                500
                                            }
                                        } else if result.changed {
                                            600
                                        } else {
                                            300
                                        });
                                }
                            }
                            Err((err, display_route_issue)) => {
                                let user_message = build_display_prepare_failure_message(
                                    &err,
                                    display_route_issue.as_deref(),
                                );
                                let allow_display_fallback =
                                    should_fallback_after_display_prepare_failure(
                                        &err,
                                        display_route_issue.as_deref(),
                                    );
                                warn!("[Stream]: display prepare failed: {err}");
                                if allow_display_fallback {
                                    append_host_stream_trace(&format!(
                                        "DISPLAY_PREPARE_FALLBACK token={} error={} route_issue={}",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(&err),
                                        sanitize_trace_value(
                                            display_route_issue.as_deref().unwrap_or("none"),
                                        )
                                    ));
                                    start_stream_settle_delay_ms.get_or_insert(
                                        if legacy_runtime_selected_for_session {
                                            500
                                        } else {
                                            250
                                        },
                                    );
                                    let _ =
                                        server_ws_sender.send(StreamServerMessage::DebugLog {
                                            message: format!(
                                                "{} Melanjutkan stream memakai display aktif host sebagai fallback.",
                                                user_message
                                            ),
                                            ty: Some(LogMessageType::Recover),
                                        });
                                } else {
                                    append_host_stream_trace(&format!(
                                        "DISPLAY_PREPARE_FAILED token={} error={} route_issue={}",
                                        dynamic_display_session_token,
                                        sanitize_trace_value(&err),
                                        sanitize_trace_value(
                                            display_route_issue.as_deref().unwrap_or("none"),
                                        )
                                    ));
                                    pre_connect_stop_reason = Some(format!(
                                        "display_prepare_failed:{}",
                                        sanitize_trace_value(
                                            display_route_issue.as_deref().unwrap_or(err.as_str()),
                                        )
                                    ));
                                    note_host_lifecycle_phase(
                                        "failed",
                                        pre_connect_stop_reason
                                            .as_deref()
                                            .unwrap_or("display_prepare_failed"),
                                    );
                                    let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                        message: user_message,
                                        ty: Some(LogMessageType::FatalDescription),
                                    });
                                    break;
                                }
                            }
                        }

                        if let Some(delay_ms) = start_stream_settle_delay_ms {
                            append_host_stream_trace(&format!(
                                "DISPLAY_PREPARE_SETTLE token={} delay_ms={}",
                                dynamic_display_session_token, delay_ms
                            ));
                            sleep(Duration::from_millis(delay_ms)).await;
                        }
                    }

                    if let StreamClientMessage::ResizeStream { width, height, fps } = &mut message {
                        let requested_width = *width;
                        let requested_height = *height;
                        let requested_fps = *fps;

                        match apply_dynamic_display_match(
                            &dynamic_display_session_token,
                            requested_width,
                            requested_height,
                            requested_fps,
                        )
                        .await
                        {
                            Ok(result) => {
                                if let Some(applied) = result.applied.as_ref() {
                                    if should_preserve_requested_stream_surface_after_display_fallback(
                                        runtime_profile_snapshot.as_ref(),
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                        &result.reason,
                                    ) {
                                        append_host_stream_trace(&format!(
                                            "NVENC_KEEP_RESIZE_STREAM_SURFACE token={} requested={}x{} applied={}x{} reason={}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                            sanitize_trace_value(&result.reason)
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else if legacy_runtime_selected_for_session
                                        && should_preserve_legacy_nvenc_stream_surface(
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                        )
                                    {
                                        append_host_stream_trace(&format!(
                                            "LEGACY_NVENC_KEEP_RESIZE_SURFACE token={} requested={}x{} applied={}x{}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else {
                                        *width = applied.width.max(1);
                                        *height = applied.height.max(1);
                                    }
                                    cap_stream_fps_to_applied_refresh(fps, applied.frequency);
                                }

                                if result.changed {
                                    dynamic_display_match_active.store(true, Ordering::Release);
                                    prepared_display_active.store(true, Ordering::Release);
                                    info!(
                                        "[Stream]: runtime display resize applied {}x{}@{} -> {}x{}@{} ({})",
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        if result.reason.is_empty() {
                                            "ok"
                                        } else {
                                            &result.reason
                                        }
                                    );
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_RUNTIME token={} requested={}x{}@{} applied={}x{}@{} changed=true skipped=false reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "ok"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                } else if result.skipped {
                                    info!(
                                        "[Stream]: runtime display resize skipped for {}x{}@{} -> {}x{}@{} ({})",
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        if result.reason.is_empty() {
                                            "no supported display mode change was needed"
                                        } else {
                                            &result.reason
                                        }
                                    );
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_RUNTIME token={} requested={}x{}@{} applied={}x{}@{} changed=false skipped=true reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "no supported display mode change was needed"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                } else {
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_RUNTIME token={} requested={}x{}@{} applied={}x{}@{} changed=false skipped=false reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "unknown"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                }

                                let _ = server_ws_sender.send(
                                    StreamServerMessage::DisplayModeApplied {
                                        phase: DisplayModePhase::RuntimeResize,
                                        width: (*width).max(1),
                                        height: (*height).max(1),
                                        fps: (*fps).max(1),
                                        changed: result.changed,
                                        skipped: result.skipped,
                                    },
                                );
                            }
                            Err(err) => {
                                warn!("[Stream]: runtime display resize failed: {err}");
                                append_host_stream_trace(&format!(
                                    "DYNAMIC_DISPLAY_RUNTIME_FAILED token={} requested={}x{}@{} error={}",
                                    dynamic_display_session_token,
                                    requested_width,
                                    requested_height,
                                    requested_fps,
                                    sanitize_trace_value(&err)
                                ));
                                let _ = server_ws_sender.send(StreamServerMessage::DebugLog {
                                    message: format!(
                                        "Layar stream belum siap diubah ke {}x{}@{}. Mempertahankan mode sebelumnya.",
                                        requested_width,
                                        requested_height,
                                        requested_fps
                                    ),
                                    ty: Some(LogMessageType::Recover),
                                });
                                continue;
                            }
                        }
                    }

                    if dynamic_display_match_requested
                        && let StreamClientMessage::StartStream {
                            width, height, fps, ..
                        } = &mut message
                    {
                        dynamic_display_match_requested = false;
                        let requested_width = *width;
                        let requested_height = *height;
                        let requested_fps = *fps;

                        match apply_dynamic_display_match(
                            &dynamic_display_session_token,
                            requested_width,
                            requested_height,
                            requested_fps,
                        )
                        .await
                        {
                            Ok(result) => {
                                if let Some(applied) = result.applied.as_ref() {
                                    if should_preserve_requested_stream_surface_after_display_fallback(
                                        runtime_profile_snapshot.as_ref(),
                                        requested_width,
                                        requested_height,
                                        applied.width,
                                        applied.height,
                                        &result.reason,
                                    ) {
                                        append_host_stream_trace(&format!(
                                            "NVENC_KEEP_DYNAMIC_START_STREAM_SURFACE token={} requested={}x{} applied={}x{} reason={}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                            sanitize_trace_value(&result.reason)
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else if legacy_runtime_selected_for_session
                                        && should_preserve_legacy_nvenc_stream_surface(
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height,
                                        )
                                    {
                                        append_host_stream_trace(&format!(
                                            "LEGACY_NVENC_KEEP_DYNAMIC_START_SURFACE token={} requested={}x{} applied={}x{}",
                                            dynamic_display_session_token,
                                            requested_width,
                                            requested_height,
                                            applied.width,
                                            applied.height
                                        ));
                                        *width = requested_width.max(1);
                                        *height = requested_height.max(1);
                                    } else {
                                        *width = applied.width.max(1);
                                        *height = applied.height.max(1);
                                    }
                                    cap_stream_fps_to_applied_refresh(fps, applied.frequency);
                                }

                                if result.changed {
                                    dynamic_display_match_active.store(true, Ordering::Release);
                                    prepared_display_active.store(true, Ordering::Release);
                                    info!(
                                        "[Stream]: dynamic display match applied {}x{}@{} -> {}x{}@{} ({})",
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        if result.reason.is_empty() {
                                            "ok"
                                        } else {
                                            &result.reason
                                        }
                                    );
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_START token={} requested={}x{}@{} applied={}x{}@{} changed=true skipped=false reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "ok"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                } else if result.skipped {
                                    info!(
                                        "[Stream]: dynamic display match skipped for {}x{}@{} ({})",
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        if result.reason.is_empty() {
                                            "no supported display mode change was needed"
                                        } else {
                                            &result.reason
                                        }
                                    );
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_START token={} requested={}x{}@{} applied={}x{}@{} changed=false skipped=true reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "no supported display mode change was needed"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                } else {
                                    append_host_stream_trace(&format!(
                                        "DYNAMIC_DISPLAY_START token={} requested={}x{}@{} applied={}x{}@{} changed=false skipped=false reason={}",
                                        dynamic_display_session_token,
                                        requested_width,
                                        requested_height,
                                        requested_fps,
                                        *width,
                                        *height,
                                        *fps,
                                        sanitize_trace_value(if result.reason.is_empty() {
                                            "unknown"
                                        } else {
                                            &result.reason
                                        })
                                    ));
                                }
                            }
                            Err(err) => {
                                warn!("[Stream]: dynamic display match failed: {err}");
                                append_host_stream_trace(&format!(
                                    "DYNAMIC_DISPLAY_START_FAILED token={} requested={}x{}@{} error={}",
                                    dynamic_display_session_token,
                                    requested_width,
                                    requested_height,
                                    requested_fps,
                                    sanitize_trace_value(&err)
                                ));
                            }
                        }
                    }

                    if matches!(message, StreamClientMessage::StartStream { .. })
                        && !stream_connection_established.load(Ordering::Acquire)
                    {
                        match timeout(
                            Duration::from_millis(GOLDEN_PATH_CONTROL_PROBE_TIMEOUT_MS),
                            stream.recv(),
                        )
                        .await
                        {
                            Ok(Some(Ok(Message::Close(reason)))) => {
                                ws_close_logged = true;
                                let reason_text = reason
                                    .as_ref()
                                    .map(|reason| {
                                        let description = sanitize_trace_value(
                                            reason.description.as_deref().unwrap_or(""),
                                        );
                                        if description.is_empty() {
                                            format!("code={:?}", reason.code)
                                        } else {
                                            format!("code={:?} desc={description}", reason.code)
                                        }
                                    })
                                    .unwrap_or_else(|| "none".to_string());
                                append_host_stream_trace(&format!(
                                    "WS_CLOSE token={} start_stream_received={} connection_established=false reason={} checkpoint=before_start_forward",
                                    dynamic_display_session_token,
                                    start_stream_received.load(Ordering::Acquire),
                                    reason_text
                                ));
                                append_host_stream_trace(&format!(
                                    "GOLDEN_PATH_ABORT token={} reason=control_channel_closed_before_start_forward detail={}",
                                    dynamic_display_session_token,
                                    sanitize_trace_value(&reason_text)
                                ));
                                remember_recent_stream_close(&reason_text, false).await;
                                pre_connect_stop_reason =
                                    Some("control_channel_closed_before_start_forward".to_string());
                                break;
                            }
                            Ok(Some(Ok(probe_message))) => {
                                pending_control_message = Some(probe_message);
                            }
                            Ok(Some(Err(err))) => {
                                warn!(
                                    "[Stream]: websocket receive failed before forwarding StartStream: {err}"
                                );
                                append_host_stream_trace(&format!(
                                    "WS_RECV_FAILED token={} error={} checkpoint=before_start_forward",
                                    dynamic_display_session_token,
                                    sanitize_trace_value(&err.to_string())
                                ));
                                append_host_stream_trace(&format!(
                                    "GOLDEN_PATH_ABORT token={} reason=control_channel_receive_failed_before_start_forward error={}",
                                    dynamic_display_session_token,
                                    sanitize_trace_value(&err.to_string())
                                ));
                                pre_connect_stop_reason = Some(
                                    "control_channel_receive_failed_before_start_forward"
                                        .to_string(),
                                );
                                break;
                            }
                            Ok(None) => {
                                append_host_stream_trace(&format!(
                                    "GOLDEN_PATH_ABORT token={} reason=control_channel_ended_before_start_forward",
                                    dynamic_display_session_token
                                ));
                                pre_connect_stop_reason =
                                    Some("control_channel_ended_before_start_forward".to_string());
                                break;
                            }
                            Err(_) => {}
                        }

                        if let StreamClientMessage::StartStream {
                            width, height, fps, ..
                        } = &message
                        {
                            append_host_stream_trace(&format!(
                                "GOLDEN_PATH_START_FORWARDED token={} stream={}x{}@{}",
                                dynamic_display_session_token, width, height, fps
                            ));
                        }
                    }

                    ipc_sender.send(ServerIpcMessage::WebSocket(message)).await;
                }
                Message::Binary(binary) => {
                    ipc_sender
                        .send(ServerIpcMessage::WebSocketTransport(binary))
                        .await;
                }
                Message::Close(reason) => {
                    ws_close_logged = true;
                    let connection_established =
                        stream_connection_established.load(Ordering::Acquire);
                    let reason_text = reason
                        .as_ref()
                        .map(|reason| {
                            let description =
                                sanitize_trace_value(reason.description.as_deref().unwrap_or(""));
                            if description.is_empty() {
                                format!("code={:?}", reason.code)
                            } else {
                                format!("code={:?} desc={description}", reason.code)
                            }
                        })
                        .unwrap_or_else(|| "none".to_string());
                    append_host_stream_trace(&format!(
                        "WS_CLOSE token={} start_stream_received={} connection_established={} reason={}",
                        dynamic_display_session_token,
                        start_stream_received.load(Ordering::Acquire),
                        connection_established,
                        reason_text
                    ));
                    if connection_established
                        && let Some(lifecycle_reason) =
                            lifecycle_recovery_reason_from_close_reason(&reason_text)
                    {
                        note_host_lifecycle_phase("recovering", lifecycle_reason);
                    }
                    remember_recent_stream_close(&reason_text, connection_established).await;
                    debug!(
                        "[Stream]: websocket closed by client (start_stream_received={}, connection_established={}, reason={:?})",
                        start_stream_received.load(Ordering::Acquire),
                        connection_established,
                        reason
                    );
                    break;
                }
                Message::Ping(bytes) => {
                    let _ = session.pong(&bytes).await;
                }
                _ => {}
            }
        }

        if !ws_close_logged {
            append_host_stream_trace(&format!(
                "WS_STREAM_ENDED token={} start_stream_received={} connection_established={}",
                dynamic_display_session_token,
                start_stream_received.load(Ordering::Acquire),
                stream_connection_established.load(Ordering::Acquire)
            ));
        }

        if stream_connection_established.load(Ordering::Acquire) {
            append_host_stream_trace(&format!(
                "CONTROL_CHANNEL_CLOSED_AFTER_CONNECT token={}",
                dynamic_display_session_token
            ));
            info!(
                "[Stream]: websocket control channel closed after connection; stopping staged streamer"
            );
            let _ = ipc_sender.send(ServerIpcMessage::Stop).await;
            stop_active_window_watch(
                &dynamic_display_session_token,
                "post_connect_control_channel_closed",
            )
            .await;
            restore_runtime_dynamic_display_if_active(
                &dynamic_display_match_active,
                &dynamic_display_session_token,
            )
            .await;
            if should_skip_prepared_stream_restore_after_session_cleanup() {
                let _ = prepared_display_active.swap(false, Ordering::AcqRel);
                append_host_stream_trace(&format!(
                    "DISPLAY_RESTORE_BEST_EFFORT_SKIPPED token={} stage=post_connect_control_channel_closed reason=mtt_vdd_cleanup_invariant",
                    dynamic_display_session_token
                ));
            } else {
                restore_prepared_stream_display_if_active(
                    &prepared_display_active,
                    &dynamic_display_session_token,
                )
                .await;
                restore_prepared_stream_display_best_effort(
                    &dynamic_display_session_token,
                    "post_connect_control_channel_closed",
                )
                .await;
            }
            sleep(Duration::from_millis(500)).await;
            force_kill_child(&child, "Stream").await;
            cleanup_staged_child_binary(&staged_streamer_path);
            if let Some(window_watch_child) = current_window_watch_child.as_ref() {
                force_kill_child(window_watch_child, "Window Watch").await;
            }
            note_host_lifecycle_phase(
                "ready",
                "post_connect_control_channel_closed_cleanup_completed",
            );
        } else {
            let stop_before_connect_reason = pre_connect_stop_reason
                .clone()
                .unwrap_or_else(|| "session_stopped_before_connect".to_string());
            append_host_stream_trace(&format!(
                "STOP_BEFORE_CONNECT token={} start_stream_received={} reason={}",
                dynamic_display_session_token,
                start_stream_received.load(Ordering::Acquire),
                sanitize_trace_value(&stop_before_connect_reason)
            ));
            note_host_lifecycle_phase("failed", &stop_before_connect_reason);
            if pre_connect_stop_reason.is_none() {
                maybe_schedule_host_failure_recovery("session_stopped_before_connect");
            }
            warn!(
                "[Stream]: stopping before connection completed (start_stream_received={}, reason={})",
                start_stream_received.load(Ordering::Acquire),
                stop_before_connect_reason
            );

            if pre_connect_stop_reason.is_none()
                && legacy_runtime_selected_for_session
                && start_stream_received.load(Ordering::Acquire)
            {
                let failure_count = note_legacy_runtime_startup_failure().await;
                append_host_stream_trace(&format!(
                    "LEGACY_RUNTIME_STARTUP_FAILURE token={} stage=start_stream failure_count={} reason=legacy_start_stream_no_connection_complete",
                    dynamic_display_session_token, failure_count
                ));
                if failure_count >= 2 {
                    maybe_schedule_legacy_runtime_auto_fallback(
                        &dynamic_display_session_token,
                        "legacy_start_stream_no_connection_complete",
                    );
                }
            }

            ipc_sender.send(ServerIpcMessage::Stop).await;

            stop_active_window_watch(
                &dynamic_display_session_token,
                "pre_connect_stop_before_display_restore",
            )
            .await;
            restore_runtime_dynamic_display_if_active(
                &dynamic_display_match_active,
                &dynamic_display_session_token,
            )
            .await;
            restore_prepared_stream_display_if_active(
                &prepared_display_active,
                &dynamic_display_session_token,
            )
            .await;
            if start_stream_received.load(Ordering::Acquire) {
                restore_prepared_stream_display_best_effort(
                    &dynamic_display_session_token,
                    "pre_connect_stream_stopped",
                )
                .await;
            }

            let child = child.clone();
            spawn(async move {
                sleep(Duration::from_secs(3)).await;
                force_kill_child(&child, "Stream").await;
                if let Some(window_watch_child) = {
                    let mut slot = ACTIVE_WINDOW_WATCH_CHILD.lock().await;
                    slot.take()
                } {
                    force_kill_child(&window_watch_child, "Window Watch").await;
                }
            });
        }
    });

    Ok(response)
}

#[get("/android-native/shared-session/player2/ws")]
pub async fn shared_session_player2_ws(
    web_app: Data<App>,
    request: HttpRequest,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    let invite_token =
        actix_web::web::Query::<SharedPlayer2InviteQuery>::from_query(request.query_string())
            .ok()
            .and_then(|query| {
                query
                    .invite_token_camel
                    .clone()
                    .or_else(|| query.invite_token.clone())
            })
            .or_else(|| extract_shared_player2_invite_token(&request));

    let Some(invite_token) = invite_token.filter(|value| !value.trim().is_empty()) else {
        return Ok(HttpResponse::BadRequest().body("missing invite token"));
    };

    let invite = match web_app
        .consume_android_native_shared_session_invite(&invite_token)
        .await
    {
        Ok(invite) => invite,
        Err(AppError::AndroidNativeSharedSessionInviteNotFound) => {
            return Ok(HttpResponse::NotFound().body("shared session invite not found"));
        }
        Err(AppError::AndroidNativeSharedSessionInviteExpired) => {
            return Ok(HttpResponse::Unauthorized().body("shared session invite expired"));
        }
        Err(err) => {
            warn!("[SharedPlayer2]: failed to consume invite: {err}");
            return Ok(HttpResponse::InternalServerError().finish());
        }
    };

    if invite.role != common::api_bindings::AndroidNativeSharedSessionRole::Player2 {
        return Ok(HttpResponse::Forbidden().body("shared session role is not player2"));
    }

    if !web_app
        .android_native_owner_session_is_active(&invite.owner_session_id)
        .await
    {
        return Ok(HttpResponse::Conflict().body("owner session is no longer active"));
    }

    let Some(bridge) =
        get_shared_player2_bridge(&invite.owner_session_id, invite.host_id, invite.app_id).await
    else {
        return Ok(HttpResponse::Conflict()
            .body("player2 attach is waiting for the owner stream to be ready"));
    };

    let (response, mut session, mut stream) = actix_ws::handle(&request, payload)?;
    let web_app = web_app.clone();
    actix_rt::spawn(async move {
        let connection_id = Uuid::new_v4().to_string();
        let owner_session_id = invite.owner_session_id.clone();
        let host_id = invite.host_id;
        let app_id = invite.app_id;

        append_host_stream_trace(&format!(
            "SHARED_PLAYER2_WS_ACCEPTED token={} owner_session_id={} host_id={} app_id={} connection_id={}",
            sanitize_trace_value(&bridge.runtime_session_token),
            sanitize_trace_value(&owner_session_id),
            host_id.0,
            app_id.0,
            sanitize_trace_value(&connection_id)
        ));

        claim_shared_player2_joiner(&owner_session_id, &connection_id).await;
        send_shared_player2_transport_bytes(
            &bridge,
            build_shared_player2_controller_disconnected_packet(),
        )
        .await;

        let _ = send_json_ws_message(
            &mut session,
            SharedPlayer2ServerMessage::Ready {
                owner_session_id: owner_session_id.clone(),
                host_id: host_id.0,
                app_id: app_id.0,
                connection_id: connection_id.clone(),
            },
        )
        .await;
        let _ = send_json_ws_message(
            &mut session,
            SharedPlayer2ServerMessage::Status {
                message: "Player 2 pad is attached. Connect one controller on this device."
                    .to_string(),
            },
        )
        .await;

        loop {
            let next_message = match timeout(STREAM_CONTROL_IDLE_TIMEOUT, stream.recv()).await {
                Ok(message) => message,
                Err(_) => {
                    if !shared_player2_joiner_is_current(&owner_session_id, &connection_id).await {
                        break;
                    }
                    if !web_app
                        .android_native_owner_session_is_active(&owner_session_id)
                        .await
                    {
                        break;
                    }
                    let _ = send_json_ws_message(
                        &mut session,
                        SharedPlayer2ServerMessage::Status {
                            message: "Waiting for controller input...".to_string(),
                        },
                    )
                    .await;
                    continue;
                }
            };

            let Some(message) = next_message else {
                break;
            };

            let message = match message {
                Ok(message) => message,
                Err(err) => {
                    warn!("[SharedPlayer2]: websocket receive failed: {err}");
                    break;
                }
            };

            if !shared_player2_joiner_is_current(&owner_session_id, &connection_id).await {
                let _ = send_json_ws_message(
                    &mut session,
                    SharedPlayer2ServerMessage::Error {
                        message: "This Player 2 link was replaced by a newer device connection."
                            .to_string(),
                    },
                )
                .await;
                break;
            }

            if !web_app
                .android_native_owner_session_is_active(&owner_session_id)
                .await
            {
                let _ = send_json_ws_message(
                    &mut session,
                    SharedPlayer2ServerMessage::Error {
                        message: "Owner session is no longer active.".to_string(),
                    },
                )
                .await;
                break;
            }

            let Some(active_bridge) =
                get_shared_player2_bridge(&owner_session_id, host_id, app_id).await
            else {
                let _ = send_json_ws_message(
                    &mut session,
                    SharedPlayer2ServerMessage::Error {
                        message: "Owner stream is not ready for Player 2 attach right now."
                            .to_string(),
                    },
                )
                .await;
                break;
            };

            match message {
                Message::Text(text) => {
                    let Ok(client_message) =
                        serde_json::from_str::<SharedPlayer2ClientMessage>(&text)
                    else {
                        let _ = send_json_ws_message(
                            &mut session,
                            SharedPlayer2ServerMessage::Error {
                                message: "Invalid Player 2 message.".to_string(),
                            },
                        )
                        .await;
                        continue;
                    };

                    match client_message {
                        SharedPlayer2ClientMessage::ControllerConnected {
                            controller_type,
                            supported_buttons,
                            capabilities,
                        } => {
                            append_host_stream_trace(&format!(
                                "SHARED_PLAYER2_CONTROLLER_CONNECTED token={} owner_session_id={} connection_id={} controller_type={} supported_buttons={} capabilities={}",
                                sanitize_trace_value(&active_bridge.runtime_session_token),
                                sanitize_trace_value(&owner_session_id),
                                sanitize_trace_value(&connection_id),
                                controller_type,
                                supported_buttons,
                                capabilities
                            ));
                            send_shared_player2_transport_bytes(
                                &active_bridge,
                                build_shared_player2_controller_connected_packet(
                                    controller_type,
                                    supported_buttons,
                                    capabilities,
                                ),
                            )
                            .await;
                        }
                        SharedPlayer2ClientMessage::ControllerState {
                            button_flags,
                            left_trigger,
                            right_trigger,
                            left_stick_x,
                            left_stick_y,
                            right_stick_x,
                            right_stick_y,
                        } => {
                            send_shared_player2_transport_bytes(
                                &active_bridge,
                                build_shared_player2_controller_state_packet(
                                    button_flags,
                                    left_trigger,
                                    right_trigger,
                                    left_stick_x,
                                    left_stick_y,
                                    right_stick_x,
                                    right_stick_y,
                                ),
                            )
                            .await;
                        }
                        SharedPlayer2ClientMessage::ControllerDisconnected => {
                            send_shared_player2_transport_bytes(
                                &active_bridge,
                                build_shared_player2_controller_disconnected_packet(),
                            )
                            .await;
                        }
                        SharedPlayer2ClientMessage::Heartbeat => {
                            let _ = send_json_ws_message(
                                &mut session,
                                SharedPlayer2ServerMessage::Status {
                                    message: "Player 2 link is still attached.".to_string(),
                                },
                            )
                            .await;
                        }
                    }
                }
                Message::Binary(_) => {}
                Message::Close(_) => break,
                Message::Ping(bytes) => {
                    let _ = session.pong(&bytes).await;
                }
                Message::Pong(_) => {}
                _ => {}
            }
        }

        send_shared_player2_disconnect_if_ready(&owner_session_id, host_id, app_id).await;
        clear_shared_player2_joiner(&owner_session_id, Some(&connection_id)).await;
        let _ = session.close(None).await;
        append_host_stream_trace(&format!(
            "SHARED_PLAYER2_WS_CLOSED token={} owner_session_id={} connection_id={}",
            sanitize_trace_value(&bridge.runtime_session_token),
            sanitize_trace_value(&owner_session_id),
            sanitize_trace_value(&connection_id)
        ));
    });

    Ok(response)
}

#[get("/host/mic")]
#[instrument(name = "start_host_mic", skip(web_app, auth, payload))]
pub async fn start_host_mic(
    web_app: Data<App>,
    auth: UserAuth,
    request: HttpRequest,
    payload: Payload,
) -> Result<HttpResponse, Error> {
    let (response, mut session, mut stream) = actix_ws::handle(&request, payload)?;
    append_host_stream_trace("MIC_SIDECAR_WS_ACCEPTED");

    let web_app = web_app.clone();
    actix_rt::spawn(async move {
        let host_id = loop {
            let message = match stream.recv().await {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Binary(_))) => return,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return,
                None => return,
            };

            let message = match serde_json::from_str::<MicSidecarClientMessage>(&message) {
                Ok(value) => value,
                Err(err) => {
                    append_host_stream_trace(&format!(
                        "MIC_SIDECAR_INIT_PARSE_FAILED err={}",
                        sanitize_trace_value(&err.to_string())
                    ));
                    return;
                }
            };

            match message {
                MicSidecarClientMessage::Init { host_id } => break host_id,
                MicSidecarClientMessage::Heartbeat { .. } => {
                    append_host_stream_trace("MIC_SIDECAR_INIT_WAIT_HEARTBEAT");
                    continue;
                }
                other => {
                    append_host_stream_trace(&format!(
                        "MIC_SIDECAR_INIT_MISSING {}",
                        describe_mic_sidecar_client_message(&other)
                    ));
                    let _ = session.close(None).await;
                    warn!("Mic WebSocket didn't send init before signaling, closing it");
                    return;
                }
            }
        };
        append_host_stream_trace(&format!("MIC_SIDECAR_INIT host_id={host_id}"));

        let auth_error = if let Some(stream_ticket) = extract_android_native_stream_ticket(&request)
            .or_else(|| extract_android_native_stream_ticket_query(&request))
        {
            web_app
                .authorize_android_native_stream_ticket_for_host(&stream_ticket, HostId(host_id))
                .await
                .map(|_| ())
        } else {
            match web_app.user_by_auth(auth).await {
                Ok(mut user) => user.host(HostId(host_id)).await.map(|_| ()),
                Err(err) => Err(err),
            }
        };

        if let Err(err) = auth_error {
            warn!("failed to authorize mic sidecar for host {host_id:?}: {err}");
            append_host_stream_trace(&format!(
                "MIC_SIDECAR_AUTH_FAILED host_id={host_id} err={err}"
            ));
            let _ = send_json_ws_message(
                &mut session,
                MicSidecarServerMessage::DebugLog {
                    message: format!(
                        "Failed to start mic sidecar because authorization failed: {err}"
                    ),
                    ty: Some(LogMessageType::FatalDescription),
                },
            )
            .await;
            let _ = session.close(None).await;
            return;
        }
        append_host_stream_trace(&format!("MIC_SIDECAR_AUTH_OK host_id={host_id}"));

        let _ = send_json_ws_message(
            &mut session,
            MicSidecarServerMessage::DebugLog {
                message: "Launching mic sidecar".to_string(),
                ty: None,
            },
        )
        .await;

        let mic_sidecar_session_token = Uuid::new_v4().to_string();
        let staged_mic_sidecar_path =
            match resolve_runtime_binary_path(&web_app.config().mic_sidecar_path)
                .and_then(|path| stage_child_binary(&path, &mic_sidecar_session_token))
            {
                Ok(path) => path,
                Err(err) => {
                    error!("[Mic Sidecar]: failed to stage sidecar process: {err}");
                    append_host_stream_trace(&format!("MIC_SIDECAR_STAGE_FAILED err={err}"));
                    let _ = send_json_ws_message(
                        &mut session,
                        MicSidecarServerMessage::DebugLog {
                            message: "Failed to start mic sidecar because of a server error"
                                .to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    return;
                }
            };
        debug!(
            "[Mic Sidecar]: staged child binary at {}",
            staged_mic_sidecar_path.display()
        );

        let (mut child, stdin, stdout) = match Command::new(&staged_mic_sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.take()
                    && let Some(stdout) = child.stdout.take()
                {
                    (child, stdin, stdout)
                } else {
                    error!("[Mic Sidecar]: process didn't include a stdin or stdout");
                    append_host_stream_trace("MIC_SIDECAR_SPAWN_FAILED missing_stdio");
                    let _ = send_json_ws_message(
                        &mut session,
                        MicSidecarServerMessage::DebugLog {
                            message: "Failed to start mic sidecar because of a server error"
                                .to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    )
                    .await;
                    let _ = session.close(None).await;
                    if let Err(err) = child.kill().await {
                        warn!("[Mic Sidecar]: failed to kill child: {err}");
                    }
                    cleanup_staged_child_binary(&staged_mic_sidecar_path);
                    return;
                }
            }
            Err(err) => {
                error!("[Mic Sidecar]: failed to spawn sidecar process: {err}");
                append_host_stream_trace(&format!("MIC_SIDECAR_SPAWN_FAILED err={err}"));
                let _ = send_json_ws_message(
                    &mut session,
                    MicSidecarServerMessage::DebugLog {
                        message: "Failed to start mic sidecar because of a server error"
                            .to_string(),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                )
                .await;
                let _ = session.close(None).await;
                cleanup_staged_child_binary(&staged_mic_sidecar_path);
                return;
            }
        };
        append_host_stream_trace(&format!(
            "MIC_SIDECAR_SPAWNED path={}",
            staged_mic_sidecar_path.display()
        ));

        static MIC_CHILD_COUNTER: AtomicUsize = AtomicUsize::new(10_000);
        let id = MIC_CHILD_COUNTER.fetch_add(1, Ordering::Relaxed);
        let span = span!(Level::INFO, "mic_ipc", child_id = id);

        let (mut ipc_sender, mut ipc_receiver) = create_child_ipc::<
            MicSidecarServerIpcMessage,
            MicSidecarIpcMessage,
        >(span, stdin, stdout, child.stderr.take())
        .await;
        let child = Arc::new(Mutex::new(child));
        replace_active_child(&ACTIVE_MIC_SIDECAR_CHILD, child.clone(), "Mic Sidecar").await;

        let mut write_session = session.clone();
        spawn({
            let mut ipc_sender = ipc_sender.clone();
            let child = child.clone();
            let staged_mic_sidecar_path = staged_mic_sidecar_path.clone();
            async move {
                let mut warned_closed = false;
                while let Some(message) = ipc_receiver.recv().await {
                    match message {
                        MicSidecarIpcMessage::WebSocket(message) => {
                            append_host_stream_trace(&format!(
                                "MIC_SIDECAR_WS_TX {}",
                                describe_mic_sidecar_server_message(&message)
                            ));
                            if let Err(Closed) = send_json_ws_message(&mut write_session, message).await
                                && !warned_closed
                            {
                                warn!(
                                    "[Mic Sidecar]: Tried to send a ws message but the socket is already closed"
                                );
                                ipc_sender.send(MicSidecarServerIpcMessage::Stop).await;
                                warned_closed = true;
                            }
                        }
                        MicSidecarIpcMessage::Stop => {
                            debug!("[Mic Sidecar]: ipc receiver stopped by child");
                            break;
                        }
                    }
                }

                if let Err(err) = write_session.close(None).await {
                    warn!("[Mic Sidecar]: failed to close websocket: {err}");
                }
                force_kill_child(&child, "Mic Sidecar").await;
                cleanup_staged_child_binary(&staged_mic_sidecar_path);
            }
        });

        ipc_sender
            .send(MicSidecarServerIpcMessage::Init {
                config: StreamerConfig {
                    webrtc: web_app.config().webrtc.clone(),
                    log_level: web_app.config().log.level_filter,
                },
            })
            .await;

        while let Some(Ok(message)) = stream.recv().await {
            match message {
                Message::Text(text) => {
                    let Ok(message) = serde_json::from_str::<MicSidecarClientMessage>(&text) else {
                        warn!("[Mic Sidecar]: failed to deserialize from json");
                        append_host_stream_trace("MIC_SIDECAR_WS_IGNORED_UNPARSEABLE");
                        continue;
                    };

                    append_host_stream_trace(&format!(
                        "MIC_SIDECAR_WS_RX {}",
                        describe_mic_sidecar_client_message(&message)
                    ));

                    if matches!(message, MicSidecarClientMessage::Heartbeat { .. }) {
                        continue;
                    }

                    ipc_sender
                        .send(MicSidecarServerIpcMessage::WebSocket(message))
                        .await;
                }
                Message::Binary(_) => {}
                Message::Ping(bytes) => {
                    let _ = session.pong(&bytes).await;
                }
                _ => {}
            }
        }

        ipc_sender.send(MicSidecarServerIpcMessage::Stop).await;

        let child = child.clone();
        spawn(async move {
            sleep(Duration::from_secs(3)).await;
            force_kill_child(&child, "Mic Sidecar").await;
        });
    });

    Ok(response)
}

async fn send_ws_message(sender: &mut Session, message: StreamServerMessage) -> Result<(), Closed> {
    send_json_ws_message(sender, message).await
}

async fn send_json_ws_message<T: serde::Serialize>(
    sender: &mut Session,
    message: T,
) -> Result<(), Closed> {
    let Some(json) = serialize_json(&message) else {
        return Ok(());
    };

    sender.text(json).await
}

#[post("/host/cancel")]
pub async fn cancel_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostCancelRequest>,
) -> Result<Json<PostCancelResponse>, AppError> {
    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    host.cancel_app(&mut user).await?;

    Ok(Json(PostCancelResponse { success: true }))
}
