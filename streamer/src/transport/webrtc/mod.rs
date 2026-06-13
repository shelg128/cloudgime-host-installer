use std::{
    collections::VecDeque,
    future::ready,
    net::IpAddr,
    pin::Pin,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use common::{
    StreamSettings,
    api_bindings::{
        RtcIceCandidate, RtcSdpType, RtcSessionDescription, StreamClientMessage,
        StreamServerMessage, StreamSignalingMessage, TransportChannelId,
    },
    config::{PortRange, WebRtcConfig},
    ipc::{ServerIpcMessage, StreamerIpcMessage},
};
use log::{debug, error, trace, warn};
use moonlight_common::stream::{
    audio::{AudioConfig, OpusMultistreamConfig},
    video::{DecodeResult, SupportedVideoFormats, VideoDecodeUnit, VideoSetup},
};
use tokio::{
    runtime::Handle,
    spawn,
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::sleep,
};
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
        setting_engine::SettingEngine,
    },
    data_channel::{RTCDataChannel, data_channel_message::DataChannelMessage},
    ice::udp_network::{EphemeralUDP, UDPNetwork},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_connection_state::RTCIceConnectionState,
    },
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::{sdp_type::RTCSdpType, session_description::RTCSessionDescription},
    },
};

use crate::{
    TIMEOUT_DURATION,
    convert::{
        from_webrtc_sdp, into_webrtc_ice, into_webrtc_ice_candidate, into_webrtc_network_type,
    },
    transport::{
        InboundPacket, OutboundPacket, TransportChannel, TransportError, TransportEvent,
        TransportEvents, TransportSender,
        webrtc::{
            audio::{WebRtcAudio, register_audio_codecs},
            sender::register_header_extensions,
            video::{WebRtcVideo, register_video_codecs},
        },
    },
};

mod audio;
mod sender;
mod video;

fn summarize_ice_candidate(candidate: &str) -> String {
    let parts: Vec<&str> = candidate.split_whitespace().collect();
    let address = parts.get(4).copied().unwrap_or("unknown");
    let protocol = parts.get(2).copied().unwrap_or("unknown");
    let port = parts.get(5).copied().unwrap_or("unknown");
    let candidate_type = parts
        .windows(2)
        .find_map(|window| {
            if window[0].eq_ignore_ascii_case("typ") {
                Some(window[1])
            } else {
                None
            }
        })
        .unwrap_or("unknown");
    format!("type={candidate_type} protocol={protocol} address={address}:{port}")
}

fn is_overlay_interface_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    [
        "zerotier",
        "wireguard",
        "tailscale",
        "wintun",
        "tap-windows",
        "openvpn",
        "tun",
        "utun",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn should_allow_candidate_interface(name: &str) -> bool {
    !is_overlay_interface_name(name)
}

fn should_allow_candidate_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => !ipv4.is_loopback() && !ipv4.is_link_local(),
        IpAddr::V6(ipv6) => !ipv6.is_loopback() && !ipv6.is_unspecified(),
    }
}

struct WebRtcInner {
    peer: Arc<RTCPeerConnection>,
    event_sender: Sender<TransportEvent>,
    general_channel: Arc<RTCDataChannel>,
    client_general_channel: Mutex<Option<Arc<RTCDataChannel>>>,
    stats_channel: Mutex<Option<Arc<RTCDataChannel>>>,
    rtt_channel: Mutex<Option<Arc<RTCDataChannel>>>,
    rtt: Mutex<(Instant, u16)>,
    rtt_loop_started: AtomicBool,
    video: Mutex<WebRtcVideo>,
    audio: Mutex<WebRtcAudio>,
    remote_description_ready: AtomicBool,
    pending_remote_ice_candidates: Mutex<VecDeque<RTCIceCandidateInit>>,
    // Timeout / Terminate
    pub timeout_terminate_request: Mutex<Option<Instant>>,
}

pub async fn new(
    config: &WebRtcConfig,
    video_frame_queue_size: usize,
    audio_sample_queue_size: usize,
) -> Result<(WebRTCTransportSender, WebRTCTransportEvents), anyhow::Error> {
    // -- Configure WebRTC
    let rtc_config = RTCConfiguration {
        ice_servers: config
            .ice_servers
            .clone()
            .into_iter()
            .map(into_webrtc_ice)
            .collect(),
        ..Default::default()
    };
    let mut api_settings = SettingEngine::default();

    if let Some(PortRange { min, max }) = config.port_range {
        match EphemeralUDP::new(min, max) {
            Ok(udp) => {
                api_settings.set_udp_network(UDPNetwork::Ephemeral(udp));
            }
            Err(err) => {
                warn!("[Stream]: Invalid port range in config: {err:?}");
            }
        }
    }
    if let Some(mapping) = config.nat_1to1.as_ref() {
        api_settings.set_nat_1to1_ips(
            mapping.ips.clone(),
            into_webrtc_ice_candidate(mapping.ice_candidate_type),
        );
    }
    api_settings.set_network_types(
        config
            .network_types
            .iter()
            .copied()
            .map(into_webrtc_network_type)
            .collect(),
    );
    api_settings.set_interface_filter(Box::new(|name| {
        let allowed = should_allow_candidate_interface(name);
        if !allowed {
            warn!(
                "[Stream]: filtering ICE interface '{name}' because overlay/tunnel paths are disabled"
            );
        }
        allowed
    }));
    api_settings.set_ip_filter(Box::new(|ip| {
        let allowed = should_allow_candidate_ip(ip);
        if !allowed {
            warn!("[Stream]: filtering ICE ip '{ip}' because it is loopback/link-local");
        }
        allowed
    }));

    api_settings.set_include_loopback_candidate(config.include_loopback_candidates);
    // The legacy NVENC mobile path was repeatedly dropping at roughly the same
    // wall-clock point (~155s after "Stream is ready"). That pattern points to
    // an ICE failure timeout, not random packet loss. Give the peer much more
    // time to survive transient consent/check hiccups before the transport is
    // considered failed.
    api_settings.set_ice_timeouts(
        Some(Duration::from_secs(60)),
        Some(Duration::from_secs(900)),
        Some(Duration::from_secs(1)),
    );

    // -- Register media codecs
    // TODO: register them based on the sdp
    let mut api_media = MediaEngine::default();
    register_audio_codecs(&mut api_media).expect("failed to register audio codecs");
    register_video_codecs(&mut api_media).expect("failed to register video codecs");
    register_header_extensions(&mut api_media).expect("failed to register header extensions");

    // -- Build Api
    let mut api_registry = Registry::new();

    // Use the default set of Interceptors
    api_registry = register_default_interceptors(api_registry, &mut api_media)
        .expect("failed to register webrtc default interceptors");

    let api = APIBuilder::new()
        .with_setting_engine(api_settings)
        .with_media_engine(api_media)
        .with_interceptor_registry(api_registry)
        .build();

    let (event_sender, event_receiver) = channel::<TransportEvent>(20);

    let peer = Arc::new(api.new_peer_connection(rtc_config).await?);

    let general_channel = peer.create_data_channel("general", None).await?;

    let runtime = Handle::current();
    let this_owned = Arc::new(WebRtcInner {
        peer: peer.clone(),
        event_sender,
        general_channel: general_channel.clone(),
        client_general_channel: Mutex::new(None),
        stats_channel: Mutex::new(None),
        rtt_channel: Mutex::new(None),
        rtt: Mutex::new((Instant::now(), 0)),
        rtt_loop_started: AtomicBool::new(false),
        video: Mutex::new(WebRtcVideo::new(
            runtime.clone(),
            Arc::downgrade(&peer),
            video_frame_queue_size,
        )),
        audio: Mutex::new(WebRtcAudio::new(
            runtime,
            Arc::downgrade(&peer),
            audio_sample_queue_size,
        )),
        remote_description_ready: AtomicBool::new(false),
        pending_remote_ice_candidates: Mutex::new(VecDeque::new()),
        timeout_terminate_request: Mutex::new(None),
    });

    // don't forget to register the general channel created by us
    {
        let this = this_owned.clone();
        this.on_data_channel(general_channel).await;
    }

    let this = Arc::downgrade(&this_owned);

    // -- Connection state
    peer.on_ice_connection_state_change(create_event_handler(
        this.clone(),
        async move |this, state| {
            this.on_ice_connection_state_change(state).await;
        },
    ));
    peer.on_peer_connection_state_change(create_event_handler(
        this.clone(),
        async move |this, state| {
            this.on_peer_connection_state_change(state).await;
        },
    ));

    // -- Signaling
    peer.on_ice_candidate(create_event_handler(
        this.clone(),
        async move |this, candidate| {
            this.on_ice_candidate(candidate).await;
        },
    ));

    // -- Data Channels
    peer.on_data_channel(create_event_handler(
        this.clone(),
        async move |this, channel| {
            this.on_data_channel(channel).await;
        },
    ));

    drop(peer);

    Ok((
        WebRTCTransportSender {
            inner: this_owned.clone(),
        },
        WebRTCTransportEvents { event_receiver },
    ))
}

// It compiling...
#[allow(clippy::complexity)]
fn create_event_handler<F, Args>(
    inner: Weak<WebRtcInner>,
    f: F,
) -> Box<
    dyn FnMut(Args) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync + 'static,
>
where
    Args: Send + 'static,
    F: AsyncFn(Arc<WebRtcInner>, Args) + Send + Sync + Clone + 'static,
    for<'a> F::CallRefFuture<'a>: Send,
{
    Box::new(move |args: Args| {
        let inner = inner.clone();
        let Some(inner) = inner.upgrade() else {
            debug!("Called webrtc event handler while the main type is already deallocated");
            return Box::pin(ready(())) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
        };

        let future = f.clone();
        Box::pin(async move {
            future(inner, args).await;
        }) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>
    })
        as Box<
            dyn FnMut(Args) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
                + Send
                + Sync
                + 'static,
        >
}
#[allow(clippy::complexity)]
fn create_channel_message_handler(
    inner: Weak<WebRtcInner>,
    channel: TransportChannel,
) -> Box<
    dyn FnMut(DataChannelMessage) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        + Send
        + Sync
        + 'static,
> {
    debug!("setting up channel {:?}", channel);
    create_event_handler(inner, async move |inner, message: DataChannelMessage| {
        let Some(packet) = InboundPacket::deserialize(channel, &message.data) else {
            return;
        };

        if let Err(err) = inner
            .event_sender
            .send(TransportEvent::RecvPacket(packet))
            .await
        {
            warn!("Failed to dispatch RecvPacket event: {err:?}");
        };
    })
}

impl WebRtcInner {
    async fn queue_remote_ice_candidate(&self, candidate: RTCIceCandidateInit, reason: &str) {
        let summary = summarize_ice_candidate(&candidate.candidate);
        let mut pending = self.pending_remote_ice_candidates.lock().await;
        pending.push_back(candidate);
        let depth = pending.len();
        warn!("[Signaling] Queued remote ice candidate {summary} reason={reason} depth={depth}");
    }

    async fn flush_pending_remote_ice_candidates(&self) {
        loop {
            let Some(candidate) = ({
                let mut pending = self.pending_remote_ice_candidates.lock().await;
                pending.pop_front()
            }) else {
                return;
            };

            let summary = summarize_ice_candidate(&candidate.candidate);
            match self.peer.add_ice_candidate(candidate.clone()).await {
                Ok(()) => {
                    debug!("[Signaling] Flushed queued remote ice candidate {summary}");
                }
                Err(err) => {
                    let err_text = format!("{err:?}");
                    if err_text.contains("ErrNoRemoteDescription") {
                        self.remote_description_ready
                            .store(false, Ordering::Release);
                        let mut pending = self.pending_remote_ice_candidates.lock().await;
                        pending.push_front(candidate);
                        warn!(
                            "[Signaling] Remote description not ready while flushing candidate {summary}; remaining_depth={}",
                            pending.len()
                        );
                        return;
                    }

                    warn!(
                        "[Signaling] Failed to flush queued remote ice candidate {summary}: {err_text}"
                    );
                }
            }
        }
    }

    // -- Handle Connection State
    async fn on_ice_connection_state_change(self: &Arc<Self>, state: RTCIceConnectionState) {
        debug!("[WebRTC] ICE connection state changed to {:?}", state);
    }
    async fn on_peer_connection_state_change(self: Arc<Self>, state: RTCPeerConnectionState) {
        #[allow(clippy::collapsible_if)]
        if matches!(state, RTCPeerConnectionState::Closed) {
            if let Err(err) = self.event_sender.send(TransportEvent::Closed).await {
                warn!("Failed to send peer closed event to stream: {err:?}");
                self.request_terminate().await;
            };
        } else {
            if matches!(
                state,
                RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected
            ) {
                debug!(
                    "Peer entered {:?}; keeping session alive until it explicitly closes",
                    state
                );
            }
            self.clear_terminate_request().await;
        }
    }

    // -- Handle Signaling
    #[allow(unused)]
    async fn send_answer(&self) -> bool {
        let local_description = match self.peer.create_answer(None).await {
            Err(err) => {
                warn!("[Signaling]: failed to create answer: {err:?}");
                return false;
            }
            Ok(value) => value,
        };

        if let Err(err) = self
            .peer
            .set_local_description(local_description.clone())
            .await
        {
            warn!("[Signaling]: failed to set local description: {err:?}");
            return false;
        }

        debug!(
            "[Signaling] Sending Local Description as Answer: {:?}",
            local_description.sdp
        );

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                StreamServerMessage::WebRtc(StreamSignalingMessage::Description(
                    RtcSessionDescription {
                        ty: from_webrtc_sdp(local_description.sdp_type),
                        sdp: local_description.sdp,
                    },
                )),
            )))
            .await
        {
            warn!("Failed to send local description (answer) via web socket from peer: {err:?}");
        }

        true
    }
    async fn send_offer(&self) -> bool {
        let local_description = match self.peer.create_offer(None).await {
            Err(err) => {
                error!("[Signaling]: failed to create offer: {err:?}");
                return false;
            }
            Ok(value) => value,
        };

        if let Err(err) = self
            .peer
            .set_local_description(local_description.clone())
            .await
        {
            error!("[Signaling]: failed to set local description: {err:?}");
            return false;
        }

        debug!(
            "[Signaling] Sending Local Description as Offer: {:?}",
            local_description.sdp
        );

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                StreamServerMessage::WebRtc(StreamSignalingMessage::Description(
                    RtcSessionDescription {
                        ty: from_webrtc_sdp(local_description.sdp_type),
                        sdp: local_description.sdp,
                    },
                )),
            )))
            .await
        {
            warn!("Failed to send local description (offer) via web socket from peer: {err:?}");
        };

        true
    }

    async fn on_ws_message(&self, message: StreamClientMessage) {
        match message {
            StreamClientMessage::StartStream {
                bitrate,
                packet_size,
                fps,
                width,
                height,
                adaptive_bitrate,
                adaptive_fps,
                host_mouse_emulation,
                play_audio_local,
                video_supported_formats,
                video_colorspace,
                video_color_range_full,
                hdr,
            } => {
                let video_supported_formats = SupportedVideoFormats::from_bits(video_supported_formats).unwrap_or_else(|| {
                    warn!("Failed to deserialize SupportedVideoFormats: {video_supported_formats}, falling back to only H264");
                    SupportedVideoFormats::H264
                });
                {
                    let mut video = self.video.lock().await;
                    video.set_codecs(video_supported_formats).await;
                }

                // TODO: check peer for supported formats via sdp

                if let Err(err) = self
                    .event_sender
                    .send(TransportEvent::StartStream {
                        settings: StreamSettings {
                            bitrate,
                            packet_size,
                            fps,
                            width,
                            height,
                            adaptive_bitrate,
                            adaptive_fps,
                            host_mouse_emulation,
                            video_supported_formats,
                            video_color_range_full,
                            video_colorspace: video_colorspace.into(),
                            play_audio_local,
                            hdr,
                        },
                    })
                    .await
                {
                    error!("Failed to send start stream: {err}");
                }
            }
            StreamClientMessage::ResizeStream { fps, width, height } => {
                if let Err(err) = self
                    .event_sender
                    .send(TransportEvent::ResizeStream { width, height, fps })
                    .await
                {
                    error!("Failed to send runtime resize event: {err}");
                }
            }
            StreamClientMessage::UpdateClarity {
                bitrate,
                adaptive_bitrate,
                adaptive_fps,
                allow_restart_fallback,
            } => {
                if let Err(err) = self
                    .event_sender
                    .send(TransportEvent::UpdateClarity {
                        bitrate,
                        adaptive_bitrate,
                        adaptive_fps,
                        allow_restart_fallback,
                    })
                    .await
                {
                    error!("Failed to send live clarity update event: {err}");
                }
            }
            StreamClientMessage::SetHostMouseEmulation {
                host_mouse_emulation,
            } => {
                if let Err(err) = self
                    .event_sender
                    .send(TransportEvent::SetHostMouseEmulation {
                        mode: host_mouse_emulation,
                    })
                    .await
                {
                    error!("Failed to send host mouse emulation change: {err}");
                }
            }
            StreamClientMessage::WebRtc(StreamSignalingMessage::Description(description)) => {
                debug!("[Signaling] Received Remote Description: {:?}", description);

                let description = match &description.ty {
                    RtcSdpType::Offer => RTCSessionDescription::offer(description.sdp),
                    RtcSdpType::Answer => RTCSessionDescription::answer(description.sdp),
                    RtcSdpType::Pranswer => RTCSessionDescription::pranswer(description.sdp),
                    _ => {
                        error!(
                            "[Signaling]: failed to handle RTCSdpType {:?}",
                            description.ty
                        );
                        return;
                    }
                };

                let Ok(description) = description else {
                    error!("[Signaling]: Received invalid RTCSessionDescription");
                    return;
                };

                let remote_ty = description.sdp_type;

                if remote_ty == RTCSdpType::Offer {
                    {
                        let mut audio = self.audio.lock().await;
                        if let Err(err) = audio.prepare(self).await {
                            warn!(
                                "[Stream] Failed to prepare WebRTC audio track before accepting offer: {err:?}"
                            );
                        }
                    }
                }

                if let Err(err) = self.peer.set_remote_description(description).await {
                    let err_text = format!("{err:?}");
                    if remote_ty == RTCSdpType::Answer
                        && err_text.contains("ErrSignalingStateProposedTransitionInvalid")
                    {
                        debug!(
                            "[Signaling]: ignoring duplicate remote answer while already stable: {err_text}"
                        );
                        return;
                    }
                    error!("[Signaling]: failed to set remote description: {err:?}");
                    return;
                }

                self.remote_description_ready.store(true, Ordering::Release);
                self.flush_pending_remote_ice_candidates().await;

                if remote_ty == RTCSdpType::Offer {
                    if !self.send_answer().await {
                        warn!("[Signaling]: failed to answer remote offer");
                    }
                }
            }
            StreamClientMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(description)) => {
                warn!(
                    "[Signaling] Received Ice Candidate {} ufrag_present={}",
                    summarize_ice_candidate(&description.candidate),
                    description.username_fragment.is_some()
                );

                let candidate = RTCIceCandidateInit {
                    candidate: description.candidate,
                    sdp_mid: description.sdp_mid,
                    sdp_mline_index: description.sdp_mline_index,
                    username_fragment: description.username_fragment,
                };

                if !self.remote_description_ready.load(Ordering::Acquire) {
                    self.queue_remote_ice_candidate(candidate, "remote_description_not_ready")
                        .await;
                    return;
                }

                if let Err(err) = self.peer.add_ice_candidate(candidate.clone()).await {
                    let err_text = format!("{err:?}");
                    if err_text.contains("ErrNoRemoteDescription") {
                        self.remote_description_ready
                            .store(false, Ordering::Release);
                        self.queue_remote_ice_candidate(
                            candidate,
                            "peer_reported_remote_description_not_ready",
                        )
                        .await;
                        return;
                    }
                    warn!("[Signaling]: failed to add ice candidate: {err_text}");
                }
            }
            _ => {}
        }
    }

    async fn on_ice_candidate(&self, candidate: Option<RTCIceCandidate>) {
        let Some(candidate) = candidate else {
            return;
        };

        let Ok(candidate_json) = candidate.to_json() else {
            return;
        };

        debug!(
            "[Signaling] Sending Ice Candidate: {}",
            candidate_json.candidate
        );
        warn!(
            "[Signaling] Emitting Local Ice Candidate {} ufrag_present={}",
            summarize_ice_candidate(&candidate_json.candidate),
            candidate_json.username_fragment.is_some()
        );

        let message =
            StreamServerMessage::WebRtc(StreamSignalingMessage::AddIceCandidate(RtcIceCandidate {
                candidate: candidate_json.candidate,
                sdp_mid: candidate_json.sdp_mid,
                sdp_mline_index: candidate_json.sdp_mline_index,
                username_fragment: candidate_json.username_fragment,
            }));

        if let Err(err) = self
            .event_sender
            .send(TransportEvent::SendIpc(StreamerIpcMessage::WebSocket(
                message,
            )))
            .await
        {
            error!("Failed to send web socket message from peer: {err:?}");
        };
    }

    async fn on_data_channel(self: Arc<Self>, channel: Arc<RTCDataChannel>) {
        let label = channel.label();
        debug!("adding data channel: \"{label}\"");

        let inner = Arc::downgrade(&self);

        match label {
            "general" => {
                debug!("setting up general channel message handler");
                {
                    let mut client_general = self.client_general_channel.lock().await;
                    *client_general = Some(channel.clone());
                }
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::GENERAL),
                ));
            }
            "stats" => {
                let mut stats = self.stats_channel.lock().await;

                channel.on_close({
                    let this = Arc::downgrade(&self);

                    Box::new(move ||{
                        let this = this.clone();

                        Box::pin(async move {
                            let Some(this) = this.upgrade() else {
                                warn!("Failed to close stats channel because the main type is already deallocated");
                                return;
                            };

                            this.close_stats().await;
                        })
                    })
                });

                *stats = Some(channel);
            }
            "rtt" => {
                {
                    let mut rtt = self.rtt_channel.lock().await;
                    *rtt = Some(channel.clone());
                }

                channel.on_close({
                    let this = Arc::downgrade(&self);

                    Box::new(move || {
                        let this = this.clone();

                        Box::pin(async move {
                            let Some(this) = this.upgrade() else {
                                warn!("Failed to close rtt channel because the main type is already deallocated");
                                return;
                            };

                            let mut rtt = this.rtt_channel.lock().await;
                            *rtt = None;
                            this.rtt_loop_started.store(false, Ordering::Relaxed);
                        })
                    })
                });

                channel.on_message(create_event_handler(
                    inner,
                    async move |inner, message: DataChannelMessage| {
                        let Some(InboundPacket::Rtt { sequence_number }) =
                            InboundPacket::deserialize(
                                TransportChannel(TransportChannelId::RTT),
                                &message.data,
                            )
                        else {
                            return;
                        };

                        inner.on_rtt(sequence_number).await;
                    },
                ));

                if !self.rtt_loop_started.swap(true, Ordering::Relaxed) {
                    let this = self.clone();
                    spawn(async move {
                        this.on_rtt(0).await;
                    });
                }
            }
            "mouse_reliable" | "mouse_absolute" | "mouse_relative" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::MOUSE_ABSOLUTE),
                ));
            }
            "touch" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::TOUCH),
                ));
            }
            "keyboard" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::KEYBOARD),
                ));
            }
            "controllers" => {
                channel.on_message(create_channel_message_handler(
                    inner,
                    TransportChannel(TransportChannelId::CONTROLLERS),
                ));
            }
            _ => {
                if let Some(number) = label.strip_prefix("controller")
                    && let Ok(id) = number.parse::<usize>()
                    && id < InboundPacket::CONTROLLER_CHANNELS.len()
                {
                    channel.on_message(create_channel_message_handler(
                        inner,
                        TransportChannel(InboundPacket::CONTROLLER_CHANNELS[id]),
                    ));
                }
            }
        };
    }

    async fn close_stats(&self) {
        let mut stats = self.stats_channel.lock().await;

        *stats = None;
    }

    async fn send_packet(&self, packet: OutboundPacket) -> Result<(), TransportError> {
        let mut buffer = Vec::new();

        let Some((channel, range)) = packet.serialize(&mut buffer) else {
            warn!("Failed to serialize packet: {packet:?}");
            return Ok(());
        };

        let bytes = Bytes::from(buffer);
        let bytes = bytes.slice(range);

        match channel.0 {
            TransportChannelId::GENERAL => {
                let client_general = {
                    let lock = self.client_general_channel.lock().await;
                    lock.clone()
                };
                let target_channel = match client_general {
                    Some(ref chan) => chan,
                    None => &self.general_channel,
                };
                match target_channel.send(&bytes).await {
                    Ok(_) => {}
                    Err(webrtc::Error::ErrDataChannelNotOpen) => {
                        return Err(TransportError::ChannelClosed);
                    }
                    _ => {}
                }
            }
            TransportChannelId::STATS => {
                let stats = self.stats_channel.lock().await;
                if let Some(stats) = stats.as_ref() {
                    match stats.send(&bytes).await {
                        Ok(_) => {}
                        Err(webrtc::Error::ErrDataChannelNotOpen) => {
                            return Err(TransportError::ChannelClosed);
                        }
                        _ => {}
                    }
                } else {
                    return Err(TransportError::ChannelClosed);
                }
            }
            TransportChannelId::RTT => {
                let rtt = self.rtt_channel.lock().await;
                if let Some(rtt) = rtt.as_ref() {
                    match rtt.send(&bytes).await {
                        Ok(_) => {}
                        Err(webrtc::Error::ErrDataChannelNotOpen) => {
                            return Err(TransportError::ChannelClosed);
                        }
                        _ => {}
                    }
                } else {
                    return Err(TransportError::ChannelClosed);
                }
            }
            _ => {
                warn!("Cannot send data on channel {channel:?}");
                return Err(TransportError::ChannelClosed);
            }
        }

        Ok(())
    }

    async fn send_audio_data_channel_sample(&self, data: &[u8]) -> Result<(), TransportError> {
        let channel = {
            let client_general = self.client_general_channel.lock().await;
            client_general.clone()
        };

        let Some(channel) = channel else {
            return Ok(());
        };

        let mut packet = Vec::with_capacity(4 + data.len());
        packet.extend_from_slice(b"CGA1");
        packet.extend_from_slice(data);
        match channel.send(&Bytes::from(packet)).await {
            Ok(_) => Ok(()),
            Err(webrtc::Error::ErrDataChannelNotOpen) => Err(TransportError::ChannelClosed),
            Err(err) => {
                warn!("[Stream]: failed to send host audio data-channel sample: {err}");
                Ok(())
            }
        }
    }

    async fn on_rtt(self: &Arc<Self>, recv_sequence_number: u16) {
        let (sent_at, current_sequence_number) = {
            let rtt = self.rtt.lock().await;
            *rtt
        };

        if recv_sequence_number != current_sequence_number {
            warn!(
                "Expected WebRTC rtt packet with sequence_number {current_sequence_number} but got {recv_sequence_number}"
            );
        }

        let rtt = Instant::now().saturating_duration_since(sent_at);
        if let Err(err) = self
            .send_packet(OutboundPacket::Stats(
                common::api_bindings::StreamerStatsUpdate::BrowserRtt {
                    rtt_ms: rtt.as_secs_f64() * 1000.0,
                },
            ))
            .await
        {
            warn!("Failed to send WebRTC browser rtt stats update: {err:?}");
        }

        sleep(Duration::from_millis(200)).await;

        let next_sequence_number = current_sequence_number.wrapping_add(1);
        {
            let mut state = self.rtt.lock().await;
            *state = (Instant::now(), next_sequence_number);
        }

        if let Err(err) = self
            .send_packet(OutboundPacket::Rtt {
                sequence_number: next_sequence_number,
            })
            .await
        {
            warn!(
                "Failed to send WebRTC rtt packet with sequence number {next_sequence_number}: {err:?}"
            );
        }
    }

    // -- Termination
    async fn request_terminate(self: &Arc<Self>) {
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
                && let Err(err) = this.event_sender.send(TransportEvent::Closed).await
            {
                warn!("Failed to send that the peer is closed: {err:?}");
            };
        });
    }
    async fn clear_terminate_request(&self) {
        let mut request = self.timeout_terminate_request.lock().await;

        *request = None;
    }
}

pub struct WebRTCTransportEvents {
    event_receiver: Receiver<TransportEvent>,
}

#[async_trait]
impl TransportEvents for WebRTCTransportEvents {
    async fn poll_event(&mut self) -> Result<TransportEvent, TransportError> {
        trace!("Polling WebRTCEvents");
        self.event_receiver
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }
}

pub struct WebRTCTransportSender {
    inner: Arc<WebRtcInner>,
}

#[async_trait]
impl TransportSender for WebRTCTransportSender {
    async fn setup_video(&self, setup: VideoSetup) -> i32 {
        let mut video = self.inner.video.lock().await;
        if video.setup(&self.inner, setup).await {
            0
        } else {
            -1
        }
    }
    async fn send_video_unit<'a>(
        &'a self,
        unit: &'a VideoDecodeUnit<'a>,
    ) -> Result<DecodeResult, TransportError> {
        let mut video = self.inner.video.lock().await;
        Ok(video.send_decode_unit(unit).await)
    }

    async fn setup_audio(
        &self,
        audio_config: AudioConfig,
        stream_config: OpusMultistreamConfig,
    ) -> i32 {
        let mut audio = self.inner.audio.lock().await;

        audio.setup(&self.inner, audio_config, stream_config).await
    }
    async fn send_audio_sample(&self, data: &[u8]) -> Result<(), TransportError> {
        let mut audio = self.inner.audio.lock().await;

        audio.send_audio_sample(data).await;
        drop(audio);
        self.inner.send_audio_data_channel_sample(data).await?;

        Ok(())
    }

    async fn send(&self, packet: OutboundPacket) -> Result<(), TransportError> {
        self.inner.send_packet(packet).await
    }

    async fn on_ipc_message(&self, message: ServerIpcMessage) -> Result<(), TransportError> {
        if let ServerIpcMessage::WebSocket(message) = message {
            self.inner.on_ws_message(message).await;
        }
        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.inner
            .peer
            .close()
            .await
            .map_err(|err| TransportError::Implementation(err.into()))?;

        Ok(())
    }
}
