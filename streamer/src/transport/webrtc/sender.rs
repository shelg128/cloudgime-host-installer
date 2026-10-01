use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use log::{debug, info, warn};
use tokio::{
    runtime::Handle,
    sync::{Mutex, Notify},
};
use webrtc::{
    api::media_engine::MediaEngine,
    media::Sample,
    peer_connection::RTCPeerConnection,
    rtcp::packet::Packet,
    rtp::{
        self,
        extension::{
            HeaderExtension, abs_send_time_extension::AbsSendTimeExtension,
            playout_delay_extension::PlayoutDelayExtension,
        },
    },
    rtp_transceiver::{
        RTCRtpTransceiverInit,
        rtp_codec::{RTCRtpHeaderExtensionCapability, RTPCodecType},
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    sdp::extmap::ABS_SEND_TIME_URI,
    track::track_local::{
        TrackLocal, track_local_static_rtp::TrackLocalStaticRTP,
        track_local_static_sample::TrackLocalStaticSample,
    },
};

#[path = "sender_queue.rs"]
mod sender_queue;
use sender_queue::{FrameQueue, SenderWorkers, next_frame};
#[path = "nack_probe.rs"]
mod nack_probe;
use nack_probe::NackProbe;
use super::scoped_video_recovery::{Recovery,RecoveryTrack};

const PLAYOUT_DELAY_URI: &str = "http://www.webrtc.org/experiments/rtp-hdrext/playout-delay";
const VIDEO_PACKET_PACING_BATCH: usize = 8;
const VIDEO_PACKET_PACING_DELAY_US: u64 = 350;
const RTP_BINDING_UNPAUSE_WAIT: Duration = Duration::from_secs(5);
const RTP_BINDING_UNPAUSE_POLL: Duration = Duration::from_millis(20);
const RTP_WRITE_TIMEOUT: Duration = Duration::from_millis(1500);

fn video_playout_delay(label: &str) -> PlayoutDelayExtension {
    // RTP extension units are 10ms. QA only: permit bounded jitter buffering
    // without forcing a minimum delay. No route/codec/budget changes.
    if cfg!(feature = "cloudgime-jitter-window-qa") && label == "video" {
        PlayoutDelayExtension::new(0, 10)
    } else { PlayoutDelayExtension::new(0, 0) }
}

pub fn register_header_extensions(api_media: &mut MediaEngine) -> Result<(), webrtc::Error> {
    api_media.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: PLAYOUT_DELAY_URI.to_string(),
        },
        RTPCodecType::Video,
        None,
    )?;
    api_media.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: PLAYOUT_DELAY_URI.to_string(),
        },
        RTPCodecType::Audio,
        None,
    )?;

    api_media.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: ABS_SEND_TIME_URI.to_string(),
        },
        RTPCodecType::Video,
        None,
    )?;
    api_media.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: ABS_SEND_TIME_URI.to_string(),
        },
        RTPCodecType::Audio,
        None,
    )?;

    Ok(())
}

pub struct TrackLocalSender<Track>
where
    Track: TrackLike,
{
    label: &'static str,
    runtime: Handle,
    peer: Weak<RTCPeerConnection>,
    queue: Arc<FrameQueue<Track::Sample>>,
    workers: SenderWorkers,
    track_sender: Option<Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>>,
    sequence_number: Arc<Mutex<u16>>,
    nack_probe: Arc<Mutex<NackProbe>>,
    recovery: Option<Arc<Recovery>>,
}

impl<Track> TrackLocalSender<Track>
where
    Track: TrackLike,
{
    pub fn new(
        label: &'static str,
        runtime: Handle,
        peer: Weak<RTCPeerConnection>,
        channel_queue_size: usize,
    ) -> Self {
        Self {
            label,
            runtime,
            peer,
            queue: Arc::new(FrameQueue::new(channel_queue_size)),
            workers: SenderWorkers::default(),
            track_sender: None,
            sequence_number: Arc::new(Mutex::new(0)),
            nack_probe: Arc::new(Mutex::new(NackProbe::default())),
            recovery: None,
        }
    }

    pub fn with_recovery(mut self,recovery:Option<Arc<Recovery>>)->Self{self.recovery=recovery;self}

    pub async fn create_track(
        &mut self,
        mut track: Track,
        mut on_packet: impl FnMut(Box<dyn Packet + Send + Sync>) + Send + 'static,
    ) -> Result<(), anyhow::Error> {
        let Some(peer) = self.peer.upgrade() else {
            return Err(anyhow!(
                "Failed to create track because of missing webrtc peer!"
            ));
        };

        self.workers.stop().await;
        self.queue.clear(true).await;
        track.set_sequence_counter(self.sequence_number.clone());
        track.set_nack_probe(self.nack_probe.clone());
        track.set_recovery(self.recovery.clone());
        let track = Arc::new(track);

        let rtp_track = track.clone().track();
        let kind = rtp_track.kind();

        let transceivers = peer.get_transceivers().await;
        let mut track_sender = None;
        if let Some(previous) = self.track_sender.as_ref() {
            // A codec replacement belongs to this media sender, not a new
            // transceiver. Preserve the established transport and RTCP path.
            previous.replace_track(Some(rtp_track.clone())).await?;
            track_sender = Some(previous.clone());
        }

        for transceiver in transceivers {
            if track_sender.is_some() { break; }
            if transceiver.kind() == kind {
                let sender = transceiver.sender().await;
                if sender.track().await.is_none() {
                    if let Err(err) = sender.replace_track(Some(rtp_track.clone())).await {
                        log::warn!(
                            "[Stream]: failed to replace track on transceiver for kind {:?}: {:?}",
                            kind,
                            err
                        );
                    } else {
                        log::info!(
                            "[Stream]: successfully reused existing transceiver for kind {:?}",
                            kind
                        );
                        track_sender = Some(sender);
                        break;
                    }
                }
            }
        }

        let track_sender = match track_sender {
            Some(sender) => sender,
            None => {
                let transceiver = peer
                    .add_transceiver_from_track(
                        rtp_track,
                        Some(RTCRtpTransceiverInit {
                            direction: RTCRtpTransceiverDirection::Sendonly,
                            send_encodings: vec![],
                        }),
                    )
                    .await?;
                transceiver.sender().await
            }
        };

        self.track_sender = Some(track_sender.clone());
        // Read incoming RTCP packets
        // Before these packets are returned they are processed by interceptors. For things
        // like NACK this needs to be called.
        let nack_probe = self.nack_probe.clone();
        let recovery = self.recovery.clone();
        let label = self.label;
        self.workers.push(self.runtime.spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            let mut last_probe = Instant::now();
            while let Ok((packets, _)) = track_sender.read(&mut rtcp_buf).await {
                for packet in packets {
                    if let Some(nack) = packet.as_any().downcast_ref::<webrtc::rtcp::transport_feedbacks::transport_layer_nack::TransportLayerNack>() {
                        let mut probe = nack_probe.lock().await;
                        for pair in &nack.nacks { for seq in *pair { probe.nack(seq, Instant::now()); } }
                    }
                    on_packet(packet);
                }
                if label == "video" && last_probe.elapsed() >= Duration::from_secs(5) {
                    let w = nack_probe.lock().await.take_window();
                    if let Some(r) = &recovery {r.loss_window(w.sent.iter().sum(),w.first_nacks.iter().sum());}
                    info!("[Stream]: loss_bins sent={:?} first={:?} repeat={} unknown={} age_ms={}",w.sent,w.first_nacks,w.repeats,w.unknown,w.max_age_ms);
                    last_probe = Instant::now();
                }
            }
        }));

        let wake = self.queue.wake.clone();
        let queue = Arc::downgrade(&self.queue);
        let label = self.label;
        self.workers.push(self.runtime.spawn(async move {
            sample_sender(label, track, &wake, queue).await;
        }));
        Ok(())
    }

    /// Returns if the frame will be delivered
    pub async fn send_samples(&self, samples: Vec<Track::Sample>, important: bool) -> bool {
        let accepted = self.queue.push(samples, important).await;
        if !accepted {
            warn!("[Stream]: {} sample queue full; dropping non-important sample", self.label);
        }
        accepted
    }

    pub async fn clear_queue(&self, clear_important: bool) {
        self.queue.clear(clear_important).await;
    }

}

async fn sample_sender<Track>(
    label: &'static str,
    track: Arc<Track>,
    new_samples_notify: &Notify,
    queue: Weak<FrameQueue<Track::Sample>>,
) where
    Track: TrackLike,
{
    let mut first_write_logged = false;
    let mut samples_written = 0u64;
    let mut bytes_written_total = 0u64;
    let mut last_stats_log_at = Instant::now();
    let mut queue_max_us = 0u64;
    let mut frame_send_max_us = 0u64;
    let mut frames_dequeued = 0u64;
    let mut zero_writes = 0u64;
    loop {
        let Some(frame) = next_frame(&queue, new_samples_notify).await else {
            debug!("[Stream]: {label} queue dropped; sender exiting");
            break;
        };
        let queue_age_us = frame.enqueued_at.elapsed().as_micros() as u64;
        queue_max_us = queue_max_us.max(queue_age_us);
        frames_dequeued += 1;
        let send_started = Instant::now();
        let should_micro_pace = frame.samples.len() >= VIDEO_PACKET_PACING_BATCH * 2;

        for (index, sample) in frame.samples.into_iter().enumerate() {
            if should_micro_pace && index > 0 && index % VIDEO_PACKET_PACING_BATCH == 0 {
                tokio::time::sleep(Duration::from_micros(VIDEO_PACKET_PACING_DELAY_US)).await;
            }

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock went backwards");
            let now_secs = now.as_secs() as f64 + now.subsec_nanos() as f64 * 1e-9;
            let abs_send_time: u64 = (now_secs * 262_144.0) as u64;

            match track
                .write_with_extensions(
                    sample,
                    &[
                        HeaderExtension::PlayoutDelay(video_playout_delay(label)),
                        HeaderExtension::AbsSendTime(AbsSendTimeExtension {
                            timestamp: abs_send_time,
                        }),
                    ],
                )
                .await
            {
                Ok(bytes_written) => {
                    if bytes_written == 0 { zero_writes += 1; continue; }
                    samples_written = samples_written.saturating_add(1);
                    bytes_written_total = bytes_written_total.saturating_add(bytes_written as u64);
                    if !first_write_logged {
                        first_write_logged = true;
                        info!("[Stream]: first {label} WebRTC sample written");
                    }

                }
                Err(err) => {
                    warn!("[Stream]: {label} track.write_sample failed: {err}");
                }
            }
        }
        frame_send_max_us = frame_send_max_us.max(send_started.elapsed().as_micros() as u64);
        if last_stats_log_at.elapsed() >= Duration::from_secs(5) {
            info!("[Stream]: pace {label} q_us={queue_max_us} send_us={frame_send_max_us} frames={frames_dequeued} zero={zero_writes}");
            info!("[Stream]: {label} WebRTC RTP stats samples_written={samples_written} bytes_written={bytes_written_total}");
            queue_max_us = 0; frame_send_max_us = 0;
            last_stats_log_at = Instant::now();
        }
    }
}

pub trait TrackLike: Send + Sync + 'static {
    type Sample: Send + 'static;

    // Raw-RTP tracks reuse the established SSRC and must not rewind sequence
    // numbers on replacement. Static-sample tracks own their own packetizer.
    fn set_sequence_counter(&mut self, _counter: Arc<Mutex<u16>>) {}
    fn set_nack_probe(&mut self, _probe: Arc<Mutex<NackProbe>>) {}
    fn set_recovery(&mut self,_recovery:Option<Arc<Recovery>>) {}

    fn write_with_extensions(
        &self,
        sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> impl Future<Output = Result<usize, anyhow::Error>> + Send;

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static>;
}

impl TrackLike for TrackLocalStaticSample {
    type Sample = Sample;

    async fn write_with_extensions(
        &self,
        sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> Result<usize, anyhow::Error> {
        let bytes_written = sample.data.len();
        self.write_sample_with_extensions(&sample, extensions)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(bytes_written)
    }

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static> {
        self
    }
}

pub struct SequencedTrackLocalStaticRTP {
    track: Arc<TrackLocalStaticRTP>,
    sequence_number: Arc<Mutex<u16>>,
    all_paused_logged: AtomicBool,
    nack_probe: Option<Arc<Mutex<NackProbe>>>,
    recovery_track: Option<Arc<RecoveryTrack>>,
}

impl From<TrackLocalStaticRTP> for SequencedTrackLocalStaticRTP {
    fn from(value: TrackLocalStaticRTP) -> Self {
        Self {
            track: Arc::new(value),
            sequence_number: Arc::new(Mutex::new(0)),
            all_paused_logged: AtomicBool::new(false),
            nack_probe: None,
            recovery_track: None,
        }
    }
}

fn advance_rtp_sequence(counter: &mut u16) -> u16 {
    let value = *counter;
    *counter = counter.wrapping_add(1);
    value
}

impl TrackLike for SequencedTrackLocalStaticRTP {
    type Sample = rtp::packet::Packet;

    fn set_sequence_counter(&mut self, counter: Arc<Mutex<u16>>) {
        self.sequence_number = counter;
    }
    fn set_nack_probe(&mut self, probe: Arc<Mutex<NackProbe>>) { self.nack_probe = Some(probe); }
    fn set_recovery(&mut self,recovery:Option<Arc<Recovery>>){if recovery.is_some(){self.recovery_track=Some(Arc::new(RecoveryTrack::new(self.track.clone(),recovery)));}}

    async fn write_with_extensions(
        &self,
        mut sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> Result<usize, anyhow::Error> {
        let mut any_paused = self.track.any_binding_paused().await;
        let mut all_paused = self.track.all_binding_paused().await;

        if all_paused {
            if !self.all_paused_logged.swap(true, Ordering::Relaxed) {
                warn!("[Stream]: RTP write waiting because all bindings are paused");
            }

            let wait_started = tokio::time::Instant::now();
            while all_paused && wait_started.elapsed() < RTP_BINDING_UNPAUSE_WAIT {
                tokio::time::sleep(RTP_BINDING_UNPAUSE_POLL).await;
                any_paused = self.track.any_binding_paused().await;
                all_paused = self.track.all_binding_paused().await;
            }

            if all_paused {
                warn!("[Stream]: RTP write skipped because all bindings stayed paused");
                // Abort already here to not increment sequence numbers.
                return Ok(0);
            }

            self.all_paused_logged.store(false, Ordering::Relaxed);
            info!("[Stream]: RTP write resumed after bindings became active");
        }
        if any_paused {
            warn!("WebRTC: not all paused but any paused");
        }

        let mut sequence_number = self.sequence_number.lock().await;
        sample.header.sequence_number = advance_rtp_sequence(&mut sequence_number);
        if let Some(probe) = &self.nack_probe {
            probe.lock().await.sent(sample.header.sequence_number, sample.payload.len(), Instant::now());
        }

        let permission=if let Some(track)=&self.recovery_track {track.permission(sample.header.sequence_number).await}else{None};
        let send=async {let send=self.track.write_rtp_with_extensions(&sample,extensions);if let Some(scope)=permission{webrtc_util::conn::scoped_recovery::within(scope,send).await}else{send.await}};
        let bytes_written = match tokio::time::timeout(
            RTP_WRITE_TIMEOUT,
            send,
        )
        .await
        {
            Ok(result) => result.map_err(anyhow::Error::from)?,
            Err(_) => {
                warn!(
                    "[Stream]: RTP write timed out after {}ms; dropping packet seq={} payload_bytes={}",
                    RTP_WRITE_TIMEOUT.as_millis(),
                    sample.header.sequence_number,
                    sample.payload.len()
                );
                return Ok(0);
            }
        };

        if bytes_written == 0 {
            warn!("[Stream]: RTP write completed with 0 bytes");
        }

        Ok(bytes_written)
    }

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static> {
        if let Some(track)=&self.recovery_track {track.clone()} else {self.track.clone()}
    }
}

#[cfg(test)]
mod sender_sequence_tests {
    use super::*;
    use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

    fn raw_track() -> SequencedTrackLocalStaticRTP {
        TrackLocalStaticRTP::new(
            RTCRtpCodecCapability { mime_type: "video/H264".into(), clock_rate: 90_000, ..Default::default() },
            "video".into(), "test".into(),
        ).into()
    }

    #[tokio::test]
    async fn replacement_uses_the_same_next_sequence() {
        let counter = Arc::new(Mutex::new(65_534));
        let mut first = raw_track(); first.set_sequence_counter(counter.clone());
        assert_eq!(advance_rtp_sequence(&mut *first.sequence_number.lock().await), 65_534);
        let mut replacement = raw_track(); replacement.set_sequence_counter(counter);
        assert!(Arc::ptr_eq(&first.sequence_number, &replacement.sequence_number));
        assert_eq!(advance_rtp_sequence(&mut *replacement.sequence_number.lock().await), 65_535);
        assert_eq!(advance_rtp_sequence(&mut *replacement.sequence_number.lock().await), 0);
    }

    #[tokio::test]
    async fn independent_media_senders_do_not_share_sequence() {
        let a = raw_track(); let b = raw_track();
        assert!(!Arc::ptr_eq(&a.sequence_number, &b.sequence_number));
        assert_eq!(advance_rtp_sequence(&mut *a.sequence_number.lock().await), 0);
        assert_eq!(*b.sequence_number.lock().await, 0);
        assert_eq!(*a.sequence_number.lock().await, 1);
    }

    #[test]
    fn playout_window_is_qa_only_and_never_changes_audio() {
        assert_eq!(video_playout_delay("audio"),PlayoutDelayExtension::new(0,0));
        assert_eq!(video_playout_delay("video"),PlayoutDelayExtension::new(0,if cfg!(feature="cloudgime-jitter-window-qa"){10}else{0}));
        assert_eq!(video_playout_delay("other"),PlayoutDelayExtension::new(0,0));
    }

    #[test]
    fn no_backward_jump_at_u16_wrap() {
        let mut counter = 65_535;
        let a = advance_rtp_sequence(&mut counter);
        let b = advance_rtp_sequence(&mut counter);
        assert_eq!(b.wrapping_sub(a), 1);
        assert_eq!(counter, 1);
    }
}
