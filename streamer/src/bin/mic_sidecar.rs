#![feature(async_fn_traits)]

use std::{
    collections::VecDeque,
    future::{Future, ready},
    pin::Pin,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
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

type IpcSender = common::ipc::IpcSender<MicSidecarIpcMessage>;

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
    pending_remote_ice: Mutex<VecDeque<RTCIceCandidateInit>>,
    loopback: std::sync::Mutex<Option<HostMicrophoneLoopback>>,
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
                            match HostMicrophoneLoopback::new(preferred_channels) {
                                Ok(next) => {
                                    let hint =
                                        next.capture_hint().unwrap_or("virtual microphone host");
                                    let default_capture_name =
                                        next.default_capture_name().map(str::to_owned);
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
                                    Err(user_friendly_microphone_start_error(&error))
                                }
                            }
                        } else {
                            Ok("Mic menerima suara dari perangkat ini.".to_owned())
                        }
                    }
                    Err(_) => Err(
                        "Mic belum bisa aktif di PC host. Coba matikan lalu hidupkan mic lagi."
                            .to_owned(),
                    ),
                }
            };

            match result {
                Ok(message) => message,
                Err(message) => {
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
