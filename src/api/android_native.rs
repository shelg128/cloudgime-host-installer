use actix_web::{
    HttpRequest, HttpResponse, post,
    web::{Data, Json},
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::{
    cmp, env, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use common::api_bindings::{
    AndroidNativeHiddenHostBinding, AndroidNativeSessionLifecycleState,
    AndroidNativeSharedSessionInvite, AndroidNativeTransportEndpoint, AndroidNativeTransportPolicy,
    AndroidNativeTrustBootstrap, DetailedHost, HostCapabilityProfile,
    PostAndroidNativeConsumeLaunchRequest, PostAndroidNativeConsumeLaunchResponse,
    PostAndroidNativeLaunchTokenRequest, PostAndroidNativeLaunchTokenResponse,
    PostAndroidNativeRefreshStreamTicketRequest, PostAndroidNativeRefreshStreamTicketResponse,
    PostAndroidNativeSessionEventRequest, PostAndroidNativeSessionEventResponse,
    PostAndroidNativeSessionLifecycleRequest, PostAndroidNativeSessionLifecycleResponse,
    PostAndroidNativeSharedSessionConsumeInviteRequest,
    PostAndroidNativeSharedSessionConsumeInviteResponse,
    PostAndroidNativeSharedSessionInviteRequest, PostAndroidNativeSharedSessionInviteResponse,
    RtcIceServer,
};
use serde::{Deserialize, Serialize};
use stun::{
    agent::TransactionId,
    client::ClientBuilder,
    message::{BINDING_REQUEST, Getter, Message},
    xoraddr::XorMappedAddress,
};
use tokio::{
    net::{UdpSocket, lookup_host},
    time::timeout,
};
use url::Url;

use crate::api::auth::build_cookie;
use crate::api::stream::{
    append_host_stream_trace, cleanup_android_native_stream_session,
    remember_prewarmed_display_session, run_display_helper_command,
    shared_player2_attach_available_now,
};
use crate::app::{
    AndroidNativeHostBindingRecord, AndroidNativeSharedSessionInviteRecord,
    AndroidNativeTrustBootstrapRecord, App, AppError,
    auth::UserAuth,
    host::{AppId, HostId},
    shared_session_attach_available_now, shared_session_status_message,
    user::AuthenticatedUser,
};

fn parse_host_ip(address: &str) -> Option<IpAddr> {
    address.trim().parse().ok()
}

fn prefixed_path(path_prefix: &str, suffix: &str) -> String {
    let prefix = path_prefix.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    if prefix.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{prefix}/{suffix}")
    }
}

fn build_absolute_web_base_url(req: &HttpRequest, path_prefix: &str) -> String {
    let info = req.connection_info();
    let host_only = info.host().split(':').next().unwrap_or("").trim();
    let is_loopback = host_only == "127.0.0.1"
        || host_only == "localhost"
        || host_only == "::1"
        || host_only == "[::1]"
        || host_only.starts_with("127.");
    let scheme = if is_loopback { "http" } else { info.scheme() };
    let origin = format!("{}://{}", scheme, info.host());
    let base_path = prefixed_path(path_prefix, "");
    format!("{}{}", origin.trim_end_matches('/'), base_path)
}

fn build_native_scheme_url(
    host_id: u32,
    app_id: u32,
    launch_token: &str,
    web_base_url: &str,
    native_shell: &str,
) -> String {
    let mut url = Url::parse("moonlightnative://stream")
        .expect("failed to build cloudgime native scheme url");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("hostId", &host_id.to_string());
        query.append_pair("appId", &app_id.to_string());
        query.append_pair("launchToken", launch_token);
        query.append_pair("webBaseUrl", web_base_url);
        query.append_pair("nativeShell", native_shell);
    }

    url.into()
}

fn normalize_native_shell(
    native_shell: Option<&str>,
    client_os: Option<&str>,
    client_platform: Option<&str>,
) -> &'static str {
    for value in [native_shell, client_os, client_platform]
        .into_iter()
        .flatten()
    {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("android")
            || normalized.eq_ignore_ascii_case("android-native")
        {
            return "android";
        }
        if normalized.eq_ignore_ascii_case("windows")
            || normalized.eq_ignore_ascii_case("windows-native")
        {
            return "windows";
        }
    }
    "windows"
}

fn apply_keeper_tunnel_session_to_launch_response(
    req: &HttpRequest,
    response: &mut PostAndroidNativeLaunchTokenResponse,
) {
    let Some(kt_session) = super::request_query_param(req, "kt_session") else {
        return;
    };

    response.native_scheme_url =
        super::append_query_param_to_url(&response.native_scheme_url, "kt_session", &kt_session);
    response.open_native_path =
        super::append_query_param_to_path(&response.open_native_path, "kt_session", &kt_session);
    response.web_stream_path =
        super::append_query_param_to_path(&response.web_stream_path, "kt_session", &kt_session);
}

fn build_session_event_path(path_prefix: &str) -> String {
    prefixed_path(path_prefix, "api/android-native/session-event")
}

fn build_stream_websocket_path(path_prefix: &str) -> String {
    prefixed_path(path_prefix, "api/host/stream")
}

fn build_shared_session_share_url(path_prefix: &str, invite_token: &str) -> String {
    prefixed_path(
        path_prefix,
        &format!("open-native.html?sharedInviteToken={invite_token}"),
    )
}

fn request_origin_is_loopback(req: &HttpRequest) -> bool {
    req.peer_addr().is_some_and(|peer| peer.ip().is_loopback())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsNativeDiagnosticReportsFile {
    schema_version: u32,
    reports: Vec<WindowsNativeDiagnosticReportEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsNativeDiagnosticReportEntry {
    token_id: String,
    session_id: String,
    user_id: u64,
    sequence: u32,
    event_name: String,
    stage: String,
    recorded_at_unix_ms: u64,
    client_time_unix_ms: Option<u64>,
    detail_json: Option<serde_json::Value>,
    detail_text: Option<String>,
}

fn current_runtime_dir() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    current_exe.parent().map(Path::to_path_buf)
}

fn current_bundle_root() -> Option<PathBuf> {
    current_runtime_dir()?.parent().map(Path::to_path_buf)
}

fn resolve_display_prepare_helper_path() -> Option<PathBuf> {
    Some(
        current_bundle_root()?
            .join("moonlight")
            .join("server")
            .join("display-prepare-helper.exe"),
    )
}

const ANDROID_NATIVE_DISPLAY_PREWARM_TIMEOUT: Duration = Duration::from_secs(45);

async fn run_android_native_display_helper(
    arguments: &[String],
) -> Result<AndroidNativeDisplayControlHelperResult, String> {
    let helper_path = resolve_display_prepare_helper_path()
        .ok_or_else(|| "failed to resolve display prepare helper path".to_string())?;
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
        helper_arguments.push(bundle_root.display().to_string());
    }

    let output = run_display_helper_command(
        &helper_path,
        &helper_arguments,
        ANDROID_NATIVE_DISPLAY_PREWARM_TIMEOUT,
        Some("android_native"),
    )
    .await?;

    let stdout = output.stdout;
    let stderr = output.stderr;

    if stdout.is_empty() {
        if output.exit_code == 0 {
            return Ok(AndroidNativeDisplayControlHelperResult {
                ok: true,
                changed: false,
                restored: false,
                skipped: false,
                reason: String::new(),
                displays: Vec::new(),
                stream_display_preference: None,
                selected_display_label: None,
                active_display_label: None,
            });
        }

        return Err(if !stderr.is_empty() {
            stderr
        } else {
            format!(
                "display prepare helper exited with status {}",
                output.exit_code
            )
        });
    }

    let parsed = serde_json::from_str::<AndroidNativeDisplayControlHelperResult>(&stdout).map_err(
        |err| {
            format!(
                "failed to parse display helper output: {err}; stdout={stdout}; stderr={stderr}"
            )
        },
    )?;

    if parsed.ok {
        return Ok(parsed);
    }

    Err(if parsed.reason.trim().is_empty() {
        if !stderr.is_empty() {
            stderr
        } else {
            format!(
                "display prepare helper exited with status {}",
                output.exit_code
            )
        }
    } else {
        parsed.reason
    })
}

async fn prewarm_android_native_stream_display(
    session_id: &str,
    requested_mode: Option<(u32, u32, u32)>,
) -> Result<(), String> {
    append_host_stream_trace(&format!(
        "ANDROID_NATIVE_PREWARM_BEGIN session_id={} requested_mode={}",
        session_id,
        requested_mode
            .map(|(width, height, fps)| format!("{width}x{height}@{fps}"))
            .unwrap_or_else(|| "none".to_string())
    ));
    let mut arguments = vec![
        "prepare".to_string(),
        "--session-token".to_string(),
        session_id.to_string(),
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
    let result = run_android_native_display_helper(&arguments).await?;

    remember_prewarmed_display_session(session_id).await;
    append_host_stream_trace(&format!(
        "ANDROID_NATIVE_PREWARM_READY session_id={} changed={} skipped={} reason={}",
        session_id, result.changed, result.skipped, result.reason
    ));

    if !result.reason.trim().is_empty() {
        log::info!(
            "android-native display prewarm session_id={} changed={} skipped={} reason={}",
            session_id,
            result.changed,
            result.skipped,
            result.reason
        );
    }

    Ok(())
}

fn normalize_display_control_action(action: &str) -> String {
    action.trim().to_ascii_lowercase().replace('-', "_")
}

async fn run_powershell_script(
    script_path: &Path,
    arguments: &[&str],
) -> Result<String, String> {
    let mut command = tokio::process::Command::new("powershell.exe");
    command
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let child = command.spawn().map_err(|err| format!("Failed to spawn powershell: {err}"))?;
    let output = child.wait_with_output().await.map_err(|err| format!("Failed to wait: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "Powershell exited with code {:?}. stderr: {}. stdout: {}",
            output.status.code(),
            stderr,
            stdout
        ))
    }
}


#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AndroidNativeRemoteDiscoveryCache {
    schema_version: u32,
    checked_at_unix_ms: u64,
    status: String,
    address: Option<String>,
    source: Option<String>,
    server: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Clone)]
struct AndroidNativeRemoteCandidate {
    address: String,
    source: String,
}

fn android_native_remote_discovery_cache_path() -> Option<PathBuf> {
    Some(
        current_bundle_root()?
            .join("moonlight")
            .join("server")
            .join("android_native_remote_discovery_cache.json"),
    )
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn load_cached_remote_candidate(max_age: Duration) -> Option<AndroidNativeRemoteCandidate> {
    let cache_path = android_native_remote_discovery_cache_path()?;
    let raw = fs::read_to_string(cache_path).ok()?;
    let cache = serde_json::from_str::<AndroidNativeRemoteDiscoveryCache>(&raw).ok()?;
    if cache.status != "ready" {
        return None;
    }

    let address = cache.address?;
    if !is_public_remote_host_address(&address) {
        return None;
    }

    if now_unix_millis().saturating_sub(cache.checked_at_unix_ms) > max_age.as_millis() as u64 {
        return None;
    }

    Some(AndroidNativeRemoteCandidate {
        address,
        source: cache
            .source
            .unwrap_or_else(|| "cache:stun_reflexive".to_string()),
    })
}

fn persist_remote_discovery_cache(
    status: &str,
    address: Option<&str>,
    source: Option<&str>,
    server: Option<&str>,
    detail: Option<&str>,
) {
    let Some(cache_path) = android_native_remote_discovery_cache_path() else {
        return;
    };

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let cache = AndroidNativeRemoteDiscoveryCache {
        schema_version: 1,
        checked_at_unix_ms: now_unix_millis(),
        status: status.to_string(),
        address: address.map(str::to_string),
        source: source.map(str::to_string),
        server: server.map(str::to_string),
        detail: detail.map(str::to_string),
    };

    if let Ok(serialized) = serde_json::to_string_pretty(&cache) {
        let _ = fs::write(cache_path, format!("{serialized}\n"));
    }
}

fn windows_native_diagnostic_reports_path() -> Option<PathBuf> {
    Some(
        current_bundle_root()?
            .join("moonlight")
            .join("server")
            .join("windows_native_diagnostic_reports.json"),
    )
}

fn persist_windows_native_diagnostic_report(
    event: &crate::app::AndroidNativeSessionEventRecord,
) -> io::Result<()> {
    let Some(report_path) = windows_native_diagnostic_reports_path() else {
        return Ok(());
    };

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = fs::read_to_string(&report_path)
        .unwrap_or_else(|_| "{\"schemaVersion\":1,\"reports\":[]}".to_string());
    let mut file = serde_json::from_str::<WindowsNativeDiagnosticReportsFile>(&raw).unwrap_or(
        WindowsNativeDiagnosticReportsFile {
            schema_version: 1,
            reports: Vec::new(),
        },
    );

    let parsed_detail = event
        .detail
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
    let detail_text = if parsed_detail.is_none() {
        event.detail.clone()
    } else {
        None
    };

    file.reports.push(WindowsNativeDiagnosticReportEntry {
        token_id: event.token_id.clone(),
        session_id: event.session_id.clone(),
        user_id: u64::from(event.user_id.0),
        sequence: event.sequence,
        event_name: event.event_name.clone(),
        stage: event.stage.clone(),
        recorded_at_unix_ms: event.recorded_at_unix_ms,
        client_time_unix_ms: event.client_time_unix_ms,
        detail_json: parsed_detail,
        detail_text,
    });

    if file.reports.len() > 80 {
        let drain_count = file.reports.len() - 80;
        file.reports.drain(0..drain_count);
    }

    fs::write(&report_path, serde_json::to_string_pretty(&file)?)
}

#[derive(Debug, Deserialize)]
struct PostAndroidNativeSharedSessionLoopbackInviteRequest {
    role: common::api_bindings::AndroidNativeSharedSessionRole,
    host_id: Option<u32>,
    app_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PostAndroidNativeBootstrapWebSessionRequest {
    launch_token: String,
}

#[derive(Debug, Deserialize)]
struct PostAndroidNativeBootstrapWebSessionFromNativeSessionRequest {
    token_id: Option<String>,
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct PostAndroidNativeTunnelLaunchTokenRequest {
    host_id: Option<u32>,
    app_id: Option<u32>,
    #[serde(default)]
    native_shell: Option<String>,
    #[serde(default)]
    client_os: Option<String>,
    #[serde(default)]
    client_platform: Option<String>,
}

#[derive(Debug, Serialize)]
struct PostAndroidNativeBootstrapWebSessionResponse {
    token_id: String,
    session_id: String,
    host_id: u32,
    app_id: u32,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    web_stream_path: String,
}

#[derive(Debug, Deserialize)]
struct PostAndroidNativeDisplayControlRequest {
    launch_token: Option<String>,
    token_id: Option<String>,
    session_id: Option<String>,
    action: String,
    display_mode: Option<String>,
    device_name: Option<String>,
    device_id: Option<String>,
    label: Option<String>,
    client_time_unix_ms: Option<u64>,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone)]
#[serde(default)]
struct AndroidNativeDisplayPreferenceInfo {
    mode: String,
    manual_override: bool,
    custom_device_name: String,
    custom_device_id: String,
    custom_label: String,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone)]
#[serde(default)]
struct AndroidNativeDisplayInfo {
    display_id: i32,
    device_name: String,
    device_id: String,
    device_string: String,
    label: String,
    width: i32,
    height: i32,
    frequency: i32,
    active: bool,
    primary: bool,
    is_virtual_display: bool,
    is_mtt_vdd: bool,
    selected_preference: bool,
    current_stream_target: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AndroidNativeDisplayControlHelperResult {
    ok: bool,
    changed: bool,
    restored: bool,
    skipped: bool,
    reason: String,
    displays: Vec<AndroidNativeDisplayInfo>,
    stream_display_preference: Option<AndroidNativeDisplayPreferenceInfo>,
    selected_display_label: Option<String>,
    active_display_label: Option<String>,
}

#[derive(Debug, Serialize)]
struct PostAndroidNativeDisplayControlResponse {
    ok: bool,
    token_id: String,
    session_id: String,
    changed: bool,
    restored: bool,
    skipped: bool,
    reason: String,
    displays: Vec<AndroidNativeDisplayInfo>,
    stream_display_preference: Option<AndroidNativeDisplayPreferenceInfo>,
    selected_display_label: Option<String>,
    active_display_label: Option<String>,
}

async fn build_shared_session_invite_response(
    app: &App,
    record: AndroidNativeSharedSessionInviteRecord,
) -> PostAndroidNativeSharedSessionInviteResponse {
    let owner_session_active = app
        .android_native_owner_session_is_active(&record.owner_session_id)
        .await;
    let attach_available = if record.role
        == common::api_bindings::AndroidNativeSharedSessionRole::Player2
    {
        shared_player2_attach_available_now(&record.owner_session_id, record.host_id, record.app_id)
            .await
    } else {
        shared_session_attach_available_now(record.role, owner_session_active)
    };
    let status_message = if record.role
        == common::api_bindings::AndroidNativeSharedSessionRole::Player2
    {
        if !owner_session_active {
            "The owner session is no longer active, so this Player 2 invite cannot attach right now."
                .to_string()
        } else if attach_available {
            "Player 2 share is ready. Open the companion pad link on the second device.".to_string()
        } else {
            "Player 2 share is issued, but the owner stream is still preparing the live controller lane."
                .to_string()
        }
    } else {
        shared_session_status_message(record.role, owner_session_active)
    };

    PostAndroidNativeSharedSessionInviteResponse {
        invite: AndroidNativeSharedSessionInvite {
            invite_token: record.invite_token.clone(),
            shared_session_id: record.shared_session_id.clone(),
            owner_session_id: record.owner_session_id.clone(),
            host_id: record.host_id.0,
            app_id: record.app_id.0,
            role: record.role,
            issued_at_unix_ms: record.issued_at_unix_ms,
            expires_at_unix_ms: record.expires_at_unix_ms,
            share_url: build_shared_session_share_url(
                &app.config().web_server.url_path_prefix,
                &record.invite_token,
            ),
            attach_available,
            status_message,
            capabilities: record.capabilities,
        },
    }
}

fn pick_effective_launch_app_id(
    apps: Vec<crate::app::host::App>,
    requested_app_id: AppId,
) -> Result<AppId, AppError> {
    if apps.iter().any(|app| app.id == requested_app_id) {
        return Ok(requested_app_id);
    }

    let selected_app = apps
        .iter()
        .find(|app| app.title.eq_ignore_ascii_case("Desktop"))
        .or_else(|| apps.first())
        .ok_or(AppError::BadRequest)?;

    Ok(selected_app.id)
}

fn is_loopback_host_address(address: &str) -> bool {
    matches!(address.trim(), "127.0.0.1" | "localhost" | "::1")
}

fn is_offline_host_address(address: &str) -> bool {
    address.trim().eq_ignore_ascii_case("offline")
}

fn is_ipv4_shared_carrier_grade_nat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_private_or_overlay_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_private() || is_ipv4_shared_carrier_grade_nat(ipv4),
        IpAddr::V6(ipv6) => ipv6.is_unique_local(),
    }
}

fn is_same_host_ip(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn is_valid_local_host_address(address: &str) -> bool {
    if address.is_empty() || is_offline_host_address(address) || is_loopback_host_address(address) {
        return false;
    }

    match parse_host_ip(address) {
        Some(IpAddr::V4(ipv4)) => !ipv4.is_link_local(),
        Some(IpAddr::V6(ipv6)) => !ipv6.is_unspecified() && !ipv6.is_loopback(),
        None => true,
    }
}

fn is_usable_public_host_address(address: &str, local_ip: Option<&str>) -> bool {
    if address.is_empty() || is_offline_host_address(address) || is_loopback_host_address(address) {
        return false;
    }

    match parse_host_ip(address) {
        Some(IpAddr::V4(ipv4)) => {
            if ipv4.is_link_local() {
                return false;
            }
            if !is_private_or_overlay_ip(IpAddr::V4(ipv4)) {
                return true;
            }
            local_ip.is_some_and(|local| is_same_host_ip(address, local))
        }
        Some(IpAddr::V6(ipv6)) => {
            if ipv6.is_unspecified() || ipv6.is_loopback() {
                return false;
            }
            if !is_private_or_overlay_ip(IpAddr::V6(ipv6)) {
                return true;
            }
            local_ip.is_some_and(|local| is_same_host_ip(address, local))
        }
        None => true,
    }
}

fn is_public_remote_host_address(address: &str) -> bool {
    if address.is_empty() || is_offline_host_address(address) || is_loopback_host_address(address) {
        return false;
    }

    match parse_host_ip(address) {
        Some(IpAddr::V4(ipv4)) => {
            !ipv4.is_link_local() && !is_private_or_overlay_ip(IpAddr::V4(ipv4))
        }
        Some(IpAddr::V6(ipv6)) => {
            !ipv6.is_unspecified()
                && !ipv6.is_loopback()
                && !is_private_or_overlay_ip(IpAddr::V6(ipv6))
        }
        None => true,
    }
}

fn discover_streamer_host_address() -> Option<String> {
    let socket = StdUdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let local_addr = socket.local_addr().ok()?;

    match local_addr.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => Some(ip.to_string()),
        IpAddr::V6(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip.to_string()),
        _ => None,
    }
}

fn extract_public_turn_host(ice_servers: &[RtcIceServer]) -> Option<String> {
    for server in ice_servers {
        for url in &server.urls {
            let remainder = if let Some(val) = url.strip_prefix("turn:") {
                val
            } else if let Some(val) = url.strip_prefix("turns:") {
                val
            } else {
                continue;
            };

            let host_part = remainder
                .trim_start_matches("//")
                .split('?')
                .next()
                .unwrap_or_default()
                .split('@')
                .last()
                .unwrap_or_default()
                .split(':')
                .next()
                .unwrap_or_default()
                .trim();

            if !host_part.is_empty() && is_public_remote_host_address(host_part) {
                return Some(host_part.to_string());
            }
        }
    }
    None
}

fn resolve_android_native_host_address(app: &App, detailed: &DetailedHost) -> String {
    let local_ip = if is_valid_local_host_address(&detailed.local_ip) {
        Some(detailed.local_ip.as_str())
    } else {
        None
    };

    if is_usable_public_host_address(&detailed.address, local_ip) {
        return detailed.address.clone();
    }

    if let Some(public_ip) = app.config().webrtc.nat_1to1.as_ref().and_then(|mapping| {
        mapping
            .ips
            .iter()
            .find(|ip| is_usable_public_host_address(ip, local_ip))
    }) {
        return public_ip.clone();
    }

    if let Some(turn_host) = extract_public_turn_host(&app.config().webrtc.ice_servers) {
        if is_usable_public_host_address(&turn_host, local_ip) {
            return turn_host;
        }
    }

    if let Some(discovered_ip) =
        discover_streamer_host_address().filter(|ip| is_valid_local_host_address(ip))
    {
        return discovered_ip;
    }

    if let Some(local_ip) = local_ip {
        return local_ip.to_string();
    }

    if is_valid_local_host_address(&detailed.address) {
        return detailed.address.clone();
    }

    if local_ip.is_some() {
        return detailed.local_ip.clone();
    }

    detailed.address.clone()
}

fn parse_stun_server_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let (scheme, remainder) = if let Some(value) = trimmed.strip_prefix("stun:") {
        ("stun", value)
    } else if let Some(value) = trimmed.strip_prefix("stuns:") {
        ("stuns", value)
    } else {
        return None;
    };

    if scheme == "stuns" {
        return None;
    }

    let endpoint = remainder
        .trim_start_matches("//")
        .split('?')
        .next()
        .unwrap_or_default()
        .trim();
    if endpoint.is_empty() {
        return None;
    }

    if endpoint.contains(':') {
        Some(endpoint.to_string())
    } else {
        Some(format!("{endpoint}:3478"))
    }
}

fn collect_stun_server_candidates(ice_servers: &[RtcIceServer]) -> Vec<(String, String)> {
    let mut candidates = Vec::new();
    for server in ice_servers {
        for raw_url in &server.urls {
            if let Some(endpoint) = parse_stun_server_url(raw_url)
                && !candidates.iter().any(|(value, _)| value == &endpoint)
            {
                candidates.push((endpoint, raw_url.clone()));
            }
        }
    }
    candidates
}

async fn query_stun_reflexive_ip(server: &str) -> Option<IpAddr> {
    let mut resolved = lookup_host(server).await.ok()?;
    let remote_addr = resolved.find(|candidate| matches!(candidate, SocketAddr::V4(_)))?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    socket.connect(remote_addr).await.ok()?;

    let mut client = ClientBuilder::new()
        .with_conn(Arc::new(socket))
        .with_no_retransmit()
        .build()
        .ok()?;

    let (handler_tx, mut handler_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut msg = Message::new();
    msg.build(&[Box::<TransactionId>::default(), Box::new(BINDING_REQUEST)])
        .ok()?;
    client.send(&msg, Some(Arc::new(handler_tx))).await.ok()?;

    let event = timeout(Duration::from_secs(2), handler_rx.recv())
        .await
        .ok()
        .flatten()?;
    let body = event.event_body.ok()?;
    let mut xor_addr = XorMappedAddress::default();
    xor_addr.get_from(&body).ok()?;
    let _ = client.close().await;
    Some(xor_addr.ip)
}

async fn discover_remote_candidate_from_stun(
    app: &App,
    local_ip: Option<&str>,
) -> Option<AndroidNativeRemoteCandidate> {
    if let Some(cached) = load_cached_remote_candidate(Duration::from_secs(10 * 60)) {
        return Some(cached);
    }

    let candidates = collect_stun_server_candidates(&app.config().webrtc.ice_servers);
    if candidates.is_empty() {
        persist_remote_discovery_cache(
            "unavailable",
            None,
            None,
            None,
            Some("No STUN servers configured for WebRTC."),
        );
        return None;
    }

    for (endpoint, source_url) in candidates {
        if let Some(ip) = query_stun_reflexive_ip(&endpoint).await {
            let address = ip.to_string();
            if is_usable_public_host_address(&address, local_ip)
                && is_public_remote_host_address(&address)
            {
                persist_remote_discovery_cache(
                    "ready",
                    Some(&address),
                    Some("stun_reflexive"),
                    Some(&source_url),
                    Some("Public reflexive address discovered from STUN."),
                );
                return Some(AndroidNativeRemoteCandidate {
                    address,
                    source: format!("stun_reflexive:{source_url}"),
                });
            }
        }
    }

    persist_remote_discovery_cache(
        "failed",
        None,
        None,
        None,
        Some("No STUN server returned a usable public address."),
    );
    None
}

async fn resolve_remote_candidate(
    app: &App,
    host_binding: &AndroidNativeHostBindingRecord,
) -> Option<AndroidNativeRemoteCandidate> {
    let local_ip = host_binding.local_ip.as_deref();

    if is_public_remote_host_address(&host_binding.address) {
        return Some(AndroidNativeRemoteCandidate {
            address: host_binding.address.clone(),
            source: "host_binding.address".to_string(),
        });
    }

    if let Some(public_ip) = app.config().webrtc.nat_1to1.as_ref().and_then(|mapping| {
        mapping
            .ips
            .iter()
            .find(|ip| is_usable_public_host_address(ip, local_ip))
    }) {
        return Some(AndroidNativeRemoteCandidate {
            address: public_ip.clone(),
            source: "config.webrtc.nat_1to1".to_string(),
        });
    }

    if let Some(turn_host) = extract_public_turn_host(&app.config().webrtc.ice_servers) {
        if is_usable_public_host_address(&turn_host, local_ip) {
            return Some(AndroidNativeRemoteCandidate {
                address: turn_host,
                source: "ice_servers.turn_host".to_string(),
            });
        }
    }

    discover_remote_candidate_from_stun(app, local_ip).await
}

async fn build_transport_policy(
    app: &App,
    host_binding: &AndroidNativeHostBindingRecord,
    requested_route: Option<&str>,
) -> AndroidNativeTransportPolicy {
    let mut preferred_endpoints = Vec::new();
    let selected_local_ip = discover_streamer_host_address()
        .filter(|ip| is_valid_local_host_address(ip))
        .or_else(|| {
            host_binding
                .local_ip
                .as_deref()
                .filter(|ip| is_valid_local_host_address(ip))
                .map(|ip| ip.to_string())
        });
    let local_ip = selected_local_ip.as_deref();
    let remote_candidate = resolve_remote_candidate(app, host_binding).await;
    let remote_address = remote_candidate
        .as_ref()
        .map(|candidate| candidate.address.as_str());

    let mut push_endpoint = |kind: &str, address: &str, port: u16| {
        if preferred_endpoints
            .iter()
            .any(|endpoint: &AndroidNativeTransportEndpoint| {
                endpoint.kind == kind && endpoint.port == port && endpoint.address == address
            })
        {
            return;
        }
        preferred_endpoints.push(AndroidNativeTransportEndpoint {
            kind: kind.to_string(),
            address: address.to_string(),
            port,
        });
    };

    if let Some(local_ip) = local_ip {
        push_endpoint("local-http", local_ip, host_binding.http_port);
        if let Some(https_port) = host_binding.https_port {
            push_endpoint("local-https", local_ip, https_port);
        }
    }

    if let Some(remote_address) = remote_address {
        push_endpoint("remote-http", remote_address, host_binding.http_port);
        if let Some(https_port) = host_binding.https_port {
            push_endpoint("remote-https", remote_address, https_port);
        }
        if let Some(external_port) = host_binding.external_port {
            push_endpoint("remote-external", remote_address, external_port);
        }
    }

    let direct_remote_ready = remote_address.is_some();
    let direct_remote_reason = if direct_remote_ready {
        None
    } else {
        Some(
            "Cloudgime host ini belum berhasil menemukan endpoint publik yang valid dari alamat host, NAT 1:1, atau STUN untuk koneksi cepat dari luar jaringan."
                .to_string(),
        )
    };

    let relay_requested = requested_route
        .map(str::trim)
        .map(|value| value.eq_ignore_ascii_case("relay"))
        .unwrap_or(false);

    AndroidNativeTransportPolicy {
        lane: "android-native".to_string(),
        connection_mode: if relay_requested {
            "relay-native-webrtc".to_string()
        } else {
            "direct-p2p-native".to_string()
        },
        relay_allowed: true,
        web_fallback_allowed: false,
        direct_remote_ready,
        direct_remote_reason,
        direct_remote_candidate_source: remote_candidate
            .as_ref()
            .map(|candidate| candidate.source.clone()),
        direct_remote_candidate_address: remote_candidate.map(|candidate| candidate.address),
        preferred_endpoints,
    }
}

fn read_android_native_selected_encoder() -> Option<String> {
    let runtime_dir = env::current_exe().ok()?.parent()?.to_path_buf();
    let profile_path = runtime_dir
        .join("server")
        .join("host_capability_profile.json");
    let raw = fs::read(profile_path).ok()?;
    let profile = serde_json::from_slice::<HostCapabilityProfile>(&raw).ok()?;
    let selected_encoder = profile.selected_encoder.trim();
    if selected_encoder.is_empty() {
        None
    } else {
        Some(selected_encoder.to_string())
    }
}

pub(crate) async fn issue_android_native_launch_response(
    app: &App,
    req: &HttpRequest,
    user: &mut AuthenticatedUser,
    host: &mut crate::app::host::Host,
    host_id: HostId,
    requested_app_id: AppId,
    native_shell: &str,
) -> Result<PostAndroidNativeLaunchTokenResponse, AppError> {
    let app_id = pick_effective_launch_app_id(host.list_apps(user).await?, requested_app_id)?;
    let detailed = host.detailed_host(user).await?;
    let pair_info = match host.pair_info(user).await {
        Ok(value) => Some(value),
        Err(AppError::HostNotPaired) => None,
        Err(err) => return Err(err),
    };
    let client_unique_id = user.host_unique_id().await?;

    let host_binding = AndroidNativeHostBindingRecord {
        name: detailed.name.clone(),
        address: resolve_android_native_host_address(app, &detailed),
        http_port: detailed.http_port,
        https_port: if detailed.https_port > 0 {
            Some(detailed.https_port)
        } else {
            None
        },
        external_port: if detailed.external_port > 0 {
            Some(detailed.external_port)
        } else {
            None
        },
        unique_id: if detailed.unique_id != "Offline" {
            Some(detailed.unique_id.clone())
        } else {
            None
        },
        local_ip: if detailed.local_ip != "Offline" {
            Some(detailed.local_ip.clone())
        } else {
            None
        },
    };
    let trust_bootstrap = if let Some(pair_info) = pair_info {
        AndroidNativeTrustBootstrapRecord {
            paired: true,
            pair_mode: "reuse_host_pair_info".to_string(),
            client_unique_id: Some(client_unique_id),
            client_certificate_pem: Some(pair_info.client_certificate.to_string()),
            client_private_key_pem: Some(pair_info.client_private_key.to_string()),
            server_certificate_pem: Some(pair_info.server_certificate.to_string()),
        }
    } else {
        AndroidNativeTrustBootstrapRecord {
            paired: false,
            pair_mode: "token_bootstrap_required".to_string(),
            client_unique_id: Some(client_unique_id),
            client_certificate_pem: None,
            client_private_key_pem: None,
            server_certificate_pem: None,
        }
    };
    let transport_policy = build_transport_policy(app, &host_binding, None).await;

    let record = app
        .issue_android_native_launch_token(
            user.id(),
            host_id,
            app_id,
            host_binding,
            trust_bootstrap,
        )
        .await;

    let open_native_path = prefixed_path(
        &app.config().web_server.url_path_prefix,
        &format!(
            "open-native.html?hostId={}&appId={}&launchToken={}",
            host_id.0, record.app_id.0, record.launch_token
        ),
    );
    let web_stream_path = prefixed_path(
        &app.config().web_server.url_path_prefix,
        &format!(
            "stream.html?hostId={}&appId={}&launchToken={}",
            host_id.0, record.app_id.0, record.launch_token
        ),
    );
    let web_base_url = build_absolute_web_base_url(req, &app.config().web_server.url_path_prefix);

    let mut response = PostAndroidNativeLaunchTokenResponse {
        launch_token: record.launch_token.clone(),
        session_id: record.session_id.clone(),
        host_id: record.host_id.0,
        app_id: record.app_id.0,
        issued_at_unix_ms: record.issued_at_unix_ms,
        expires_at_unix_ms: record.expires_at_unix_ms,
        open_native_path,
        web_stream_path,
        session_event_path: build_session_event_path(&app.config().web_server.url_path_prefix),
        native_scheme_url: build_native_scheme_url(
            host_id.0,
            record.app_id.0,
            &record.launch_token,
            &web_base_url,
            native_shell,
        ),
        session_policy: record.session_policy,
        transport_policy,
        feature_profile: record.feature_profile.clone(),
    };
    apply_keeper_tunnel_session_to_launch_response(req, &mut response);

    Ok(response)
}

async fn resolve_tunnel_launch_host(
    user: &mut AuthenticatedUser,
    requested_host_id: Option<u32>,
) -> Result<(HostId, crate::app::host::Host), AppError> {
    if let Some(host_id) = requested_host_id {
        let resolved_host_id = HostId(host_id);
        let host = user.host(resolved_host_id).await?;
        return Ok((resolved_host_id, host));
    }

    let mut best_match: Option<(u8, HostId, crate::app::host::Host)> = None;
    for mut candidate in user.hosts().await? {
        let detailed = candidate.detailed_host(user).await?;
        let mut score = 0u8;
        if detailed.server_state.is_some() {
            score = score.saturating_add(4);
        }
        if detailed.unique_id != "Offline" {
            score = score.saturating_add(2);
        }
        if detailed.local_ip != "Offline" {
            score = score.saturating_add(1);
        }

        let host_id = HostId(detailed.host_id);
        match &best_match {
            Some((best_score, _, _)) if *best_score >= score => {}
            _ => {
                best_match = Some((score, host_id, candidate));
            }
        }
    }

    best_match
        .map(|(_, host_id, host)| (host_id, host))
        .ok_or(AppError::HostNotFound)
}

#[post("/android-native/launch-token")]
pub async fn post_android_native_launch_token(
    app: Data<App>,
    req: HttpRequest,
    mut user: AuthenticatedUser,
    Json(request): Json<PostAndroidNativeLaunchTokenRequest>,
) -> Result<Json<PostAndroidNativeLaunchTokenResponse>, AppError> {
    let host_id = HostId(request.host_id);
    let requested_app_id = AppId(request.app_id);
    let native_shell = normalize_native_shell(
        request.native_shell.as_deref(),
        request.client_os.as_deref(),
        request.client_platform.as_deref(),
    );

    let mut host = user.host(host_id).await?;
    let response = issue_android_native_launch_response(
        &app,
        &req,
        &mut user,
        &mut host,
        host_id,
        requested_app_id,
        native_shell,
    )
    .await?;

    Ok(Json(response))
}

#[post("/android-native/launch-token-from-tunnel")]
pub async fn post_android_native_launch_token_from_tunnel(
    app: Data<App>,
    req: HttpRequest,
    Json(request): Json<PostAndroidNativeTunnelLaunchTokenRequest>,
) -> Result<Json<PostAndroidNativeLaunchTokenResponse>, AppError> {
    let requested_app_id = AppId(request.app_id.unwrap_or_default());
    let native_shell = normalize_native_shell(
        request.native_shell.as_deref(),
        request.client_os.as_deref(),
        request.client_platform.as_deref(),
    );
    let mut user = app.user_by_auth(UserAuth::None).await?;
    let (host_id, mut host) = resolve_tunnel_launch_host(&mut user, request.host_id).await?;
    let response = issue_android_native_launch_response(
        &app,
        &req,
        &mut user,
        &mut host,
        host_id,
        requested_app_id,
        native_shell,
    )
    .await?;

    Ok(Json(response))
}

#[post("/android-native/consume-launch")]
pub async fn post_android_native_consume_launch(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeConsumeLaunchRequest>,
) -> Result<Json<PostAndroidNativeConsumeLaunchResponse>, AppError> {
    let (record, stream_ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&request.launch_token)
        .await?;
    let transport_policy = build_transport_policy(
        &app,
        &record.host_binding,
        request.connection_route.as_deref(),
    )
    .await;
    let selected_encoder = read_android_native_selected_encoder();

    Ok(Json(PostAndroidNativeConsumeLaunchResponse {
        token_id: record.token_id,
        session_id: record.session_id,
        host_id: record.host_id.0,
        app_id: record.app_id.0,
        issued_at_unix_ms: record.issued_at_unix_ms,
        expires_at_unix_ms: record.expires_at_unix_ms,
        session_event_path: build_session_event_path(&app.config().web_server.url_path_prefix),
        session_policy: record.session_policy,
        transport_policy,
        feature_profile: record.feature_profile.clone(),
        selected_encoder,
        stream_ticket: stream_ticket.stream_ticket,
        stream_ticket_expires_at_unix_ms: stream_ticket.expires_at_unix_ms,
        stream_websocket_path: build_stream_websocket_path(
            &app.config().web_server.url_path_prefix,
        ),
        host_binding: AndroidNativeHiddenHostBinding {
            name: record.host_binding.name,
            address: record.host_binding.address,
            http_port: record.host_binding.http_port,
            https_port: record.host_binding.https_port,
            external_port: record.host_binding.external_port,
            unique_id: record.host_binding.unique_id,
            local_ip: record.host_binding.local_ip,
        },
        trust_bootstrap: AndroidNativeTrustBootstrap {
            paired: record.trust_bootstrap.paired,
            pair_mode: record.trust_bootstrap.pair_mode,
            client_unique_id: record.trust_bootstrap.client_unique_id,
            client_certificate_pem: record.trust_bootstrap.client_certificate_pem,
            client_private_key_pem: record.trust_bootstrap.client_private_key_pem,
            server_certificate_pem: record.trust_bootstrap.server_certificate_pem,
        },
    }))
}

#[post("/android-native/bootstrap-web-session")]
pub async fn post_android_native_bootstrap_web_session(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeBootstrapWebSessionRequest>,
) -> Result<HttpResponse, AppError> {
    let launch_record = app
        .consume_android_native_launch_token(&request.launch_token)
        .await?;
    let trusted_user = app
        .authenticated_user_by_id_trusted(launch_record.user_id)
        .await?;
    let session_expiration = cmp::min(
        app.config().web_server.session_cookie_expiration,
        Duration::from_secs(12 * 60 * 60),
    );
    let session = trusted_user.new_session(session_expiration).await?;
    let mut session_bytes = [0; 64];
    let session_str = session.encode(&mut session_bytes);

    Ok(HttpResponse::Ok()
        .cookie(build_cookie(&app, session_expiration, session_str))
        .json(PostAndroidNativeBootstrapWebSessionResponse {
            token_id: launch_record.token_id,
            session_id: launch_record.session_id,
            host_id: launch_record.host_id.0,
            app_id: launch_record.app_id.0,
            issued_at_unix_ms: launch_record.issued_at_unix_ms,
            expires_at_unix_ms: launch_record.expires_at_unix_ms,
            web_stream_path: prefixed_path(&app.config().web_server.url_path_prefix, "stream.html"),
        }))
}

#[post("/android-native/bootstrap-web-session-from-native-session")]
pub async fn post_android_native_bootstrap_web_session_from_native_session(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeBootstrapWebSessionFromNativeSessionRequest>,
) -> Result<HttpResponse, AppError> {
    let stream_record = app
        .bootstrap_android_native_web_session_from_native_session(
            request.token_id.as_deref(),
            &request.session_id,
        )
        .await?;
    let trusted_user = app
        .authenticated_user_by_id_trusted(stream_record.user_id)
        .await?;
    let session_expiration = cmp::min(
        app.config().web_server.session_cookie_expiration,
        Duration::from_secs(12 * 60 * 60),
    );
    let session = trusted_user.new_session(session_expiration).await?;
    let mut session_bytes = [0; 64];
    let session_str = session.encode(&mut session_bytes);

    Ok(HttpResponse::Ok()
        .cookie(build_cookie(&app, session_expiration, session_str))
        .json(PostAndroidNativeBootstrapWebSessionResponse {
            token_id: stream_record.token_id,
            session_id: stream_record.session_id,
            host_id: stream_record.host_id.0,
            app_id: stream_record.app_id.0,
            issued_at_unix_ms: stream_record.issued_at_unix_ms,
            expires_at_unix_ms: stream_record.expires_at_unix_ms,
            web_stream_path: prefixed_path(&app.config().web_server.url_path_prefix, "stream.html"),
        }))
}

#[post("/android-native/shared-session/invite")]
pub async fn post_android_native_shared_session_invite(
    app: Data<App>,
    user: AuthenticatedUser,
    Json(request): Json<PostAndroidNativeSharedSessionInviteRequest>,
) -> Result<Json<PostAndroidNativeSharedSessionInviteResponse>, AppError> {
    let record = app
        .issue_android_native_shared_session_invite(
            user.id(),
            HostId(request.host_id),
            AppId(request.app_id),
            request.role,
        )
        .await?;

    Ok(Json(
        build_shared_session_invite_response(&app, record).await,
    ))
}

#[post("/android-native/shared-session/invite-loopback")]
pub async fn post_android_native_shared_session_invite_loopback(
    app: Data<App>,
    req: HttpRequest,
    Json(request): Json<PostAndroidNativeSharedSessionLoopbackInviteRequest>,
) -> Result<Json<PostAndroidNativeSharedSessionInviteResponse>, AppError> {
    if !request_origin_is_loopback(&req) {
        return Err(AppError::Forbidden);
    }

    let record = app
        .issue_android_native_shared_session_invite_for_active_owner(
            request.host_id.map(HostId),
            request.app_id.map(AppId),
            request.role,
        )
        .await?;

    Ok(Json(
        build_shared_session_invite_response(&app, record).await,
    ))
}

#[post("/android-native/shared-session/consume-invite")]
pub async fn post_android_native_shared_session_consume_invite(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeSharedSessionConsumeInviteRequest>,
) -> Result<Json<PostAndroidNativeSharedSessionConsumeInviteResponse>, AppError> {
    let response = build_shared_session_invite_response(
        &app,
        app.consume_android_native_shared_session_invite(&request.invite_token)
            .await?,
    )
    .await;

    Ok(Json(PostAndroidNativeSharedSessionConsumeInviteResponse {
        invite: response.invite,
    }))
}

#[post("/android-native/refresh-stream-ticket")]
pub async fn post_android_native_refresh_stream_ticket(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeRefreshStreamTicketRequest>,
) -> Result<Json<PostAndroidNativeRefreshStreamTicketResponse>, AppError> {
    let refreshed = app
        .refresh_android_native_stream_ticket(
            &request.stream_ticket,
            request.token_id.as_deref(),
            request.session_id.as_deref(),
        )
        .await?;

    if request.prepare_display {
        let requested_mode = match (request.width, request.height, request.fps) {
            (Some(width), Some(height), Some(fps)) if width > 0 && height > 0 && fps > 0 => {
                Some((width, height, fps))
            }
            _ => None,
        };
        match prewarm_android_native_stream_display(&refreshed.session_id, requested_mode).await {
            Ok(()) => {}
            Err(error) => {
                append_host_stream_trace(&format!(
                    "ANDROID_NATIVE_PREWARM_FAILED session_id={} token_id={} error={}",
                    refreshed.session_id, refreshed.token_id, error
                ));
                log::warn!(
                    "android-native display prewarm failed: session_id={} token_id={} error={}",
                    refreshed.session_id,
                    refreshed.token_id,
                    error
                );
            }
        }
    }

    Ok(Json(PostAndroidNativeRefreshStreamTicketResponse {
        token_id: refreshed.token_id,
        session_id: refreshed.session_id,
        stream_ticket: refreshed.stream_ticket,
        stream_ticket_expires_at_unix_ms: refreshed.expires_at_unix_ms,
        stream_websocket_path: build_stream_websocket_path(
            &app.config().web_server.url_path_prefix,
        ),
    }))
}

#[post("/android-native/display-control")]
pub async fn post_android_native_display_control(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeDisplayControlRequest>,
) -> Result<Json<PostAndroidNativeDisplayControlResponse>, AppError> {
    let resolved = app
        .resolve_android_native_session(
            request.launch_token.as_deref(),
            request.token_id.as_deref(),
            request.session_id.as_deref(),
        )
        .await?;

    let action = normalize_display_control_action(&request.action);
    let mut result = match action.as_str() {
        "query" | "refresh" | "list" => {
            run_android_native_display_helper(&["list-displays".to_string()]).await
        }
        "cloud_only" => {
            if let Err(error) = run_android_native_display_helper(&[
                "set-stream-display".to_string(),
                "--display-mode".to_string(),
                "mtt_vdd".to_string(),
            ])
            .await
            {
                Err(error)
            } else {
                run_android_native_display_helper(&[
                    "prepare".to_string(),
                    "--session-token".to_string(),
                    resolved.session_id.clone(),
                ])
                .await
            }
        }
        "host_primary_only" | "primary_only" => {
            if let Err(error) = run_android_native_display_helper(&[
                "set-stream-display".to_string(),
                "--display-mode".to_string(),
                "primary".to_string(),
            ])
            .await
            {
                Err(error)
            } else {
                run_android_native_display_helper(&[
                    "prepare".to_string(),
                    "--session-token".to_string(),
                    resolved.session_id.clone(),
                ])
                .await
            }
        }
        "select_display" | "custom_display" => {
            let mut set_arguments = vec![
                "set-stream-display".to_string(),
                "--display-mode".to_string(),
                "custom".to_string(),
            ];
            if let Some(device_name) = request
                .device_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                set_arguments.push("--device-name".to_string());
                set_arguments.push(device_name.to_string());
            }
            if let Some(device_id) = request
                .device_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                set_arguments.push("--device-id".to_string());
                set_arguments.push(device_id.to_string());
            }
            if let Some(label) = request
                .label
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                set_arguments.push("--label".to_string());
                set_arguments.push(label.to_string());
            }

            if let Err(error) = run_android_native_display_helper(&set_arguments).await {
                Err(error)
            } else {
                run_android_native_display_helper(&[
                    "prepare".to_string(),
                    "--session-token".to_string(),
                    resolved.session_id.clone(),
                ])
                .await
            }
        }
        "repair" | "repair_host" => {
            if let Some(bundle_root) = current_bundle_root() {
                let mut script_path = bundle_root.join("fix-host-vdd-and-sunshine.ps1");
                if !script_path.exists() {
                    script_path = bundle_root.join("tools").join("fix-host-vdd-and-sunshine.ps1");
                }

                if !script_path.exists() {
                    Err("Repair script (fix-host-vdd-and-sunshine.ps1) was not found in bundle root or tools folder.".to_string())
                } else {
                    match run_powershell_script(&script_path, &[]).await {
                        Ok(_) => Ok(AndroidNativeDisplayControlHelperResult {
                            ok: true,
                            changed: true,
                            reason: "Display driver dan Sunshine berhasil dipulihkan di PC Host.".to_string(),
                            ..Default::default()
                        }),
                        Err(err) => Err(format!("Gagal menjalankan script perbaikan: {err}")),
                    }
                }
            } else {
                Err("Failed to resolve bundle root".to_string())
            }
        }
        "restore_previous" | "restore" => {
            run_android_native_display_helper(&[
                "restore".to_string(),
                "--session-token".to_string(),
                resolved.session_id.clone(),
            ])
            .await
        }
        _ => Err(format!(
            "unsupported display control action: {}",
            request.action
        )),
    };

    if let Ok(helper_result) = &mut result {
        let _ = app
            .record_android_native_session_event(
                request.launch_token.as_deref(),
                Some(resolved.token_id.as_str()),
                Some(resolved.session_id.as_str()),
                "windows_native_display_control".to_string(),
                action.clone(),
                request
                    .display_mode
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_string),
                request.client_time_unix_ms,
            )
            .await;

        if action != "query"
            && let Ok(snapshot) =
                run_android_native_display_helper(&["list-displays".to_string()]).await
        {
            helper_result.displays = snapshot.displays;
            helper_result.stream_display_preference = snapshot.stream_display_preference;
            helper_result.selected_display_label = snapshot.selected_display_label;
            helper_result.active_display_label = snapshot.active_display_label;
        }
    }

    let helper_result = match result {
        Ok(result) => result,
        Err(error) => {
            let snapshot = run_android_native_display_helper(&["list-displays".to_string()])
                .await
                .unwrap_or_default();
            AndroidNativeDisplayControlHelperResult {
                ok: false,
                changed: false,
                restored: false,
                skipped: false,
                reason: error,
                displays: snapshot.displays,
                stream_display_preference: snapshot.stream_display_preference,
                selected_display_label: snapshot.selected_display_label,
                active_display_label: snapshot.active_display_label,
            }
        }
    };

    Ok(Json(PostAndroidNativeDisplayControlResponse {
        ok: helper_result.ok,
        token_id: resolved.token_id,
        session_id: resolved.session_id,
        changed: helper_result.changed,
        restored: helper_result.restored,
        skipped: helper_result.skipped,
        reason: helper_result.reason,
        displays: helper_result.displays,
        stream_display_preference: helper_result.stream_display_preference,
        selected_display_label: helper_result.selected_display_label,
        active_display_label: helper_result.active_display_label,
    }))
}

#[post("/android-native/session-event")]
pub async fn post_android_native_session_event(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeSessionEventRequest>,
) -> Result<Json<PostAndroidNativeSessionEventResponse>, AppError> {
    let (event, event_count) = app
        .record_android_native_session_event(
            request.launch_token.as_deref(),
            request.token_id.as_deref(),
            request.session_id.as_deref(),
            request.event_name,
            request.stage,
            request.detail,
            request.client_time_unix_ms,
        )
        .await?;

    if event.event_name == "windows_native_diagnostic_report"
        && let Err(error) = persist_windows_native_diagnostic_report(&event)
    {
        log::warn!(
            "failed to persist windows native diagnostic report: session_id={} token_id={} error={}",
            event.session_id,
            event.token_id,
            error
        );
    }

    Ok(Json(PostAndroidNativeSessionEventResponse {
        token_id: event.token_id,
        session_id: event.session_id,
        event_count,
        sequence: event.sequence,
        recorded_at_unix_ms: event.recorded_at_unix_ms,
    }))
}

#[post("/android-native/session-lifecycle")]
pub async fn post_android_native_session_lifecycle(
    app: Data<App>,
    Json(request): Json<PostAndroidNativeSessionLifecycleRequest>,
) -> Result<Json<PostAndroidNativeSessionLifecycleResponse>, AppError> {
    let cleanup_action = matches!(
        request.action.as_str(),
        "session_revoked"
            | "session_expired_cleanup"
            | "session_failed_cleanup"
            | "session_abandoned_cleanup"
    );
    let cleanup_reason = request
        .detail
        .clone()
        .unwrap_or_else(|| request.action.clone());
    let (event, lifecycle_state) = app
        .update_android_native_session_lifecycle(
            request.launch_token.as_deref(),
            request.token_id.as_deref(),
            request.session_id.as_deref(),
            request.action,
            request.detail,
            request.client_time_unix_ms,
        )
        .await?;

    if cleanup_action {
        let cleanup_session_id = event.session_id.clone();
        tokio::spawn(async move {
            cleanup_android_native_stream_session(&cleanup_session_id, &cleanup_reason).await;
        });
    }

    Ok(Json(PostAndroidNativeSessionLifecycleResponse {
        token_id: event.token_id,
        session_id: event.session_id,
        recorded_at_unix_ms: event.recorded_at_unix_ms,
        lifecycle_state: AndroidNativeSessionLifecycleState {
            trust_bootstrap_status: lifecycle_state.trust_bootstrap_status,
            session_status: lifecycle_state.session_status,
            hidden_state_status: lifecycle_state.hidden_state_status,
            last_action: lifecycle_state.last_action,
            last_reason: lifecycle_state.last_reason,
            last_updated_unix_ms: lifecycle_state.last_updated_unix_ms,
            completed_at_unix_ms: lifecycle_state.completed_at_unix_ms,
        },
    }))
}
