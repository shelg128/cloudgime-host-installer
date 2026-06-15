#![feature(async_fn_traits)]

use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    env,
    hash::{DefaultHasher, Hash, Hasher},
    io, panic,
    path::PathBuf,
    process::exit,
    ptr::null_mut,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::sync::mpsc;

use common::{
    StreamSettings,
    api_bindings::{
        ClipboardPayload, GeneralClientMessage, GeneralServerMessage, HostMouseEmulationMode,
        LogMessageType, StreamClientMessage, TransportType, VideoFlowPhase,
    },
    ipc::{
        IpcReceiver, IpcSender, ServerIpcMessage, StreamerConfig, StreamerIpcMessage,
        create_process_ipc,
    },
};
use moonlight_common::{
    MoonlightError,
    crypto::openssl::OpenSSLCryptoBackend,
    high::{MoonlightClientError, StreamConfigError, tokio::MoonlightHost},
    http::{
        ClientIdentifier, ClientSecret, ServerIdentifier, client::tokio_hyper::TokioHyperClient,
    },
    stream::{
        AesIv, AesKey, EncryptionFlags, HostFeatures, MoonlightStreamSettings, StreamingConfig,
        audio::{AudioConfig, OpusMultistreamConfig},
        c::{
            MoonlightInstance, MoonlightStream,
            bindings::{ConnectionStatus, Stage},
            connection::ConnectionListenerC,
        },
        connection::ConnectionListener,
        control::{ActiveGamepads, ControllerButtons, MouseButton, MouseButtonAction},
        video::{ColorRange, ColorSpace, SupportedVideoFormats, VideoFormat, VideoSetup},
    },
};
use tokio::{
    io::{stdin, stdout},
    runtime::Handle,
    spawn,
    sync::{Mutex, Notify, RwLock},
    task::spawn_blocking,
    time::{sleep, timeout},
};
use tracing::{Level, level_filters::LevelFilter, span};
use tracing::{debug, error, info, trace, warn};

use common::api_bindings::{StreamCapabilities, StreamServerMessage};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use tracing_log::LogTracer;
use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, mouse_event,
};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HGLOBAL, POINT};

#[cfg(windows)]
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};

#[cfg(windows)]
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

#[cfg(windows)]
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

#[cfg(windows)]
use windows_sys::Win32::System::StationsAndDesktops::{
    DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS, GetThreadDesktop, HDESK,
    OpenInputDesktop, SetThreadDesktop,
};

#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateCursor, GetCursorPos, GetSystemMetrics, OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_HELP,
    OCR_IBEAM, OCR_NO, OCR_NORMAL, OCR_SIZEALL, OCR_SIZENESW, OCR_SIZENS, OCR_SIZENWSE, OCR_SIZEWE,
    OCR_UP, OCR_WAIT, SM_CXSCREEN, SM_CYSCREEN, SPI_SETCURSORS, SetCursorPos, SetSystemCursor,
    ShowCursor, SystemParametersInfoW,
};

use crate::{
    audio::StreamAudioDecoder,
    dynamic_ice_servers::load_dynamic_ice_servers,
    gamepad_sidecar::{GamepadBroker, GamepadProfile},
    transport::{
        InboundPacket, OutboundPacket, TransportError, TransportEvent, TransportEvents,
        TransportSender, web_socket,
        webrtc::{self},
    },
    video::StreamVideoDecoder,
};

pub type RequestClient = TokioHyperClient;

pub const TIMEOUT_DURATION: Duration = Duration::from_secs(60);
const HOST_IDENTITY_TIMEOUT: Duration = Duration::from_secs(8);
const STREAM_CONNECTION_SETUP_TIMEOUT: Duration = Duration::from_secs(8);
const ABSOLUTE_FOLLOW_GAIN_BASE: f32 = 1.04;
const ABSOLUTE_FOLLOW_GAIN_FAST: f32 = 1.12;
const ABSOLUTE_FOLLOW_GAIN_FAST_DISTANCE: f32 = 24.0;
const WINDOWS_XBUTTON1_DATA: i32 = 0x0001;
const WINDOWS_XBUTTON2_DATA: i32 = 0x0002;

#[cfg(windows)]
thread_local! {
    static THREAD_INPUT_DESKTOP_HANDLE: Cell<HDESK> = const { Cell::new(0 as HDESK) };
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct HostInputMoveResult {
    success: bool,
    cursor: Option<(i32, i32)>,
    set_cursor_err: u32,
}

#[cfg(windows)]
enum HostInputCommand {
    MoveAbsolute {
        screen_x: i32,
        screen_y: i32,
        reply: mpsc::SyncSender<HostInputMoveResult>,
    },
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
        reply: mpsc::SyncSender<bool>,
    },
}

#[cfg(windows)]
static HOST_INPUT_SENDER: OnceLock<mpsc::Sender<HostInputCommand>> = OnceLock::new();

const CLIPBOARD_TEXT_MAX_CHARS: usize = 8_192;
const CLIPBOARD_MARKUP_MAX_CHARS: usize = 262_144;
const CLIPBOARD_SYNC_INTERVAL: Duration = Duration::from_millis(100);

fn normalize_clipboard_text(text: &str) -> Option<String> {
    let mut normalized = text.replace('\0', "");
    if normalized.is_empty() {
        return None;
    }

    if normalized.chars().count() > CLIPBOARD_TEXT_MAX_CHARS {
        normalized = normalized.chars().take(CLIPBOARD_TEXT_MAX_CHARS).collect();
    }

    Some(normalized)
}

fn normalize_clipboard_markup(text: &str) -> Option<String> {
    let mut normalized = text.replace('\0', "");
    if normalized.is_empty() {
        return None;
    }

    if normalized.chars().count() > CLIPBOARD_MARKUP_MAX_CHARS {
        normalized = normalized
            .chars()
            .take(CLIPBOARD_MARKUP_MAX_CHARS)
            .collect();
    }

    Some(normalized)
}

fn clipboard_payload_signature(payload: &ClipboardPayload) -> String {
    let mut hasher = DefaultHasher::new();
    payload.hash(&mut hasher);
    format!("{:016X}", hasher.finish())
}

fn normalize_clipboard_payload(
    text: Option<String>,
    html: Option<String>,
    rtf: Option<String>,
) -> Option<ClipboardPayload> {
    let text = text.and_then(|value| normalize_clipboard_text(&value));
    let html = html.and_then(|value| normalize_clipboard_markup(&value));
    let rtf = rtf.and_then(|value| normalize_clipboard_markup(&value));
    if text.is_none() && html.is_none() && rtf.is_none() {
        return None;
    }

    let mut payload = ClipboardPayload {
        signature: String::new(),
        text,
        html,
        rtf,
    };
    payload.signature = clipboard_payload_signature(&payload);
    Some(payload)
}

#[cfg(windows)]
fn registered_clipboard_format_id(name: &str) -> Option<u32> {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if format == 0 { None } else { Some(format) }
}

#[cfg(windows)]
fn html_clipboard_format_id() -> Option<u32> {
    static FORMAT: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT.get_or_init(|| registered_clipboard_format_id("HTML Format"))
}

#[cfg(windows)]
fn rtf_clipboard_format_id() -> Option<u32> {
    static FORMAT: OnceLock<Option<u32>> = OnceLock::new();
    *FORMAT.get_or_init(|| registered_clipboard_format_id("Rich Text Format"))
}

#[cfg(windows)]
fn read_clipboard_format_string(format: u32) -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }

        let handle = GetClipboardData(format);
        if handle.is_null() {
            return None;
        }

        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return None;
        }

        let size = GlobalSize(handle);
        let bytes = std::slice::from_raw_parts(ptr as *const u8, size);
        let end = bytes
            .iter()
            .rposition(|value| *value != 0)
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let decoded = String::from_utf8_lossy(&bytes[..end]).into_owned();
        GlobalUnlock(handle);
        Some(decoded)
    }
}

#[cfg(windows)]
fn read_windows_clipboard_payload() -> anyhow::Result<Option<ClipboardPayload>> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return Ok(None);
        }

        struct ClipboardGuard;
        impl Drop for ClipboardGuard {
            fn drop(&mut self) {
                unsafe {
                    CloseClipboard();
                }
            }
        }
        let _guard = ClipboardGuard;

        let text = if IsClipboardFormatAvailable(CF_UNICODETEXT as u32) != 0 {
            let handle = GetClipboardData(CF_UNICODETEXT as u32);
            if handle.is_null() {
                None
            } else {
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    None
                } else {
                    let size_words = GlobalSize(handle) / std::mem::size_of::<u16>();
                    let wide = std::slice::from_raw_parts(ptr as *const u16, size_words);
                    let len = wide
                        .iter()
                        .position(|value| *value == 0)
                        .unwrap_or(size_words);
                    let text = String::from_utf16_lossy(&wide[..len]);
                    GlobalUnlock(handle);
                    Some(text)
                }
            }
        } else {
            None
        };
        let html = html_clipboard_format_id().and_then(read_clipboard_format_string);
        let rtf = rtf_clipboard_format_id().and_then(read_clipboard_format_string);
        Ok(normalize_clipboard_payload(text, html, rtf))
    }
}

#[cfg(not(windows))]
fn read_windows_clipboard_payload() -> anyhow::Result<Option<ClipboardPayload>> {
    Ok(None)
}

#[cfg(windows)]
fn write_clipboard_utf16_format(format: u32, text: &str) -> anyhow::Result<()> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle.is_null() {
            anyhow::bail!("clipboard memory allocation failed");
        }

        struct GlobalMemoryGuard {
            handle: HGLOBAL,
            transferred: bool,
        }

        impl Drop for GlobalMemoryGuard {
            fn drop(&mut self) {
                if !self.transferred {
                    unsafe {
                        GlobalFree(self.handle);
                    }
                }
            }
        }

        let mut memory = GlobalMemoryGuard {
            handle,
            transferred: false,
        };

        let ptr = GlobalLock(memory.handle) as *mut u16;
        if ptr.is_null() {
            anyhow::bail!("clipboard memory lock failed");
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        GlobalUnlock(memory.handle);

        if SetClipboardData(format, memory.handle).is_null() {
            anyhow::bail!("clipboard data could not be set");
        }
        memory.transferred = true;
    }

    Ok(())
}

#[cfg(windows)]
fn write_clipboard_utf8_format(format: u32, text: &str) -> anyhow::Result<()> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            anyhow::bail!("clipboard memory allocation failed");
        }

        struct GlobalMemoryGuard {
            handle: HGLOBAL,
            transferred: bool,
        }

        impl Drop for GlobalMemoryGuard {
            fn drop(&mut self) {
                if !self.transferred {
                    unsafe {
                        GlobalFree(self.handle);
                    }
                }
            }
        }

        let mut memory = GlobalMemoryGuard {
            handle,
            transferred: false,
        };

        let ptr = GlobalLock(memory.handle) as *mut u8;
        if ptr.is_null() {
            anyhow::bail!("clipboard memory lock failed");
        }

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(memory.handle);

        if SetClipboardData(format, memory.handle).is_null() {
            anyhow::bail!("clipboard data could not be set");
        }
        memory.transferred = true;
    }

    Ok(())
}

#[cfg(windows)]
fn write_windows_clipboard_payload(payload: ClipboardPayload) -> anyhow::Result<()> {
    let Some(payload) = normalize_clipboard_payload(payload.text, payload.html, payload.rtf) else {
        return Ok(());
    };

    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            anyhow::bail!("clipboard is busy");
        }

        struct ClipboardGuard;
        impl Drop for ClipboardGuard {
            fn drop(&mut self) {
                unsafe {
                    CloseClipboard();
                }
            }
        }
        let _guard = ClipboardGuard;

        if EmptyClipboard() == 0 {
            anyhow::bail!("clipboard could not be emptied");
        }

        if let Some(text) = payload.text.as_deref() {
            write_clipboard_utf16_format(CF_UNICODETEXT as u32, text)?;
        }
        if let (Some(format), Some(html)) = (html_clipboard_format_id(), payload.html.as_deref()) {
            write_clipboard_utf8_format(format, html)?;
        }
        if let (Some(format), Some(rtf)) = (rtf_clipboard_format_id(), payload.rtf.as_deref()) {
            write_clipboard_utf8_format(format, rtf)?;
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn write_windows_clipboard_payload(_payload: ClipboardPayload) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn current_windows_clipboard_sequence() -> Option<u32> {
    let value = unsafe { GetClipboardSequenceNumber() };
    if value == 0 { None } else { Some(value) }
}

#[cfg(not(windows))]
fn current_windows_clipboard_sequence() -> Option<u32> {
    None
}

fn legacy_nvenc_compat_enabled() -> bool {
    env::var("ML_LEGACY_NVENC_COMPAT")
        .ok()
        .is_some_and(|value| !matches!(value.trim(), "" | "0" | "false" | "False" | "FALSE"))
}

fn legacy_nvenc_surface_max_fps(width: u32, height: u32) -> u32 {
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

fn legacy_nvenc_surface_max_bitrate(width: u32, height: u32, fps: u32) -> u32 {
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

async fn send_stream_debug_message(
    ipc_sender: &mut common::ipc::IpcSender<StreamerIpcMessage>,
    message: impl Into<String>,
    ty: Option<LogMessageType>,
) {
    ipc_sender
        .send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::DebugLog {
                message: message.into(),
                ty,
            },
        ))
        .await;
}

fn is_transient_start_stream_error(err: &MoonlightClientError) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("10061")
        || text.contains("connection refused")
        || text.contains("timed out")
        || text.contains("connection reset")
        || text.contains("temporarily unavailable")
}

fn is_transient_start_connection_error(err: &MoonlightError) -> bool {
    if matches!(err, MoonlightError::ConnectionFailed) {
        return true;
    }

    let text = err.to_string().to_ascii_lowercase();
    text.contains("10061")
        || text.contains("rtsp handshake")
        || text.contains("connection refused")
        || text.contains("timed out")
        || text.contains("connection reset")
        || text.contains("temporarily unavailable")
}

fn classify_live_clarity_error(err: &MoonlightError) -> String {
    match err {
        MoonlightError::EventSendError(code) if *code > 0 => {
            format!("live_rtsp_status_{code}")
        }
        MoonlightError::EventSendError(code) if *code < 0 => {
            format!("live_rtsp_errno_{}", code.abs())
        }
        MoonlightError::EventSendError(_) => "live_rtsp_transport_unknown".to_string(),
        MoonlightError::NotSupportedOnHost => "live_update_not_supported_on_host".to_string(),
        MoonlightError::ConnectionFailed => "live_update_connection_failed".to_string(),
        _ => "live_rtsp_update_failed".to_string(),
    }
}

mod audio;
mod buffer;
mod convert;
mod dynamic_ice_servers;
mod gamepad_sidecar;
mod transport;
mod video;

#[tokio::main]
async fn main() {
    init_rustls_crypto_provider();
    restore_windows_cursor_defaults();

    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_panic(info);
        exit(0);
    }));

    // At this point we're authenticated
    let span = span!(Level::TRACE, "ipc");
    let (mut ipc_sender, mut ipc_receiver) =
        create_process_ipc::<ServerIpcMessage, StreamerIpcMessage>(span, stdin(), stdout()).await;

    // Send stage
    ipc_sender
        .send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::DebugLog {
                message: "Completed Stage: Launch Streamer".to_string(),
                ty: None,
            },
        ))
        .await;

    let (
        mut config,
        host_address,
        host_http_port,
        client_unique_id,
        client_private_key,
        client_certificate,
        server_certificate,
        app_id,
        video_frame_queue_size,
        audio_sample_queue_size,
    ) = loop {
        match ipc_receiver.recv().await {
            Some(ServerIpcMessage::Init {
                config,
                host_address,
                host_http_port,
                client_unique_id,
                client_private_key,
                client_certificate,
                server_certificate,
                app_id,
                video_frame_queue_size,
                audio_sample_queue_size,
            }) => {
                break (
                    config,
                    host_address,
                    host_http_port,
                    client_unique_id,
                    client_private_key,
                    client_certificate,
                    server_certificate,
                    app_id,
                    video_frame_queue_size,
                    audio_sample_queue_size,
                );
            }
            _ => continue,
        }
    };

    // -- Init logger
    let config_level_filter = match config.log_level {
        log::LevelFilter::Off => LevelFilter::OFF,
        log::LevelFilter::Error => LevelFilter::ERROR,
        log::LevelFilter::Info => LevelFilter::INFO,
        log::LevelFilter::Warn => LevelFilter::WARN,
        log::LevelFilter::Debug => LevelFilter::DEBUG,
        log::LevelFilter::Trace => LevelFilter::TRACE,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(config_level_filter.into())
        .from_env_lossy()
        .add_directive(
            "webrtc_sctp=off"
                .parse()
                .expect("failed to parse webrtc directive"),
        );

    let stderr_output = fmt::layer().with_writer(io::stderr).with_ansi(false);

    let _ = LogTracer::init();

    Registry::default()
        .with(env_filter)
        .with(stderr_output)
        .try_init()
        .ok();

    // Send stage
    ipc_sender
        .send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::DebugLog {
                message: "Waiting for Transport to negotiate".to_string(),
                ty: None,
            },
        ))
        .await;

    // -- Create the host and pair it
    let host = {
        let mut prepared_host = None;
        let mut last_error = String::new();

        for attempt in 1..=2 {
            send_stream_debug_message(
                &mut ipc_sender,
                format!("Preparing paired host session ({attempt}/2)"),
                None,
            )
            .await;

            let host = match MoonlightHost::new(
                host_address.clone(),
                host_http_port,
                client_unique_id.clone(),
            ) {
                Ok(host) => host,
                Err(err) => {
                    last_error = format!("Failed to create host: {err}");
                    if attempt < 2 {
                        send_stream_debug_message(
                            &mut ipc_sender,
                            format!("{last_error}. Retrying host session prepare..."),
                            None,
                        )
                        .await;
                        sleep(Duration::from_millis(1500)).await;
                        continue;
                    }

                    send_stream_debug_message(
                        &mut ipc_sender,
                        last_error.clone(),
                        Some(LogMessageType::FatalDescription),
                    )
                    .await;
                    exit(0);
                }
            };

            send_stream_debug_message(&mut ipc_sender, "Applying pairing identity", None).await;
            match timeout(
                HOST_IDENTITY_TIMEOUT,
                host.set_identity(
                    ClientIdentifier::from_pem(client_certificate.clone()),
                    ClientSecret::from_pem(client_private_key.clone()),
                    ServerIdentifier::from_pem(server_certificate.clone()),
                ),
            )
            .await
            {
                Ok(Ok(())) => {
                    send_stream_debug_message(&mut ipc_sender, "Pairing identity ready", None)
                        .await;
                    prepared_host = Some(host);
                    break;
                }
                Ok(Err(err)) => {
                    last_error = format!("Failed to set pairing info: {err}");
                }
                Err(_) => {
                    last_error = format!(
                        "Timed out while setting pairing info after {}s",
                        HOST_IDENTITY_TIMEOUT.as_secs()
                    );
                }
            }

            if attempt < 2 {
                send_stream_debug_message(
                    &mut ipc_sender,
                    format!("{last_error}. Retrying host session prepare..."),
                    None,
                )
                .await;
                sleep(Duration::from_millis(1500)).await;
                continue;
            }

            send_stream_debug_message(
                &mut ipc_sender,
                last_error,
                Some(LogMessageType::FatalDescription),
            )
            .await;
            exit(0);
        }

        match prepared_host {
            Some(host) => host,
            None => {
                send_stream_debug_message(
                    &mut ipc_sender,
                    "Failed to prepare host session".to_string(),
                    Some(LogMessageType::FatalDescription),
                )
                .await;
                exit(0);
            }
        }
    };

    // -- Configure moonlight
    let moonlight = MoonlightInstance::global().expect("failed to find moonlight");

    // Load dynamic ice servers and append them to the current ice servers
    let dynamic_ice_servers = load_dynamic_ice_servers(&config.webrtc).await;
    config
        .webrtc
        .ice_servers
        .extend_from_slice(&dynamic_ice_servers);

    // -- Create and Configure Peer
    let ice_servers = config.webrtc.ice_servers.clone();

    send_stream_debug_message(&mut ipc_sender, "Preparing stream connection", None).await;
    let connection = match timeout(
        STREAM_CONNECTION_SETUP_TIMEOUT,
        StreamConnection::new(
            moonlight,
            StreamInfo { host, app_id },
            ipc_sender.clone(),
            ipc_receiver,
            config,
            video_frame_queue_size,
            audio_sample_queue_size,
        ),
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(err)) => {
            send_stream_debug_message(
                &mut ipc_sender,
                format!("Failed to create connection: {err}"),
                Some(LogMessageType::FatalDescription),
            )
            .await;
            exit(0);
        }
        Err(_) => {
            send_stream_debug_message(
                &mut ipc_sender,
                format!(
                    "Timed out while preparing stream connection after {}s",
                    STREAM_CONNECTION_SETUP_TIMEOUT.as_secs()
                ),
                Some(LogMessageType::FatalDescription),
            )
            .await;
            exit(0);
        }
    };

    // Send Info for streamer
    send_stream_debug_message(&mut ipc_sender, "Transport negotiation ready", None).await;
    ipc_sender
        .send(StreamerIpcMessage::WebSocket(StreamServerMessage::Setup {
            ice_servers,
        }))
        .await;

    // Wait for termination
    connection.terminate.notified().await;

    // Wait for everything to shutdown (e.g. Moonlight Client, IPC messages)
    sleep(Duration::from_secs(10)).await;

    info!("Terminating Self");
    // Exit streamer
    exit(0);
}

fn init_rustls_crypto_provider() {
    if CryptoProvider::get_default().is_none() {
        let _ = aws_lc_rs::default_provider().install_default();
    }
}

struct StreamInfo {
    host: MoonlightHost<RequestClient>,
    app_id: u32,
}

struct StreamSetup {
    video: Option<VideoSetup>,
    audio: Option<OpusMultistreamConfig>,
}

#[derive(Debug, Clone, Copy)]
struct PendingVideoFlowReady {
    phase: VideoFlowPhase,
    width: u32,
    height: u32,
    fps: u32,
    frames_remaining: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveClarityRequest {
    bitrate: u32,
    adaptive_bitrate: bool,
    adaptive_fps: bool,
}

#[derive(Debug, Clone, Copy)]
struct HostMouseCursorState {
    mode: HostMouseEmulationMode,
    stream_width: u32,
    stream_height: u32,
    normalized_x: f32,
    normalized_y: f32,
    follow_carry_x: f32,
    follow_carry_y: f32,
}

impl Default for HostMouseCursorState {
    fn default() -> Self {
        Self {
            mode: HostMouseEmulationMode::RelativeNative,
            stream_width: 4096,
            stream_height: 4096,
            normalized_x: 0.5,
            normalized_y: 0.5,
            follow_carry_x: 0.0,
            follow_carry_y: 0.0,
        }
    }
}

impl HostMouseCursorState {
    fn set_mode(&mut self, mode: HostMouseEmulationMode, width: u32, height: u32) {
        self.mode = mode;
        self.stream_width = width.max(1);
        self.stream_height = height.max(1);
        if !self.normalized_x.is_finite() || !self.normalized_y.is_finite() {
            self.normalized_x = 0.5;
            self.normalized_y = 0.5;
        }
        if !self.follow_carry_x.is_finite() || !self.follow_carry_y.is_finite() {
            self.follow_carry_x = 0.0;
            self.follow_carry_y = 0.0;
        }
    }

    fn set_stream_size(&mut self, width: u32, height: u32) {
        self.stream_width = width.max(1);
        self.stream_height = height.max(1);
    }

    fn observe_absolute(&mut self, x: i16, y: i16, reference_width: i16, reference_height: i16) {
        let ref_width = i32::from(reference_width).max(1) as f32;
        let ref_height = i32::from(reference_height).max(1) as f32;
        let width_denominator = (ref_width - 1.0).max(1.0);
        let height_denominator = (ref_height - 1.0).max(1.0);
        self.normalized_x =
            (f32::from(x).clamp(0.0, ref_width - 1.0) / width_denominator).clamp(0.0, 1.0);
        self.normalized_y =
            (f32::from(y).clamp(0.0, ref_height - 1.0) / height_denominator).clamp(0.0, 1.0);
    }

    fn apply_relative_as_absolute(&mut self, delta_x: i16, delta_y: i16) -> (i16, i16, i16, i16) {
        let width = self.stream_width.max(2) as f32;
        let height = self.stream_height.max(2) as f32;
        let max_x = (width - 1.0).max(1.0);
        let max_y = (height - 1.0).max(1.0);
        let next_x = (self.normalized_x * max_x + f32::from(delta_x)).clamp(0.0, max_x);
        let next_y = (self.normalized_y * max_y + f32::from(delta_y)).clamp(0.0, max_y);
        self.normalized_x = (next_x / max_x).clamp(0.0, 1.0);
        self.normalized_y = (next_y / max_y).clamp(0.0, 1.0);
        (
            next_x.round() as i16,
            next_y.round() as i16,
            width as i16,
            height as i16,
        )
    }

    fn shape_follow_delta(&mut self, delta_x: i16, delta_y: i16) -> (i16, i16) {
        let distance = ((delta_x as f32).powi(2) + (delta_y as f32).powi(2)).sqrt();
        let gain_ramp = (distance / ABSOLUTE_FOLLOW_GAIN_FAST_DISTANCE).clamp(0.0, 1.0);
        let gain = ABSOLUTE_FOLLOW_GAIN_BASE
            + ((ABSOLUTE_FOLLOW_GAIN_FAST - ABSOLUTE_FOLLOW_GAIN_BASE) * gain_ramp);

        let scaled_x = (delta_x as f32) * gain + self.follow_carry_x;
        let scaled_y = (delta_y as f32) * gain + self.follow_carry_y;
        let send_x = scaled_x.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        let send_y = scaled_y.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;

        self.follow_carry_x = (scaled_x - send_x as f32).clamp(-0.99, 0.99);
        self.follow_carry_y = (scaled_y - send_y as f32).clamp(-0.99, 0.99);

        (send_x, send_y)
    }

    fn current_absolute(&self) -> (i16, i16, i16, i16) {
        let width = self.stream_width.max(2) as f32;
        let height = self.stream_height.max(2) as f32;
        let max_x = (width - 1.0).max(1.0);
        let max_y = (height - 1.0).max(1.0);
        (
            (self.normalized_x.clamp(0.0, 1.0) * max_x).round() as i16,
            (self.normalized_y.clamp(0.0, 1.0) * max_y).round() as i16,
            width as i16,
            height as i16,
        )
    }
}

#[derive(Debug, Default)]
struct HostCursorVisibilityState {
    hidden_for_stream: bool,
    blank_system_cursors: bool,
}

#[cfg(windows)]
fn set_windows_cursor_hidden(hidden: bool) {
    // ShowCursor uses a process-global display count. Drive it until the intended sign is reached.
    unsafe {
        if hidden {
            while ShowCursor(0) >= 0 {}
        } else {
            while ShowCursor(1) < 0 {}
        }
    }
}

#[cfg(not(windows))]
fn set_windows_cursor_hidden(_hidden: bool) {}

#[cfg(windows)]
fn set_windows_system_cursors_blank(blank: bool) {
    unsafe {
        if blank {
            const CURSOR_DIM: usize = 32;
            const CURSOR_MASK_BYTES: usize = (CURSOR_DIM * CURSOR_DIM) / 8;
            const CURSOR_IDS: [u32; 14] = [
                OCR_NORMAL,
                OCR_IBEAM,
                OCR_WAIT,
                OCR_CROSS,
                OCR_UP,
                OCR_SIZENWSE,
                OCR_SIZENESW,
                OCR_SIZEWE,
                OCR_SIZENS,
                OCR_SIZEALL,
                OCR_NO,
                OCR_HAND,
                OCR_APPSTARTING,
                OCR_HELP,
            ];

            let and_mask = [0xFFu8; CURSOR_MASK_BYTES];
            let xor_mask = [0x00u8; CURSOR_MASK_BYTES];
            for cursor_id in CURSOR_IDS {
                let cursor = CreateCursor(
                    null_mut(),
                    0,
                    0,
                    CURSOR_DIM as i32,
                    CURSOR_DIM as i32,
                    and_mask.as_ptr().cast(),
                    xor_mask.as_ptr().cast(),
                );
                if !cursor.is_null() {
                    let _ = SetSystemCursor(cursor, cursor_id);
                }
            }
        } else {
            let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
        }
    }
}

#[cfg(not(windows))]
fn set_windows_system_cursors_blank(_blank: bool) {}

#[cfg(windows)]
fn restore_windows_cursor_defaults() {
    set_windows_system_cursors_blank(false);
    set_windows_cursor_hidden(false);
}

#[cfg(not(windows))]
fn restore_windows_cursor_defaults() {}

#[cfg(windows)]
fn sync_windows_host_cursor_absolute(
    absolute_x: i16,
    absolute_y: i16,
    reference_width: i16,
    reference_height: i16,
) -> bool {
    if !ensure_current_thread_input_desktop() {
        return false;
    }

    let reference_width = i32::from(reference_width).max(1);
    let reference_height = i32::from(reference_height).max(1);
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);

    let max_input_x = (reference_width - 1).max(1) as f32;
    let max_input_y = (reference_height - 1).max(1) as f32;
    let max_screen_x = (screen_width - 1).max(1) as f32;
    let max_screen_y = (screen_height - 1).max(1) as f32;

    let normalized_x =
        (f32::from(absolute_x).clamp(0.0, max_input_x) / max_input_x).clamp(0.0, 1.0);
    let normalized_y =
        (f32::from(absolute_y).clamp(0.0, max_input_y) / max_input_y).clamp(0.0, 1.0);

    let screen_x = (normalized_x * max_screen_x)
        .round()
        .clamp(0.0, max_screen_x) as u32;
    let screen_y = (normalized_y * max_screen_y)
        .round()
        .clamp(0.0, max_screen_y) as u32;
    let Some(sender) = host_input_sender() else {
        return false;
    };

    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    if sender
        .send(HostInputCommand::MoveAbsolute {
            screen_x: screen_x as i32,
            screen_y: screen_y as i32,
            reply: reply_tx,
        })
        .is_err()
    {
        return false;
    }

    match reply_rx.recv_timeout(Duration::from_millis(250)) {
        Ok(HostInputMoveResult { success: true, .. }) => true,
        Ok(HostInputMoveResult {
            success: false,
            cursor: Some((cursor_x, cursor_y)),
            set_cursor_err,
        }) => {
            warn!(
                "[Stream]: cursor sync fallback missed target target={}x{} actual={}x{} set_cursor_err={}",
                screen_x, screen_y, cursor_x, cursor_y, set_cursor_err
            );
            false
        }
        Ok(HostInputMoveResult {
            success: false,
            cursor: None,
            set_cursor_err,
        }) => {
            warn!(
                "[Stream]: cursor sync fallback unknown cursor target={}x{} set_cursor_err={}",
                screen_x, screen_y, set_cursor_err
            );
            false
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
fn host_input_sender() -> Option<&'static mpsc::Sender<HostInputCommand>> {
    Some(HOST_INPUT_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("cloudgime-host-input".to_string())
            .spawn(move || host_input_worker_loop(rx))
            .expect("failed to spawn host input worker");
        tx
    }))
}

#[cfg(windows)]
fn host_input_worker_loop(rx: mpsc::Receiver<HostInputCommand>) {
    for command in rx {
        match command {
            HostInputCommand::MoveAbsolute {
                screen_x,
                screen_y,
                reply,
            } => {
                let _ = reply.send(move_windows_host_cursor_absolute_worker(screen_x, screen_y));
            }
            HostInputCommand::MouseButton {
                action,
                button,
                reply,
            } => {
                let _ = reply.send(inject_windows_host_mouse_button_worker(action, button));
            }
        }
    }
}

#[cfg(windows)]
fn move_windows_host_cursor_absolute_worker(screen_x: i32, screen_y: i32) -> HostInputMoveResult {
    if !ensure_current_thread_input_desktop() {
        return HostInputMoveResult {
            success: false,
            cursor: current_windows_cursor_position(),
            set_cursor_err: unsafe { GetLastError() },
        };
    }

    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
    let target_x = screen_x.clamp(0, screen_width - 1);
    let target_y = screen_y.clamp(0, screen_height - 1);

    let set_cursor_ok = unsafe { SetCursorPos(target_x, target_y) != 0 };
    let set_cursor_err = if set_cursor_ok {
        0
    } else {
        unsafe { GetLastError() }
    };

    if !set_cursor_ok {
        let max_screen_x = (screen_width - 1).max(1) as f32;
        let max_screen_y = (screen_height - 1).max(1) as f32;
        let absolute_mouse_x = ((target_x as f32 / max_screen_x) * 65535.0)
            .round()
            .clamp(0.0, 65535.0) as i32;
        let absolute_mouse_y = ((target_y as f32 / max_screen_y) * 65535.0)
            .round()
            .clamp(0.0, 65535.0) as i32;
        unsafe {
            mouse_event(
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                absolute_mouse_x,
                absolute_mouse_y,
                0,
                0,
            );
        }
    }

    let cursor = current_windows_cursor_position();
    let success = matches!(
        cursor,
        Some((cursor_x, cursor_y))
            if (cursor_x - target_x).abs() <= 1 && (cursor_y - target_y).abs() <= 1
    );

    HostInputMoveResult {
        success,
        cursor,
        set_cursor_err,
    }
}

#[cfg(windows)]
fn ensure_current_thread_input_desktop() -> bool {
    THREAD_INPUT_DESKTOP_HANDLE.with(|desktop_handle: &Cell<HDESK>| unsafe {
        if !desktop_handle.get().is_null() {
            return true;
        }

        let input_desktop = OpenInputDesktop(
            0,
            0,
            DESKTOP_READOBJECTS | DESKTOP_WRITEOBJECTS | DESKTOP_SWITCHDESKTOP,
        );
        if input_desktop.is_null() {
            let err = GetLastError();
            warn!("[Stream]: OpenInputDesktop failed: {err}");
            return false;
        }

        let current_desktop = GetThreadDesktop(GetCurrentThreadId());
        if current_desktop != input_desktop && SetThreadDesktop(input_desktop) == 0 {
            let err = GetLastError();
            warn!("[Stream]: SetThreadDesktop failed: {err}");
            return false;
        }

        desktop_handle.set(input_desktop);
        true
    })
}

#[cfg(windows)]
fn current_windows_cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    if ok == 0 {
        None
    } else {
        Some((point.x, point.y))
    }
}

#[cfg(windows)]
fn inject_windows_host_mouse_button(action: MouseButtonAction, button: MouseButton) -> bool {
    let Some(sender) = host_input_sender() else {
        return false;
    };
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    if sender
        .send(HostInputCommand::MouseButton {
            action,
            button,
            reply: reply_tx,
        })
        .is_err()
    {
        return false;
    }

    reply_rx
        .recv_timeout(Duration::from_millis(250))
        .unwrap_or(false)
}

#[cfg(windows)]
fn inject_windows_host_mouse_button_worker(action: MouseButtonAction, button: MouseButton) -> bool {
    if !ensure_current_thread_input_desktop() {
        return false;
    }

    let (flags, data) = match (action, button) {
        (MouseButtonAction::Press, MouseButton::Left) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButtonAction::Release, MouseButton::Left) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButtonAction::Press, MouseButton::Right) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButtonAction::Release, MouseButton::Right) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButtonAction::Press, MouseButton::Middle) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButtonAction::Release, MouseButton::Middle) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButtonAction::Press, MouseButton::X1) => (MOUSEEVENTF_XDOWN, WINDOWS_XBUTTON1_DATA),
        (MouseButtonAction::Release, MouseButton::X1) => (MOUSEEVENTF_XUP, WINDOWS_XBUTTON1_DATA),
        (MouseButtonAction::Press, MouseButton::X2) => (MOUSEEVENTF_XDOWN, WINDOWS_XBUTTON2_DATA),
        (MouseButtonAction::Release, MouseButton::X2) => (MOUSEEVENTF_XUP, WINDOWS_XBUTTON2_DATA),
    };
    unsafe {
        mouse_event(flags, 0, 0, data, 0);
    }
    true
}

#[cfg(not(windows))]
fn sync_windows_host_cursor_absolute(
    _absolute_x: i16,
    _absolute_y: i16,
    _reference_width: i16,
    _reference_height: i16,
) -> bool {
    false
}

#[cfg(not(windows))]
fn current_windows_cursor_position() -> Option<(i32, i32)> {
    None
}

#[cfg(not(windows))]
fn inject_windows_host_mouse_button(_action: MouseButtonAction, _button: MouseButton) -> bool {
    false
}

fn pending_message_kind(message: &ServerIpcMessage) -> &'static str {
    match message {
        ServerIpcMessage::Init { .. } => "init",
        ServerIpcMessage::WebSocket(StreamClientMessage::SetTransport(_)) => "set_transport",
        ServerIpcMessage::WebSocket(StreamClientMessage::WebRtc(_)) => "webrtc_signaling",
        ServerIpcMessage::WebSocket(StreamClientMessage::StartStream { .. }) => "start_stream",
        ServerIpcMessage::WebSocket(StreamClientMessage::ResizeStream { .. }) => "resize_stream",
        ServerIpcMessage::WebSocket(StreamClientMessage::UpdateClarity { .. }) => "update_clarity",
        ServerIpcMessage::WebSocket(StreamClientMessage::SetHostMouseEmulation { .. }) => {
            "set_host_mouse_emulation"
        }
        ServerIpcMessage::WebSocket(StreamClientMessage::Heartbeat { .. }) => "heartbeat",
        ServerIpcMessage::WebSocket(StreamClientMessage::RouteTelemetry { .. }) => {
            "route_telemetry"
        }
        ServerIpcMessage::WebSocket(StreamClientMessage::ProjectDisplay { .. }) => {
            "project_display"
        }
        ServerIpcMessage::WebSocket(StreamClientMessage::Init { .. }) => "stream_init",
        ServerIpcMessage::WebSocketTransport(_) => "websocket_transport",
        ServerIpcMessage::Stop => "stop",
    }
}

struct StreamConnection {
    pub runtime: Handle,
    pub moonlight: MoonlightInstance,
    pub config: StreamerConfig,
    pub info: StreamInfo,
    pub ipc_sender: IpcSender<StreamerIpcMessage>,
    // Video
    pub video_frame_queue_size: usize,
    pub audio_sample_queue_size: usize,
    pub stream_setup: Mutex<StreamSetup>,
    // Stream
    pub stream: RwLock<Option<MoonlightStream>>,
    active_stream_settings: Mutex<Option<StreamSettings>>,
    active_stream_generation: AtomicU32,
    pub active_gamepads: RwLock<ActiveGamepads>,
    controller_id_map: Mutex<HashMap<u8, u8>>,
    gamepad_broker_path: Option<PathBuf>,
    gamepad_broker: Mutex<Option<Arc<GamepadBroker>>>,
    pub transport_sender: Mutex<Option<Box<dyn TransportSender + Send + Sync + 'static>>>,
    pending_pre_transport_messages: Mutex<VecDeque<ServerIpcMessage>>,
    mouse_cursor_state: Mutex<HostMouseCursorState>,
    host_cursor_visibility: Mutex<HostCursorVisibilityState>,
    pending_video_flow_ready: Mutex<Option<PendingVideoFlowReady>>,
    clipboard_last_payload: Mutex<Option<ClipboardPayload>>,
    clipboard_last_sequence: AtomicU32,
    live_clarity_update_lock: Mutex<()>,
    preserve_client_during_stream_replace_generation: AtomicU32,
    mouse_position_packets: AtomicU32,
    mouse_move_packets: AtomicU32,
    mouse_input_errors: AtomicU32,
    mouse_button_states: Mutex<[bool; 5]>,
    // Timeout / Terminate
    pub timeout_terminate_request: Mutex<Option<Instant>>,
    pub terminate: Notify,
    is_terminating: AtomicBool,
}

impl StreamConnection {
    async fn ensure_host_cursor_restored_if_idle(&self, reason: &str) {
        let has_active_stream = self.stream.read().await.is_some();
        let has_transport = self.transport_sender.lock().await.is_some();

        if has_active_stream || has_transport {
            return;
        }

        self.set_host_cursor_hidden_for_stream(false, reason).await;
    }

    async fn set_host_cursor_hidden_for_stream(&self, hidden: bool, reason: &str) {
        let mut visibility = self.host_cursor_visibility.lock().await;

        // When hiding: always re-apply even if state says already hidden.
        // Windows can silently restore system cursors (e.g. fullscreen transitions,
        // UAC prompts, DPI changes) causing the host cursor to reappear mid-stream.
        // Unconditional re-apply ensures blanking stays enforced.
        if !hidden
            && visibility.hidden_for_stream == false
            && visibility.blank_system_cursors == false
        {
            return; // already visible, nothing to do
        }

        if hidden {
            set_windows_system_cursors_blank(true);
            set_windows_cursor_hidden(true);
        } else {
            restore_windows_cursor_defaults();
        }
        let changed = visibility.hidden_for_stream != hidden;
        visibility.hidden_for_stream = hidden;
        visibility.blank_system_cursors = hidden;
        if changed {
            info!(
                "[Stream]: host cursor visibility set to {} for stream ({reason})",
                if hidden { "hidden" } else { "visible" }
            );
        } else {
            debug!("[Stream]: host cursor blanking re-enforced while streaming ({reason})");
        }
    }

    /// Periodically re-applies cursor blanking while a stream is active.
    /// Runs every 1 second. Windows may silently restore system cursors during
    /// fullscreen transitions, UAC prompts, or DPI changes — this watchdog
    /// ensures the host cursor stays invisible for the entire stream session.
    async fn enforce_cursor_hidden_while_streaming(&self) {
        let has_active_stream = self.stream.read().await.is_some();
        if has_active_stream {
            self.set_host_cursor_hidden_for_stream(true, "cursor_enforcer")
                .await;
        }
    }

    pub async fn new(
        moonlight: MoonlightInstance,
        info: StreamInfo,
        ipc_sender: IpcSender<StreamerIpcMessage>,
        mut ipc_receiver: IpcReceiver<ServerIpcMessage>,
        config: StreamerConfig,
        video_frame_queue_size: usize,
        audio_sample_queue_size: usize,
    ) -> Result<Arc<Self>, anyhow::Error> {
        let effective_video_frame_queue_size = video_frame_queue_size.max(1);

        if effective_video_frame_queue_size != video_frame_queue_size {
            info!(
                "[Stream]: clamping legacy NVENC video frame queue {} -> {}",
                video_frame_queue_size, effective_video_frame_queue_size
            );
        }

        restore_windows_cursor_defaults();

        let this = Arc::new(Self {
            runtime: Handle::current(),
            moonlight,
            config,
            info,
            ipc_sender,
            stream_setup: Mutex::new(StreamSetup {
                video: None,
                audio: None,
            }),
            video_frame_queue_size: effective_video_frame_queue_size,
            audio_sample_queue_size,
            stream: RwLock::new(None),
            active_stream_settings: Mutex::new(None),
            active_stream_generation: AtomicU32::new(0),
            active_gamepads: RwLock::new(ActiveGamepads::empty()),
            controller_id_map: Mutex::new(HashMap::new()),
            gamepad_broker_path: env::var_os("ML_GAMEPAD_SIDECAR_PATH")
                .map(PathBuf::from)
                .filter(|path| path.exists()),
            gamepad_broker: Mutex::new(None),
            transport_sender: Mutex::new(None),
            pending_pre_transport_messages: Mutex::new(VecDeque::new()),
            mouse_cursor_state: Mutex::new(HostMouseCursorState::default()),
            host_cursor_visibility: Mutex::new(HostCursorVisibilityState::default()),
            pending_video_flow_ready: Mutex::new(None),
            clipboard_last_payload: Mutex::new(None),
            clipboard_last_sequence: AtomicU32::new(0),
            live_clarity_update_lock: Mutex::new(()),
            preserve_client_during_stream_replace_generation: AtomicU32::new(0),
            mouse_position_packets: AtomicU32::new(0),
            mouse_move_packets: AtomicU32::new(0),
            mouse_input_errors: AtomicU32::new(0),
            mouse_button_states: Mutex::new([false; 5]),
            timeout_terminate_request: Default::default(),
            terminate: Notify::default(),
            is_terminating: AtomicBool::new(false),
        });

        spawn({
            let this = Arc::downgrade(&this);

            async move {
                while let Some(message) = ipc_receiver.recv().await {
                    let Some(this) = this.upgrade() else {
                        debug!("Received ipc message while the main type is already deallocated");
                        return;
                    };

                    if let ServerIpcMessage::Stop = &message {
                        this.on_ipc_message(ServerIpcMessage::Stop).await;
                        return;
                    }

                    this.on_ipc_message(message).await;
                }
            }
        });

        // Watchdog: restore cursor if session goes idle
        spawn({
            let this = Arc::downgrade(&this);

            async move {
                loop {
                    sleep(Duration::from_secs(5)).await;

                    let Some(this) = this.upgrade() else {
                        return;
                    };

                    this.ensure_host_cursor_restored_if_idle("watchdog").await;
                }
            }
        });

        // Cursor enforcer: aggressively re-apply cursor blanking every 1 second
        // while a stream is active to counteract Windows silently restoring cursors.
        spawn({
            let this = Arc::downgrade(&this);

            async move {
                loop {
                    sleep(Duration::from_secs(1)).await;

                    let Some(this) = this.upgrade() else {
                        return;
                    };

                    this.enforce_cursor_hidden_while_streaming().await;
                }
            }
        });

        spawn({
            let this = Arc::downgrade(&this);

            async move {
                loop {
                    sleep(CLIPBOARD_SYNC_INTERVAL).await;

                    let Some(this) = this.upgrade() else {
                        return;
                    };

                    this.sync_host_clipboard_to_client().await;
                }
            }
        });

        Ok(this)
    }

    async fn set_transport(
        self: &Arc<Self>,
        new_sender: Box<dyn TransportSender + Send + Sync + 'static>,
        mut events: Box<dyn TransportEvents + Send + Sync + 'static>,
    ) {
        let this = self.clone();

        let old_transport = {
            let mut sender = this.transport_sender.lock().await;
            sender.replace(new_sender)
        };

        spawn({
            let mut ipc_sender = this.ipc_sender.clone();
            let this = Arc::downgrade(&this);

            async move {
                loop {
                    trace!("Polling new transport event");
                    let event = events.poll_event().await;
                    trace!("Polled transport event: {event:?}");

                    match event {
                        Ok(TransportEvent::SendIpc(message)) => {
                            ipc_sender.send(message).await;
                        }
                        Ok(TransportEvent::StartStream { settings }) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            let this = this.clone();
                            spawn(async move {
                                this.clear_terminate_request().await;

                                if let Err(err) = this.start_stream(settings).await {
                                    error!("Failed to start stream, stopping: {err}");

                                    this.stop().await;
                                }
                            });
                        }
                        Ok(TransportEvent::ResizeStream { width, height, fps }) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            let this = this.clone();
                            spawn(async move {
                                if let Err(err) = this.resize_stream(width, height, fps).await {
                                    warn!(
                                        "Failed to apply runtime stream resize to {}x{}@{}: {err}",
                                        width, height, fps
                                    );
                                }
                            });
                        }
                        Ok(TransportEvent::UpdateClarity {
                            bitrate,
                            adaptive_bitrate,
                            adaptive_fps,
                            allow_restart_fallback,
                        }) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            let this = this.clone();
                            spawn(async move {
                                this.update_clarity(
                                    bitrate,
                                    adaptive_bitrate,
                                    adaptive_fps,
                                    allow_restart_fallback,
                                )
                                .await;
                            });
                        }
                        Ok(TransportEvent::SetHostMouseEmulation { mode }) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            let this = this.clone();
                            spawn(async move {
                                let mut mouse_state = this.mouse_cursor_state.lock().await;
                                let width = mouse_state.stream_width;
                                let height = mouse_state.stream_height;
                                mouse_state.set_mode(mode, width, height);
                                drop(mouse_state);
                                info!(
                                    "[Stream]: switched host mouse emulation to {:?} without reconnect",
                                    mode
                                );
                            });
                        }
                        Ok(TransportEvent::RecvPacket(packet)) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            this.on_packet(packet).await;
                        }
                        Err(TransportError::Closed) | Ok(TransportEvent::Closed) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed request session termination because of missing stream (maybe it was already terminated)"
                                );
                                return;
                            };

                            this.request_terminate().await;

                            break;
                        }
                        // It wouldn't make sense to return this
                        Err(TransportError::ChannelClosed) => unreachable!(),
                        Err(TransportError::Implementation(err)) => {
                            let Some(this) = this.upgrade() else {
                                warn!(
                                    "Failed to get stream connection, stopping listening to events"
                                );
                                return;
                            };

                            info!(
                                "Stopping stream because of transport implementation error: {err}"
                            );

                            this.stop().await;
                            break;
                        }
                    }
                }
            }
        });

        if let Some(old_transport) = old_transport {
            spawn(async move {
                if let Err(err) = old_transport.close().await {
                    warn!("Failed to close old transport: {err:?}");
                }
            });
        }
    }

    async fn map_inbound_controller_id(&self, inbound_id: u8) -> u8 {
        if !legacy_nvenc_compat_enabled() || inbound_id < 4 {
            return inbound_id;
        }

        let mut controller_id_map = self.controller_id_map.lock().await;
        if let Some(mapped_id) = controller_id_map.get(&inbound_id).copied() {
            return mapped_id;
        }

        let active_gamepads = self.active_gamepads.read().await;
        let mapped_id = (0u8..4)
            .find(|candidate| {
                let Some(gamepad) = ActiveGamepads::from_id(*candidate) else {
                    return false;
                };
                !active_gamepads.contains(gamepad)
                    && !controller_id_map
                        .values()
                        .any(|mapped| *mapped == *candidate)
            })
            .unwrap_or(0);

        controller_id_map.insert(inbound_id, mapped_id);
        info!(
            "[Stream]: remapped legacy controller slot {} -> {}",
            inbound_id, mapped_id
        );
        mapped_id
    }

    async fn resolve_inbound_controller_id(&self, inbound_id: u8) -> u8 {
        if !legacy_nvenc_compat_enabled() || inbound_id < 4 {
            return inbound_id;
        }

        self.controller_id_map
            .lock()
            .await
            .get(&inbound_id)
            .copied()
            .unwrap_or(inbound_id)
    }

    async fn release_inbound_controller_id(&self, inbound_id: u8) -> u8 {
        if !legacy_nvenc_compat_enabled() || inbound_id < 4 {
            return inbound_id;
        }

        self.controller_id_map
            .lock()
            .await
            .remove(&inbound_id)
            .unwrap_or(inbound_id)
    }

    async fn ensure_gamepad_broker(&self) -> Option<Arc<GamepadBroker>> {
        let Some(path) = self.gamepad_broker_path.as_ref() else {
            return None;
        };

        let mut broker_guard = self.gamepad_broker.lock().await;
        if let Some(broker) = broker_guard.as_ref() {
            return Some(broker.clone());
        }

        match GamepadBroker::spawn(path).await {
            Ok(broker) => {
                let broker = Arc::new(broker);
                broker_guard.replace(broker.clone());
                Some(broker)
            }
            Err(err) => {
                warn!(
                    "[Stream]: failed to spawn host gamepad broker {}: {err}",
                    path.display()
                );
                None
            }
        }
    }

    async fn shutdown_gamepad_broker(&self) {
        let broker = { self.gamepad_broker.lock().await.take() };
        if let Some(broker) = broker {
            broker.shutdown().await;
        }
    }

    fn mouse_button_index(button: MouseButton) -> usize {
        match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::X1 => 3,
            MouseButton::X2 => 4,
        }
    }
    async fn try_send_packet(&self, packet: OutboundPacket, packet_ty: &str, should_warn: bool) {
        let mut sender = self.transport_sender.lock().await;

        if let Some(sender) = sender.as_mut() {
            if let Err(err) = sender.send(packet).await {
                if should_warn {
                    warn!("Failed to send outbound packet: {packet_ty}, {err:?}");
                } else {
                    debug!("Failed to send outbound packet: {packet_ty}, {err:?}");
                }
            }
        } else {
            debug!("Dropping packet {packet:?} because no transport is selected!");
        }
    }

    fn should_log_mouse_probe(count: u32) -> bool {
        count <= 5 || count % 200 == 0
    }

    async fn send_debug_log_message(&self, message: String) {
        let mut ipc_sender = self.ipc_sender.clone();
        ipc_sender
            .send(StreamerIpcMessage::WebSocket(
                StreamServerMessage::DebugLog { message, ty: None },
            ))
            .await;
    }

    async fn sync_host_clipboard_to_client(&self) {
        if self.stream.read().await.is_none() || self.transport_sender.lock().await.is_none() {
            return;
        }

        let sequence = current_windows_clipboard_sequence().unwrap_or(0);
        if sequence != 0 {
            let last_sequence = self.clipboard_last_sequence.load(Ordering::Relaxed);
            if last_sequence == sequence {
                return;
            }
        }

        let payload = match spawn_blocking(read_windows_clipboard_payload).await {
            Ok(Ok(Some(payload))) => payload,
            Ok(Ok(None)) => return,
            Ok(Err(error)) => {
                debug!("[Clipboard]: failed to read host clipboard: {error:#}");
                return;
            }
            Err(error) => {
                debug!("[Clipboard]: host clipboard task failed: {error:#}");
                return;
            }
        };

        {
            let mut last_payload = self.clipboard_last_payload.lock().await;
            if last_payload.as_ref() == Some(&payload) {
                if sequence != 0 {
                    self.clipboard_last_sequence
                        .store(sequence, Ordering::Relaxed);
                }
                return;
            }

            *last_payload = Some(payload.clone());
        }
        if sequence != 0 {
            self.clipboard_last_sequence
                .store(sequence, Ordering::Relaxed);
        }

        self.try_send_packet(
            OutboundPacket::General {
                message: GeneralServerMessage::ClipboardData {
                    payload: payload.clone(),
                },
            },
            "clipboard_data",
            false,
        )
        .await;
    }

    async fn apply_client_clipboard_payload(&self, payload: ClipboardPayload) {
        let Some(payload) = normalize_clipboard_payload(payload.text, payload.html, payload.rtf)
        else {
            return;
        };

        {
            let mut last_payload = self.clipboard_last_payload.lock().await;
            if last_payload.as_ref() == Some(&payload) {
                return;
            }

            *last_payload = Some(payload.clone());
        }

        let result = spawn_blocking(move || write_windows_clipboard_payload(payload)).await;
        match result {
            Ok(Ok(())) => {
                if let Some(sequence) = current_windows_clipboard_sequence() {
                    self.clipboard_last_sequence
                        .store(sequence, Ordering::Relaxed);
                }
                debug!("[Clipboard]: client clipboard payload applied to host.");
            }
            Ok(Err(error)) => debug!("[Clipboard]: failed to apply client clipboard: {error:#}"),
            Err(error) => debug!("[Clipboard]: client clipboard task failed: {error:#}"),
        }
    }

    async fn on_general_message(&self, message: GeneralClientMessage) {
        debug!("General message: {message:?}");

        match message {
            GeneralClientMessage::Stop => {
                debug!("Received stop from client. Stopping stream now!");
                self.stop().await;
            }
            GeneralClientMessage::RequestIdr => {
                debug!("Received RequestIdr from client. Requesting IDR now.");
                self.moonlight.request_idr_frame();
            }
            GeneralClientMessage::ClipboardData { payload } => {
                self.apply_client_clipboard_payload(payload).await;
            }
            GeneralClientMessage::ClipboardText { text } => {
                self.apply_client_clipboard_payload(ClipboardPayload {
                    signature: String::new(),
                    text: Some(text),
                    html: None,
                    rtf: None,
                })
                .await;
            }
        }
    }

    async fn on_packet(&self, packet: InboundPacket) {
        if let InboundPacket::General { message } = packet {
            self.on_general_message(message).await;
            return;
        }

        let stream_lock = self.stream.read().await;
        let Some(stream) = stream_lock.as_ref() else {
            warn!("Failed to send packet {packet:?} because of missing stream");
            return;
        };

        let mut probe_message: Option<String> = None;
        let err = match packet {
            InboundPacket::MousePosition {
                x,
                y,
                reference_width,
                reference_height,
            } => {
                let mode = {
                    let mut mouse_state = self.mouse_cursor_state.lock().await;
                    mouse_state.observe_absolute(x, y, reference_width, reference_height);
                    mouse_state.mode
                };

                let count = self.mouse_position_packets.fetch_add(1, Ordering::Relaxed) + 1;
                let stream_err = stream
                    .send_mouse_position(x, y, reference_width, reference_height)
                    .err()
                    .map(anyhow::Error::from);

                if Self::should_log_mouse_probe(count) {
                    probe_message = Some(format!(
                        "MOUSE_ABS count={count} x={x} y={y} ref={}x{}",
                        reference_width, reference_height
                    ));
                }

                stream_err
            }
            InboundPacket::MouseButton { action, button } => {
                let desired_down = matches!(action, MouseButtonAction::Press);
                let should_forward = {
                    let mut button_states = self.mouse_button_states.lock().await;
                    let index = Self::mouse_button_index(button);
                    if button_states[index] == desired_down {
                        false
                    } else {
                        button_states[index] = desired_down;
                        true
                    }
                };

                probe_message = Some(format!(
                    "MOUSE_BUTTON action={action:?} button={button:?} forwarded={should_forward}"
                ));

                if !should_forward {
                    None
                } else {
                    let absolute_sync = {
                        let mouse_state = self.mouse_cursor_state.lock().await;
                        if mouse_state.mode == HostMouseEmulationMode::AbsoluteFollow {
                            Some(mouse_state.current_absolute())
                        } else {
                            None
                        }
                    };

                    if let Some((x, y, reference_width, reference_height)) = absolute_sync {
                        let absolute_sync_err = stream
                            .send_mouse_position(x, y, reference_width, reference_height)
                            .err()
                            .map(anyhow::Error::from);
                        if matches!(
                            button,
                            MouseButton::Left | MouseButton::Right | MouseButton::Middle
                        ) {
                            probe_message = Some(format!(
                                "MOUSE_BUTTON action={action:?} button={button:?} forwarded={should_forward} abs={}x{} ref={}x{}",
                                x, y, reference_width, reference_height
                            ));
                        }
                        if let Some(err) = absolute_sync_err {
                            Some(err)
                        } else {
                            stream
                                .send_mouse_button(action, button)
                                .err()
                                .map(anyhow::Error::from)
                        }
                    } else {
                        stream
                            .send_mouse_button(action, button)
                            .err()
                            .map(anyhow::Error::from)
                    }
                }
            }
            InboundPacket::MouseMove { delta_x, delta_y } => {
                let count = self.mouse_move_packets.fetch_add(1, Ordering::Relaxed) + 1;
                let mode = {
                    let mouse_state = self.mouse_cursor_state.lock().await;
                    mouse_state.mode
                };

                if mode == HostMouseEmulationMode::AbsoluteFollow {
                    let (delta_x, delta_y, x, y, reference_width, reference_height) = {
                        let mut mouse_state = self.mouse_cursor_state.lock().await;
                        let (next_delta_x, next_delta_y) =
                            mouse_state.shape_follow_delta(delta_x, delta_y);
                        let (x, y, reference_width, reference_height) =
                            mouse_state.apply_relative_as_absolute(next_delta_x, next_delta_y);
                        (
                            next_delta_x,
                            next_delta_y,
                            x,
                            y,
                            reference_width,
                            reference_height,
                        )
                    };
                    if Self::should_log_mouse_probe(count) {
                        probe_message = Some(format!(
                            "MOUSE_FOLLOW count={count} dx={delta_x} dy={delta_y} -> x={x} y={y} ref={}x{}",
                            reference_width, reference_height
                        ));
                    }

                    let stream_err = stream
                        .send_mouse_move_as_position(
                            delta_x,
                            delta_y,
                            reference_width,
                            reference_height,
                        )
                        .err()
                        .map(anyhow::Error::from);

                    if Self::should_log_mouse_probe(count) {
                        probe_message = Some(format!(
                            "MOUSE_FOLLOW count={count} dx={delta_x} dy={delta_y} -> x={x} y={y} ref={}x{}",
                            reference_width, reference_height
                        ));
                    }

                    stream_err
                } else {
                    if Self::should_log_mouse_probe(count) {
                        probe_message =
                            Some(format!("MOUSE_REL count={count} dx={delta_x} dy={delta_y}"));
                    }
                    stream
                        .send_mouse_move(delta_x, delta_y)
                        .err()
                        .map(anyhow::Error::from)
                }
            }
            InboundPacket::HighResScroll { delta_x, delta_y } => {
                probe_message = Some(format!("MOUSE_SCROLL_HIRES dx={delta_x} dy={delta_y}"));
                let mut err = None;
                if delta_y != 0 {
                    err = stream
                        .send_high_res_scroll(delta_y)
                        .err()
                        .map(anyhow::Error::from)
                }
                if delta_x != 0 {
                    err = stream
                        .send_high_res_horizontal_scroll(delta_x)
                        .err()
                        .map(anyhow::Error::from)
                }
                err
            }
            InboundPacket::Scroll { delta_x, delta_y } => {
                probe_message = Some(format!("MOUSE_SCROLL dx={delta_x} dy={delta_y}"));
                let mut err = None;
                if delta_y != 0 {
                    err = stream.send_scroll(delta_y).err().map(anyhow::Error::from);
                }
                if delta_x != 0 {
                    err = stream
                        .send_horizontal_scroll(delta_x)
                        .err()
                        .map(anyhow::Error::from);
                }
                err
            }
            InboundPacket::Key {
                action,
                modifiers,
                key,
                flags,
            } => stream
                .send_keyboard_event_non_standard(key as i16, action, modifiers, flags)
                .err()
                .map(anyhow::Error::from),
            InboundPacket::Text { text } => stream.send_text(&text).err().map(anyhow::Error::from),
            InboundPacket::Touch {
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_major,
                contact_area_minor,
                rotation,
                event_type,
            } => stream
                .send_touch(
                    pointer_id,
                    x,
                    y,
                    pressure_or_distance,
                    contact_area_major,
                    contact_area_minor,
                    rotation,
                    event_type,
                )
                .err()
                .map(anyhow::Error::from),
            InboundPacket::ControllerConnected {
                id,
                ty,
                supported_buttons,
                capabilities,
            } => {
                let effective_id = self.map_inbound_controller_id(id).await;
                let Some(gamepad) = ActiveGamepads::from_id(effective_id) else {
                    warn!(
                        "Failed to add gamepad because it is out of range: inbound={}, effective={}",
                        id, effective_id
                    );
                    return;
                };

                let mut active_gamepads = self.active_gamepads.write().await;

                active_gamepads.insert(gamepad);

                if let Some(broker) = self.ensure_gamepad_broker().await {
                    match broker.connect(effective_id, GamepadProfile::from(ty)).await {
                        Ok(_) => {
                            probe_message = Some(format!(
                                "GAMEPAD_BROKER_CONNECT inbound={id} effective={effective_id} profile={:?} buttons=0x{:X} caps=0x{:X}",
                                GamepadProfile::from(ty),
                                supported_buttons.bits(),
                                capabilities.bits()
                            ));
                            None
                        }
                        Err(err) => {
                            warn!(
                                "[Stream]: gamepad broker connect failed for inbound={} effective={}: {err}",
                                id, effective_id
                            );
                            stream
                                .send_controller_arrival(
                                    effective_id,
                                    *active_gamepads,
                                    ty,
                                    supported_buttons,
                                    capabilities,
                                )
                                .err()
                                .map(anyhow::Error::from)
                        }
                    }
                } else {
                    stream
                        .send_controller_arrival(
                            effective_id,
                            *active_gamepads,
                            ty,
                            supported_buttons,
                            capabilities,
                        )
                        .err()
                        .map(anyhow::Error::from)
                }
            }
            InboundPacket::ControllerDisconnected { id } => {
                let effective_id = self.release_inbound_controller_id(id).await;
                let Some(gamepad) = ActiveGamepads::from_id(effective_id) else {
                    warn!(
                        "Failed to remove gamepad because it is out of range: inbound={}, effective={}",
                        id, effective_id
                    );
                    return;
                };

                let mut active_gamepads = self.active_gamepads.write().await;
                active_gamepads.remove(gamepad);

                if let Some(broker) = self.ensure_gamepad_broker().await {
                    match broker.disconnect(effective_id).await {
                        Ok(_) => {
                            probe_message = Some(format!(
                                "GAMEPAD_BROKER_DISCONNECT inbound={id} effective={effective_id}"
                            ));
                            None
                        }
                        Err(err) => {
                            warn!(
                                "[Stream]: gamepad broker disconnect failed for inbound={} effective={}: {err}",
                                id, effective_id
                            );
                            stream
                                .send_multi_controller(
                                    effective_id,
                                    *active_gamepads,
                                    ControllerButtons::empty(),
                                    0,
                                    0,
                                    0,
                                    0,
                                    0,
                                    0,
                                )
                                .err()
                                .map(anyhow::Error::from)
                        }
                    }
                } else {
                    stream
                        .send_multi_controller(
                            effective_id,
                            *active_gamepads,
                            ControllerButtons::empty(),
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                        )
                        .err()
                        .map(anyhow::Error::from)
                }
            }
            InboundPacket::ControllerState {
                id,
                buttons,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            } => {
                let effective_id = self.resolve_inbound_controller_id(id).await;
                let Some(gamepad) = ActiveGamepads::from_id(effective_id) else {
                    warn!(
                        "Failed to update gamepad state because it is out of range: inbound={}, effective={}",
                        id, effective_id
                    );
                    return;
                };

                let active_gamepads = self.active_gamepads.read().await;
                if !active_gamepads.contains(gamepad) {
                    warn!(
                        "Failed to send gamepad event for not registered gamepad, inbound={}, effective={}, currently active: {:?}",
                        id, effective_id, *active_gamepads
                    );
                    return;
                }

                if let Some(broker) = self.ensure_gamepad_broker().await {
                    match broker
                        .update_state(
                            effective_id,
                            buttons.bits(),
                            left_trigger,
                            right_trigger,
                            left_stick_x,
                            left_stick_y,
                            right_stick_x,
                            right_stick_y,
                        )
                        .await
                    {
                        Ok(_) => None,
                        Err(err) => {
                            warn!(
                                "[Stream]: gamepad broker state failed for inbound={} effective={}: {err}",
                                id, effective_id
                            );
                            stream
                                .send_multi_controller(
                                    effective_id,
                                    *active_gamepads,
                                    buttons,
                                    left_trigger,
                                    right_trigger,
                                    left_stick_x,
                                    left_stick_y,
                                    right_stick_x,
                                    right_stick_y,
                                )
                                .err()
                                .map(anyhow::Error::from)
                        }
                    }
                } else {
                    stream
                        .send_multi_controller(
                            effective_id,
                            *active_gamepads,
                            buttons,
                            left_trigger,
                            right_trigger,
                            left_stick_x,
                            left_stick_y,
                            right_stick_x,
                            right_stick_y,
                        )
                        .err()
                        .map(anyhow::Error::from)
                }
            }
            _ => None,
        };

        drop(stream_lock);

        if let Some(message) = probe_message {
            self.send_debug_log_message(message).await;
        }

        if let Some(err) = err {
            let count = self.mouse_input_errors.fetch_add(1, Ordering::Relaxed) + 1;
            if count <= 5 || count % 50 == 0 {
                self.send_debug_log_message(format!("MOUSE_INPUT_ERROR count={count} err={err}"))
                    .await;
            }
            warn!("Failed to handle packet: {err:?}");
        }
    }

    async fn on_ipc_message(self: &Arc<StreamConnection>, message: ServerIpcMessage) {
        match &message {
            ServerIpcMessage::WebSocket(StreamClientMessage::SetTransport(transport_type)) => {
                self.clear_terminate_request().await;

                match transport_type {
                    TransportType::WebRTC => {
                        info!("Trying WebRTC transport");

                        let (sender, events) = match webrtc::new(
                            &self.config.webrtc,
                            self.video_frame_queue_size,
                            self.audio_sample_queue_size,
                        )
                        .await
                        {
                            Ok(value) => value,
                            Err(err) => {
                                error!("Failed to start webrtc transport: {err}");
                                return;
                            }
                        };
                        self.set_transport(Box::new(sender), Box::new(events)).await;
                    }
                    TransportType::WebSocket => {
                        info!("Trying Web Socket transport");

                        let (sender, events) = match web_socket::new().await {
                            Ok(value) => value,
                            Err(err) => {
                                error!("Failed to start web socket transport: {err}");
                                return;
                            }
                        };
                        self.set_transport(Box::new(sender), Box::new(events)).await;
                    }
                }

                self.flush_pending_pre_transport_messages().await;
                return;
            }
            ServerIpcMessage::Stop => {
                self.stop().await;
                return;
            }
            _ => {}
        }

        let message = match self.forward_to_transport_or_return(message).await {
            Ok(()) => return,
            Err(message) => message,
        };

        let mut pending = self.pending_pre_transport_messages.lock().await;
        let depth = pending.len() + 1;
        if depth <= 5 || depth % 25 == 0 {
            info!(
                "[Stream]: queueing pre-transport ipc message depth={depth}: {}",
                pending_message_kind(&message)
            );
        }
        pending.push_back(message);
    }

    async fn forward_to_transport_or_return(
        &self,
        message: ServerIpcMessage,
    ) -> Result<(), ServerIpcMessage> {
        let mut sender = self.transport_sender.lock().await;
        if let Some(sender) = sender.as_mut() {
            if let Err(err) = sender.on_ipc_message(message).await {
                warn!("Failed to send ipc message: {err}");
            }
            Ok(())
        } else {
            Err(message)
        }
    }

    async fn flush_pending_pre_transport_messages(&self) {
        loop {
            let Some(message) = ({
                let mut pending = self.pending_pre_transport_messages.lock().await;
                pending.pop_front()
            }) else {
                return;
            };

            let message = match self.forward_to_transport_or_return(message).await {
                Ok(()) => continue,
                Err(message) => message,
            };

            let mut pending = self.pending_pre_transport_messages.lock().await;
            pending.push_front(message);
            warn!(
                "[Stream]: transport disappeared while flushing pre-transport ipc queue; remaining_depth={}",
                pending.len()
            );
            return;
        }
    }

    async fn resize_stream(
        self: &Arc<Self>,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<(), anyhow::Error> {
        let current_setup = {
            let setup = self.stream_setup.lock().await;
            setup.video
        };

        let Some(current_setup) = current_setup else {
            anyhow::bail!("missing current video setup");
        };

        let next_setup = VideoSetup {
            format: current_setup.format,
            width: width.max(1),
            height: height.max(1),
            redraw_rate: fps.max(1),
        };

        if current_setup.format as u32 == next_setup.format as u32
            && current_setup.width == next_setup.width
            && current_setup.height == next_setup.height
            && current_setup.redraw_rate == next_setup.redraw_rate
        {
            return Ok(());
        }

        let result = {
            let mut sender = self.transport_sender.lock().await;
            let Some(sender) = sender.as_mut() else {
                anyhow::bail!("missing active transport sender");
            };

            sender.setup_video(next_setup).await
        };

        if result != 0 {
            anyhow::bail!("transport setup_video returned {result}");
        }

        {
            let mut setup = self.stream_setup.lock().await;
            setup.video = Some(next_setup);
        }

        {
            let mut mouse_state = self.mouse_cursor_state.lock().await;
            mouse_state.set_stream_size(next_setup.width, next_setup.height);
        }

        {
            let mut active_settings = self.active_stream_settings.lock().await;
            if let Some(settings) = active_settings.as_mut() {
                settings.width = next_setup.width;
                settings.height = next_setup.height;
                settings.fps = next_setup.redraw_rate;
            }
        }

        self.moonlight.request_idr_frame();
        debug!(
            "Requested IDR frame after runtime resize to {}x{}@{}",
            next_setup.width, next_setup.height, next_setup.redraw_rate
        );

        {
            let mut pending = self.pending_video_flow_ready.lock().await;
            *pending = Some(PendingVideoFlowReady {
                phase: VideoFlowPhase::RuntimeResize,
                width: next_setup.width,
                height: next_setup.height,
                fps: next_setup.redraw_rate,
                frames_remaining: 2,
            });
        }

        self.try_send_packet(
            OutboundPacket::General {
                message: GeneralServerMessage::VideoReconfigured {
                    format: next_setup.format as u32,
                    width: next_setup.width,
                    height: next_setup.height,
                    fps: next_setup.redraw_rate,
                },
            },
            "runtime video reconfigured",
            false,
        )
        .await;

        Ok(())
    }

    async fn mark_video_flow_ready_if_pending(self: &Arc<Self>) {
        let pending = {
            let mut pending = self.pending_video_flow_ready.lock().await;
            let Some(current) = pending.as_mut() else {
                return;
            };

            if current.frames_remaining > 1 {
                current.frames_remaining -= 1;
                return;
            }

            pending.take()
        };

        let Some(pending) = pending else {
            return;
        };

        let mut ipc_sender = self.ipc_sender.clone();
        ipc_sender
            .send(StreamerIpcMessage::WebSocket(
                StreamServerMessage::VideoFlowReady {
                    phase: pending.phase,
                    width: pending.width,
                    height: pending.height,
                    fps: pending.fps,
                },
            ))
            .await;
    }

    async fn send_clarity_update_result(
        &self,
        accepted: bool,
        applied_live: bool,
        requires_reconnect: bool,
        bitrate: u32,
        adaptive_bitrate: bool,
        adaptive_fps: bool,
        reason: impl Into<String>,
    ) {
        let mut ipc_sender = self.ipc_sender.clone();
        ipc_sender
            .send(StreamerIpcMessage::WebSocket(
                StreamServerMessage::ClarityUpdateResult {
                    accepted,
                    applied_live,
                    requires_reconnect,
                    bitrate,
                    adaptive_bitrate,
                    adaptive_fps,
                    reason: reason.into(),
                },
            ))
            .await;
    }

    async fn update_clarity(
        self: &Arc<Self>,
        bitrate: u32,
        adaptive_bitrate: bool,
        adaptive_fps: bool,
        allow_restart_fallback: bool,
    ) {
        let requested = LiveClarityRequest {
            bitrate,
            adaptive_bitrate,
            adaptive_fps,
        };
        let _clarity_guard = self.live_clarity_update_lock.lock().await;

        let stream_active = self.stream.read().await.is_some();
        if !stream_active {
            self.send_clarity_update_result(
                false,
                false,
                false,
                requested.bitrate,
                requested.adaptive_bitrate,
                requested.adaptive_fps,
                "stream_not_ready",
            )
            .await;
            return;
        }

        let current_settings = self.active_stream_settings.lock().await.clone();
        let Some(current_settings) = current_settings else {
            self.send_clarity_update_result(
                false,
                false,
                false,
                requested.bitrate,
                requested.adaptive_bitrate,
                requested.adaptive_fps,
                "missing_active_settings",
            )
            .await;
            return;
        };

        if current_settings.bitrate == requested.bitrate
            && current_settings.adaptive_bitrate == requested.adaptive_bitrate
            && current_settings.adaptive_fps == requested.adaptive_fps
        {
            self.send_clarity_update_result(
                true,
                true,
                false,
                requested.bitrate,
                requested.adaptive_bitrate,
                requested.adaptive_fps,
                "already_applied",
            )
            .await;
            return;
        }

        let mut next_settings = current_settings.clone();
        next_settings.bitrate = requested.bitrate;
        next_settings.adaptive_bitrate = requested.adaptive_bitrate;
        next_settings.adaptive_fps = requested.adaptive_fps;

        if current_settings.bitrate != requested.bitrate {
            let retry_delays_ms = [0_u64, 180_u64, 420_u64];
            let mut last_live_update_error: Option<MoonlightError> = None;

            for (attempt_index, delay_ms) in retry_delays_ms.iter().enumerate() {
                if *delay_ms > 0 {
                    sleep(Duration::from_millis(*delay_ms)).await;
                }

                match self.moonlight.update_stream_bitrate(requested.bitrate) {
                    Ok(()) => {
                        {
                            let mut active_settings = self.active_stream_settings.lock().await;
                            *active_settings = Some(next_settings.clone());
                        }
                        self.send_clarity_update_result(
                            true,
                            true,
                            false,
                            requested.bitrate,
                            requested.adaptive_bitrate,
                            requested.adaptive_fps,
                            "rtsp_announce_live",
                        )
                        .await;
                        return;
                    }
                    Err(err) => {
                        warn!(
                            "[Stream]: live RTSP bitrate update attempt {}/{} failed: {err}",
                            attempt_index + 1,
                            retry_delays_ms.len()
                        );
                        last_live_update_error = Some(err);
                    }
                }
            }

            let failure_reason = if let Some(err) = last_live_update_error.as_ref() {
                warn!(
                    "[Stream]: live RTSP bitrate update unavailable, requiring in-app reconnect apply: {err}"
                );
                classify_live_clarity_error(err)
            } else {
                "live_rtsp_announce_failed".to_string()
            };

            if allow_restart_fallback {
                info!(
                    "[Stream]: live RTSP bitrate update unavailable, falling back to internal upstream restart apply"
                );
                match self.start_stream(next_settings.clone()).await {
                    Ok(()) => {
                        self.send_clarity_update_result(
                            true,
                            true,
                            false,
                            requested.bitrate,
                            requested.adaptive_bitrate,
                            requested.adaptive_fps,
                            "upstream_restart_apply",
                        )
                        .await;
                        return;
                    }
                    Err(err) => {
                        warn!(
                            "[Stream]: internal upstream restart apply failed after live RTSP rejection: {err}"
                        );
                        match self.start_stream(current_settings.clone()).await {
                            Ok(()) => {
                                warn!(
                                    "[Stream]: restored previous upstream settings after failed restart apply"
                                );
                            }
                            Err(restore_err) => {
                                error!(
                                    "[Stream]: failed to restore previous upstream settings after failed restart apply: {restore_err}"
                                );
                            }
                        }
                    }
                }
            }

            self.send_clarity_update_result(
                false,
                false,
                true,
                requested.bitrate,
                requested.adaptive_bitrate,
                requested.adaptive_fps,
                &failure_reason,
            )
            .await;
            return;
        } else {
            {
                let mut active_settings = self.active_stream_settings.lock().await;
                *active_settings = Some(next_settings.clone());
            }
            self.send_clarity_update_result(
                true,
                true,
                false,
                requested.bitrate,
                requested.adaptive_bitrate,
                requested.adaptive_fps,
                "policy_only_live",
            )
            .await;
            return;
        }
    }

    // Start Moonlight Stream
    async fn start_stream(self: &Arc<Self>, settings: StreamSettings) -> Result<(), anyhow::Error> {
        self.set_host_cursor_hidden_for_stream(true, "start_stream")
            .await;
        let result = self.start_stream_inner(settings).await;
        if result.is_err() && self.stream.read().await.is_none() {
            self.set_host_cursor_hidden_for_stream(false, "start_stream_failed")
                .await;
        }
        result
    }

    async fn start_stream_inner(
        self: &Arc<Self>,
        settings: StreamSettings,
    ) -> Result<(), anyhow::Error> {
        let stream_generation = self
            .active_stream_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let had_existing_stream = { self.stream.read().await.is_some() };
        let previous_active_settings = { self.active_stream_settings.lock().await.clone() };
        if had_existing_stream {
            self.preserve_client_during_stream_replace_generation
                .store(stream_generation, Ordering::Release);
        } else {
            self.preserve_client_during_stream_replace_generation
                .store(0, Ordering::Release);
        }
        {
            let mut mouse_state = self.mouse_cursor_state.lock().await;
            mouse_state.set_mode(
                settings.host_mouse_emulation,
                settings.width,
                settings.height,
            );
        }
        if had_existing_stream {
            self.release_all_pressed_mouse_buttons("replace_stream")
                .await;
        }
        {
            let mut button_states = self.mouse_button_states.lock().await;
            *button_states = [false; 5];
        }

        // We might already be streaming -> remove and wait for connection close firstly
        {
            let mut stream = self.stream.write().await;
            if let Some(stream) = stream.take() {
                spawn_blocking(move || {
                    stream.stop();
                });
            }
        }
        if !had_existing_stream {
            let mut active_settings = self.active_stream_settings.lock().await;
            *active_settings = None;
        }
        self.shutdown_gamepad_broker().await;
        if had_existing_stream {
            sleep(Duration::from_millis(260)).await;
        }
        info!("Starting Moonlight stream with settings: {settings}");

        // Send stage
        let mut ipc_sender = self.ipc_sender.clone();
        ipc_sender
            .send(StreamerIpcMessage::WebSocket(
                StreamServerMessage::DebugLog {
                    message: "Moonlight Stream".to_string(),
                    ty: None,
                },
            ))
            .await;

        let host = &self.info.host;

        let mut effective_settings = settings.clone();

        let mut settings = MoonlightStreamSettings {
            width: settings.width,
            height: settings.height,
            fps: settings.fps,
            fps_x100: settings.fps * 100,
            hdr: settings.hdr,
            bitrate: settings.bitrate,
            packet_size: settings.packet_size,
            encryption_flags: EncryptionFlags::ALL,
            streaming_remotely: StreamingConfig::Auto,
            sops: true,
            supported_video_formats: settings.video_supported_formats,
            color_space: settings.video_colorspace,
            color_range: if settings.video_color_range_full {
                ColorRange::Full
            } else {
                ColorRange::Limited
            },
            local_audio_play_mode: settings.play_audio_local,
            audio_config: AudioConfig::STEREO,
            gamepads_attached: ActiveGamepads::empty(),
            gamepads_persist_after_disconnect: false,
        };

        let legacy_nvenc_compat = legacy_nvenc_compat_enabled();
        if legacy_nvenc_compat {
            settings.hdr = false;
            settings.supported_video_formats = SupportedVideoFormats::H264;
            settings.fps = settings.fps.max(1).min(legacy_nvenc_surface_max_fps(
                settings.width,
                settings.height,
            ));
            settings.fps_x100 = settings.fps * 100;
            settings.bitrate = settings.bitrate.min(legacy_nvenc_surface_max_bitrate(
                settings.width,
                settings.height,
                settings.fps,
            ));
            settings.packet_size = settings.packet_size.min(960);
            settings.color_space = ColorSpace::Rec709;
            settings.color_range = ColorRange::Limited;
            effective_settings.hdr = false;
            effective_settings.video_supported_formats = SupportedVideoFormats::H264;
            effective_settings.fps = settings.fps;
            effective_settings.bitrate = settings.bitrate;
            effective_settings.packet_size = settings.packet_size;
            effective_settings.video_colorspace = ColorSpace::Rec709;
            effective_settings.video_color_range_full = false;
            info!(
                "[Stream]: enabling legacy NVENC compatibility mode (codec=H264 hdr=false fps={} bitrate={} packet_size={})",
                settings.fps, settings.bitrate, settings.packet_size
            );
        }

        let server_version = host.version().await?;
        let server_gfe_version = host.gfe_version().await?;
        let server_codec_mode_support = host.server_codec_mode_support().await?;

        match settings.adjust_for_server(
            server_version,
            &server_gfe_version,
            server_codec_mode_support,
        ) {
            Ok(_) => {}
            Err(StreamConfigError::NotSupportedHdr) => {
                ipc_sender
                    .send(StreamerIpcMessage::WebSocket(
                        StreamServerMessage::DebugLog {
                            message: "Failed to start stream because this app doesn't support HDR!"
                                .to_string(),
                            ty: Some(LogMessageType::FatalDescription),
                        },
                    ))
                    .await;
                return Err(StreamConfigError::NotSupportedHdr.into());
            }
            Err(err) => return Err(err.into()),
        }

        let aes_key = AesKey::new_random(&OpenSSLCryptoBackend)?;
        let aes_iv = AesIv::new_random(&OpenSSLCryptoBackend)?;

        let stream_config_result = {
            let retry_delays_ms = [0_u64, 250_u64, 600_u64, 1100_u64];
            let mut last_error: Option<MoonlightClientError> = None;

            let mut config = None;
            for (attempt_index, delay_ms) in retry_delays_ms.iter().enumerate() {
                if *delay_ms > 0 {
                    sleep(Duration::from_millis(*delay_ms)).await;
                }

                match host
                    .start_stream(
                        self.info.app_id,
                        &settings,
                        aes_key,
                        aes_iv,
                        self.moonlight.launch_query_parameters(),
                    )
                    .await
                {
                    Ok(value) => {
                        config = Some(value);
                        break;
                    }
                    Err(err) => {
                        let transient = is_transient_start_stream_error(&err);
                        let is_last_attempt = attempt_index + 1 == retry_delays_ms.len();
                        warn!(
                            "[Stream]: failed to start moonlight stream attempt {}/{} transient={} error={}",
                            attempt_index + 1,
                            retry_delays_ms.len(),
                            transient,
                            err
                        );
                        last_error = Some(err);
                        if !transient || is_last_attempt {
                            break;
                        }
                    }
                }
            }

            config.ok_or_else(|| {
                last_error.unwrap_or_else(|| {
                    MoonlightClientError::Moonlight(MoonlightError::ConnectionFailed)
                })
            })
        };

        let mut stream_config = match stream_config_result {
            Ok(value) => value,
            Err(err) => {
                warn!("[Stream]: failed to start moonlight stream: {err}");

                #[allow(clippy::single_match)]
                match err {
                    MoonlightClientError::Moonlight(MoonlightError::ConnectionAlreadyExists) => {
                        ipc_sender
                            .send(StreamerIpcMessage::WebSocket(
                                StreamServerMessage::DebugLog { message: "Failed to start stream because this streamer is already streaming".to_string(), ty: None },
                            ))
                            .await;
                    }
                    _ => {}
                }

                return Err(err.into());
            }
        };

        let connection_retry_delays_ms = [0_u64, 220_u64, 550_u64];
        let mut last_connection_error: Option<MoonlightError> = None;
        let mut established_stream = None;

        for (attempt_index, delay_ms) in connection_retry_delays_ms.iter().enumerate() {
            if *delay_ms > 0 {
                sleep(Duration::from_millis(*delay_ms)).await;
            }

            let video_decoder = StreamVideoDecoder {
                stream: Arc::downgrade(self),
                supported_formats: settings.supported_video_formats,
                stats: Default::default(),
            };

            let audio_decoder = StreamAudioDecoder {
                stream: Arc::downgrade(self),
            };

            let connection_listener = StreamConnectionListener {
                stream: Arc::downgrade(self),
                generation: stream_generation,
            };
            let connection_listener_c = StreamConnectionListener {
                stream: Arc::downgrade(self),
                generation: stream_generation,
            };

            let settings_clone = settings.clone();
            let stream_config_clone = stream_config.clone();
            let moonlight_instance = self.moonlight.clone();
            match spawn_blocking(move || {
                moonlight_instance.start_connection(
                    stream_config_clone,
                    settings_clone,
                    connection_listener,
                    connection_listener_c,
                    video_decoder,
                    audio_decoder,
                )
            })
            .await?
            {
                Ok(stream) => {
                    established_stream = Some(stream);
                    break;
                }
                Err(err) => {
                    let transient = is_transient_start_connection_error(&err);
                    let is_last_attempt = attempt_index + 1 == connection_retry_delays_ms.len();
                    warn!(
                        "[Stream]: failed to establish upstream connection attempt {}/{} transient={} error={}",
                        attempt_index + 1,
                        connection_retry_delays_ms.len(),
                        transient,
                        err
                    );
                    last_connection_error = Some(err);
                    if transient && !is_last_attempt {
                        let refresh_stream_config = {
                            let retry_delays_ms = [0_u64, 250_u64, 600_u64, 1100_u64];
                            let mut last_error: Option<MoonlightClientError> = None;
                            let mut refreshed = None;

                            for (refresh_attempt_index, refresh_delay_ms) in
                                retry_delays_ms.iter().enumerate()
                            {
                                if *refresh_delay_ms > 0 {
                                    sleep(Duration::from_millis(*refresh_delay_ms)).await;
                                }

                                match host
                                    .start_stream(
                                        self.info.app_id,
                                        &settings,
                                        aes_key,
                                        aes_iv,
                                        self.moonlight.launch_query_parameters(),
                                    )
                                    .await
                                {
                                    Ok(value) => {
                                        refreshed = Some(value);
                                        break;
                                    }
                                    Err(refresh_err) => {
                                        let refresh_transient =
                                            is_transient_start_stream_error(&refresh_err);
                                        let refresh_is_last_attempt =
                                            refresh_attempt_index + 1 == retry_delays_ms.len();
                                        warn!(
                                            "[Stream]: failed to refresh upstream stream config attempt {}/{} transient={} error={}",
                                            refresh_attempt_index + 1,
                                            retry_delays_ms.len(),
                                            refresh_transient,
                                            refresh_err
                                        );
                                        last_error = Some(refresh_err);
                                        if !refresh_transient || refresh_is_last_attempt {
                                            break;
                                        }
                                    }
                                }
                            }

                            refreshed.ok_or_else(|| {
                                last_error.unwrap_or_else(|| {
                                    MoonlightClientError::Moonlight(
                                        MoonlightError::ConnectionFailed,
                                    )
                                })
                            })
                        };

                        match refresh_stream_config {
                            Ok(refreshed_config) => {
                                info!(
                                    "[Stream]: refreshed upstream stream config after transient connection failure"
                                );
                                stream_config = refreshed_config;
                            }
                            Err(refresh_err) => {
                                warn!(
                                    "[Stream]: failed to refresh upstream stream config after transient connection failure: {refresh_err}"
                                );
                            }
                        }
                    }
                    if !transient || is_last_attempt {
                        break;
                    }
                }
            }
        }

        let stream = match established_stream {
            Some(stream) => stream,
            None => {
                self.preserve_client_during_stream_replace_generation
                    .store(0, Ordering::Release);
                {
                    let mut active_settings = self.active_stream_settings.lock().await;
                    *active_settings = previous_active_settings;
                }
                let err = last_connection_error.unwrap_or(MoonlightError::ConnectionFailed);
                return Err(err.into());
            }
        };

        self.preserve_client_during_stream_replace_generation
            .store(0, Ordering::Release);

        let host_features = stream.host_features().unwrap_or_else(|err| {
            warn!("[Stream]: failed to get host features: {err:?}");
            HostFeatures::empty()
        });

        let capabilities = StreamCapabilities {
            touch: host_features.contains(HostFeatures::PEN_TOUCH_EVENTS),
        };

        let (video_setup, audio_setup) = {
            let setup = self.stream_setup.lock().await;

            let video = setup.video.unwrap_or_else(|| {
                warn!("failed to query video setup information. Giving the browser guessed information");
                VideoSetup { format: VideoFormat::H264, width: settings.width, height: settings.height, redraw_rate: settings.fps }
            });

            let audio = setup.audio.clone().unwrap_or(OpusMultistreamConfig::STEREO);

            (video, audio)
        };

        info!(
            "Stream uses these settings: {:?} with {}x{}x{}",
            video_setup.format, video_setup.width, video_setup.height, video_setup.redraw_rate
        );

        spawn(async move {
            ipc_sender
                .send(StreamerIpcMessage::WebSocket(
                    StreamServerMessage::ConnectionComplete {
                        capabilities,
                        format: video_setup.format as u32,
                        width: video_setup.width,
                        height: video_setup.height,
                        fps: video_setup.redraw_rate,
                        audio_sample_rate: audio_setup.sample_rate,
                        audio_channel_count: audio_setup.channel_count,
                        audio_streams: audio_setup.streams,
                        audio_coupled_streams: audio_setup.coupled_streams,
                        audio_samples_per_frame: audio_setup.samples_per_frame,
                        audio_mapping: audio_setup.mapping,
                    },
                ))
                .await;
        });

        let mut stream_guard = self.stream.write().await;
        stream_guard.replace(stream);
        drop(stream_guard);

        {
            let mut active_settings = self.active_stream_settings.lock().await;
            *active_settings = Some(effective_settings);
        }

        Ok(())
    }

    // -- Termination
    async fn request_terminate(self: &Arc<Self>) {
        debug!("Marking for termination");

        let this = self.clone();

        let mut terminate_request = self.timeout_terminate_request.lock().await;
        *terminate_request = Some(Instant::now());
        drop(terminate_request);

        spawn(async move {
            sleep(TIMEOUT_DURATION + Duration::from_millis(200)).await;

            let now = Instant::now();

            let terminate_request = this.timeout_terminate_request.lock().await;
            if let Some(terminate_request) = *terminate_request
                && (now - terminate_request) > TIMEOUT_DURATION
            {
                info!("Stopping because of timeout");

                this.stop().await;
            }
        });
    }
    async fn clear_terminate_request(&self) {
        debug!("Clearing termination timeout");

        let mut request = self.timeout_terminate_request.lock().await;

        *request = None;
    }

    async fn stop(&self) {
        if self
            .is_terminating
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            debug!("[Stream]: stream is already terminating, won't stop twice");
            return;
        }

        debug!("[Stream]: Stopping...");
        self.release_all_pressed_mouse_buttons("stop").await;
        {
            let mut stream = self.stream.write().await;
            if let Some(stream) = stream.take() {
                spawn_blocking(move || {
                    stream.stop();
                });
            }
        }
        self.shutdown_gamepad_broker().await;
        self.set_host_cursor_hidden_for_stream(false, "stop").await;
        {
            let mut button_states = self.mouse_button_states.lock().await;
            *button_states = [false; 5];
        }

        let mut transport = self.transport_sender.lock().await;
        if let Some(transport) = transport.take() {
            if let Err(err) = transport.close().await {
                warn!("Error whilst closing transport: {err}");
            }
            drop(transport);
        }

        let mut ipc_sender = self.ipc_sender.clone();
        ipc_sender.send(StreamerIpcMessage::Stop).await;

        debug!("Notifying termination");
        self.terminate.notify_waiters();
    }

    async fn release_all_pressed_mouse_buttons(&self, reason: &str) {
        let buttons_to_release = {
            let mut button_states = self.mouse_button_states.lock().await;
            let buttons = [
                MouseButton::Left,
                MouseButton::Middle,
                MouseButton::Right,
                MouseButton::X1,
                MouseButton::X2,
            ];
            let mut buttons_to_release = Vec::new();
            for (index, button) in buttons.into_iter().enumerate() {
                if button_states[index] {
                    button_states[index] = false;
                    buttons_to_release.push(button);
                }
            }
            buttons_to_release
        };

        if buttons_to_release.is_empty() {
            return;
        }

        let stream_lock = self.stream.read().await;
        let Some(stream) = stream_lock.as_ref() else {
            return;
        };

        for button in buttons_to_release {
            if let Err(err) = stream.send_mouse_button(MouseButtonAction::Release, button) {
                warn!("[Stream]: failed to release mouse button during {reason}: {err}");
            }
        }
    }
}

struct StreamConnectionListener {
    stream: Weak<StreamConnection>,
    generation: u32,
}

impl StreamConnectionListener {
    fn upgrade_current(&self) -> Option<Arc<StreamConnection>> {
        let stream = self.stream.upgrade()?;
        let active_generation = stream.active_stream_generation.load(Ordering::Acquire);
        if active_generation != self.generation {
            return None;
        }
        Some(stream)
    }

    fn is_preserving_client_session(&self, stream: &StreamConnection) -> bool {
        stream
            .preserve_client_during_stream_replace_generation
            .load(Ordering::Acquire)
            == self.generation
    }
}

impl ConnectionListener for StreamConnectionListener {
    fn set_hdr_mode(&mut self, hdr_enabled: bool) {
        info!(
            "[HDR] Host called set_hdr_mode with enabled={}",
            hdr_enabled
        );

        let Some(stream) = self.upgrade_current() else {
            return;
        };

        stream.clone().runtime.block_on(async move {
            info!("[HDR] Sending HdrModeUpdate to client");
            stream
                .try_send_packet(
                    OutboundPacket::General {
                        message: GeneralServerMessage::HdrModeUpdate {
                            enabled: hdr_enabled,
                        },
                    },
                    "hdr mode update",
                    true,
                )
                .await
        })
    }

    fn controller_rumble(
        &mut self,
        controller_number: u16,
        low_frequency_motor: u16,
        high_frequency_motor: u16,
    ) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        stream.runtime.clone().block_on(async move {
            stream
                .try_send_packet(
                    OutboundPacket::ControllerRumble {
                        controller_number: controller_number as u8,
                        low_frequency_motor,
                        high_frequency_motor,
                    },
                    "controller rumble",
                    true,
                )
                .await;
        });
    }

    fn controller_rumble_triggers(
        &mut self,
        controller_number: u16,
        left_trigger_motor: u16,
        right_trigger_motor: u16,
    ) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        stream.runtime.clone().block_on(async move {
            stream
                .try_send_packet(
                    OutboundPacket::ControllerTriggerRumble {
                        controller_number: controller_number as u8,
                        left_trigger_motor,
                        right_trigger_motor,
                    },
                    "controller rumble triggers",
                    true,
                )
                .await;
        });
    }

    fn controller_set_motion_event_state(
        &mut self,
        _controller_number: u16,
        _motion_type: u8,
        _report_rate_hz: u16,
    ) {
        // unsupported: https://github.com/w3c/gamepad/issues/211
    }

    fn controller_set_adaptive_triggers(
        &mut self,
        _controller_number: u16,
        _event_flags: u8,
        _type_left: u8,
        _type_right: u8,
        _left: &mut u8,
        _right: &mut u8,
    ) {
        // unsupported
    }

    fn controller_set_led(&mut self, _controller_number: u16, _r: u8, _g: u8, _b: u8) {
        // unsupported
    }
}

impl ConnectionListenerC for StreamConnectionListener {
    fn stage_starting(&mut self, stage: Stage) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        let mut ipc_sender = stream.ipc_sender.clone();

        stream.runtime.spawn(async move {
            ipc_sender
                .send(StreamerIpcMessage::WebSocket(
                    StreamServerMessage::DebugLog {
                        message: format!("Starting Stage: {}", stage.name()),
                        ty: None,
                    },
                ))
                .await;
        });
    }

    fn stage_complete(&mut self, stage: Stage) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        let mut ipc_sender = stream.ipc_sender.clone();
        ipc_sender.blocking_send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::DebugLog {
                message: format!("Completed Stage: {}", stage.name()),
                ty: None,
            },
        ));
    }

    fn stage_failed(&mut self, stage: Stage, error_code: i32) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        if self.is_preserving_client_session(&stream) {
            warn!(
                "[Stream]: suppressed fatal stage failure during upstream replacement: stage={} error_code={}",
                stage.name(),
                error_code
            );
            let mut ipc_sender = stream.ipc_sender.clone();
            ipc_sender.blocking_send(StreamerIpcMessage::WebSocket(
                StreamServerMessage::DebugLog {
                    message: format!(
                        "Retrying upstream replacement after stage failure: {} ({})",
                        stage.name(),
                        error_code
                    ),
                    ty: None,
                },
            ));
            return;
        }

        let mut ipc_sender = stream.ipc_sender.clone();
        ipc_sender.blocking_send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::DebugLog {
                message: format!(
                    "Failed Stage: {} with error code {}",
                    stage.name(),
                    error_code
                ),
                ty: Some(LogMessageType::Fatal),
            },
        ));

        if matches!(stage, Stage::RtspHandshake) || error_code == 10061 {
            let stream_for_stop = stream.clone();
            stream.runtime.spawn(async move {
                stream_for_stop.stop().await;
            });
        }
    }

    fn connection_started(&mut self) {}

    fn connection_terminated(&mut self, error_code: i32) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        if self.is_preserving_client_session(&stream) {
            warn!(
                "[Stream]: suppressed connection termination during upstream replacement: error_code={error_code}"
            );
            return;
        }

        let mut ipc_sender = stream.ipc_sender.clone();
        ipc_sender.blocking_send(StreamerIpcMessage::WebSocket(
            StreamServerMessage::ConnectionTerminated { error_code },
        ));

        stream.runtime.clone().block_on(async move {
            stream.stop().await;
        });
    }

    fn log_message(&mut self, message: &str) {
        if self.upgrade_current().is_none() {
            return;
        }
        info!(target: "moonlight", "{}", message.trim());
    }

    fn connection_status_update(&mut self, status: ConnectionStatus) {
        let Some(stream) = self.upgrade_current() else {
            return;
        };

        stream.clone().runtime.block_on(async move {
            stream
                .try_send_packet(
                    OutboundPacket::General {
                        message: GeneralServerMessage::ConnectionStatusUpdate {
                            status: status.into(),
                        },
                    },
                    "connection status update",
                    true,
                )
                .await
        })
    }
}
