#![feature(async_fn_traits)]

use std::{
    collections::VecDeque,
    future::{Future, ready},
    net::UdpSocket as StdUdpSocket,
    path::Path,
    pin::Pin,
    process::Command as StdCommand,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use common::{
    api_bindings::{
        LogMessageType, MicSidecarClientMessage, MicSidecarServerMessage, RtcIceCandidate,
        RtcSdpType, RtcSessionDescription, StreamSignalingMessage,
    },
    ipc::{MicSidecarIpcMessage, MicSidecarServerIpcMessage, create_process_ipc},
};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use tokio::{
    io::{stdin, stdout},
    sync::Mutex,
};
use tracing::{Level, debug, info, level_filters::LevelFilter, span, warn};
use tracing_log::LogTracer;
use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
        setting_engine::SettingEngine,
    },
    ice::udp_network::{EphemeralUDP, UDPNetwork},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_connection_state::RTCIceConnectionState,
    },
    interceptor::registry::Registry as WebRtcRegistry,
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::{sdp_type::RTCSdpType, session_description::RTCSessionDescription},
    },
    rtp_transceiver::{
        RTCRtpTransceiver, RTCRtpTransceiverInit, rtp_codec::RTPCodecType,
        rtp_receiver::RTCRtpReceiver, rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    track::track_remote::TrackRemote,
};

#[path = "../convert.rs"]
mod convert;
#[path = "../transport/webrtc/microphone.rs"]
mod microphone;

use convert::{
    from_webrtc_sdp, into_webrtc_ice, into_webrtc_ice_candidate, into_webrtc_network_type,
};
use microphone::HostMicrophoneLoopback;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0},
    Security::{
        DuplicateTokenEx, SecurityImpersonation, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
    },
    System::{
        Console::{FreeConsole, GetConsoleWindow},
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        RemoteDesktop::{
            ProcessIdToSessionId, WTS_SESSION_INFOW, WTSActive, WTSConnected, WTSDomainName,
            WTSEnumerateSessionsW, WTSFreeMemory, WTSGetActiveConsoleSessionId,
            WTSQuerySessionInformationW, WTSQueryUserToken, WTSUserName,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
            CreateProcessWithTokenW, GetCurrentProcessId, GetExitCodeProcess, LOGON_WITH_PROFILE,
            NORMAL_PRIORITY_CLASS, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
            WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{SW_HIDE, ShowWindow},
};

type IpcSender = common::ipc::IpcSender<MicSidecarIpcMessage>;

const MIC_SINK_PACKET_AUDIO: u8 = 1;
const MIC_SINK_PACKET_GAIN: u8 = 2;
const MIC_SINK_PACKET_STOP: u8 = 3;
const MIC_SINK_PACKET_PING: u8 = 4;
const MIC_SINK_PACKET_READY: u8 = 0x80;
const MIC_SINK_START_TIMEOUT: Duration = Duration::from_secs(8);
const MIC_SINK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

fn init_rustls_crypto_provider() {
    let _ = CryptoProvider::install_default(aws_lc_rs::default_provider());
}

fn init_logging(level: log::LevelFilter) {
    let _ = LogTracer::init();
    let level = match level {
        log::LevelFilter::Off => LevelFilter::OFF,
        log::LevelFilter::Error => LevelFilter::ERROR,
        log::LevelFilter::Warn => LevelFilter::WARN,
        log::LevelFilter::Info => LevelFilter::INFO,
        log::LevelFilter::Debug => LevelFilter::DEBUG,
        log::LevelFilter::Trace => LevelFilter::TRACE,
    };
    let filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();
    let _ = Registry::default()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

#[cfg(windows)]
fn hide_audio_sink_console() {
    unsafe {
        let console = GetConsoleWindow();
        if !console.is_null() {
            let _ = ShowWindow(console, SW_HIDE);
            let _ = FreeConsole();
        }
    }
}

fn user_friendly_microphone_start_error(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("no virtual microphone sink device")
        || normalized.contains("paired virtual microphone input was not found")
    {
        return "Mic belum bisa aktif. Driver audio Cloudgime di PC host belum siap, jadi suara dari perangkat ini belum bisa masuk ke game atau aplikasi.".to_owned();
    }
    if normalized.contains("default microphone") || normalized.contains("policy config") {
        return "Mic aktif, tetapi PC host belum bisa memilih input otomatis. Buka ulang stream sebagai administrator atau pilih input Cloudgime di game/aplikasi.".to_owned();
    }

    "Mic belum bisa aktif di PC host. Coba buka ulang stream, lalu aktifkan mic lagi.".to_owned()
}

enum MicrophoneOutput {
    Direct(HostMicrophoneLoopback),
    #[cfg(windows)]
    Interactive(InteractiveMicrophoneSink),
}

impl MicrophoneOutput {
    fn new(preferred_channels: usize) -> Result<Self, String> {
        #[cfg(windows)]
        if current_process_session_id() == Some(0) {
            return InteractiveMicrophoneSink::new(preferred_channels).map(Self::Interactive);
        }

        HostMicrophoneLoopback::new(preferred_channels).map(Self::Direct)
    }

    fn set_gain_percent(&mut self, percent: u8) {
        match self {
            Self::Direct(output) => output.set_gain_percent(percent),
            #[cfg(windows)]
            Self::Interactive(output) => output.set_gain_percent(percent),
        }
    }

    fn render_opus_payload(&mut self, payload: &[u8]) -> Result<(), String> {
        match self {
            Self::Direct(output) => output.render_opus_payload(payload),
            #[cfg(windows)]
            Self::Interactive(output) => output.render_opus_payload(payload),
        }
    }

    fn capture_hint(&self) -> Option<String> {
        match self {
            Self::Direct(output) => output.capture_hint().map(str::to_owned),
            #[cfg(windows)]
            Self::Interactive(output) => Some(output.capture_hint.clone()),
        }
    }

    fn default_capture_name(&self) -> Option<String> {
        match self {
            Self::Direct(output) => output.default_capture_name(),
            #[cfg(windows)]
            Self::Interactive(_) => None,
        }
    }
}

#[cfg(windows)]
struct InteractiveMicrophoneSink {
    socket: StdUdpSocket,
    capture_hint: String,
}

#[cfg(windows)]
impl InteractiveMicrophoneSink {
    fn new(preferred_channels: usize) -> Result<Self, String> {
        let socket = StdUdpSocket::bind(("127.0.0.1", 0))
            .map_err(|error| format!("failed to bind microphone sink control socket: {error}"))?;
        let control_port = socket
            .local_addr()
            .map_err(|error| format!("failed to resolve microphone control port: {error}"))?
            .port();
        let reserved = StdUdpSocket::bind(("127.0.0.1", 0))
            .map_err(|error| format!("failed to reserve microphone sink port: {error}"))?;
        let sink_port = reserved
            .local_addr()
            .map_err(|error| format!("failed to resolve microphone sink port: {error}"))?
            .port();
        drop(reserved);

        launch_mic_audio_sink_in_active_session(sink_port, control_port, preferred_channels)?;
        socket
            .connect(("127.0.0.1", sink_port))
            .map_err(|error| format!("failed to connect microphone sink socket: {error}"))?;
        socket
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|error| format!("failed to configure microphone sink timeout: {error}"))?;

        let started = Instant::now();
        let mut response = [0_u8; 512];
        while started.elapsed() < MIC_SINK_START_TIMEOUT {
            let _ = socket.send(&[MIC_SINK_PACKET_PING]);
            match socket.recv(&mut response) {
                Ok(size) if size >= 2 && response[0] == MIC_SINK_PACKET_READY => {
                    if response[1] != 0 {
                        let message = String::from_utf8_lossy(&response[2..size]).to_string();
                        return Err(if message.trim().is_empty() {
                            "interactive microphone sink failed to start".to_owned()
                        } else {
                            message
                        });
                    }
                    let capture_hint = String::from_utf8_lossy(&response[2..size])
                        .trim()
                        .to_owned();
                    return Ok(Self {
                        socket,
                        capture_hint: if capture_hint.is_empty() {
                            "virtual microphone host".to_owned()
                        } else {
                            capture_hint
                        },
                    });
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(format!("microphone sink readiness check failed: {error}"));
                }
            }
        }

        Err(
            "interactive microphone sink timed out while opening the Windows audio device"
                .to_owned(),
        )
    }

    fn set_gain_percent(&self, percent: u8) {
        let _ = self.socket.send(&[MIC_SINK_PACKET_GAIN, percent.min(100)]);
    }

    fn render_opus_payload(&self, payload: &[u8]) -> Result<(), String> {
        if payload.is_empty() {
            return Ok(());
        }
        let mut packet = Vec::with_capacity(payload.len() + 1);
        packet.push(MIC_SINK_PACKET_AUDIO);
        packet.extend_from_slice(payload);
        self.socket
            .send(&packet)
            .map(|_| ())
            .map_err(|error| format!("failed to send microphone audio to user session: {error}"))
    }
}

#[cfg(windows)]
impl Drop for InteractiveMicrophoneSink {
    fn drop(&mut self) {
        let _ = self.socket.send(&[MIC_SINK_PACKET_STOP]);
    }
}

#[cfg(windows)]
fn current_process_session_id() -> Option<u32> {
    let mut session_id = 0u32;
    let ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) } != 0;
    ok.then_some(session_id)
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn interactive_user_session_candidates() -> Vec<u32> {
    let console_session_id = unsafe { WTSGetActiveConsoleSessionId() };
    let mut sessions_ptr: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut session_count = 0u32;
    let mut ranked_sessions = Vec::new();

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
            let is_console = session.SessionId == console_session_id;
            let rank = if session.State == WTSActive {
                Some(if is_console { 2 } else { 0 })
            } else if session.State == WTSConnected {
                Some(if is_console { 3 } else { 1 })
            } else {
                None
            };
            if let Some(rank) = rank {
                ranked_sessions.push((rank, session.SessionId));
            }
        }
        unsafe { WTSFreeMemory(sessions_ptr.cast()) };
    }

    ranked_sessions.sort_unstable();
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
fn query_wts_session_string(
    session_id: u32,
    info_class: windows_sys::Win32::System::RemoteDesktop::WTS_INFO_CLASS,
) -> Option<String> {
    let mut buffer: *mut u16 = std::ptr::null_mut();
    let mut bytes_returned = 0u32;
    let ok = unsafe {
        WTSQuerySessionInformationW(
            std::ptr::null_mut(),
            session_id,
            info_class,
            &mut buffer,
            &mut bytes_returned,
        )
    } != 0;
    if !ok || buffer.is_null() || bytes_returned < 2 {
        if !buffer.is_null() {
            unsafe { WTSFreeMemory(buffer.cast()) };
        }
        return None;
    }

    let wide_len = (bytes_returned as usize / 2).saturating_sub(1);
    let value = unsafe { std::slice::from_raw_parts(buffer, wide_len) };
    let value = String::from_utf16_lossy(value)
        .trim_matches(char::from(0))
        .trim()
        .to_owned();
    unsafe { WTSFreeMemory(buffer.cast()) };
    (!value.is_empty()).then_some(value)
}

#[cfg(windows)]
fn interactive_task_user_for_session(session_id: u32) -> Option<String> {
    let username = query_wts_session_string(session_id, WTSUserName)?;
    let domain = query_wts_session_string(session_id, WTSDomainName).unwrap_or_default();
    Some(if domain.is_empty() {
        username
    } else {
        format!("{domain}\\{username}")
    })
}

#[cfg(windows)]
fn command_output_detail(output: &std::process::Output) -> String {
    format!(
        "status={} stdout={} stderr={}",
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

#[cfg(windows)]
fn launch_mic_audio_sink_via_scheduled_task(
    session_id: u32,
    executable: &Path,
    sink_port: u16,
    control_port: u16,
    preferred_channels: usize,
) -> Result<(), String> {
    let run_as_user = interactive_task_user_for_session(session_id)
        .ok_or_else(|| format!("no interactive username found for session {session_id}"))?;
    let task_name = format!("CloudGimeMicSink-{session_id}-{sink_port}-{control_port}");
    let task_command = format!(
        "\"{}\" --mic-audio-sink {sink_port} {control_port} {}",
        executable.display(),
        preferred_channels.max(1)
    );

    let _ = StdCommand::new("schtasks")
        .args(["/Delete", "/TN", &task_name, "/F"])
        .output();
    let create_output = StdCommand::new("schtasks")
        .args([
            "/Create",
            "/TN",
            &task_name,
            "/SC",
            "DAILY",
            "/ST",
            "00:00",
            "/TR",
            &task_command,
            "/RU",
            &run_as_user,
            "/IT",
            "/F",
        ])
        .output()
        .map_err(|error| format!("failed to invoke schtasks /Create: {error}"))?;
    if !create_output.status.success() {
        return Err(format!(
            "schtasks /Create failed for {run_as_user}: {}",
            command_output_detail(&create_output)
        ));
    }

    let run_output = StdCommand::new("schtasks")
        .args(["/Run", "/TN", &task_name])
        .output()
        .map_err(|error| format!("failed to invoke schtasks /Run: {error}"))?;
    if !run_output.status.success() {
        let _ = StdCommand::new("schtasks")
            .args(["/Delete", "/TN", &task_name, "/F"])
            .output();
        return Err(format!(
            "schtasks /Run failed for {run_as_user}: {}",
            command_output_detail(&run_output)
        ));
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        let _ = StdCommand::new("schtasks")
            .args(["/Delete", "/TN", &task_name, "/F"])
            .output();
    });
    Ok(())
}

#[cfg(windows)]
fn launch_mic_audio_sink_in_active_session(
    sink_port: u16,
    control_port: u16,
    preferred_channels: usize,
) -> Result<u32, String> {
    let session_candidates = interactive_user_session_candidates();
    if session_candidates.is_empty() {
        return Err("no active Windows user session is available for microphone audio".to_owned());
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve microphone sink executable: {error}"))?;
    let mut token_launch_failures = Vec::new();
    for &session_id in &session_candidates {
        match launch_mic_audio_sink_via_user_token(
            session_id,
            &executable,
            sink_port,
            control_port,
            preferred_channels,
        ) {
            Ok(process_id) => return Ok(process_id),
            Err(error) => token_launch_failures.push(format!("session={session_id} {error}")),
        }
    }
    let mut scheduled_task_failures = Vec::new();
    for &session_id in &session_candidates {
        match launch_mic_audio_sink_via_scheduled_task(
            session_id,
            &executable,
            sink_port,
            control_port,
            preferred_channels,
        ) {
            Ok(()) => return Ok(0),
            Err(error) => scheduled_task_failures.push(format!("session={session_id} {error}")),
        }
    }

    Err(format!(
        "failed to launch microphone audio in user session: user_token={}; scheduled_task={}",
        token_launch_failures.join(" | "),
        scheduled_task_failures.join(" | ")
    ))
}

#[cfg(windows)]
fn launch_mic_audio_sink_via_user_token(
    session_id: u32,
    executable: &Path,
    sink_port: u16,
    control_port: u16,
    preferred_channels: usize,
) -> Result<u32, String> {
    let mut user_token: HANDLE = std::ptr::null_mut();
    if unsafe { WTSQueryUserToken(session_id, &mut user_token) } == 0 {
        return Err(format!(
            "failed to open active Windows user token: win32={}",
            unsafe { GetLastError() }
        ));
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
    unsafe { CloseHandle(user_token) };
    if !duplicate_ok {
        return Err(format!(
            "failed to prepare active Windows user token: win32={}",
            unsafe { GetLastError() }
        ));
    }

    let command_line_text = format!(
        "\"{}\" --mic-audio-sink {sink_port} {control_port} {}",
        executable.display(),
        preferred_channels.max(1)
    );
    let mut command_line = wide_null(&command_line_text);
    let application_name = wide_null(&executable.to_string_lossy());
    let current_directory = wide_null(
        executable
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
    let mut environment = std::ptr::null_mut();
    let environment_ready =
        unsafe { CreateEnvironmentBlock(&mut environment, primary_token, 0) } != 0;
    let creation_flags = CREATE_NO_WINDOW
        | NORMAL_PRIORITY_CLASS
        | if environment_ready {
            CREATE_UNICODE_ENVIRONMENT
        } else {
            0
        };
    let environment_ptr = if environment_ready {
        environment.cast_const()
    } else {
        std::ptr::null()
    };

    let created_as_user = unsafe {
        CreateProcessAsUserW(
            primary_token,
            application_name.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            creation_flags,
            environment_ptr,
            current_directory.as_ptr(),
            &startup_info,
            &mut process_information,
        )
    } != 0;
    let created = if created_as_user {
        true
    } else {
        let create_as_user_error = unsafe { GetLastError() };
        process_information = PROCESS_INFORMATION {
            ..unsafe { std::mem::zeroed() }
        };
        let mut token_command_line = wide_null(&command_line_text);
        let created_with_token = unsafe {
            CreateProcessWithTokenW(
                primary_token,
                LOGON_WITH_PROFILE,
                application_name.as_ptr(),
                token_command_line.as_mut_ptr(),
                creation_flags,
                environment_ptr,
                current_directory.as_ptr(),
                &startup_info,
                &mut process_information,
            )
        } != 0;
        if !created_with_token {
            let create_with_token_error = unsafe { GetLastError() };
            if environment_ready {
                unsafe { DestroyEnvironmentBlock(environment) };
            }
            unsafe { CloseHandle(primary_token) };
            return Err(format!(
                "CreateProcessAsUserW={create_as_user_error}, CreateProcessWithTokenW={create_with_token_error}"
            ));
        }
        true
    };

    if environment_ready {
        unsafe { DestroyEnvironmentBlock(environment) };
    }
    unsafe { CloseHandle(primary_token) };
    if !created {
        return Err("failed to launch microphone audio in user session".to_owned());
    }

    let process_id = process_information.dwProcessId;
    std::thread::sleep(Duration::from_millis(300));
    if unsafe { WaitForSingleObject(process_information.hProcess, 0) } == WAIT_OBJECT_0 {
        let mut exit_code = 0_u32;
        let _ = unsafe { GetExitCodeProcess(process_information.hProcess, &mut exit_code) };
        unsafe {
            CloseHandle(process_information.hThread);
            CloseHandle(process_information.hProcess);
        }
        return Err(format!(
            "user-token microphone sink exited during startup: code=0x{exit_code:08X}"
        ));
    }
    unsafe {
        CloseHandle(process_information.hThread);
        CloseHandle(process_information.hProcess);
    }
    Ok(process_id)
}

fn parse_audio_sink_args() -> Option<Result<(u16, u16, usize), String>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--mic-audio-sink") {
        return None;
    }
    let sink_port = args
        .next()
        .ok_or_else(|| "missing microphone sink port".to_owned())
        .and_then(|value| value.parse::<u16>().map_err(|error| error.to_string()));
    let control_port = args
        .next()
        .ok_or_else(|| "missing microphone control port".to_owned())
        .and_then(|value| value.parse::<u16>().map_err(|error| error.to_string()));
    let channels = args
        .next()
        .ok_or_else(|| "missing microphone channel count".to_owned())
        .and_then(|value| value.parse::<usize>().map_err(|error| error.to_string()));
    Some(match (sink_port, control_port, channels) {
        (Ok(sink_port), Ok(control_port), Ok(channels)) => {
            Ok((sink_port, control_port, channels.max(1)))
        }
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => Err(error),
    })
}

fn run_audio_sink(
    sink_port: u16,
    control_port: u16,
    preferred_channels: usize,
) -> Result<(), String> {
    let socket = StdUdpSocket::bind(("127.0.0.1", sink_port))
        .map_err(|error| format!("failed to bind user-session microphone sink: {error}"))?;
    socket
        .connect(("127.0.0.1", control_port))
        .map_err(|error| format!("failed to connect microphone sink control: {error}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("failed to configure microphone sink timeout: {error}"))?;

    let mut output = match HostMicrophoneLoopback::new(preferred_channels) {
        Ok(output) => output,
        Err(error) => {
            let mut response = vec![MIC_SINK_PACKET_READY, 1];
            response.extend_from_slice(error.as_bytes());
            let _ = socket.send(&response);
            return Err(error);
        }
    };
    let capture_hint = output.capture_hint().unwrap_or("virtual microphone host");
    let mut ready = vec![MIC_SINK_PACKET_READY, 0];
    ready.extend_from_slice(capture_hint.as_bytes());
    let _ = socket.send(&ready);

    let mut buffer = vec![0_u8; 65_536];
    let mut last_packet = Instant::now();
    loop {
        match socket.recv(&mut buffer) {
            Ok(0) => {}
            Ok(size) => {
                last_packet = Instant::now();
                match buffer[0] {
                    MIC_SINK_PACKET_AUDIO if size > 1 => {
                        let _ = output.render_opus_payload(&buffer[1..size]);
                    }
                    MIC_SINK_PACKET_GAIN if size > 1 => output.set_gain_percent(buffer[1]),
                    MIC_SINK_PACKET_STOP => break,
                    MIC_SINK_PACKET_PING => {
                        let _ = socket.send(&ready);
                    }
                    _ => {}
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if last_packet.elapsed() >= MIC_SINK_IDLE_TIMEOUT {
                    break;
                }
            }
            Err(error) => return Err(format!("microphone sink socket failed: {error}")),
        }
    }
    Ok(())
}

#[allow(clippy::complexity)]
fn create_event_handler<F, Args>(
    inner: Weak<MicPeer>,
    f: F,
) -> Box<
    dyn FnMut(Args) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync + 'static,
>
where
    Args: Send + 'static,
    F: AsyncFn(Arc<MicPeer>, Args) + Send + Sync + Clone + 'static,
    for<'a> F::CallRefFuture<'a>: Send,
{
    Box::new(move |args: Args| {
        let Some(inner) = inner.upgrade() else {
            return Box::pin(ready(())) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
        };
        let future = f.clone();
        Box::pin(async move { future(inner, args).await })
            as Pin<Box<dyn Future<Output = ()> + Send + 'static>>
    })
}

struct MicPeer {
    peer: Arc<RTCPeerConnection>,
    ipc_sender: Arc<Mutex<IpcSender>>,
    remote_description_ready: AtomicBool,
    gain_percent: AtomicU8,
    pending_remote_ice: Mutex<VecDeque<RTCIceCandidateInit>>,
    loopback: std::sync::Mutex<Option<MicrophoneOutput>>,
}

impl MicPeer {
    async fn new(
        config: &common::ipc::StreamerConfig,
        ipc_sender: Arc<Mutex<IpcSender>>,
    ) -> Result<Arc<Self>, anyhow::Error> {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;
        media_engine.register_codec(
            webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecParameters {
                capability: webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
                    mime_type: webrtc::api::media_engine::MIME_TYPE_OPUS.to_owned(),
                    clock_rate: 48_000,
                    channels: 1,
                    sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 111,
                ..Default::default()
            },
            RTPCodecType::Audio,
        )?;

        let mut registry = WebRtcRegistry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        let mut setting_engine = SettingEngine::default();
        if let Some(port_range) = &config.webrtc.port_range {
            match EphemeralUDP::new(port_range.min, port_range.max) {
                Ok(udp) => setting_engine.set_udp_network(UDPNetwork::Ephemeral(udp)),
                Err(err) => warn!("[Mic Sidecar] invalid WebRTC port range: {err:?}"),
            }
        }
        if let Some(mapping) = config.webrtc.nat_1to1.as_ref() {
            setting_engine.set_nat_1to1_ips(
                mapping.ips.clone(),
                into_webrtc_ice_candidate(mapping.ice_candidate_type),
            );
        }
        setting_engine.set_network_types(
            config
                .webrtc
                .network_types
                .iter()
                .copied()
                .map(into_webrtc_network_type)
                .collect(),
        );
        setting_engine.set_include_loopback_candidate(config.webrtc.include_loopback_candidates);

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        let peer = Arc::new(
            api.new_peer_connection(RTCConfiguration {
                ice_servers: config
                    .webrtc
                    .ice_servers
                    .iter()
                    .cloned()
                    .map(into_webrtc_ice)
                    .collect(),
                ..Default::default()
            })
            .await?,
        );

        peer.add_transceiver_from_kind(
            RTPCodecType::Audio,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Recvonly,
                send_encodings: vec![],
            }),
        )
        .await?;

        let this = Arc::new(Self {
            peer,
            ipc_sender,
            remote_description_ready: AtomicBool::new(false),
            gain_percent: AtomicU8::new(100),
            pending_remote_ice: Mutex::new(VecDeque::new()),
            loopback: std::sync::Mutex::new(None),
        });

        this.attach_callbacks();
        Ok(this)
    }

    fn attach_callbacks(self: &Arc<Self>) {
        let this = Arc::downgrade(self);
        self.peer.on_ice_candidate(create_event_handler(
            this.clone(),
            async move |this, candidate| {
                this.on_ice_candidate(candidate).await;
            },
        ));

        self.peer
            .on_ice_connection_state_change(create_event_handler(
                this.clone(),
                async move |this, state| {
                    this.on_ice_connection_state_change(state).await;
                },
            ));

        self.peer
            .on_peer_connection_state_change(create_event_handler(
                this.clone(),
                async move |this, state| {
                    this.on_peer_connection_state_change(state).await;
                },
            ));

        self.peer
            .on_track(Box::new(move |track, receiver, transceiver| {
                let Some(this) = this.upgrade() else {
                    return Box::pin(ready(())) as Pin<Box<dyn Future<Output = ()> + Send>>;
                };
                Box::pin(async move {
                    tokio::spawn(async move {
                        this.on_track(track, receiver, transceiver).await;
                    });
                }) as Pin<Box<dyn Future<Output = ()> + Send>>
            }));
    }

    async fn send_debug(&self, message: impl Into<String>, ty: Option<LogMessageType>) {
        let mut ipc_sender = self.ipc_sender.lock().await;
        ipc_sender
            .send(MicSidecarIpcMessage::WebSocket(
                MicSidecarServerMessage::DebugLog {
                    message: message.into(),
                    ty,
                },
            ))
            .await;
    }

    async fn send_webrtc(&self, message: StreamSignalingMessage) {
        let mut ipc_sender = self.ipc_sender.lock().await;
        ipc_sender
            .send(MicSidecarIpcMessage::WebSocket(
                MicSidecarServerMessage::WebRtc(message),
            ))
            .await;
    }

    async fn on_ice_candidate(&self, candidate: Option<RTCIceCandidate>) {
        let Some(candidate) = candidate else {
            return;
        };
        let Ok(candidate) = candidate.to_json() else {
            return;
        };

        self.send_webrtc(StreamSignalingMessage::AddIceCandidate(RtcIceCandidate {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_mline_index: candidate.sdp_mline_index,
            username_fragment: candidate.username_fragment,
        }))
        .await;
    }

    async fn on_ice_connection_state_change(&self, state: RTCIceConnectionState) {
        info!("[Mic Sidecar] ICE state: {state:?}");
    }

    fn stop_loopback(&self) {
        if let Ok(mut loopback) = self.loopback.lock() {
            *loopback = None;
        }
    }

    fn set_gain_percent(&self, percent: u8) {
        let percent = percent.min(100);
        self.gain_percent.store(percent, Ordering::Release);
        if let Ok(mut loopback) = self.loopback.lock()
            && let Some(loopback) = loopback.as_mut()
        {
            loopback.set_gain_percent(percent);
        }
        info!("[Mic Sidecar] microphone gain set to {percent}%");
    }

    async fn on_peer_connection_state_change(&self, state: RTCPeerConnectionState) {
        info!("[Mic Sidecar] peer state: {state:?}");
        match state {
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                self.stop_loopback();
                self.send_debug("Mic berhenti. Input PC dikembalikan seperti semula.", None)
                    .await;
            }
            RTCPeerConnectionState::Disconnected => {
                self.send_debug("Mic sedang menyambung ulang.", None).await;
            }
            _ => {}
        }
    }

    async fn on_track(
        self: Arc<Self>,
        track: Arc<TrackRemote>,
        _receiver: Arc<RTCRtpReceiver>,
        _transceiver: Arc<RTCRtpTransceiver>,
    ) {
        self.send_debug("Mic track audio diterima host.", None)
            .await;
        let codec = track.codec();
        let mime = codec.capability.mime_type.to_ascii_lowercase();
        if !mime.contains("opus") {
            self.send_debug(
                format!(
                    "Format suara mic belum didukung: {}",
                    codec.capability.mime_type
                ),
                Some(LogMessageType::InformError),
            )
            .await;
            return;
        }

        let preferred_channels = usize::from(codec.capability.channels.max(1));
        let route_message = {
            let result = {
                match self.loopback.lock() {
                    Ok(mut loopback) => {
                        if loopback.is_none() {
                            match MicrophoneOutput::new(preferred_channels) {
                                Ok(mut next) => {
                                    next.set_gain_percent(
                                        self.gain_percent.load(Ordering::Acquire),
                                    );
                                    let hint = next
                                        .capture_hint()
                                        .unwrap_or_else(|| "virtual microphone host".to_owned());
                                    let default_capture_name = next.default_capture_name();
                                    *loopback = Some(next);
                                    if default_capture_name.is_some() {
                                        Ok(
                                            "Mic aktif. Suara dari perangkat ini otomatis masuk ke PC."
                                                .to_owned(),
                                        )
                                    } else {
                                        Ok(format!(
                                            "Mic aktif. Kalau suara belum terdengar di game/aplikasi, pilih input '{hint}'."
                                        ))
                                    }
                                }
                                Err(error) => {
                                    warn!("[Mic Sidecar] microphone loopback failed: {error}");
                                    Err((user_friendly_microphone_start_error(&error), Some(error)))
                                }
                            }
                        } else {
                            Ok("Mic menerima suara dari perangkat ini.".to_owned())
                        }
                    }
                    Err(_) => Err((
                        "Mic belum bisa aktif di PC host. Coba matikan lalu hidupkan mic lagi."
                            .to_owned(),
                        None,
                    )),
                }
            };

            match result {
                Ok(message) => message,
                Err((message, detail)) => {
                    if let Some(detail) = detail {
                        self.send_debug(format!("MIC_OUTPUT_START_FAILED detail={detail}"), None)
                            .await;
                    }
                    self.send_debug(message, Some(LogMessageType::FatalDescription))
                        .await;
                    return;
                }
            }
        };

        self.send_debug(route_message, None).await;

        loop {
            let packet = match track.read_rtp().await {
                Ok((packet, _)) => packet,
                Err(error) => {
                    self.stop_loopback();
                    warn!("[Mic Sidecar] remote mic track stopped: {error}");
                    self.send_debug("Mic berhenti. Input PC dikembalikan seperti semula.", None)
                        .await;
                    return;
                }
            };

            let render_result = {
                let mut loopback = match self.loopback.lock() {
                    Ok(value) => value,
                    Err(_) => return,
                };
                let Some(loopback) = loopback.as_mut() else {
                    return;
                };
                loopback.render_opus_payload(&packet.payload)
            };

            if let Err(error) = render_result {
                warn!("[Mic Sidecar] failed to render opus payload: {error}");
            }
        }
    }

    async fn flush_pending_remote_ice(&self) {
        loop {
            let Some(candidate) = ({
                let mut pending = self.pending_remote_ice.lock().await;
                pending.pop_front()
            }) else {
                return;
            };

            if let Err(error) = self.peer.add_ice_candidate(candidate.clone()).await {
                let error_text = format!("{error:?}");
                if error_text.contains("ErrNoRemoteDescription") {
                    self.remote_description_ready
                        .store(false, Ordering::Release);
                    let mut pending = self.pending_remote_ice.lock().await;
                    pending.push_front(candidate);
                    return;
                }
                warn!("[Mic Sidecar] failed to add queued ICE candidate: {error_text}");
            }
        }
    }

    async fn handle_webrtc(&self, message: StreamSignalingMessage) {
        match message {
            StreamSignalingMessage::Description(description) => {
                let description = match description.ty {
                    RtcSdpType::Offer => RTCSessionDescription::offer(description.sdp),
                    RtcSdpType::Answer => RTCSessionDescription::answer(description.sdp),
                    RtcSdpType::Pranswer => RTCSessionDescription::pranswer(description.sdp),
                    _ => {
                        self.send_debug(
                            "Negosiasi mic memakai tipe SDP yang belum didukung.",
                            Some(LogMessageType::InformError),
                        )
                        .await;
                        return;
                    }
                };

                let Ok(description) = description else {
                    self.send_debug(
                        "Negosiasi mic mengirim SDP yang tidak valid.",
                        Some(LogMessageType::InformError),
                    )
                    .await;
                    return;
                };

                let remote_ty = description.sdp_type;
                if let Err(error) = self.peer.set_remote_description(description).await {
                    self.send_debug(
                        format!("Negosiasi mic gagal menerima SDP: {error:?}"),
                        Some(LogMessageType::InformError),
                    )
                    .await;
                    return;
                }

                self.remote_description_ready.store(true, Ordering::Release);
                self.flush_pending_remote_ice().await;

                if remote_ty == RTCSdpType::Offer {
                    let answer = match self.peer.create_answer(None).await {
                        Ok(value) => value,
                        Err(error) => {
                            self.send_debug(
                                format!("Negosiasi mic gagal membuat jawaban: {error:?}"),
                                Some(LogMessageType::InformError),
                            )
                            .await;
                            return;
                        }
                    };

                    if let Err(error) = self.peer.set_local_description(answer.clone()).await {
                        self.send_debug(
                            format!("Negosiasi mic gagal menyiapkan jawaban: {error:?}"),
                            Some(LogMessageType::InformError),
                        )
                        .await;
                        return;
                    }

                    self.send_webrtc(StreamSignalingMessage::Description(RtcSessionDescription {
                        ty: from_webrtc_sdp(answer.sdp_type),
                        sdp: answer.sdp,
                    }))
                    .await;
                }
            }
            StreamSignalingMessage::AddIceCandidate(candidate) => {
                let candidate = RTCIceCandidateInit {
                    candidate: candidate.candidate,
                    sdp_mid: candidate.sdp_mid,
                    sdp_mline_index: candidate.sdp_mline_index,
                    username_fragment: candidate.username_fragment,
                };

                if !self.remote_description_ready.load(Ordering::Acquire) {
                    let mut pending = self.pending_remote_ice.lock().await;
                    pending.push_back(candidate);
                    return;
                }

                if let Err(error) = self.peer.add_ice_candidate(candidate.clone()).await {
                    let error_text = format!("{error:?}");
                    if error_text.contains("ErrNoRemoteDescription") {
                        self.remote_description_ready
                            .store(false, Ordering::Release);
                        let mut pending = self.pending_remote_ice.lock().await;
                        pending.push_back(candidate);
                        return;
                    }
                    warn!("[Mic Sidecar] failed to add ICE candidate: {error_text}");
                }
            }
        }
    }

    async fn close(&self) {
        self.stop_loopback();
        let _ = self.peer.close().await;
    }
}

#[tokio::main]
async fn main() {
    init_rustls_crypto_provider();

    if let Some(sink_args) = parse_audio_sink_args() {
        #[cfg(windows)]
        hide_audio_sink_console();
        init_logging(log::LevelFilter::Info);
        match sink_args {
            Ok((sink_port, control_port, preferred_channels)) => {
                if let Err(error) = run_audio_sink(sink_port, control_port, preferred_channels) {
                    eprintln!("[Mic Audio Sink] {error}");
                }
            }
            Err(error) => eprintln!("[Mic Audio Sink] {error}"),
        }
        return;
    }

    let span = span!(Level::TRACE, "mic_sidecar_ipc");
    let (ipc_sender, mut ipc_receiver) = create_process_ipc::<
        MicSidecarServerIpcMessage,
        MicSidecarIpcMessage,
    >(span, stdin(), stdout())
    .await;

    let Some(MicSidecarServerIpcMessage::Init { config }) = ipc_receiver.recv().await else {
        return;
    };
    init_logging(config.log_level);

    let ipc_sender = Arc::new(Mutex::new(ipc_sender));
    {
        let mut sender = ipc_sender.lock().await;
        sender
            .send(MicSidecarIpcMessage::WebSocket(
                MicSidecarServerMessage::Setup {
                    ice_servers: config.webrtc.ice_servers.clone(),
                },
            ))
            .await;
    }

    let peer = match MicPeer::new(&config, ipc_sender.clone()).await {
        Ok(peer) => peer,
        Err(error) => {
            let mut sender = ipc_sender.lock().await;
            sender
                .send(MicSidecarIpcMessage::WebSocket(
                    MicSidecarServerMessage::DebugLog {
                        message: format!("Mic receiver gagal dimulai: {error:#}"),
                        ty: Some(LogMessageType::FatalDescription),
                    },
                ))
                .await;
            return;
        }
    };

    peer.send_debug("Mic receiver siap menerima suara remote.", None)
        .await;

    while let Some(message) = ipc_receiver.recv().await {
        match message {
            MicSidecarServerIpcMessage::Init { .. } => {}
            MicSidecarServerIpcMessage::WebSocket(MicSidecarClientMessage::WebRtc(message)) => {
                peer.handle_webrtc(message).await;
            }
            MicSidecarServerIpcMessage::WebSocket(MicSidecarClientMessage::Heartbeat {
                ..
            }) => {}
            MicSidecarServerIpcMessage::WebSocket(MicSidecarClientMessage::SetGain { percent }) => {
                peer.set_gain_percent(percent);
            }
            MicSidecarServerIpcMessage::WebSocket(MicSidecarClientMessage::Stop)
            | MicSidecarServerIpcMessage::Stop => {
                break;
            }
            MicSidecarServerIpcMessage::WebSocket(MicSidecarClientMessage::Init { .. }) => {}
        }
    }

    peer.close().await;
    {
        let mut sender = ipc_sender.lock().await;
        sender.send(MicSidecarIpcMessage::Stop).await;
    }
    debug!("[Mic Sidecar] stopped");
}
