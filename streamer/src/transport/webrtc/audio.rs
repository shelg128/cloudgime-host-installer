use std::{sync::Weak, time::Duration};

use bytes::Bytes;
use log::{error, info, warn};
use moonlight_common::stream::audio::{AudioConfig, OpusMultistreamConfig};
use tokio::runtime::Handle;
use webrtc::{
    api::media_engine::{MIME_TYPE_OPUS, MediaEngine},
    peer_connection::RTCPeerConnection,
    rtp::{header::Header, packet::Packet},
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::transport::webrtc::{
    WebRtcInner,
    sender::{SequencedTrackLocalStaticRTP, TrackLocalSender},
};

pub fn register_audio_codecs(media_engine: &mut MediaEngine) -> Result<(), webrtc::Error> {
    media_engine.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    Ok(())
}

pub struct WebRtcAudio {
    sender: TrackLocalSender<SequencedTrackLocalStaticRTP>,
    config: Option<OpusMultistreamConfig>,
    track_created: bool,
    first_sample_logged: bool,
    next_rtp_timestamp: u32,
}

impl WebRtcAudio {
    pub fn new(runtime: Handle, peer: Weak<RTCPeerConnection>, channel_queue_size: usize) -> Self {
        Self {
            sender: TrackLocalSender::new("audio", runtime, peer, channel_queue_size),
            config: None,
            track_created: false,
            first_sample_logged: false,
            next_rtp_timestamp: 0,
        }
    }
}

impl WebRtcAudio {
    pub async fn prepare(&mut self, inner: &WebRtcInner) -> Result<(), anyhow::Error> {
        self.ensure_track(inner, false).await
    }

    pub async fn setup(
        &mut self,
        inner: &WebRtcInner,
        audio_config: AudioConfig,
        stream_config: OpusMultistreamConfig,
    ) -> i32 {
        const SUPPORTED_SAMPLE_RATES: &[u32] = &[80000, 12000, 16000, 24000, 48000];
        if !SUPPORTED_SAMPLE_RATES.contains(&stream_config.sample_rate) {
            warn!(
                "[Stream] Audio could have problems because of the sample rate, Selected: {}, Expected one of: {SUPPORTED_SAMPLE_RATES:?}",
                stream_config.sample_rate
            );
        }
        if audio_config != self.config() {
            warn!(
                "[Stream] A different audio configuration than requested was selected, Expected: {:?}, Found: {audio_config:?}",
                self.config()
            );
        }

        let created_during_setup = !self.track_created;
        if let Err(err) = self.ensure_track(inner, created_during_setup).await {
            error!("Failed to create opus track: {err:?}");
            return -1;
        };

        self.config = Some(stream_config);
        self.next_rtp_timestamp = 0;
        self.sender.clear_queue(true).await;

        0
    }

    async fn ensure_track(
        &mut self,
        inner: &WebRtcInner,
        renegotiate: bool,
    ) -> Result<(), anyhow::Error> {
        if self.track_created {
            return Ok(());
        }

        self.sender
            .create_track(
                TrackLocalStaticRTP::new(
                    RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_OPUS.to_string(),
                        clock_rate: 48000,
                        channels: 2,
                        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                        rtcp_feedback: vec![],
                    },
                    "audio".to_string(),
                    "moonlight".to_string(),
                )
                .into(),
                |_| {},
            )
            .await?;
        self.track_created = true;
        info!("[Stream] WebRTC audio track prepared");

        if renegotiate && !inner.send_offer().await {
            warn!("Failed to renegotiate. Audio was added!");
        }

        Ok(())
    }

    pub async fn send_audio_sample(&mut self, data: &[u8]) {
        let Some(config) = self.config.as_ref() else {
            return;
        };

        let duration =
            Duration::from_secs_f64(config.samples_per_frame as f64 / config.sample_rate as f64);
        if !self.first_sample_logged {
            self.first_sample_logged = true;
            info!(
                "[Stream] first host audio sample queued for WebRTC: bytes={} duration_ms={:.2}",
                data.len(),
                duration.as_secs_f64() * 1000.0
            );
        }

        let data = Bytes::copy_from_slice(data);
        let timestamp = self.next_rtp_timestamp;
        self.next_rtp_timestamp = self
            .next_rtp_timestamp
            .wrapping_add(config.samples_per_frame);

        let packet = Packet {
            header: Header {
                version: 2,
                padding: false,
                extension: false,
                marker: false,
                sequence_number: 0,
                timestamp,
                payload_type: 0,
                ssrc: 0,
                ..Default::default()
            },
            payload: data,
            ..Default::default()
        };

        self.sender.send_samples(vec![packet], false).await;
    }

    fn config(&self) -> AudioConfig {
        AudioConfig::STEREO
    }
}
