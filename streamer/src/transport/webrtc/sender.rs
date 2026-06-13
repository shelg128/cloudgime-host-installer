use std::{
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    rtp_transceiver::rtp_codec::{RTCRtpHeaderExtensionCapability, RTPCodecType},
    sdp::extmap::ABS_SEND_TIME_URI,
    track::track_local::{
        TrackLocal, track_local_static_rtp::TrackLocalStaticRTP,
        track_local_static_sample::TrackLocalStaticSample,
    },
};

const PLAYOUT_DELAY_URI: &str = "http://www.webrtc.org/experiments/rtp-hdrext/playout-delay";
const VIDEO_PACKET_PACING_BATCH: usize = 8;
const VIDEO_PACKET_PACING_DELAY_US: u64 = 350;

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
    channel_queue_size: usize,
    new_samples_notify: Arc<Notify>,
    queue: Arc<Mutex<VecDeque<FrameSamples<Track>>>>,
}

struct FrameSamples<Track>
where
    Track: TrackLike,
{
    important: bool,
    samples: Vec<Track::Sample>,
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
            channel_queue_size,
            new_samples_notify: Default::default(),
            queue: Default::default(),
        }
    }

    pub async fn create_track(
        &mut self,
        track: Track,
        mut on_packet: impl FnMut(Box<dyn Packet + Send + Sync>) + Send + 'static,
    ) -> Result<(), anyhow::Error> {
        let Some(peer) = self.peer.upgrade() else {
            return Err(anyhow!(
                "Failed to create track because of missing webrtc peer!"
            ));
        };

        let track = Arc::new(track);

        let new_samples_notify = self.new_samples_notify.clone();
        let queue = Arc::downgrade(&self.queue);
        self.runtime.spawn({
            let track = track.clone();
            let label = self.label;
            async move {
                sample_sender(label, track, &new_samples_notify, queue).await;
            }
        });

        let rtp_track = track.track();
        let kind = rtp_track.kind();

        let transceivers = peer.get_transceivers().await;
        let mut track_sender = None;

        for transceiver in transceivers {
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
            None => peer.add_track(rtp_track).await?,
        };

        // Read incoming RTCP packets
        // Before these packets are returned they are processed by interceptors. For things
        // like NACK this needs to be called.
        self.runtime.spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((packets, _)) = track_sender.read(&mut rtcp_buf).await {
                for packet in packets {
                    on_packet(packet);
                }
            }
        });

        Ok(())
    }

    /// Returns if the frame will be delivered
    pub async fn send_samples(&self, samples: Vec<Track::Sample>, important: bool) -> bool {
        let mut queue = self.queue.lock().await;

        let result = if important {
            queue.push_front(FrameSamples { important, samples });
            true
        } else {
            if queue.len() >= self.channel_queue_size {
                warn!(
                    "[Stream]: {} sample queue full; dropping non-important sample",
                    self.label
                );
                return false;
            }

            queue.push_front(FrameSamples { important, samples });
            true
        };

        self.new_samples_notify.notify_waiters();

        result
    }

    /// Returns if the frame will be delivered
    pub async fn clear_queue(&self, clear_important: bool) {
        let mut queue = self.queue.lock().await;

        if clear_important {
            queue.clear();
        } else {
            queue.retain(|frame| frame.important);
        }
    }
}

async fn sample_sender<Track>(
    label: &'static str,
    track: Arc<Track>,
    new_samples_notify: &Notify,
    queue: Weak<Mutex<VecDeque<FrameSamples<Track>>>>,
) where
    Track: TrackLike,
{
    let mut first_write_logged = false;
    loop {
        let frame = {
            let Some(queue) = queue.upgrade() else {
                debug!("no sample queue available: stopping to submit samples");
                continue;
            };

            let mut queue = queue.lock().await;
            let Some(new_frame) = queue.pop_back() else {
                drop(queue); // Important: drop the mutex

                new_samples_notify.notified().await;
                continue;
            };

            new_frame
        };

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

            if let Err(err) = track
                .write_with_extensions(
                    sample,
                    &[
                        HeaderExtension::PlayoutDelay(PlayoutDelayExtension::new(0, 0)),
                        HeaderExtension::AbsSendTime(AbsSendTimeExtension {
                            timestamp: abs_send_time,
                        }),
                    ],
                )
                .await
            {
                warn!("[Stream]: {label} track.write_sample failed: {err}");
            } else if !first_write_logged {
                first_write_logged = true;
                info!("[Stream]: first {label} WebRTC sample written");
            }
        }
    }
}

pub trait TrackLike: Send + Sync + 'static {
    type Sample: Send + 'static;

    fn write_with_extensions(
        &self,
        sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> impl Future<Output = Result<(), anyhow::Error>> + Send;

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static>;
}

impl TrackLike for TrackLocalStaticSample {
    type Sample = Sample;

    async fn write_with_extensions(
        &self,
        sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> Result<(), anyhow::Error> {
        self.write_sample_with_extensions(&sample, extensions)
            .await
            .map_err(anyhow::Error::from)
    }

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static> {
        self
    }
}

pub struct SequencedTrackLocalStaticRTP {
    track: Arc<TrackLocalStaticRTP>,
    sequence_number: Mutex<u16>,
    all_paused_logged: AtomicBool,
}

impl From<TrackLocalStaticRTP> for SequencedTrackLocalStaticRTP {
    fn from(value: TrackLocalStaticRTP) -> Self {
        Self {
            track: Arc::new(value),
            sequence_number: Mutex::new(0),
            all_paused_logged: AtomicBool::new(false),
        }
    }
}

impl TrackLike for SequencedTrackLocalStaticRTP {
    type Sample = rtp::packet::Packet;

    async fn write_with_extensions(
        &self,
        mut sample: Self::Sample,
        extensions: &[HeaderExtension],
    ) -> Result<(), anyhow::Error> {
        let (any_paused, all_paused) = (
            self.track.any_binding_paused().await,
            self.track.all_binding_paused().await,
        );

        if all_paused {
            if !self.all_paused_logged.swap(true, Ordering::Relaxed) {
                warn!("[Stream]: RTP write skipped because all bindings are paused");
            }
            // Abort already here to not increment sequence numbers.
            return Ok(());
        }
        if any_paused {
            warn!("WebRTC: not all paused but any paused");
        }

        let mut sequence_number = self.sequence_number.lock().await;
        sample.header.sequence_number = *sequence_number;
        *sequence_number = sequence_number.wrapping_add(1);

        let bytes_written = self
            .track
            .write_rtp_with_extensions(&sample, extensions)
            .await
            .map_err(anyhow::Error::from)?;

        if bytes_written == 0 {
            warn!("[Stream]: RTP write completed with 0 bytes");
        }

        Ok(())
    }

    fn track(self: Arc<Self>) -> Arc<dyn TrackLocal + Send + Sync + 'static> {
        self.track.clone()
    }
}
