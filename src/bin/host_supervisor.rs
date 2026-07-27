#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    ffi::OsString,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use common::config::Config;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState as WindowsServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Console::{FreeConsole, GetConsoleWindow},
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
            PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
        },
    },
    UI::WindowsAndMessaging::{
        IDC_ARROW, LoadCursorW, SPI_SETCURSORS, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SW_HIDE,
        SetCursor, ShowCursor, ShowWindow, SystemParametersInfoW,
    },
};

#[cfg(windows)]
static SUPERVISOR_JOB_HANDLE: OnceLock<usize> = OnceLock::new();
#[cfg(windows)]
static SERVICE_BUNDLE_ROOT: OnceLock<PathBuf> = OnceLock::new();
#[cfg(windows)]
static SERVICE_NAME_OVERRIDE: OnceLock<String> = OnceLock::new();

#[cfg(windows)]
define_windows_service!(ffi_service_main, service_main_entry);

#[cfg(windows)]
fn restore_windows_cursor_defaults() {
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_SETCURSORS,
            0,
            std::ptr::null_mut(),
            SPIF_SENDCHANGE | SPIF_UPDATEINIFILE,
        );
        let cursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        if !cursor.is_null() {
            let _ = SetCursor(cursor);
        }
        while ShowCursor(1) < 0 {}
    }
}

#[cfg(not(windows))]
fn restore_windows_cursor_defaults() {}

#[cfg(windows)]
fn hide_console_window_for_background_command(command: &SupervisorCommand) {
    if !matches!(
        command,
        SupervisorCommand::RunDaemon | SupervisorCommand::RunService { .. }
    ) {
        return;
    }

    unsafe {
        let console = GetConsoleWindow();
        if !console.is_null() {
            let _ = ShowWindow(console, SW_HIDE);
            let _ = FreeConsole();
        }
    }
}

#[cfg(not(windows))]
fn hide_console_window_for_background_command(_command: &SupervisorCommand) {}

#[cfg(windows)]
fn apply_background_spawn_flags(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_background_spawn_flags(_command: &mut Command) {}

#[derive(Parser)]
#[command(version, about = "Cloudgime host supervisor")]
struct Cli {
    #[arg(long)]
    bundle_root: Option<PathBuf>,

    #[command(subcommand)]
    command: SupervisorCommand,
}

#[derive(Subcommand)]
enum SupervisorCommand {
    StartBundle,
    StopBundle,
    RestartRuntime,
    #[command(hide = true)]
    RecoverRuntime {
        #[arg(long)]
        policy: String,
    },
    #[command(hide = true)]
    RecoverFailure {
        #[arg(long)]
        strategy: String,
        #[arg(long)]
        reason: String,
    },
    Status,
    #[command(hide = true)]
    RunDaemon,
    #[command(hide = true)]
    ShutdownDaemon,
    #[command(hide = true)]
    RunService {
        #[arg(long)]
        service_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DaemonCommandKind {
    StartBundle,
    StopBundle,
    RestartRuntime,
    RecoverRuntime {
        policy: RecoveryPolicy,
    },
    RecoverFailure {
        strategy: FailureRecoveryStrategy,
        reason: String,
    },
    Shutdown,
}

impl DaemonCommandKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::StartBundle => "start_bundle",
            Self::StopBundle => "stop_bundle",
            Self::RestartRuntime => "restart_runtime",
            Self::RecoverRuntime { .. } => "recover_runtime",
            Self::RecoverFailure { .. } => "recover_failure",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RecoveryPolicy {
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

impl RecoveryPolicy {
    fn from_cli(policy: &str) -> Result<Self> {
        match policy.trim().to_ascii_lowercase().as_str() {
            "display_transition" => Ok(Self::DisplayTransition),
            "device_match" => Ok(Self::DeviceMatch),
            "video_stall" => Ok(Self::VideoStall),
            "route_failure" => Ok(Self::RouteFailure),
            "startup_recovery" => Ok(Self::StartupRecovery),
            "resume_recovery" => Ok(Self::ResumeRecovery),
            "settings" => Ok(Self::Settings),
            "manual" => Ok(Self::Manual),
            "generic" => Ok(Self::Generic),
            other => bail!("unsupported recovery policy: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FailureRecoveryStrategy {
    RestartRuntime,
    RestartBundle,
    None,
}

impl FailureRecoveryStrategy {
    fn from_cli(strategy: &str) -> Result<Self> {
        match strategy.trim().to_ascii_lowercase().as_str() {
            "restart_runtime" => Ok(Self::RestartRuntime),
            "restart_bundle" => Ok(Self::RestartBundle),
            "none" => Ok(Self::None),
            other => bail!("unsupported failure recovery strategy: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::RestartRuntime => "restart_runtime",
            Self::RestartBundle => "restart_bundle",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FailureRecoveryPlan {
    effective_strategy: FailureRecoveryStrategy,
    attempt_count: u32,
    escalated: bool,
    exhausted: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum LifecyclePhase {
    #[default]
    Idle,
    Starting,
    Ready,
    Recovering,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonCommandFile {
    id: u64,
    created_at_unix_ms: u64,
    command: DaemonCommandKind,
}

#[derive(Debug, Clone)]
struct BundlePaths {
    bundle_root: PathBuf,
    moonlight_dir: PathBuf,
    frp_dir: PathBuf,
    config_path: PathBuf,
    activation_state_path: PathBuf,
    public_url_path: PathBuf,
    selected_runtime_path: PathBuf,
    helper_path: PathBuf,
    command_path: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SupervisorState {
    #[serde(default)]
    lifecycle_phase: LifecyclePhase,
    #[serde(default)]
    lifecycle_reason: Option<String>,
    #[serde(default)]
    lifecycle_updated_at_unix_ms: Option<u64>,
    #[serde(default)]
    failure_recovery_attempt_count: u32,
    #[serde(default)]
    failure_recovery_window_started_at_unix_ms: Option<u64>,
    #[serde(default)]
    last_failure_recovery_reason: Option<String>,
    #[serde(default)]
    last_failure_recovery_strategy: Option<String>,
    #[serde(default)]
    last_failure_recovery_escalated: bool,
    #[serde(default)]
    total_failure_recovery_count: u32,
    #[serde(default)]
    total_failure_recovery_escalation_count: u32,
    #[serde(default)]
    total_service_watchdog_trigger_count: u32,
    #[serde(default)]
    daemon_started_at_unix_ms: Option<u64>,
    #[serde(default)]
    boot_failure_recovery_count: u32,
    #[serde(default)]
    boot_service_watchdog_trigger_count: u32,
    #[serde(default)]
    ready_since_unix_ms: Option<u64>,
    #[serde(default)]
    last_failure_recovery_completed_at_unix_ms: Option<u64>,
    #[serde(default)]
    last_failure_recovery_budget_cleared_at_unix_ms: Option<u64>,
    #[serde(default)]
    last_service_watchdog_reason: Option<String>,
    #[serde(default)]
    last_service_watchdog_at_unix_ms: Option<u64>,
    #[serde(default)]
    recent_incidents: Vec<SupervisorIncident>,
    daemon_pid: Option<u32>,
    sunshine_pid: Option<u32>,
    web_server_pid: Option<u32>,
    frpc_pid: Option<u32>,
    runtime_key: Option<String>,
    last_command_id: Option<u64>,
    last_command_name: Option<String>,
    last_command_status: Option<String>,
    last_command_error: Option<String>,
    last_command_started_at_unix_ms: Option<u64>,
    last_command_finished_at_unix_ms: Option<u64>,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SupervisorIncident {
    kind: String,
    reason: String,
    strategy: Option<String>,
    escalated: bool,
    at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct StatusSnapshot {
    daemon_running: bool,
    runtime_key: String,
    runtime_dir: String,
    host_port: u16,
    web_bind_address: String,
    running_processes: Vec<ProcessSnapshot>,
    state: SupervisorState,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessSnapshot {
    pid: u32,
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    application_activation_id: String,
    application_type: String,
    pc_label: String,
    credential_ref: String,
    host_http_port: i32,
    host_stream_udp_start: i32,
    host_stream_udp_end: i32,
    host_stream_proxy_route: String,
    license_assignments: Value,
    license_policies: Value,
    runtime_token: String,
    activation_record_id: String,
    activated_at_utc: String,
    redeemed_at_utc: String,
    last_heartbeat_at_utc: String,
    last_ready_for_stream: bool,
    updated_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct HostActivationStatusPayload {
    ok: bool,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    host_id: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    activation_state: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    display_name: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    host_slug: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    canonical_hostname: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    canonical_public_url: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    fallback_public_url: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    control_plane_url: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    activation_record_id: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    runtime_token_hint: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    sentinel_pc_id: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    sentinel_device_id: String,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    keeper_entry_id: String,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct HostHeartbeatPayload {
    ok: bool,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    host_id: String,
    #[serde(
        alias = "activationStatus",
        deserialize_with = "deserialize_string_or_default"
    )]
    activation_state: String,
    ready_for_stream: bool,
    next_heartbeat_in_sec: u64,
    #[serde(deserialize_with = "deserialize_string_or_default")]
    last_heartbeat_at_utc: String,
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct HostCapabilityGpuControllerSnapshot {
    name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct HostCapabilityRuntimeCandidateSnapshot {
    key: String,
    startup_validation_status: Option<String>,
    startup_validation_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct HostCapabilityProfileSnapshot {
    selected_runtime_key: String,
    selected_encoder: String,
    selected_capture: String,
    selected_capture_reason: Option<String>,
    gpu_controllers: Vec<HostCapabilityGpuControllerSnapshot>,
    runtime_candidates: Vec<HostCapabilityRuntimeCandidateSnapshot>,
}

const FAILURE_RECOVERY_WINDOW_MS: u64 = 180_000;
const MAX_RUNTIME_RECOVERY_ATTEMPTS: u32 = 3;
const MAX_BUNDLE_RECOVERY_ATTEMPTS: u32 = 5;
const STABLE_RECOVERY_CLEAR_MS: u64 = 30_000;
const MAX_RECENT_INCIDENTS: usize = 8;
const ACTIVATION_STATUS_SYNC_INTERVAL_SECS: u64 = 15;
const ACTIVATION_HEARTBEAT_INTERVAL_SECS: u64 = 30;
const LOW_VRAM_GPU_TOTAL_MIB: u64 = 1536;
const LOW_VRAM_GPU_FREE_MIB: u64 = 512;

fn push_recent_incident(
    state: &mut SupervisorState,
    kind: &str,
    reason: &str,
    strategy: Option<&str>,
    escalated: bool,
    at_unix_ms: u64,
) {
    state.recent_incidents.insert(
        0,
        SupervisorIncident {
            kind: kind.to_string(),
            reason: reason.to_string(),
            strategy: strategy.map(ToOwned::to_owned),
            escalated,
            at_unix_ms,
        },
    );
    if state.recent_incidents.len() > MAX_RECENT_INCIDENTS {
        state.recent_incidents.truncate(MAX_RECENT_INCIDENTS);
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    hide_console_window_for_background_command(&cli.command);
    let bundle_root = resolve_bundle_root(cli.bundle_root)?;
    let paths = BundlePaths::new(bundle_root)?;

    match cli.command {
        SupervisorCommand::StartBundle => {
            request_daemon_operation(&paths, DaemonCommandKind::StartBundle)
        }
        SupervisorCommand::StopBundle => {
            request_daemon_operation(&paths, DaemonCommandKind::StopBundle)
        }
        SupervisorCommand::RestartRuntime => {
            request_daemon_operation(&paths, DaemonCommandKind::RestartRuntime)
        }
        SupervisorCommand::RecoverRuntime { policy } => request_daemon_operation(
            &paths,
            DaemonCommandKind::RecoverRuntime {
                policy: RecoveryPolicy::from_cli(&policy)?,
            },
        ),
        SupervisorCommand::RecoverFailure { strategy, reason } => request_daemon_operation(
            &paths,
            DaemonCommandKind::RecoverFailure {
                strategy: FailureRecoveryStrategy::from_cli(&strategy)?,
                reason,
            },
        ),
        SupervisorCommand::Status => {
            let status = build_status_snapshot(&paths)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        SupervisorCommand::RunDaemon => run_daemon(&paths),
        SupervisorCommand::ShutdownDaemon => {
            request_daemon_operation(&paths, DaemonCommandKind::Shutdown)
        }
        SupervisorCommand::RunService { service_name } => run_service_dispatcher(
            &paths,
            service_name.unwrap_or_else(|| default_service_name(&paths)),
        ),
    }
}

impl BundlePaths {
    fn new(bundle_root: PathBuf) -> Result<Self> {
        let moonlight_dir = bundle_root.join("moonlight");
        let moonlight_server_dir = moonlight_dir.join("server");
        let config_path = moonlight_server_dir.join("config.json");
        if !config_path.exists() {
            bail!("missing config.json at {}", config_path.display());
        }

        Ok(Self {
            frp_dir: bundle_root.join("frp"),
            activation_state_path: moonlight_server_dir.join("host_activation_state.json"),
            public_url_path: bundle_root.join("PUBLIC_URL.txt"),
            selected_runtime_path: moonlight_server_dir.join("selected_sunshine_runtime.txt"),
            helper_path: moonlight_server_dir.join("display-prepare-helper.exe"),
            command_path: moonlight_server_dir.join("host_supervisor_command.json"),
            state_path: moonlight_server_dir.join("host_supervisor_state.json"),
            log_path: moonlight_server_dir.join("host_supervisor.log"),
            bundle_root,
            moonlight_dir,
            config_path,
        })
    }
}

fn resolve_bundle_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(bundle_root) = explicit {
        return canonicalize_without_verbatim_prefix(bundle_root);
    }

    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let runtime_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve supervisor runtime directory"))?;
    let bundle_root = runtime_dir
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve supervisor bundle root"))?;
    canonicalize_without_verbatim_prefix(bundle_root.to_path_buf())
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

#[cfg(windows)]
fn run_service_dispatcher(paths: &BundlePaths, service_name: String) -> Result<()> {
    let _ = SERVICE_BUNDLE_ROOT.set(paths.bundle_root.clone());
    let _ = SERVICE_NAME_OVERRIDE.set(service_name.clone());
    service_dispatcher::start(service_name, ffi_service_main)
        .context("failed to start Windows service dispatcher")
}

#[cfg(not(windows))]
fn run_service_dispatcher(_paths: &BundlePaths, _service_name: String) -> Result<()> {
    bail!("run-service is only supported on Windows")
}

#[cfg(windows)]
fn service_main_entry(_arguments: Vec<OsString>) {
    if let Err(err) = run_service_main() {
        if let Some(bundle_root) = SERVICE_BUNDLE_ROOT.get() {
            if let Ok(paths) = BundlePaths::new(bundle_root.clone()) {
                let _ =
                    append_supervisor_log(&paths, &format!("service_main_entry error: {err:#}"));
            }
        }
        eprintln!("{err:#}");
    }
}

#[cfg(windows)]
fn run_service_main() -> Result<()> {
    let bundle_root = SERVICE_BUNDLE_ROOT
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("service bundle root was not initialized"))?;
    let service_name = SERVICE_NAME_OVERRIDE
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("service name was not initialized"))?;
    let paths = BundlePaths::new(bundle_root)?;
    append_supervisor_log(
        &paths,
        &format!(
            "run_service_main begin service_name={service_name} bundle_root={}",
            paths.bundle_root.display()
        ),
    )?;
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_requested_for_handler = Arc::clone(&stop_requested);

    let status_handle =
        service_control_handler::register(service_name.clone(), move |control_event| {
            match control_event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    stop_requested_for_handler.store(true, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        })
        .context("failed to register Windows service control handler")?;
    append_supervisor_log(&paths, "service control handler registered")?;

    update_service_status(
        &status_handle,
        WindowsServiceState::StartPending,
        ServiceControlAccept::empty(),
    )?;
    append_supervisor_log(&paths, "service status START_PENDING")?;
    let startup_done = Arc::new(AtomicBool::new(false));
    let startup_done_for_thread = Arc::clone(&startup_done);
    let status_handle_for_thread = status_handle.clone();
    let startup_progress = thread::spawn(move || {
        let mut checkpoint = 1;
        while !startup_done_for_thread.load(Ordering::SeqCst) {
            let _ = update_service_status_ex(
                &status_handle_for_thread,
                WindowsServiceState::StartPending,
                ServiceControlAccept::empty(),
                ServiceExitCode::Win32(0),
                checkpoint,
                Duration::from_secs(30),
            );
            checkpoint += 1;
            sleep(Duration::from_secs(1));
        }
    });

    let startup_activation = read_activation_state(&paths);
    if activation_allows_runtime(&paths, &startup_activation) {
        if let Err(err) = ensure_user_session_daemon_running(&paths)
            .and_then(|_| request_user_daemon_operation(&paths, DaemonCommandKind::StartBundle))
        {
            append_supervisor_log(&paths, &format!("service startup failed: {err:#}"))?;
            startup_done.store(true, Ordering::SeqCst);
            let _ = startup_progress.join();
            let _ = update_service_status_ex(
                &status_handle,
                WindowsServiceState::Stopped,
                ServiceControlAccept::empty(),
                ServiceExitCode::Win32(1),
                0,
                Duration::default(),
            );
            return Err(err);
        }
    } else {
        append_supervisor_log(
            &paths,
            &format!(
                "service startup skipped bundle launch because activation_state={}",
                activation_state_label(&startup_activation)
            ),
        )?;
    }
    startup_done.store(true, Ordering::SeqCst);
    let _ = startup_progress.join();
    append_supervisor_log(&paths, "service startup completed")?;

    update_service_status(
        &status_handle,
        WindowsServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    )?;
    append_supervisor_log(&paths, "service status RUNNING")?;
    let config = load_config(&paths)?;
    let mut last_watchdog_tick = Instant::now();
    let mut last_watchdog_recovery_at: Option<Instant> = None;
    let mut last_activation_status_sync_at: Option<Instant> = None;
    let mut last_activation_heartbeat_at: Option<Instant> = None;
    while !stop_requested.load(Ordering::SeqCst) {
        if last_watchdog_tick.elapsed() >= Duration::from_secs(5) {
            if let Err(err) = service_watchdog_tick(&paths, &config, &mut last_watchdog_recovery_at)
            {
                append_supervisor_log(&paths, &format!("service watchdog error: {err:#}"))?;
            }
            if let Err(err) = service_activation_tick(
                &paths,
                &config,
                &mut last_activation_status_sync_at,
                &mut last_activation_heartbeat_at,
            ) {
                append_supervisor_log(&paths, &format!("service activation sync error: {err:#}"))?;
            }
            last_watchdog_tick = Instant::now();
        }
        sleep(Duration::from_millis(250));
    }
    append_supervisor_log(&paths, "service stop requested")?;

    update_service_status(
        &status_handle,
        WindowsServiceState::StopPending,
        ServiceControlAccept::empty(),
    )?;
    append_supervisor_log(&paths, "service status STOP_PENDING")?;
    let _ = request_user_daemon_operation(&paths, DaemonCommandKind::StopBundle);
    update_service_status(
        &status_handle,
        WindowsServiceState::Stopped,
        ServiceControlAccept::empty(),
    )?;
    append_supervisor_log(&paths, "service status STOPPED")?;
    Ok(())
}

#[cfg(windows)]
fn service_watchdog_tick(
    paths: &BundlePaths,
    config: &Config,
    last_watchdog_recovery_at: &mut Option<Instant>,
) -> Result<()> {
    maybe_restore_host_cursor_defaults_if_idle(paths)?;

    let activation = read_activation_state(paths);
    if !activation_allows_runtime(paths, &activation) {
        stop_runtime_when_activation_locked(paths, &activation)?;
        return Ok(());
    }

    if !daemon_is_running(paths) {
        if watchdog_recovery_allowed(last_watchdog_recovery_at, Duration::from_secs(15)) {
            record_service_watchdog_trigger(paths, "service_watchdog_daemon_missing")?;
            append_supervisor_log(paths, "service_watchdog daemon missing -> start_bundle")?;
            ensure_user_session_daemon_running(paths)?;
            request_user_daemon_operation(paths, DaemonCommandKind::StartBundle)?;
            *last_watchdog_recovery_at = Some(Instant::now());
        }
        return Ok(());
    }

    let mut state = read_state(paths).unwrap_or_default();
    let required_processes_ready = bundle_has_required_processes(paths)?;
    let local_http_ready = local_http_ready(config)?;
    normalize_failed_phase_if_runtime_healthy(
        paths,
        &mut state,
        required_processes_ready,
        local_http_ready,
    )?;
    maybe_clear_stable_recovery_budget(paths, &state, required_processes_ready, local_http_ready)?;
    let phase_recent = state
        .lifecycle_updated_at_unix_ms
        .is_some_and(|value| now_unix_ms().saturating_sub(value) <= 60_000);

    let needs_recovery = if matches!(state.lifecycle_phase, LifecyclePhase::Failed) {
        Some("service_watchdog_failed_phase".to_string())
    } else if !required_processes_ready {
        if matches!(
            state.lifecycle_phase,
            LifecyclePhase::Starting | LifecyclePhase::Recovering | LifecyclePhase::Stopping
        ) && phase_recent
        {
            None
        } else {
            Some("service_watchdog_processes_unhealthy".to_string())
        }
    } else if !local_http_ready {
        if matches!(
            state.lifecycle_phase,
            LifecyclePhase::Starting | LifecyclePhase::Recovering | LifecyclePhase::Stopping
        ) && phase_recent
        {
            None
        } else {
            Some("service_watchdog_local_http_unhealthy".to_string())
        }
    } else {
        None
    };

    let Some(reason) = needs_recovery else {
        return Ok(());
    };

    if !watchdog_recovery_allowed(last_watchdog_recovery_at, Duration::from_secs(20)) {
        return Ok(());
    }

    record_service_watchdog_trigger(paths, &reason)?;
    append_supervisor_log(
        paths,
        &format!(
            "service_watchdog recovering reason={} phase={:?} required_processes_ready={} local_http_ready={}",
            reason, state.lifecycle_phase, required_processes_ready, local_http_ready
        ),
    )?;
    request_user_daemon_operation(
        paths,
        DaemonCommandKind::RecoverFailure {
            strategy: FailureRecoveryStrategy::RestartBundle,
            reason,
        },
    )?;
    *last_watchdog_recovery_at = Some(Instant::now());
    Ok(())
}

#[cfg(windows)]
fn service_activation_tick(
    paths: &BundlePaths,
    config: &Config,
    last_status_sync_at: &mut Option<Instant>,
    last_heartbeat_at: &mut Option<Instant>,
) -> Result<()> {
    let mut activation = read_activation_state(paths);
    if activation.host_id.trim().is_empty()
        || activation.runtime_token.trim().is_empty()
        || activation.control_plane_url.trim().is_empty()
    {
        return Ok(());
    }

    if last_status_sync_at.is_none_or(|last| {
        last.elapsed() >= Duration::from_secs(ACTIVATION_STATUS_SYNC_INTERVAL_SECS)
    }) {
        *last_status_sync_at = Some(Instant::now());
        sync_activation_status(paths, &mut activation)?;
    }

    if !activation_allows_runtime(paths, &activation) {
        stop_runtime_when_activation_locked(paths, &activation)?;
        return Ok(());
    }

    if last_heartbeat_at.is_none_or(|last| {
        last.elapsed() >= Duration::from_secs(ACTIVATION_HEARTBEAT_INTERVAL_SECS)
    }) {
        *last_heartbeat_at = Some(Instant::now());
        send_activation_heartbeat(paths, config, &mut activation)?;
    }

    Ok(())
}

fn normalize_failed_phase_if_runtime_healthy(
    paths: &BundlePaths,
    state: &mut SupervisorState,
    required_processes_ready: bool,
    local_http_ready: bool,
) -> Result<()> {
    if !matches!(state.lifecycle_phase, LifecyclePhase::Failed) {
        return Ok(());
    }
    if !required_processes_ready || !local_http_ready {
        return Ok(());
    }

    let now = now_unix_ms();
    state.lifecycle_phase = LifecyclePhase::Ready;
    state.lifecycle_reason = Some("service_watchdog_normalized_ready_phase".to_string());
    state.lifecycle_updated_at_unix_ms = Some(now);
    state.ready_since_unix_ms.get_or_insert(now);
    state.updated_at_unix_ms = now;
    write_state(paths, state.clone())?;
    append_supervisor_log(
        paths,
        "service_watchdog normalized failed lifecycle to ready because runtime probes are healthy",
    )?;
    Ok(())
}

#[cfg(windows)]
fn sync_activation_status(
    paths: &BundlePaths,
    activation: &mut HostActivationStateRecord,
) -> Result<()> {
    let control_plane_url = normalize_control_plane_url(&activation.control_plane_url);
    if control_plane_url.is_empty() {
        return Ok(());
    }

    let client = Client::builder()
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build activation status client")?;
    let response = client
        .post(format!("{control_plane_url}/api/v1/host-activation/status"))
        .version(reqwest::Version::HTTP_2)
        .json(&serde_json::json!({
            "hostId": activation.host_id.trim(),
            "machineIdentity": empty_to_none(&activation.machine_identity),
            "installInstanceId": empty_to_none(&activation.install_instance_id),
            "runtimeToken": activation.runtime_token.trim(),
            "activationRecordId": empty_to_none(&activation.activation_record_id),
            "sentinelPcId": empty_to_none(&activation.sentinel_pc_id),
            "sentinelDeviceId": empty_to_none(&activation.sentinel_device_id),
            "keeperEntryId": empty_to_none(&activation.keeper_entry_id),
            "pcLabel": empty_to_none(&activation.pc_label),
        }))
        .send();

    let response = match response {
        Ok(value) => value,
        Err(err) => {
            append_supervisor_log(paths, &format!("activation status sync skipped: {err}"))?;
            return Ok(());
        }
    };

    let status_code = response.status();
    let raw = response.text().unwrap_or_default();
    let payload = match serde_json::from_str::<HostActivationStatusPayload>(&raw) {
        Ok(value) => value,
        Err(err) => {
            append_supervisor_log(
                paths,
                &format!(
                    "activation status payload parse failed: {err}; raw={}",
                    raw.chars().take(600).collect::<String>()
                ),
            )?;
            HostActivationStatusPayload::default()
        }
    };

    if status_code.as_u16() == 404 && !payload.ok && payload.activation_state.trim().is_empty() {
        append_supervisor_log(
            paths,
            "legacy activation status endpoint unavailable; keeping local activation state",
        )?;
        return Ok(());
    }

    if status_code.is_success() && payload.ok {
        let previous_state = activation.activation_state.clone();
        let next_state = normalize_activation_state(payload.activation_state.as_str(), "activated");
        activation.activation_state = next_state.clone();
        if !payload.display_name.trim().is_empty() {
            activation.display_name = payload.display_name.trim().to_string();
        }
        if !payload.control_plane_url.trim().is_empty() {
            activation.control_plane_url = normalize_control_plane_url(&payload.control_plane_url);
        }
        if !payload.activation_record_id.trim().is_empty() {
            activation.activation_record_id = payload.activation_record_id.trim().to_string();
        }
        if !payload.sentinel_pc_id.trim().is_empty() {
            activation.sentinel_pc_id = payload.sentinel_pc_id.trim().to_string();
        }
        if !payload.sentinel_device_id.trim().is_empty() {
            activation.sentinel_device_id = payload.sentinel_device_id.trim().to_string();
        }
        if !payload.keeper_entry_id.trim().is_empty() {
            activation.keeper_entry_id = payload.keeper_entry_id.trim().to_string();
        }
        sync_canonical_public_route(paths, &payload)?;
        activation.updated_at_utc = now_rfc3339();
        save_activation_state(paths, activation)?;
        if !previous_state.eq_ignore_ascii_case(&next_state) {
            append_supervisor_log(
                paths,
                &format!("activation status updated: {previous_state} -> {next_state}"),
            )?;
        }
        return Ok(());
    }

    let next_state =
        derive_next_activation_state(status_code.as_u16(), payload.activation_state.as_str());
    let previous_state = activation.activation_state.clone();
    activation.activation_state = next_state.clone();
    activation.last_ready_for_stream = false;
    activation.updated_at_utc = now_rfc3339();
    save_activation_state(paths, activation)?;
    if !previous_state.eq_ignore_ascii_case(&next_state) {
        append_supervisor_log(
            paths,
            &format!(
                "activation status forced local state {} -> {} (http {} {})",
                previous_state,
                next_state,
                status_code.as_u16(),
                payload
                    .error
                    .clone()
                    .or(payload.message.clone())
                    .unwrap_or_else(|| "activation sync state change".to_string())
            ),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn read_host_capability_profile_snapshot(
    paths: &BundlePaths,
) -> Option<HostCapabilityProfileSnapshot> {
    let profile_path = paths
        .helper_path
        .parent()
        .unwrap_or(paths.moonlight_dir.as_path())
        .join("host_capability_profile.json");
    let raw = fs::read(profile_path).ok()?;
    serde_json::from_slice::<HostCapabilityProfileSnapshot>(&raw).ok()
}

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
fn evaluate_stream_display_route(
    profile: Option<&HostCapabilityProfileSnapshot>,
) -> (bool, Option<String>) {
    let Some(profile) = profile else {
        return (
            false,
            Some("Display capability profile is missing. Refresh Host Control first.".to_string()),
        );
    };

    if let Some(candidate) = selected_runtime_candidate(profile)
        && let Some(status) = candidate
            .startup_validation_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        && !status.eq_ignore_ascii_case("passed")
    {
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

#[cfg(windows)]
fn build_stream_not_ready_note(
    display_route_note: Option<&str>,
    lifecycle_phase: &LifecyclePhase,
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
    if !matches!(lifecycle_phase, LifecyclePhase::Ready) {
        return format!("Host lifecycle is {}.", lifecycle_phase.as_str());
    }
    if public_url.trim().is_empty() {
        return "Public stream route is not ready yet.".to_string();
    }

    "Host is still preparing the stream route.".to_string()
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn send_host_runtime_heartbeat(
    paths: &BundlePaths,
    client: &Client,
    control_plane_url: &str,
    activation: &mut HostActivationStateRecord,
    state: &SupervisorState,
    ready_for_stream: bool,
    note: &str,
    public_url: &str,
    local_http_ready: bool,
    required_processes_ready: bool,
) -> Result<bool> {
    let runtime_token = activation.runtime_token.trim().to_string();
    if runtime_token.is_empty() {
        return Ok(false);
    }

    let runtime_status = if matches!(state.lifecycle_phase, LifecyclePhase::Ready) {
        "RUNNING"
    } else {
        "STARTING"
    };
    let stream_readiness_status = if ready_for_stream {
        "READY"
    } else {
        "NOT_READY"
    };
    let heartbeat_body = serde_json::json!({
        "runtimeStatus": runtime_status,
        "streamReadinessStatus": stream_readiness_status,
        "lifecyclePhase": state.lifecycle_phase.as_str(),
        "healthGrade": if ready_for_stream { Some("healthy".to_string()) } else { None::<String> },
        "runtimeDisplayName": runtime_display_name(read_selected_runtime_key(paths).as_str()),
        "publicUrl": empty_to_none(public_url),
        "serviceState": Some("running".to_string()),
        "localHttpReady": local_http_ready,
        "requiredProcessesReady": required_processes_ready,
        "readyForStream": ready_for_stream,
        "note": Some(note.to_string()),
    });
    let response = client
        .post(format!("{control_plane_url}/api/v1/host/heartbeat"))
        .version(reqwest::Version::HTTP_2)
        .bearer_auth(runtime_token.as_str())
        .json(&heartbeat_body)
        .send();

    let response = match response {
        Ok(value) => value,
        Err(err) => {
            append_supervisor_log(paths, &format!("host runtime heartbeat skipped: {err}"))?;
            if send_host_runtime_heartbeat_with_curl(
                paths,
                control_plane_url,
                runtime_token.as_str(),
                &heartbeat_body,
                activation,
            )? {
                return Ok(true);
            }
            return Ok(false);
        }
    };

    let status_code = response.status();
    let raw = response.text().unwrap_or_default();
    let payload = serde_json::from_str::<HostHeartbeatPayload>(&raw).unwrap_or_default();
    if status_code.is_success() && payload.ok {
        apply_host_runtime_heartbeat_payload(paths, activation, &payload)?;
        return Ok(true);
    }

    if status_code.as_u16() == 404
        && send_host_runtime_heartbeat_with_curl(
            paths,
            control_plane_url,
            runtime_token.as_str(),
            &heartbeat_body,
            activation,
        )?
    {
        return Ok(true);
    }

    append_supervisor_log(
        paths,
        &format!(
            "host runtime heartbeat rejected; falling back to legacy activation heartbeat (http {} {})",
            status_code.as_u16(),
            payload
                .error
                .or(payload.message)
                .unwrap_or_else(|| "runtime heartbeat unavailable".to_string())
        ),
    )?;
    Ok(false)
}

#[cfg(windows)]
fn apply_host_runtime_heartbeat_payload(
    paths: &BundlePaths,
    activation: &mut HostActivationStateRecord,
    payload: &HostHeartbeatPayload,
) -> Result<()> {
    activation.last_heartbeat_at_utc = payload.last_heartbeat_at_utc.trim().to_string();
    activation.last_ready_for_stream = payload.ready_for_stream;
    activation.activation_state =
        normalize_activation_state(payload.activation_state.as_str(), "activated");
    activation.updated_at_utc = now_rfc3339();
    save_activation_state(paths, activation)
}

#[cfg(windows)]
fn send_host_runtime_heartbeat_with_curl(
    paths: &BundlePaths,
    control_plane_url: &str,
    runtime_token: &str,
    heartbeat_body: &Value,
    activation: &mut HostActivationStateRecord,
) -> Result<bool> {
    if Command::new("curl.exe").arg("--version").output().is_err() {
        append_supervisor_log(paths, "host runtime heartbeat curl fallback unavailable")?;
        return Ok(false);
    }

    let body_path = std::env::temp_dir().join(format!(
        "cloudgime-host-runtime-heartbeat-{}.json",
        now_unix_ms()
    ));
    fs::write(&body_path, serde_json::to_vec(heartbeat_body)?)?;

    let url = format!("{control_plane_url}/api/v1/host/heartbeat");
    let data_arg = format!("@{}", body_path.display());
    let mut command = Command::new("curl.exe");
    command
        .arg("--silent")
        .arg("--show-error")
        .arg("--request")
        .arg("POST")
        .arg(url)
        .arg("--header")
        .arg("Content-Type: application/json")
        .arg("--data-binary")
        .arg(data_arg)
        .arg("--write-out")
        .arg("\n%{http_code}")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().context("failed to start curl fallback")?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "header = \"Authorization: Bearer {runtime_token}\"")?;
    }
    let output = child
        .wait_with_output()
        .context("failed to wait for curl fallback")?;
    let _ = fs::remove_file(&body_path);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let Some((body, status_raw)) = stdout.rsplit_once('\n') else {
        append_supervisor_log(
            paths,
            &format!("host runtime heartbeat curl fallback returned malformed output: {stderr}"),
        )?;
        return Ok(false);
    };
    let status_code = status_raw.trim().parse::<u16>().unwrap_or_default();
    let payload = serde_json::from_str::<HostHeartbeatPayload>(body).unwrap_or_default();
    if (200..300).contains(&status_code) && payload.ok {
        apply_host_runtime_heartbeat_payload(paths, activation, &payload)?;
        append_supervisor_log(
            paths,
            "host runtime heartbeat accepted via curl http2 fallback",
        )?;
        return Ok(true);
    }

    let reason = payload.error.or(payload.message).unwrap_or_else(|| {
        if stderr.is_empty() {
            "runtime heartbeat unavailable".to_string()
        } else {
            stderr
        }
    });
    append_supervisor_log(
        paths,
        &format!("host runtime heartbeat curl fallback rejected (http {status_code} {reason})"),
    )?;
    Ok(false)
}

#[cfg(windows)]
fn send_activation_heartbeat(
    paths: &BundlePaths,
    config: &Config,
    activation: &mut HostActivationStateRecord,
) -> Result<()> {
    let control_plane_url = normalize_control_plane_url(&activation.control_plane_url);
    if control_plane_url.is_empty() {
        return Ok(());
    }

    let state = read_state(paths).unwrap_or_default();
    let capability_profile = read_host_capability_profile_snapshot(paths);
    let required_processes_ready = bundle_has_required_processes(paths)?;
    let local_http_ready = local_http_ready(config)?;
    let public_url = read_public_url(paths);
    let (display_route_ready, display_route_note) =
        evaluate_stream_display_route(capability_profile.as_ref());
    let ready_for_stream = display_route_ready
        && matches!(state.lifecycle_phase, LifecyclePhase::Ready)
        && required_processes_ready
        && local_http_ready
        && !public_url.is_empty();
    let note = if ready_for_stream {
        "ready_for_stream".to_string()
    } else {
        build_stream_not_ready_note(
            display_route_note.as_deref(),
            &state.lifecycle_phase,
            required_processes_ready,
            local_http_ready,
            &public_url,
        )
    };

    let client = Client::builder()
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(12))
        .build()
        .context("failed to build activation heartbeat client")?;
    if send_host_runtime_heartbeat(
        paths,
        &client,
        &control_plane_url,
        activation,
        &state,
        ready_for_stream,
        &note,
        &public_url,
        local_http_ready,
        required_processes_ready,
    )? {
        return Ok(());
    }

    let response = client
        .post(format!("{control_plane_url}/api/v1/host-activation/heartbeat"))
        .version(reqwest::Version::HTTP_2)
        .json(&serde_json::json!({
            "hostId": activation.host_id.trim(),
            "machineIdentity": empty_to_none(&activation.machine_identity),
            "installInstanceId": empty_to_none(&activation.install_instance_id),
            "runtimeToken": activation.runtime_token.trim(),
            "activationRecordId": empty_to_none(&activation.activation_record_id),
            "displayName": empty_to_none(&activation.display_name),
            "lifecyclePhase": state.lifecycle_phase.as_str(),
            "healthGrade": if ready_for_stream { Some("healthy".to_string()) } else { None::<String> },
            "runtimeDisplayName": runtime_display_name(read_selected_runtime_key(paths).as_str()),
            "publicUrl": empty_to_none(&public_url),
            "serviceState": Some("running".to_string()),
            "localHttpReady": local_http_ready,
            "requiredProcessesReady": required_processes_ready,
            "readyForStream": ready_for_stream,
            "note": Some(note),
            "sentinelPcId": empty_to_none(&activation.sentinel_pc_id),
            "sentinelDeviceId": empty_to_none(&activation.sentinel_device_id),
            "keeperEntryId": empty_to_none(&activation.keeper_entry_id),
            "pcLabel": empty_to_none(&activation.pc_label),
        }))
        .send();

    let response = match response {
        Ok(value) => value,
        Err(err) => {
            append_supervisor_log(paths, &format!("activation heartbeat skipped: {err}"))?;
            return Ok(());
        }
    };

    let status_code = response.status();
    let raw = response.text().unwrap_or_default();
    let payload = serde_json::from_str::<HostHeartbeatPayload>(&raw).unwrap_or_default();

    if status_code.as_u16() == 404 && !payload.ok && payload.activation_state.trim().is_empty() {
        append_supervisor_log(
            paths,
            "legacy activation heartbeat endpoint unavailable; keeping local activation state",
        )?;
        return Ok(());
    }

    if status_code.is_success() && payload.ok {
        activation.last_heartbeat_at_utc = payload.last_heartbeat_at_utc.trim().to_string();
        activation.last_ready_for_stream = payload.ready_for_stream;
        if !payload.activation_state.trim().is_empty() {
            activation.activation_state =
                normalize_activation_state(payload.activation_state.as_str(), "activated");
        }
        activation.updated_at_utc = now_rfc3339();
        save_activation_state(paths, activation)?;
        return Ok(());
    }

    let next_state =
        derive_next_activation_state(status_code.as_u16(), payload.activation_state.as_str());
    let previous_state = activation.activation_state.clone();
    activation.activation_state = next_state.clone();
    activation.last_ready_for_stream = false;
    activation.updated_at_utc = now_rfc3339();
    save_activation_state(paths, activation)?;
    append_supervisor_log(
        paths,
        &format!(
            "activation heartbeat forced local state {} -> {} (http {} {})",
            previous_state,
            next_state,
            status_code.as_u16(),
            payload
                .error
                .clone()
                .or(payload.message.clone())
                .unwrap_or_else(|| "heartbeat state change".to_string())
        ),
    )?;
    stop_runtime_when_activation_locked(paths, activation)?;
    Ok(())
}

#[cfg(windows)]
fn stop_runtime_when_activation_locked(
    paths: &BundlePaths,
    activation: &HostActivationStateRecord,
) -> Result<()> {
    let daemon_running = daemon_is_running(paths);
    let has_processes = !list_bundle_processes(paths, StopScope::All)?.is_empty();
    if !daemon_running && !has_processes {
        return Ok(());
    }

    append_supervisor_log(
        paths,
        &format!(
            "activation gate stopping bundle because activation_state={}",
            activation_state_label(activation)
        ),
    )?;
    if daemon_running {
        let _ = request_user_daemon_operation(paths, DaemonCommandKind::StopBundle);
    }
    let _ = stop_bundle_processes(paths, StopScope::All);
    Ok(())
}

#[cfg(windows)]
fn maybe_restore_host_cursor_defaults_if_idle(paths: &BundlePaths) -> Result<()> {
    if bundle_has_active_stream_session(paths)? {
        return Ok(());
    }

    restore_windows_cursor_defaults();
    Ok(())
}

#[cfg(not(windows))]
fn maybe_restore_host_cursor_defaults_if_idle(_paths: &BundlePaths) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn record_service_watchdog_trigger(paths: &BundlePaths, reason: &str) -> Result<()> {
    let now = now_unix_ms();
    let mut state = read_state(paths).unwrap_or_default();
    state.total_service_watchdog_trigger_count =
        state.total_service_watchdog_trigger_count.saturating_add(1);
    state.boot_service_watchdog_trigger_count =
        state.boot_service_watchdog_trigger_count.saturating_add(1);
    state.last_service_watchdog_reason = Some(reason.to_string());
    state.last_service_watchdog_at_unix_ms = Some(now);
    push_recent_incident(
        &mut state,
        "service_watchdog",
        reason,
        Some("restart_bundle"),
        false,
        now,
    );
    state.updated_at_unix_ms = now;
    write_state(paths, state)
}

#[cfg(windows)]
fn maybe_clear_stable_recovery_budget(
    paths: &BundlePaths,
    state: &SupervisorState,
    required_processes_ready: bool,
    local_http_ready: bool,
) -> Result<()> {
    if state.failure_recovery_attempt_count == 0 {
        return Ok(());
    }
    if !matches!(state.lifecycle_phase, LifecyclePhase::Ready) {
        return Ok(());
    }
    if !required_processes_ready || !local_http_ready {
        return Ok(());
    }

    let stable_since = state
        .lifecycle_updated_at_unix_ms
        .or(state.last_command_finished_at_unix_ms);
    let Some(stable_since) = stable_since else {
        return Ok(());
    };
    if now_unix_ms().saturating_sub(stable_since) < STABLE_RECOVERY_CLEAR_MS {
        return Ok(());
    }

    let mut next_state = read_state(paths).unwrap_or_default();
    if next_state.failure_recovery_attempt_count == 0 {
        return Ok(());
    }
    if !matches!(next_state.lifecycle_phase, LifecyclePhase::Ready) {
        return Ok(());
    }

    let cleared_at = now_unix_ms();
    clear_active_failure_recovery_budget(&mut next_state);
    next_state.last_failure_recovery_budget_cleared_at_unix_ms = Some(cleared_at);
    push_recent_incident(
        &mut next_state,
        "recovery_budget_cleared",
        "stable_ready_window",
        None,
        false,
        cleared_at,
    );
    next_state.updated_at_unix_ms = cleared_at;
    write_state(paths, next_state)?;
    append_supervisor_log(
        paths,
        "service_watchdog cleared active recovery budget after stable ready window",
    )?;
    Ok(())
}

fn watchdog_recovery_allowed(
    last_watchdog_recovery_at: &Option<Instant>,
    min_interval: Duration,
) -> bool {
    last_watchdog_recovery_at.is_none_or(|last_attempt| last_attempt.elapsed() >= min_interval)
}

#[cfg(windows)]
fn update_service_status(
    handle: &windows_service::service_control_handler::ServiceStatusHandle,
    state: WindowsServiceState,
    accepted_controls: ServiceControlAccept,
) -> Result<()> {
    update_service_status_ex(
        handle,
        state,
        accepted_controls,
        ServiceExitCode::Win32(0),
        0,
        Duration::from_secs(5),
    )
}

#[cfg(windows)]
fn update_service_status_ex(
    handle: &windows_service::service_control_handler::ServiceStatusHandle,
    state: WindowsServiceState,
    accepted_controls: ServiceControlAccept,
    exit_code: ServiceExitCode,
    checkpoint: u32,
    wait_hint: Duration,
) -> Result<()> {
    handle
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accepted_controls,
            exit_code,
            checkpoint,
            wait_hint,
            process_id: None,
        })
        .context("failed to update Windows service status")
}

fn request_daemon_operation(paths: &BundlePaths, command: DaemonCommandKind) -> Result<()> {
    if matches!(command, DaemonCommandKind::Shutdown) {
        if !daemon_is_running(paths) {
            return Ok(());
        }
    } else {
        #[cfg(windows)]
        {
            if let Err(err) = ensure_user_session_daemon_running(paths) {
                append_supervisor_log(
                    paths,
                    &format!(
                        "request_daemon_operation user-session task fallback -> direct daemon start because: {err:#}"
                    ),
                )?;
                ensure_daemon_running(paths)?;
            }
        }
        #[cfg(not(windows))]
        {
            ensure_daemon_running(paths)?;
        }
    }

    let command_id = now_unix_ms();
    record_command_queued(paths, command_id, &command)?;
    write_command_file(
        paths,
        &DaemonCommandFile {
            id: command_id,
            created_at_unix_ms: now_unix_ms(),
            command,
        },
    )?;
    wait_for_command_completion(paths, command_id, Duration::from_secs(90))
}

fn request_user_daemon_operation(paths: &BundlePaths, command: DaemonCommandKind) -> Result<()> {
    if matches!(command, DaemonCommandKind::Shutdown) {
        return Ok(());
    }

    ensure_user_session_daemon_running(paths)?;

    let command_id = now_unix_ms();
    record_command_queued(paths, command_id, &command)?;
    write_command_file(
        paths,
        &DaemonCommandFile {
            id: command_id,
            created_at_unix_ms: now_unix_ms(),
            command,
        },
    )?;
    wait_for_command_completion(paths, command_id, Duration::from_secs(90))
}

fn ensure_user_session_daemon_running(paths: &BundlePaths) -> Result<()> {
    if daemon_is_running(paths) {
        append_supervisor_log(
            paths,
            "ensure_user_session_daemon_running daemon already running",
        )?;
        return Ok(());
    }

    clear_command_file(paths)?;
    append_supervisor_log(
        paths,
        "ensure_user_session_daemon_running cleared stale command file",
    )?;

    let task_name = default_user_agent_task_name(paths);
    append_supervisor_log(
        paths,
        &format!("ensure_user_session_daemon_running starting task={task_name}"),
    )?;
    let output = Command::new("schtasks")
        .args(["/Run", "/TN", &task_name])
        .output()
        .with_context(|| format!("failed to invoke schtasks /Run for {task_name}"))?;

    if !output.status.success() {
        let combined = format!(
            "{} {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        bail!(
            "failed to start user-session supervisor task {}: {}",
            task_name,
            combined.trim()
        );
    }

    wait_for_daemon_ready(paths, Duration::from_secs(30)).with_context(|| {
        format!("timed out waiting for user-session daemon from scheduled task {task_name}")
    })
}

fn run_daemon(paths: &BundlePaths) -> Result<()> {
    let daemon_pid = initialize_daemon_state(paths)?;
    daemon_event_loop(paths, daemon_pid, None)
}

fn ensure_daemon_running(paths: &BundlePaths) -> Result<()> {
    if daemon_is_running(paths) {
        return Ok(());
    }

    clear_command_file(paths)?;
    let current_exe = std::env::current_exe().context("failed to resolve supervisor executable")?;
    let mut child = Command::new(current_exe);
    child
        .arg("--bundle-root")
        .arg(paths.bundle_root.as_os_str())
        .arg("run-daemon")
        .current_dir(&paths.bundle_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    child
        .spawn()
        .context("failed to spawn host supervisor daemon")?;
    wait_for_daemon_ready(paths, Duration::from_secs(10))
}

fn initialize_daemon_state(paths: &BundlePaths) -> Result<u32> {
    let daemon_pid = std::process::id();
    let started_at = now_unix_ms();
    #[cfg(windows)]
    ensure_supervisor_job_object()?;
    take_over_existing_daemon(paths, daemon_pid)?;

    let mut state = read_state(paths).unwrap_or_default();
    state.daemon_pid = Some(daemon_pid);
    state.daemon_started_at_unix_ms = Some(started_at);
    state.boot_failure_recovery_count = 0;
    state.boot_service_watchdog_trigger_count = 0;
    state.ready_since_unix_ms = None;
    state.recent_incidents.clear();
    state.updated_at_unix_ms = started_at;
    write_state(paths, state)?;
    append_supervisor_log(
        paths,
        &format!("initialize_daemon_state daemon_pid={daemon_pid}"),
    )?;
    Ok(daemon_pid)
}

fn take_over_existing_daemon(paths: &BundlePaths, current_pid: u32) -> Result<()> {
    let Some(existing_pid) = read_state(paths).and_then(|state| state.daemon_pid) else {
        return Ok(());
    };
    if existing_pid == current_pid {
        return Ok(());
    }
    if !process_exists(existing_pid)? {
        return Ok(());
    }

    append_supervisor_log(
        paths,
        &format!("take_over_existing_daemon old_pid={existing_pid} new_pid={current_pid}"),
    )?;
    let _ = taskkill_pid(existing_pid);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if !process_exists(existing_pid)? {
            return Ok(());
        }
        sleep(Duration::from_millis(150));
    }
    bail!("timed out waiting for previous daemon pid {existing_pid} to stop")
}

fn daemon_event_loop(
    paths: &BundlePaths,
    daemon_pid: u32,
    stop_requested: Option<&AtomicBool>,
) -> Result<()> {
    let config = load_config(paths)?;
    let mut last_idle_cursor_restore_tick = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);
    let mut last_activation_tick = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);
    let mut last_activation_status_sync_at: Option<Instant> = None;
    let mut last_activation_heartbeat_at: Option<Instant> = None;
    loop {
        if stop_requested.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return Ok(());
        }

        if last_idle_cursor_restore_tick.elapsed() >= Duration::from_secs(3) {
            maybe_restore_host_cursor_defaults_if_idle(paths)?;
            last_idle_cursor_restore_tick = Instant::now();
        }

        if last_activation_tick.elapsed() >= Duration::from_secs(5) {
            if let Err(err) = service_activation_tick(
                paths,
                &config,
                &mut last_activation_status_sync_at,
                &mut last_activation_heartbeat_at,
            ) {
                append_supervisor_log(paths, &format!("daemon activation sync error: {err:#}"))?;
            }
            last_activation_tick = Instant::now();
        }

        let command = match read_command_file(paths)? {
            Some(command) => command,
            None => {
                sleep(Duration::from_millis(250));
                continue;
            }
        };

        record_command_started(paths, daemon_pid, &command)?;
        append_supervisor_log(
            paths,
            &format!("daemon command start {}", command.command.as_str()),
        )?;
        let result = match command.command {
            DaemonCommandKind::StartBundle => start_bundle_inner(paths),
            DaemonCommandKind::StopBundle => stop_bundle_inner(paths),
            DaemonCommandKind::RestartRuntime => restart_runtime_inner(paths),
            DaemonCommandKind::RecoverRuntime { policy } => recover_runtime_inner(paths, policy),
            DaemonCommandKind::RecoverFailure {
                strategy,
                ref reason,
            } => recover_failure_inner(paths, strategy, &reason),
            DaemonCommandKind::Shutdown => stop_bundle_inner(paths),
        };
        record_command_finished(paths, daemon_pid, &command, result.as_ref().err())?;
        clear_command_file(paths)?;
        match &result {
            Ok(_) => append_supervisor_log(
                paths,
                &format!("daemon command ok {}", command.command.as_str()),
            )?,
            Err(err) => append_supervisor_log(
                paths,
                &format!(
                    "daemon command failed {} err={err:#}",
                    command.command.as_str()
                ),
            )?,
        }
        result?;

        if matches!(command.command, DaemonCommandKind::Shutdown) {
            clear_daemon_pid(paths)?;
            return Ok(());
        }
    }
}

fn clear_daemon_pid(paths: &BundlePaths) -> Result<()> {
    let mut state = read_state(paths).unwrap_or_default();
    state.daemon_pid = None;
    state.updated_at_unix_ms = now_unix_ms();
    write_state(paths, state)
}

fn daemon_is_running(paths: &BundlePaths) -> bool {
    let Some(pid) = read_state(paths).and_then(|state| state.daemon_pid) else {
        return false;
    };
    process_exists(pid).unwrap_or(false)
}

fn wait_for_daemon_ready(paths: &BundlePaths, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        let Some(pid) = read_state(paths).and_then(|state| state.daemon_pid) else {
            if started.elapsed() >= timeout {
                bail!("timed out waiting for host supervisor daemon state");
            }
            sleep(Duration::from_millis(150));
            continue;
        };

        if process_exists(pid)? {
            return Ok(());
        }

        if started.elapsed() >= timeout {
            bail!("timed out waiting for host supervisor daemon process");
        }
        sleep(Duration::from_millis(150));
    }
}

fn write_command_file(paths: &BundlePaths, command: &DaemonCommandFile) -> Result<()> {
    let serialized = serde_json::to_string_pretty(command)?;
    fs::write(&paths.command_path, format!("{serialized}\n")).with_context(|| {
        format!(
            "failed to write supervisor command {}",
            paths.command_path.display()
        )
    })
}

fn read_command_file(paths: &BundlePaths) -> Result<Option<DaemonCommandFile>> {
    if !paths.command_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&paths.command_path).with_context(|| {
        format!(
            "failed to read supervisor command {}",
            paths.command_path.display()
        )
    })?;
    let command = serde_json::from_str::<DaemonCommandFile>(&raw).with_context(|| {
        format!(
            "failed to parse supervisor command {}",
            paths.command_path.display()
        )
    })?;
    Ok(Some(command))
}

fn clear_command_file(paths: &BundlePaths) -> Result<()> {
    if paths.command_path.exists() {
        fs::remove_file(&paths.command_path).with_context(|| {
            format!(
                "failed to remove supervisor command {}",
                paths.command_path.display()
            )
        })?;
    }
    Ok(())
}

fn append_supervisor_log(paths: &BundlePaths, message: &str) -> Result<()> {
    let line = format!("[{}] {message}\r\n", now_unix_ms());
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_path)
        .with_context(|| format!("failed to open supervisor log {}", paths.log_path.display()))?;
    use std::io::Write as _;
    file.write_all(line.as_bytes()).with_context(|| {
        format!(
            "failed to write supervisor log {}",
            paths.log_path.display()
        )
    })
}

fn lifecycle_phase_for_command_start(command: &DaemonCommandKind) -> LifecyclePhase {
    match command {
        DaemonCommandKind::StartBundle => LifecyclePhase::Starting,
        DaemonCommandKind::StopBundle | DaemonCommandKind::Shutdown => LifecyclePhase::Stopping,
        DaemonCommandKind::RestartRuntime
        | DaemonCommandKind::RecoverRuntime { .. }
        | DaemonCommandKind::RecoverFailure { .. } => LifecyclePhase::Recovering,
    }
}

fn lifecycle_phase_for_command_success(command: &DaemonCommandKind) -> LifecyclePhase {
    match command {
        DaemonCommandKind::StartBundle
        | DaemonCommandKind::RestartRuntime
        | DaemonCommandKind::RecoverRuntime { .. }
        | DaemonCommandKind::RecoverFailure { .. } => LifecyclePhase::Ready,
        DaemonCommandKind::StopBundle | DaemonCommandKind::Shutdown => LifecyclePhase::Idle,
    }
}

fn lifecycle_reason_for_command(command: &DaemonCommandKind) -> String {
    match command {
        DaemonCommandKind::RecoverRuntime { policy } => {
            format!("recover_runtime:{}", policy.as_str())
        }
        DaemonCommandKind::RecoverFailure { strategy, reason } => {
            format!("recover_failure:{}:{reason}", strategy.as_str())
        }
        _ => command.as_str().to_string(),
    }
}

fn compute_failure_recovery_plan(
    state: &mut SupervisorState,
    requested_strategy: FailureRecoveryStrategy,
    reason: &str,
) -> FailureRecoveryPlan {
    let now = now_unix_ms();
    let window_expired = state
        .failure_recovery_window_started_at_unix_ms
        .is_none_or(|started| now.saturating_sub(started) > FAILURE_RECOVERY_WINDOW_MS);

    if window_expired {
        state.failure_recovery_window_started_at_unix_ms = Some(now);
        state.failure_recovery_attempt_count = 0;
        state.last_failure_recovery_reason = None;
        state.last_failure_recovery_strategy = None;
        state.last_failure_recovery_escalated = false;
    }

    state.failure_recovery_attempt_count = state.failure_recovery_attempt_count.saturating_add(1);
    state.total_failure_recovery_count = state.total_failure_recovery_count.saturating_add(1);
    state.boot_failure_recovery_count = state.boot_failure_recovery_count.saturating_add(1);
    let attempt_count = state.failure_recovery_attempt_count;

    let mut effective_strategy = requested_strategy;
    let mut escalated = false;
    let mut exhausted = false;

    match requested_strategy {
        FailureRecoveryStrategy::RestartRuntime
            if attempt_count > MAX_RUNTIME_RECOVERY_ATTEMPTS =>
        {
            effective_strategy = FailureRecoveryStrategy::RestartBundle;
            escalated = true;
        }
        FailureRecoveryStrategy::RestartBundle if attempt_count > MAX_BUNDLE_RECOVERY_ATTEMPTS => {
            effective_strategy = FailureRecoveryStrategy::None;
            escalated = true;
            exhausted = true;
        }
        FailureRecoveryStrategy::RestartRuntime if attempt_count > MAX_BUNDLE_RECOVERY_ATTEMPTS => {
            effective_strategy = FailureRecoveryStrategy::None;
            escalated = true;
            exhausted = true;
        }
        _ => {}
    }

    state.last_failure_recovery_reason = Some(reason.to_string());
    state.last_failure_recovery_strategy = Some(effective_strategy.as_str().to_string());
    state.last_failure_recovery_escalated = escalated;
    if escalated {
        state.total_failure_recovery_escalation_count = state
            .total_failure_recovery_escalation_count
            .saturating_add(1);
    }
    push_recent_incident(
        state,
        "failure_recovery",
        reason,
        Some(effective_strategy.as_str()),
        escalated,
        now,
    );
    state.updated_at_unix_ms = now;

    FailureRecoveryPlan {
        effective_strategy,
        attempt_count,
        escalated,
        exhausted,
    }
}

fn reset_failure_recovery_budget(state: &mut SupervisorState) {
    state.failure_recovery_attempt_count = 0;
    state.failure_recovery_window_started_at_unix_ms = None;
    state.last_failure_recovery_reason = None;
    state.last_failure_recovery_strategy = None;
    state.last_failure_recovery_escalated = false;
}

fn clear_active_failure_recovery_budget(state: &mut SupervisorState) {
    state.failure_recovery_attempt_count = 0;
    state.failure_recovery_window_started_at_unix_ms = None;
    state.last_failure_recovery_escalated = false;
}

fn record_command_started(
    paths: &BundlePaths,
    daemon_pid: u32,
    command: &DaemonCommandFile,
) -> Result<()> {
    let now = now_unix_ms();
    let mut state = read_state(paths).unwrap_or_default();
    state.daemon_pid = Some(daemon_pid);
    state.lifecycle_phase = lifecycle_phase_for_command_start(&command.command);
    state.lifecycle_reason = Some(lifecycle_reason_for_command(&command.command));
    state.lifecycle_updated_at_unix_ms = Some(now);
    state.ready_since_unix_ms = None;
    state.last_command_id = Some(command.id);
    state.last_command_name = Some(command.command.as_str().to_string());
    state.last_command_status = Some("running".to_string());
    state.last_command_error = None;
    state.last_command_started_at_unix_ms = Some(now);
    state.last_command_finished_at_unix_ms = None;
    state.updated_at_unix_ms = now;
    write_state(paths, state)
}

fn record_command_queued(
    paths: &BundlePaths,
    command_id: u64,
    command: &DaemonCommandKind,
) -> Result<()> {
    let now = now_unix_ms();
    let mut state = read_state(paths).unwrap_or_default();
    state.last_command_id = Some(command_id);
    state.last_command_name = Some(command.as_str().to_string());
    state.last_command_status = Some("queued".to_string());
    state.last_command_error = None;
    state.last_command_started_at_unix_ms = None;
    state.last_command_finished_at_unix_ms = None;
    state.updated_at_unix_ms = now;
    write_state(paths, state)
}

fn record_command_finished(
    paths: &BundlePaths,
    daemon_pid: u32,
    command: &DaemonCommandFile,
    error: Option<&anyhow::Error>,
) -> Result<()> {
    let now = now_unix_ms();
    let mut state = read_state(paths).unwrap_or_default();
    let previous_phase = state.lifecycle_phase;
    state.daemon_pid = Some(daemon_pid);
    state.lifecycle_phase = if error.is_some() {
        LifecyclePhase::Failed
    } else {
        lifecycle_phase_for_command_success(&command.command)
    };
    state.lifecycle_reason = Some(if let Some(err) = error {
        format!("{}:{err}", command.command.as_str())
    } else {
        lifecycle_reason_for_command(&command.command)
    });
    state.lifecycle_updated_at_unix_ms = Some(now);
    state.ready_since_unix_ms =
        if error.is_none() && matches!(state.lifecycle_phase, LifecyclePhase::Ready) {
            if matches!(previous_phase, LifecyclePhase::Ready) {
                state.ready_since_unix_ms
            } else {
                Some(now)
            }
        } else {
            None
        };
    state.last_command_id = Some(command.id);
    state.last_command_name = Some(command.command.as_str().to_string());
    state.last_command_status = Some(if error.is_some() { "error" } else { "ok" }.to_string());
    state.last_command_error = error.map(|value| format!("{value:#}"));
    state.last_command_finished_at_unix_ms = Some(now);
    state.updated_at_unix_ms = now;
    if error.is_none() {
        match command.command {
            DaemonCommandKind::StartBundle
            | DaemonCommandKind::RestartRuntime
            | DaemonCommandKind::RecoverRuntime { .. } => reset_failure_recovery_budget(&mut state),
            DaemonCommandKind::RecoverFailure { .. }
            | DaemonCommandKind::StopBundle
            | DaemonCommandKind::Shutdown => {}
        }
    }
    write_state(paths, state)
}

fn wait_for_command_completion(
    paths: &BundlePaths,
    command_id: u64,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    loop {
        if let Some(state) = read_state(paths) {
            if state.last_command_id == Some(command_id) {
                match state.last_command_status.as_deref() {
                    Some("ok") => return Ok(()),
                    Some("error") => {
                        bail!(
                            "{}",
                            state
                                .last_command_error
                                .unwrap_or_else(|| "host supervisor command failed".to_string())
                        );
                    }
                    _ => {}
                }
            }
        }

        if started.elapsed() >= timeout {
            bail!("timed out waiting for host supervisor command to finish");
        }
        sleep(Duration::from_millis(150));
    }
}

fn start_bundle_inner(paths: &BundlePaths) -> Result<()> {
    append_supervisor_log(paths, "start_bundle_inner begin")?;
    run_preflight(paths)?;
    append_supervisor_log(paths, "start_bundle_inner preflight complete")?;
    stop_bundle_processes(paths, StopScope::All)?;
    append_supervisor_log(paths, "start_bundle_inner stop all complete")?;
    if let Err(err) = ensure_stream_qos_policy(paths) {
        append_supervisor_log(paths, &format!("start_bundle_inner qos warn err={err:#}"))?;
    }

    let config = load_config(paths)?;
    let runtime_key = read_selected_runtime_key(paths);
    let runtime_dir = resolve_runtime_dir(paths, &runtime_key);
    let web_server_path = paths.moonlight_dir.join("web-server.exe");
    let frpc_path = paths.frp_dir.join("frpc.exe");

    let sunshine_pid =
        start_sunshine_runtime_ready(paths, &runtime_dir, config.moonlight.default_http_port)?;
    append_supervisor_log(
        paths,
        &format!("start_bundle_inner sunshine ready pid={sunshine_pid}"),
    )?;

    let web_server_pid = spawn_process(
        &web_server_path,
        &["--config-path", "server\\config.json"],
        &paths.moonlight_dir,
    )?;
    append_supervisor_log(
        paths,
        &format!("start_bundle_inner spawned web-server pid={web_server_pid}"),
    )?;
    wait_for_tcp_ready(
        config.web_server.bind_address.ip().to_string().as_str(),
        config.web_server.bind_address.port(),
        Duration::from_secs(20),
    )?;
    append_supervisor_log(paths, "start_bundle_inner web-server port ready")?;
    ensure_pid_alive(web_server_pid, Duration::from_secs(2))
        .with_context(|| format!("web-server exited too early: {}", web_server_path.display()))?;

    let frpc_pid = if frpc_path.exists() {
        let pid = spawn_process(&frpc_path, &["-c", "frpc.toml"], &paths.frp_dir)?;
        append_supervisor_log(paths, &format!("start_bundle_inner spawned frpc pid={pid}"))?;
        Some(pid)
    } else {
        append_supervisor_log(
            paths,
            "start_bundle_inner skipping frpc because the bundle is running in managed tunnel mode",
        )?;
        None
    };

    let mut state = read_state(paths).unwrap_or_default();
    state.sunshine_pid = Some(sunshine_pid);
    state.web_server_pid = Some(web_server_pid);
    state.frpc_pid = frpc_pid;
    state.runtime_key = Some(runtime_key);
    state.updated_at_unix_ms = now_unix_ms();
    write_state(paths, state)?;
    append_supervisor_log(paths, "start_bundle_inner completed")?;

    Ok(())
}

fn ensure_stream_qos_policy(paths: &BundlePaths) -> Result<()> {
    let host_installer_path = paths.bundle_root.join("host-installer.exe");
    if !host_installer_path.exists() {
        append_supervisor_log(
            paths,
            &format!(
                "ensure_stream_qos_policy skipped missing={}",
                host_installer_path.display()
            ),
        )?;
        return Ok(());
    }

    let mut command = Command::new(&host_installer_path);
    apply_background_spawn_flags(&mut command);
    let output = command
        .args([
            "--bundle-root",
            &paths.bundle_root.to_string_lossy(),
            "configure-qos",
        ])
        .current_dir(&paths.bundle_root)
        .output()
        .with_context(|| {
            format!(
                "failed to run host installer {} configure-qos",
                host_installer_path.display()
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        if !stdout.is_empty() {
            append_supervisor_log(paths, &format!("ensure_stream_qos_policy stdout={stdout}"))?;
        }
        if !stderr.is_empty() {
            append_supervisor_log(paths, &format!("ensure_stream_qos_policy note={stderr}"))?;
        }
        return Ok(());
    }

    bail!("host-installer configure-qos failed: {} {}", stdout, stderr)
}

fn stop_bundle_inner(paths: &BundlePaths) -> Result<()> {
    append_supervisor_log(paths, "stop_bundle_inner begin")?;
    stop_bundle_processes(paths, StopScope::All)
}

fn restart_runtime_inner(paths: &BundlePaths) -> Result<()> {
    append_supervisor_log(paths, "restart_runtime_inner begin")?;
    run_preflight(paths)?;
    append_supervisor_log(paths, "restart_runtime_inner preflight complete")?;
    stop_bundle_processes(paths, StopScope::RuntimeOnly)?;
    append_supervisor_log(paths, "restart_runtime_inner runtime stop complete")?;

    let config = load_config(paths)?;
    let runtime_key = read_selected_runtime_key(paths);
    let runtime_dir = resolve_runtime_dir(paths, &runtime_key);
    let sunshine_pid =
        start_sunshine_runtime_ready(paths, &runtime_dir, config.moonlight.default_http_port)?;
    append_supervisor_log(
        paths,
        &format!("restart_runtime_inner sunshine ready pid={sunshine_pid}"),
    )?;

    let mut state = read_state(paths).unwrap_or_default();
    state.sunshine_pid = Some(sunshine_pid);
    state.runtime_key = Some(runtime_key);
    state.updated_at_unix_ms = now_unix_ms();
    write_state(paths, state)?;
    append_supervisor_log(paths, "restart_runtime_inner completed")?;
    Ok(())
}

fn recover_runtime_inner(paths: &BundlePaths, policy: RecoveryPolicy) -> Result<()> {
    append_supervisor_log(
        paths,
        &format!("recover_runtime_inner begin policy={}", policy.as_str()),
    )?;

    if policy.allows_soft_reuse() {
        let config = load_config(paths)?;
        if wait_for_tcp_ready(
            "127.0.0.1",
            config.moonlight.default_http_port,
            Duration::from_millis(2500),
        )
        .is_ok()
        {
            let runtime_key = read_selected_runtime_key(paths);
            let mut state = read_state(paths).unwrap_or_default();
            state.runtime_key = Some(runtime_key);
            state.updated_at_unix_ms = now_unix_ms();
            write_state(paths, state)?;
            append_supervisor_log(
                paths,
                &format!(
                    "recover_runtime_inner completed action=soft_reuse policy={}",
                    policy.as_str()
                ),
            )?;
            return Ok(());
        }

        append_supervisor_log(
            paths,
            &format!(
                "recover_runtime_inner soft_reuse_unavailable policy={} action=restart_runtime",
                policy.as_str()
            ),
        )?;
    } else {
        append_supervisor_log(
            paths,
            &format!(
                "recover_runtime_inner policy_requires_restart policy={} action=restart_runtime",
                policy.as_str()
            ),
        )?;
    }

    restart_runtime_inner(paths)
}

fn recover_failure_inner(
    paths: &BundlePaths,
    strategy: FailureRecoveryStrategy,
    reason: &str,
) -> Result<()> {
    let mut state = read_state(paths).unwrap_or_default();
    let plan = compute_failure_recovery_plan(&mut state, strategy, reason);
    write_state(paths, state)?;

    append_supervisor_log(
        paths,
        &format!(
            "recover_failure_inner begin requested_strategy={} effective_strategy={} attempts={} escalated={} exhausted={} reason={}",
            strategy.as_str(),
            plan.effective_strategy.as_str(),
            plan.attempt_count,
            plan.escalated,
            plan.exhausted,
            reason
        ),
    )?;

    if plan.exhausted {
        let now = now_unix_ms();
        let mut state = read_state(paths).unwrap_or_default();
        state.lifecycle_phase = LifecyclePhase::Failed;
        state.lifecycle_reason = Some(format!("recovery_budget_exhausted:{reason}"));
        state.lifecycle_updated_at_unix_ms = Some(now);
        state.ready_since_unix_ms = None;
        push_recent_incident(
            &mut state,
            "recovery_budget_exhausted",
            reason,
            Some(plan.effective_strategy.as_str()),
            true,
            now,
        );
        state.updated_at_unix_ms = now;
        write_state(paths, state)?;
        bail!("failure recovery budget exhausted for reason={reason}");
    }

    let result = match plan.effective_strategy {
        FailureRecoveryStrategy::RestartRuntime => restart_runtime_inner(paths),
        FailureRecoveryStrategy::RestartBundle => start_bundle_inner(paths),
        FailureRecoveryStrategy::None => {
            append_supervisor_log(
                paths,
                &format!("recover_failure_inner completed action=none reason={reason}"),
            )?;
            Ok(())
        }
    };

    if result.is_ok() {
        let mut state = read_state(paths).unwrap_or_default();
        state.last_failure_recovery_completed_at_unix_ms = Some(now_unix_ms());
        state.updated_at_unix_ms = now_unix_ms();
        write_state(paths, state)?;
    }

    result
}

fn build_status_snapshot(paths: &BundlePaths) -> Result<StatusSnapshot> {
    let config = load_config(paths)?;
    let runtime_key = read_selected_runtime_key(paths);
    let runtime_dir = resolve_runtime_dir(paths, &runtime_key);
    let processes = list_bundle_processes(paths, StopScope::All)?;
    let state = read_state(paths).unwrap_or_default();
    let daemon_running = state
        .daemon_pid
        .map(|pid| process_exists(pid).unwrap_or(false))
        .unwrap_or(false);
    Ok(StatusSnapshot {
        daemon_running,
        host_port: config.moonlight.default_http_port,
        runtime_key,
        runtime_dir: runtime_dir.display().to_string(),
        web_bind_address: config.web_server.bind_address.to_string(),
        running_processes: processes
            .into_iter()
            .map(|(pid, path)| ProcessSnapshot {
                pid,
                path: path.display().to_string(),
            })
            .collect(),
        state,
    })
}

fn bundle_has_required_processes(paths: &BundlePaths) -> Result<bool> {
    let processes = list_bundle_processes(paths, StopScope::All)?;
    let mut has_sunshine = false;
    let mut has_web_server = false;
    let mut has_frpc = !paths.frp_dir.join("frpc.exe").exists();
    for (_, path) in processes {
        let lowered = path.display().to_string().to_ascii_lowercase();
        if lowered.ends_with("\\sunshine.exe") || lowered.ends_with("\\sunshinesvc.exe") {
            has_sunshine = true;
        } else if lowered.ends_with("\\web-server.exe") {
            has_web_server = true;
        } else if lowered.ends_with("\\frpc.exe") {
            has_frpc = true;
        }
    }

    Ok(has_sunshine && has_web_server && has_frpc)
}

fn bundle_has_active_stream_session(paths: &BundlePaths) -> Result<bool> {
    let processes = list_bundle_processes(paths, StopScope::RuntimeOnly)?;
    Ok(processes
        .iter()
        .any(|(_, path)| is_active_stream_session_process(path)))
}

fn is_active_stream_session_process(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower == "streamer.exe"
        || lower == "mic_sidecar.exe"
        || lower == "gamepad_sidecar.exe"
        || (lower.starts_with("streamer-") && lower.ends_with(".exe"))
        || (lower.starts_with("mic_sidecar-") && lower.ends_with(".exe"))
        || (lower.starts_with("gamepad_sidecar-") && lower.ends_with(".exe"))
}

#[derive(Clone, Copy)]
enum StopScope {
    RuntimeOnly,
    All,
}

fn load_config(paths: &BundlePaths) -> Result<Config> {
    let raw = fs::read_to_string(&paths.config_path).with_context(|| {
        format!(
            "failed to read bundle config {}",
            paths.config_path.display()
        )
    })?;
    Ok(serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse bundle config {}",
            paths.config_path.display()
        )
    })?)
}

fn run_preflight(paths: &BundlePaths) -> Result<()> {
    if !paths.helper_path.exists() {
        return Ok(());
    }

    let mut command = Command::new(&paths.helper_path);
    apply_background_spawn_flags(&mut command);
    let status = command
        .args([
            "preflight",
            "--bundle-root",
            paths.bundle_root.to_string_lossy().as_ref(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "failed to run preflight helper {}",
                paths.helper_path.display()
            )
        })?;

    if !status.success() {
        bail!("preflight helper returned non-zero status: {status}");
    }

    Ok(())
}

fn read_selected_runtime_key(paths: &BundlePaths) -> String {
    fs::read_to_string(&paths.selected_runtime_path)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "sunshine".to_string())
}

fn resolve_runtime_dir(paths: &BundlePaths, runtime_key: &str) -> PathBuf {
    let candidate = paths.bundle_root.join(runtime_key);
    if candidate.join("sunshine.exe").exists() {
        candidate
    } else {
        paths.bundle_root.join("sunshine")
    }
}

fn spawn_process(program: &Path, arguments: &[&str], workdir: &Path) -> Result<u32> {
    let mut command = Command::new(program);
    apply_background_spawn_flags(&mut command);
    let child = command
        .args(arguments)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn {}", program.display()))?;
    let pid = child.id();
    #[cfg(windows)]
    assign_pid_to_supervisor_job(pid).with_context(|| {
        format!(
            "failed to assign {} to supervisor job object",
            program.display()
        )
    })?;
    Ok(pid)
}

#[cfg(windows)]
fn ensure_supervisor_job_object() -> Result<HANDLE> {
    if let Some(handle) = SUPERVISOR_JOB_HANDLE.get() {
        return Ok(*handle as HANDLE);
    }

    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if handle.is_null() {
        bail!("CreateJobObjectW returned null");
    }

    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let status = unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if status == 0 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        bail!("SetInformationJobObject failed");
    }

    let _ = SUPERVISOR_JOB_HANDLE.set(handle as usize);
    Ok(handle)
}

#[cfg(not(windows))]
fn ensure_supervisor_job_object() -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn assign_pid_to_supervisor_job(pid: u32) -> Result<()> {
    let job = ensure_supervisor_job_object()?;
    let process_handle = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
    if process_handle.is_null() {
        bail!("OpenProcess returned null for pid {pid}");
    }

    let assigned = unsafe { AssignProcessToJobObject(job, process_handle) };
    unsafe {
        let _ = CloseHandle(process_handle);
    }
    if assigned == 0 {
        bail!("AssignProcessToJobObject failed for pid {pid}");
    }
    Ok(())
}

#[cfg(not(windows))]
fn assign_pid_to_supervisor_job(_pid: u32) -> Result<()> {
    Ok(())
}

fn read_state(paths: &BundlePaths) -> Option<SupervisorState> {
    let raw = fs::read_to_string(&paths.state_path).ok()?;
    serde_json::from_str::<SupervisorState>(strip_utf8_bom(&raw)).ok()
}

fn read_activation_state(paths: &BundlePaths) -> HostActivationStateRecord {
    let mut state = fs::read_to_string(&paths.activation_state_path)
        .ok()
        .and_then(|raw| {
            serde_json::from_str::<HostActivationStateRecord>(strip_utf8_bom(&raw)).ok()
        })
        .unwrap_or_default();

    let env_changed = hydrate_activation_binding_from_keeper_env(paths, &mut state);
    let local_assignment_changed = hydrate_activation_binding_from_local_assignments(&mut state);
    if env_changed || local_assignment_changed {
        let _ = save_activation_state(paths, &state);
    }

    state
}

fn strip_utf8_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

fn deserialize_string_or_default<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn save_activation_state(paths: &BundlePaths, state: &HostActivationStateRecord) -> Result<()> {
    let serialized = serde_json::to_string_pretty(state)?;
    fs::write(&paths.activation_state_path, format!("{serialized}\n")).with_context(|| {
        format!(
            "failed to write host activation state {}",
            paths.activation_state_path.display()
        )
    })
}

fn write_state(paths: &BundlePaths, state: SupervisorState) -> Result<()> {
    let serialized = serde_json::to_string_pretty(&state)?;
    fs::write(&paths.state_path, format!("{serialized}\n")).with_context(|| {
        format!(
            "failed to write supervisor state {}",
            paths.state_path.display()
        )
    })
}

fn keeper_tunnel_env_path(paths: &BundlePaths) -> PathBuf {
    paths
        .bundle_root
        .join("keeper-tunnel")
        .join("data")
        .join("cloudrental.env")
}

fn read_keeper_tunnel_env_value(paths: &BundlePaths, key: &str) -> String {
    let raw = match fs::read_to_string(keeper_tunnel_env_path(paths)) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };

    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            let (entry_key, entry_value) = trimmed.split_once('=')?;
            if entry_key.trim().eq_ignore_ascii_case(key) {
                Some(entry_value.trim().to_string())
            } else {
                None
            }
        })
        .next()
        .unwrap_or_default()
}

fn normalize_pc_id(raw: &str) -> String {
    let digits: String = raw.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return String::new();
    }

    digits
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn normalize_pc_number(raw: &str) -> String {
    let pc_id = normalize_pc_id(raw);
    if pc_id.is_empty() {
        return String::new();
    }

    pc_id
        .parse::<u32>()
        .ok()
        .map(|value| format!("{value:02}"))
        .unwrap_or_default()
}

fn read_assignment_string(assignments: &Value, requirement_key: &str, field: &str) -> String {
    assignments
        .get(requirement_key)
        .and_then(|entry| entry.get(field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn read_pc_id_from_license_assignments(assignments: &Value) -> String {
    let namespace = read_assignment_string(assignments, "HOST_DATA_NAMESPACE", "value");
    let pc_id = normalize_pc_id(&namespace);
    if !pc_id.is_empty() {
        return pc_id;
    }

    String::new()
}

fn hydrate_activation_binding_from_local_assignments(
    state: &mut HostActivationStateRecord,
) -> bool {
    let mut changed = false;

    if state.sentinel_pc_id.trim().is_empty() {
        let pc_id = normalize_pc_id(&state.pc_label);
        let pc_id = if pc_id.is_empty() {
            read_pc_id_from_license_assignments(&state.license_assignments)
        } else {
            pc_id
        };
        if !pc_id.is_empty() {
            state.sentinel_pc_id = pc_id;
            changed = true;
        }
    }

    if state.keeper_entry_id.trim().is_empty() {
        let pc_number = normalize_pc_number(&state.sentinel_pc_id);
        if !pc_number.is_empty() {
            state.keeper_entry_id = format!("pc-{pc_number}");
            changed = true;
        }
    }

    if state.host_stream_proxy_route.trim().is_empty()
        || state
            .host_stream_proxy_route
            .trim()
            .trim_matches('"')
            .eq_ignore_ascii_case("HOST_STREAM_PROXY_ROUTE")
    {
        let route = read_assignment_string(
            &state.license_assignments,
            "HOST_STREAM_PROXY_ROUTE",
            "value",
        );
        if !route.is_empty() {
            state.host_stream_proxy_route = route;
            changed = true;
        }
    }

    changed
}

fn hydrate_activation_binding_from_keeper_env(
    paths: &BundlePaths,
    state: &mut HostActivationStateRecord,
) -> bool {
    let mut changed = false;

    if state.control_plane_url.trim().is_empty() {
        let api_base = read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_API_BASE");
        if !api_base.trim().is_empty() {
            state.control_plane_url = api_base.trim().trim_end_matches('/').to_string();
            changed = true;
        }
    }

    if state.sentinel_pc_id.trim().is_empty() {
        let pc_id = read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_PC_ID_DEFAULT");
        if !pc_id.trim().is_empty() {
            state.sentinel_pc_id = pc_id.trim().to_string();
            changed = true;
        }
    }

    if state.sentinel_device_id.trim().is_empty() {
        let device_id = read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_DEVICE_ID_DEFAULT");
        if !device_id.trim().is_empty() {
            state.sentinel_device_id = device_id.trim().to_string();
            changed = true;
        }
    }

    if state.keeper_entry_id.trim().is_empty() {
        let keeper_entry_id = read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_KEEPER_ENTRY_ID");
        if !keeper_entry_id.trim().is_empty() {
            state.keeper_entry_id = keeper_entry_id.trim().to_string();
            changed = true;
        }
    }

    changed
}

fn keeper_tunnel_binding_ready(paths: &BundlePaths, state: &HostActivationStateRecord) -> bool {
    let phase_ready = state
        .activation_state
        .eq_ignore_ascii_case("prepared_local")
        || state
            .activation_state
            .eq_ignore_ascii_case("locked_waiting_token");
    if !phase_ready {
        return false;
    }

    let pc_id = if state.sentinel_pc_id.trim().is_empty() {
        read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_PC_ID_DEFAULT")
    } else {
        state.sentinel_pc_id.trim().to_string()
    };
    let device_id = if state.sentinel_device_id.trim().is_empty() {
        read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_DEVICE_ID_DEFAULT")
    } else {
        state.sentinel_device_id.trim().to_string()
    };

    !state.host_id.trim().is_empty()
        && !pc_id.trim().is_empty()
        && !device_id.trim().is_empty()
        && !read_keeper_tunnel_env_value(paths, "CLOUDRENTAL_DEVICE_TOKEN")
            .trim()
            .is_empty()
}

fn activation_allows_runtime(paths: &BundlePaths, state: &HostActivationStateRecord) -> bool {
    (state.activation_state.eq_ignore_ascii_case("activated")
        && !state.host_id.trim().is_empty()
        && !state.runtime_token.trim().is_empty())
        || keeper_tunnel_binding_ready(paths, state)
}

fn activation_state_label(state: &HostActivationStateRecord) -> &str {
    let phase = state.activation_state.trim();
    if phase.is_empty() {
        "locked_waiting_token"
    } else {
        phase
    }
}

fn normalize_activation_state(raw: &str, fallback: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    if matches!(
        value.as_str(),
        "activated" | "suspended" | "revoked" | "locked_waiting_token"
    ) {
        value
    } else {
        fallback.to_string()
    }
}

fn derive_next_activation_state(status_code: u16, activation_state: &str) -> String {
    let normalized = normalize_activation_state(activation_state, "");
    if !normalized.is_empty() {
        return normalized;
    }

    match status_code {
        403 => "suspended".to_string(),
        401 | 404 | 409 => "revoked".to_string(),
        _ => "revoked".to_string(),
    }
}

fn normalize_control_plane_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn read_public_url(paths: &BundlePaths) -> String {
    fs::read_to_string(&paths.public_url_path)
        .ok()
        .map(|raw| raw.trim().to_string())
        .unwrap_or_default()
}

fn write_public_url(paths: &BundlePaths, value: &str) -> Result<()> {
    fs::write(&paths.public_url_path, format!("{}\r\n", value.trim()))
        .with_context(|| format!("failed to write {}", paths.public_url_path.display()))
}

fn extract_hostname(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = reqwest::Url::parse(trimmed).ok()?;
    parsed.host_str().map(|host| host.trim().to_string())
}

fn extract_first_label(hostname: &str) -> Option<String> {
    let trimmed = hostname.trim().trim_matches('.');
    if trimmed.is_empty() {
        return None;
    }
    let label = trimmed.split('.').next()?.trim();
    if label.is_empty() {
        return None;
    }
    Some(label.to_string())
}

fn rewrite_frpc_public_host(paths: &BundlePaths, hostname: &str) -> Result<bool> {
    let frpc_path = paths.frp_dir.join("frpc.toml");
    if !frpc_path.exists() {
        return Ok(false);
    }

    let raw = fs::read_to_string(&frpc_path)
        .with_context(|| format!("failed to read {}", frpc_path.display()))?;
    let subdomain = extract_first_label(hostname).unwrap_or_default();
    let mut changed = false;
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("subdomain = ") && !subdomain.is_empty() {
            let replacement = format!("subdomain = \"{subdomain}\"");
            lines.push(replacement.clone());
            if trimmed != replacement {
                changed = true;
            }
            continue;
        }
        if trimmed.starts_with("customDomains = [") {
            let replacement = format!("customDomains = [\"{hostname}\"]");
            lines.push(replacement.clone());
            if trimmed != replacement {
                changed = true;
            }
            continue;
        }
        lines.push(line.to_string());
    }

    if !changed {
        return Ok(false);
    }

    fs::write(&frpc_path, format!("{}\r\n", lines.join("\r\n")))
        .with_context(|| format!("failed to write {}", frpc_path.display()))?;
    Ok(true)
}

fn restart_frpc_process(paths: &BundlePaths) -> Result<()> {
    let frpc_path = paths.frp_dir.join("frpc.exe");
    if !frpc_path.exists() {
        return Ok(());
    }

    let mut state = read_state(paths).unwrap_or_default();
    if let Some(pid) = state.frpc_pid {
        let _ = taskkill_pid(pid);
    }

    let frpc_pid = spawn_process(&frpc_path, &["-c", "frpc.toml"], &paths.frp_dir)?;
    state.frpc_pid = Some(frpc_pid);
    state.updated_at_unix_ms = now_unix_ms();
    write_state(paths, state)?;
    append_supervisor_log(
        paths,
        &format!("restart_frpc_process spawned frpc pid={frpc_pid}"),
    )?;
    Ok(())
}

fn sync_canonical_public_route(
    paths: &BundlePaths,
    payload: &HostActivationStatusPayload,
) -> Result<()> {
    let target_public_url = payload.canonical_public_url.trim();
    if target_public_url.is_empty() {
        return Ok(());
    }

    let current_public_url = read_public_url(paths);
    let current_hostname = extract_hostname(&current_public_url).unwrap_or_default();
    let target_hostname = extract_hostname(target_public_url).unwrap_or_default();
    if target_hostname.is_empty() {
        return Ok(());
    }

    let mut route_changed = false;
    if !current_public_url.eq_ignore_ascii_case(target_public_url) {
        write_public_url(paths, target_public_url)?;
        route_changed = true;
        append_supervisor_log(
            paths,
            &format!(
                "canonical public url updated: {} -> {}",
                current_public_url, target_public_url
            ),
        )?;
    }

    if !route_changed && current_hostname.eq_ignore_ascii_case(&target_hostname) {
        return Ok(());
    }

    let frpc_changed = rewrite_frpc_public_host(paths, &target_hostname)?;
    if frpc_changed {
        append_supervisor_log(
            paths,
            &format!(
                "frpc public host updated: {} -> {}",
                current_hostname, target_hostname
            ),
        )?;
    }

    if frpc_changed {
        let state = read_state(paths).unwrap_or_default();
        if state.frpc_pid.is_some()
            || state.web_server_pid.is_some()
            || state.sunshine_pid.is_some()
        {
            restart_frpc_process(paths)?;
        }
    } else if route_changed {
        append_supervisor_log(paths, "canonical public url updated without frpc change")?;
    }

    Ok(())
}

fn runtime_display_name(runtime_key: &str) -> String {
    let key = runtime_key.trim().to_ascii_lowercase();
    if key.contains("legacy") {
        "Cloudgime Compatibility Runtime".to_string()
    } else {
        "Cloudgime Modern Runtime".to_string()
    }
}

impl LifecyclePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Recovering => "recovering",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

fn clear_managed_process_state(paths: &BundlePaths) -> Result<()> {
    let mut state = read_state(paths).unwrap_or_default();
    state.sunshine_pid = None;
    state.web_server_pid = None;
    state.frpc_pid = None;
    state.runtime_key = None;
    state.updated_at_unix_ms = now_unix_ms();
    write_state(paths, state)
}

fn stop_bundle_processes(paths: &BundlePaths, scope: StopScope) -> Result<()> {
    append_supervisor_log(
        paths,
        &format!(
            "stop_bundle_processes begin scope={}",
            match scope {
                StopScope::RuntimeOnly => "runtime",
                StopScope::All => "all",
            }
        ),
    )?;
    let config = load_config(paths)?;
    let _ = stop_managed_windows_service(paths, &default_sunshine_service_name(paths));
    if let Some(state) = read_state(paths) {
        let pids = match scope {
            StopScope::RuntimeOnly => vec![state.sunshine_pid],
            StopScope::All => vec![state.sunshine_pid, state.web_server_pid, state.frpc_pid],
        };
        for pid in pids.into_iter().flatten() {
            let _ = taskkill_pid(pid);
        }
    }

    let processes = list_bundle_processes(paths, scope)?;
    for (pid, _) in processes {
        let _ = taskkill_pid(pid);
    }

    purge_staged_runtime(paths)?;
    append_supervisor_log(paths, "stop_bundle_processes staged runtime purged")?;

    wait_for_bundle_processes_closed(paths, scope, Duration::from_secs(15))?;
    append_supervisor_log(paths, "stop_bundle_processes processes closed")?;

    match scope {
        StopScope::RuntimeOnly => {
            let _ = wait_for_tcp_closed(
                "127.0.0.1",
                config.moonlight.default_http_port,
                Duration::from_secs(15),
            );
        }
        StopScope::All => {
            let _ = wait_for_tcp_closed(
                "127.0.0.1",
                config.moonlight.default_http_port,
                Duration::from_secs(15),
            );
            let bind = config.web_server.bind_address;
            let _ = wait_for_tcp_closed(
                bind.ip().to_string().as_str(),
                bind.port(),
                Duration::from_secs(15),
            );
        }
    }

    sleep(Duration::from_millis(1500));
    append_supervisor_log(paths, "stop_bundle_processes settle sleep complete")?;

    match scope {
        StopScope::All => clear_managed_process_state(paths),
        StopScope::RuntimeOnly => {
            let mut state = read_state(paths).unwrap_or_default();
            state.sunshine_pid = None;
            state.runtime_key = None;
            state.updated_at_unix_ms = now_unix_ms();
            write_state(paths, state)
        }
    }?;

    restore_windows_cursor_defaults();
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NvidiaMemorySnapshot {
    total_mib: u64,
    free_mib: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum SunshineEncoderStartupState {
    Pending,
    Ready(String),
    Failed(String),
}

struct RemoteDesktopStartupRelief {
    paths: BundlePaths,
    restart_parsec: bool,
    restored: bool,
}

impl RemoteDesktopStartupRelief {
    fn begin(paths: &BundlePaths) -> Result<Self> {
        let mut relief = Self {
            paths: paths.clone(),
            restart_parsec: false,
            restored: false,
        };

        let snapshots = match query_nvidia_memory_snapshots() {
            Ok(snapshots) => snapshots,
            Err(error) => {
                append_supervisor_log(
                    paths,
                    &format!("sunshine startup VRAM probe skipped err={error:#}"),
                )?;
                return Ok(relief);
            }
        };
        let snapshot_text = format_nvidia_memory_snapshots(&snapshots);
        if !should_release_remote_desktop_vram(&snapshots) {
            append_supervisor_log(
                paths,
                &format!("sunshine startup VRAM sufficient gpus={snapshot_text}"),
            )?;
            return Ok(relief);
        }

        if !process_image_name_exists("parsecd.exe")? && !process_image_name_exists("pservice.exe")?
        {
            append_supervisor_log(
                paths,
                &format!(
                    "sunshine startup low VRAM detected but Parsec processes are not running gpus={snapshot_text}"
                ),
            )?;
            return Ok(relief);
        }

        relief.restart_parsec = true;
        append_supervisor_log(
            paths,
            &format!("sunshine startup low VRAM relief begin service=Parsec gpus={snapshot_text}"),
        )?;
        if let Err(error) = stop_managed_windows_service(paths, "Parsec") {
            append_supervisor_log(
                paths,
                &format!(
                    "sunshine startup Parsec service stop fallback to process cleanup err={error:#}"
                ),
            )?;
        }
        release_parsec_processes(Duration::from_secs(8))?;
        sleep(Duration::from_millis(1200));

        let after = query_nvidia_memory_snapshots().unwrap_or_default();
        append_supervisor_log(
            paths,
            &format!(
                "sunshine startup low VRAM relief ready service=Parsec gpus={}",
                format_nvidia_memory_snapshots(&after)
            ),
        )?;
        Ok(relief)
    }

    fn restore(&mut self) -> Result<()> {
        if !self.restart_parsec || self.restored {
            return Ok(());
        }

        restore_parsec_service(&self.paths)?;
        self.restored = true;
        append_supervisor_log(
            &self.paths,
            "sunshine startup low VRAM relief restored service=Parsec",
        )
    }
}

impl Drop for RemoteDesktopStartupRelief {
    fn drop(&mut self) {
        if self.restart_parsec && !self.restored {
            let _ = restore_parsec_service(&self.paths);
        }
    }
}

fn start_sunshine_runtime_ready(
    paths: &BundlePaths,
    runtime_dir: &Path,
    http_port: u16,
) -> Result<u32> {
    let mut remote_relief = RemoteDesktopStartupRelief::begin(paths)?;
    let sunshine_log_path = runtime_dir.join("config").join("sunshine.log");
    let sunshine_log_offset = fs::metadata(&sunshine_log_path)
        .map(|metadata| metadata.len())
        .unwrap_or_default();

    let startup_result = (|| {
        let sunshine_pid = start_sunshine_runtime(paths, runtime_dir)?;
        append_supervisor_log(
            paths,
            &format!("sunshine runtime spawned pid={sunshine_pid}"),
        )?;
        wait_for_tcp_ready("127.0.0.1", http_port, Duration::from_secs(20))?;
        append_supervisor_log(paths, "sunshine runtime port ready")?;
        wait_for_sunshine_encoder_ready(
            &sunshine_log_path,
            sunshine_log_offset,
            Duration::from_secs(25),
        )?;
        append_supervisor_log(paths, "sunshine runtime H.264 encoder ready")?;
        Ok(sunshine_pid)
    })();
    let restore_result = remote_relief.restore();

    match (startup_result, restore_result) {
        (Ok(pid), Ok(())) => Ok(pid),
        (Err(startup_error), Ok(())) => Err(startup_error),
        (Ok(_), Err(restore_error)) => Err(restore_error)
            .context("Sunshine started but the temporary Parsec VRAM relief was not restored"),
        (Err(startup_error), Err(restore_error)) => Err(anyhow!(
            "Sunshine startup failed: {startup_error:#}; restoring Parsec also failed: {restore_error:#}"
        )),
    }
}

fn query_nvidia_memory_snapshots() -> Result<Vec<NvidiaMemorySnapshot>> {
    let mut command = Command::new("nvidia-smi");
    apply_background_spawn_flags(&mut command);
    let output = command
        .args([
            "--query-gpu=memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .context("failed to run nvidia-smi")?;
    if !output.status.success() {
        bail!(
            "nvidia-smi memory query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let snapshots = parse_nvidia_memory_snapshots(&String::from_utf8_lossy(&output.stdout));
    if snapshots.is_empty() {
        bail!("nvidia-smi returned no parseable GPU memory rows");
    }
    Ok(snapshots)
}

fn parse_nvidia_memory_snapshots(raw: &str) -> Vec<NvidiaMemorySnapshot> {
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let total_mib = fields.next()?.parse::<u64>().ok()?;
            let free_mib = fields.next()?.parse::<u64>().ok()?;
            Some(NvidiaMemorySnapshot {
                total_mib,
                free_mib,
            })
        })
        .collect()
}

fn should_release_remote_desktop_vram(snapshots: &[NvidiaMemorySnapshot]) -> bool {
    snapshots.iter().any(|snapshot| {
        snapshot.total_mib <= LOW_VRAM_GPU_TOTAL_MIB && snapshot.free_mib < LOW_VRAM_GPU_FREE_MIB
    })
}

fn format_nvidia_memory_snapshots(snapshots: &[NvidiaMemorySnapshot]) -> String {
    if snapshots.is_empty() {
        return "unavailable".to_string();
    }
    snapshots
        .iter()
        .enumerate()
        .map(|(index, snapshot)| {
            format!(
                "gpu{index}:total={}MiB,free={}MiB",
                snapshot.total_mib, snapshot.free_mib
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn process_image_name_exists(image_name: &str) -> Result<bool> {
    let target = image_name.to_ascii_lowercase();
    for pid in enumerate_process_ids().context("failed to enumerate processes for VRAM relief")? {
        let Some(path) = query_process_image_path(pid) else {
            continue;
        };
        let matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase() == target)
            .unwrap_or(false);
        if matches {
            return Ok(true);
        }
    }
    Ok(false)
}

fn terminate_processes_by_image_name(image_name: &str) -> Result<()> {
    let target = image_name.to_ascii_lowercase();
    for pid in enumerate_process_ids().context("failed to enumerate processes for VRAM relief")? {
        let Some(path) = query_process_image_path(pid) else {
            continue;
        };
        let matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase() == target)
            .unwrap_or(false);
        if matches {
            if terminate_pid_direct(pid).is_err() {
                let _ = taskkill_pid(pid);
            }
        }
    }
    Ok(())
}

fn release_parsec_processes(timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        terminate_processes_by_image_name("parsecd.exe")?;
        terminate_processes_by_image_name("pservice.exe")?;
        sleep(Duration::from_millis(250));
        if !process_image_name_exists("parsecd.exe")? && !process_image_name_exists("pservice.exe")?
        {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            bail!("timed out releasing Parsec processes for Sunshine startup");
        }
    }
}

fn restore_parsec_service(paths: &BundlePaths) -> Result<()> {
    if process_image_name_exists("pservice.exe")? {
        return Ok(());
    }

    let started = Instant::now();
    loop {
        match start_managed_windows_service(paths, "Parsec") {
            Ok(()) => break,
            Err(error) if started.elapsed() < Duration::from_secs(10) => {
                append_supervisor_log(
                    paths,
                    &format!("waiting to restore Parsec service err={error:#}"),
                )?;
                sleep(Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }

    let started = Instant::now();
    loop {
        if process_image_name_exists("pservice.exe")? {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(10) {
            bail!("Parsec service started but pservice.exe did not appear");
        }
        sleep(Duration::from_millis(250));
    }
}

fn wait_for_sunshine_encoder_ready(
    log_path: &Path,
    initial_offset: u64,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    let mut last_log = String::new();
    loop {
        if let Ok(log) = read_log_since(log_path, initial_offset) {
            last_log = log;
            match classify_sunshine_encoder_startup(&last_log) {
                SunshineEncoderStartupState::Ready(_) => return Ok(()),
                SunshineEncoderStartupState::Failed(reason) => {
                    bail!("Sunshine encoder initialization failed: {reason}")
                }
                SunshineEncoderStartupState::Pending => {}
            }
        }

        if started.elapsed() >= timeout {
            bail!(
                "timed out waiting for Sunshine H.264 encoder readiness; latest log={}",
                compact_log_tail(&last_log, 500)
            );
        }
        sleep(Duration::from_millis(250));
    }
}

fn read_log_since(path: &Path, initial_offset: u64) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open Sunshine log {}", path.display()))?;
    let length = file.metadata()?.len();
    let offset = if length < initial_offset {
        0
    } else {
        initial_offset
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn classify_sunshine_encoder_startup(raw: &str) -> SunshineEncoderStartupState {
    if let Some(line) = raw
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("found h.264 encoder:"))
    {
        return SunshineEncoderStartupState::Ready(line.trim().to_string());
    }

    let fatal_markers = [
        "0x8007000e",
        "failed to create a d3d11 device",
        "couldn't find any working encoder",
        "no working encoder",
        "fatal: please check that a display is connected",
    ];
    for line in raw.lines() {
        let lowered = line.to_ascii_lowercase();
        if fatal_markers.iter().any(|marker| lowered.contains(marker)) {
            return SunshineEncoderStartupState::Failed(line.trim().to_string());
        }
    }
    SunshineEncoderStartupState::Pending
}

fn compact_log_tail(raw: &str, max_chars: usize) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    compact
        .chars()
        .rev()
        .take(max_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn start_sunshine_runtime(paths: &BundlePaths, runtime_dir: &Path) -> Result<u32> {
    let sunshine_service_binary = runtime_dir.join("tools").join("sunshinesvc.exe");
    if sunshine_service_binary.exists() {
        match ensure_managed_sunshine_service(paths, runtime_dir).and_then(|sunshine_service| {
            start_managed_windows_service(paths, &sunshine_service)?;
            Ok(())
        }) {
            Ok(()) => {
                return resolve_sunshine_runtime_pid(paths).map(|value| value.unwrap_or(0));
            }
            Err(error) if service_permission_denied(&error.to_string()) => {
                append_supervisor_log(
                    paths,
                    &format!(
                        "managed runtime service fallback to direct sunshine.exe start because service access was denied: {error:#}"
                    ),
                )?;
            }
            Err(error) => return Err(error),
        }
    }

    let sunshine_path = runtime_dir.join("sunshine.exe");
    spawn_process(&sunshine_path, &["config\\sunshine.conf"], runtime_dir)
}

fn service_permission_denied(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("access is denied")
        || lowered.contains("openscmanager failed 5")
        || lowered.contains("error 5")
        || lowered.contains("5:")
}

fn resolve_sunshine_runtime_pid(paths: &BundlePaths) -> Result<Option<u32>> {
    let processes = list_bundle_processes(paths, StopScope::RuntimeOnly)?;
    let mut service_pid = None;
    for (pid, path) in processes {
        let lowered = path.display().to_string().to_ascii_lowercase();
        if lowered.ends_with("\\sunshine.exe") {
            return Ok(Some(pid));
        }
        if lowered.ends_with("\\sunshinesvc.exe") {
            service_pid = Some(pid);
        }
    }

    Ok(service_pid)
}

#[cfg(windows)]
fn ensure_managed_sunshine_service(paths: &BundlePaths, runtime_dir: &Path) -> Result<String> {
    let service_name = default_sunshine_service_name(paths);
    let display_name = format!(
        "Cloudgime Runtime {}",
        service_name.trim_start_matches("CloudgimeRuntime-")
    );
    let service_bin = runtime_dir.join("tools").join("sunshinesvc.exe");
    if !service_bin.exists() {
        bail!(
            "missing runtime service binary at {}",
            service_bin.display()
        );
    }

    let service_info = ServiceInfo {
        name: OsString::from(&service_name),
        display_name: OsString::from(&display_name),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_bin.clone(),
        launch_arguments: Vec::new(),
        dependencies: Vec::new(),
        account_name: None,
        account_password: None,
    };
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .context("failed to connect to the Windows service manager")?;
    match manager.open_service(
        &service_name,
        ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG,
    ) {
        Ok(service) => {
            let current = service.query_config().with_context(|| {
                format!("failed to query Windows service config {service_name}")
            })?;
            if current.executable_path != service_bin
                || current.service_type != ServiceType::OWN_PROCESS
                || current.start_type != ServiceStartType::OnDemand
                || current.error_control != ServiceErrorControl::Normal
                || current.display_name != OsString::from(&display_name)
            {
                service.change_config(&service_info).with_context(|| {
                    format!("failed to update Windows service config {service_name}")
                })?;
            }
        }
        Err(error) if windows_service_error_code(&error) == Some(1060) => {
            manager
                .create_service(&service_info, ServiceAccess::QUERY_STATUS)
                .with_context(|| format!("failed to create Windows service {service_name}"))?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to open Windows service {service_name}"));
        }
    }

    Ok(service_name)
}

#[cfg(not(windows))]
fn ensure_managed_sunshine_service(_paths: &BundlePaths, _runtime_dir: &Path) -> Result<String> {
    bail!("managed Sunshine services are only supported on Windows")
}

#[cfg(windows)]
fn windows_service_error_code(error: &windows_service::Error) -> Option<i32> {
    match error {
        windows_service::Error::Winapi(error) => error.raw_os_error(),
        _ => None,
    }
}

#[cfg(windows)]
fn start_managed_windows_service(paths: &BundlePaths, service_name: &str) -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("failed to connect to the Windows service manager")?;
    let service = manager
        .open_service(
            service_name,
            ServiceAccess::QUERY_STATUS | ServiceAccess::START,
        )
        .with_context(|| format!("failed to open Windows service {service_name} for start"))?;
    let mut status = service
        .query_status()
        .with_context(|| format!("failed to query Windows service {service_name}"))?;

    if status.current_state == WindowsServiceState::Running {
        append_supervisor_log(
            paths,
            &format!("managed Windows service {service_name} already running"),
        )?;
        return Ok(());
    }
    if status.current_state == WindowsServiceState::StopPending {
        wait_for_service_api_state(
            &service,
            service_name,
            WindowsServiceState::Stopped,
            Duration::from_secs(35),
        )?;
        status = service.query_status()?;
    }
    if status.current_state != WindowsServiceState::StartPending {
        service
            .start::<&str>(&[])
            .with_context(|| format!("failed to start Windows service {service_name}"))?;
    }
    wait_for_service_api_state(
        &service,
        service_name,
        WindowsServiceState::Running,
        Duration::from_secs(35),
    )?;
    append_supervisor_log(
        paths,
        &format!("started managed Windows service {service_name}"),
    )
}

#[cfg(not(windows))]
fn start_managed_windows_service(_paths: &BundlePaths, service_name: &str) -> Result<()> {
    bail!("Windows service {service_name} cannot be started on this platform")
}

#[cfg(windows)]
fn stop_managed_windows_service(paths: &BundlePaths, service_name: &str) -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("failed to connect to the Windows service manager")?;
    let service = manager
        .open_service(
            service_name,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
        )
        .with_context(|| format!("failed to open Windows service {service_name} for stop"))?;
    let status = service
        .query_status()
        .with_context(|| format!("failed to query Windows service {service_name}"))?;
    if status.current_state == WindowsServiceState::Stopped {
        return Ok(());
    }
    if status.current_state != WindowsServiceState::StopPending {
        service
            .stop()
            .with_context(|| format!("failed to stop Windows service {service_name}"))?;
    }
    wait_for_service_api_state(
        &service,
        service_name,
        WindowsServiceState::Stopped,
        Duration::from_secs(35),
    )?;
    append_supervisor_log(
        paths,
        &format!("stopped managed Windows service {service_name}"),
    )
}

#[cfg(not(windows))]
fn stop_managed_windows_service(_paths: &BundlePaths, service_name: &str) -> Result<()> {
    bail!("Windows service {service_name} cannot be stopped on this platform")
}

#[cfg(windows)]
fn wait_for_service_api_state(
    service: &windows_service::service::Service,
    service_name: &str,
    expected_state: WindowsServiceState,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    loop {
        let status = service
            .query_status()
            .with_context(|| format!("failed to query Windows service {service_name}"))?;
        if status.current_state == expected_state {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            bail!(
                "timed out waiting for Windows service {service_name} state={expected_state:?}; current={:?}",
                status.current_state
            );
        }
        sleep(Duration::from_millis(250));
    }
}

fn purge_staged_runtime(paths: &BundlePaths) -> Result<()> {
    let staged_dir = paths.moonlight_dir.join("staged-runtime");
    if !staged_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&staged_dir)
        .with_context(|| format!("failed to read {}", staged_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let managed = (lower.starts_with("streamer-")
            || lower.starts_with("mic_sidecar-")
            || lower.starts_with("gamepad_sidecar-"))
            && lower.ends_with(".exe");
        if managed {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

fn list_bundle_processes(paths: &BundlePaths, scope: StopScope) -> Result<Vec<(u32, PathBuf)>> {
    let include_web = matches!(scope, StopScope::All);
    let root = normalized_windows_prefix(&paths.bundle_root);
    let mut processes = Vec::new();
    for pid in enumerate_process_ids().context("failed to enumerate processes")? {
        let Some(path) = query_process_image_path(pid) else {
            continue;
        };
        if !path_matches_bundle_root(&path, &root) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !matches_bundle_process_name(name, include_web) {
            continue;
        }
        processes.push((pid, path));
    }
    Ok(processes)
}

fn taskkill_pid(pid: u32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to terminate pid {pid}"))?;

    if !status.success() {
        bail!("taskkill returned non-zero status for pid {pid}: {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_pid_direct(pid: u32) -> Result<()> {
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        if !process_exists(pid)? {
            return Ok(());
        }
        bail!(
            "OpenProcess(PROCESS_TERMINATE) failed for pid {pid}: {}",
            std::io::Error::last_os_error()
        );
    }

    let terminated = unsafe { TerminateProcess(handle, 1) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if terminated == 0 {
        if !process_exists(pid)? {
            return Ok(());
        }
        bail!(
            "TerminateProcess failed for pid {pid}: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn terminate_pid_direct(pid: u32) -> Result<()> {
    taskkill_pid(pid)
}

fn wait_for_bundle_processes_closed(
    paths: &BundlePaths,
    scope: StopScope,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    loop {
        if list_bundle_processes(paths, scope)?.is_empty() {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            bail!("timed out waiting for bundle processes to stop");
        }
        sleep(Duration::from_millis(250));
    }
}

fn ensure_pid_alive(pid: u32, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        if process_exists(pid)? {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            bail!("pid {pid} is not alive");
        }
        sleep(Duration::from_millis(100));
    }
}

fn process_exists(pid: u32) -> Result<bool> {
    Ok(enumerate_process_ids()
        .with_context(|| format!("failed to query pid {pid}"))?
        .into_iter()
        .any(|candidate| candidate == pid))
}

fn wait_for_tcp_ready(address: &str, port: u16, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let target = parse_socket_address(address, port)?;

    loop {
        let attempt_error = match TcpStream::connect_timeout(&target, Duration::from_millis(500)) {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(err) => err.to_string(),
        };

        if started.elapsed() >= timeout {
            bail!("timed out waiting for {address}:{port} to accept connections: {attempt_error}");
        }
        sleep(Duration::from_millis(250));
    }
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
    use std::io::{Read as _, Write as _};
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

fn wait_for_tcp_closed(address: &str, port: u16, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let target = parse_socket_address(address, port)?;

    loop {
        if TcpStream::connect_timeout(&target, Duration::from_millis(400)).is_err() {
            return Ok(());
        }

        if started.elapsed() >= timeout {
            bail!("timed out waiting for {address}:{port} to stop accepting connections");
        }
        sleep(Duration::from_millis(250));
    }
}

fn parse_socket_address(address: &str, port: u16) -> Result<SocketAddr> {
    format!("{address}:{port}")
        .parse::<SocketAddr>()
        .map_err(|err| anyhow!("invalid socket address {address}:{port}: {err}"))
}

#[cfg(windows)]
fn matches_bundle_process_name(name: &str, include_web: bool) -> bool {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "sunshine.exe"
        | "sunshinesvc.exe"
        | "streamer.exe"
        | "mic_sidecar.exe"
        | "gamepad_sidecar.exe" => true,
        "web-server.exe" | "frpc.exe" => include_web,
        _ => {
            lower.starts_with("streamer-")
                || lower.starts_with("mic_sidecar-")
                || lower.starts_with("gamepad_sidecar-")
        }
    }
}

#[cfg(windows)]
fn normalized_windows_prefix(path: &Path) -> String {
    let mut normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    if !normalized.ends_with('\\') {
        normalized.push('\\');
    }
    normalized
}

#[cfg(windows)]
fn path_matches_bundle_root(path: &Path, root: &str) -> bool {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
        .starts_with(root)
}

#[cfg(windows)]
fn enumerate_process_ids() -> Result<Vec<u32>> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            bail!("CreateToolhelp32Snapshot failed");
        }

        let mut processes = Vec::new();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                processes.push(entry.th32ProcessID);
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
        Ok(processes)
    }
}

#[cfg(windows)]
fn query_process_image_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let success = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if success == 0 || size == 0 {
            return None;
        }

        buffer.truncate(size as usize);
        Some(PathBuf::from(OsString::from_wide(&buffer)))
    }
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

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_memory_rows() {
        assert_eq!(
            parse_nvidia_memory_snapshots("1024, 296\n24576, 22000\ninvalid\n"),
            vec![
                NvidiaMemorySnapshot {
                    total_mib: 1024,
                    free_mib: 296,
                },
                NvidiaMemorySnapshot {
                    total_mib: 24576,
                    free_mib: 22000,
                },
            ]
        );
    }

    #[test]
    fn releases_remote_desktop_vram_only_for_constrained_gpu() {
        assert!(should_release_remote_desktop_vram(&[
            NvidiaMemorySnapshot {
                total_mib: 1024,
                free_mib: 296,
            }
        ]));
        assert!(!should_release_remote_desktop_vram(&[
            NvidiaMemorySnapshot {
                total_mib: 1024,
                free_mib: 700,
            },
            NvidiaMemorySnapshot {
                total_mib: 24576,
                free_mib: 300,
            },
        ]));
    }

    #[test]
    fn classifies_sunshine_encoder_readiness_and_failure() {
        assert!(matches!(
            classify_sunshine_encoder_startup(
                "[time] Info: Found H.264 encoder: libx264 [software]"
            ),
            SunshineEncoderStartupState::Ready(_)
        ));
        assert!(matches!(
            classify_sunshine_encoder_startup(
                "[time] Error: Failed to create a D3D11 device: 0x8007000E"
            ),
            SunshineEncoderStartupState::Failed(_)
        ));
        assert_eq!(
            classify_sunshine_encoder_startup("[time] Info: Web UI available"),
            SunshineEncoderStartupState::Pending
        );
    }
}
