use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use common::api_bindings::{GeneralServerMessage, StatsHostProcessingLatency, StreamerStatsUpdate};
use log::{debug, error, warn};
use moonlight_common::stream::{
    c::bindings::EstimatedRttInfo,
    video::{
        DecodeResult, SupportedVideoFormats, VideoCapabilities, VideoDecodeUnit, VideoDecoder,
        VideoSetup,
    },
};

use crate::{StreamConnection, transport::OutboundPacket};

pub(crate) struct StreamVideoDecoder {
    pub(crate) stream: Weak<StreamConnection>,
    pub(crate) supported_formats: SupportedVideoFormats,
    pub(crate) stats: VideoStats,
}

impl VideoDecoder for StreamVideoDecoder {
    fn setup(&mut self, setup: VideoSetup) -> i32 {
        let Some(stream) = self.stream.upgrade() else {
            warn!("Failed to setup video because stream is deallocated");
            return -1;
        };

        let previous_setup = {
            let stream_info = stream.stream_setup.blocking_lock();
            stream_info.video
        };
        let is_reconfigure = previous_setup
            .map(|previous| {
                previous.format as u32 != setup.format as u32
                    || previous.width != setup.width
                    || previous.height != setup.height
                    || previous.redraw_rate != setup.redraw_rate
            })
            .unwrap_or(false);

        {
            let mut stream_info = stream.stream_setup.blocking_lock();
            stream_info.video = Some(setup);
        }

        let stream_for_setup = stream.clone();
        let result = {
            stream.runtime.clone().block_on(async move {
                let mut sender = stream_for_setup.transport_sender.lock().await;

                if let Some(sender) = sender.as_mut() {
                    sender.setup_video(setup).await
                } else {
                    error!("Failed to setup video because of missing transport!");
                    -1
                }
            })
        };

        if result == 0 && is_reconfigure {
            let runtime = stream.runtime.clone();
            let stream_clone = stream.clone();
            runtime.spawn(async move {
                stream_clone
                    .try_send_packet(
                        OutboundPacket::General {
                            message: GeneralServerMessage::VideoReconfigured {
                                format: setup.format as u32,
                                width: setup.width,
                                height: setup.height,
                                fps: setup.redraw_rate,
                            },
                        },
                        "video reconfigured",
                        false,
                    )
                    .await;
            });
        }

        if result == 0 {
            let frames_remaining = if is_reconfigure { 2 } else { 1 };
            let mut pending = stream.pending_video_flow_ready.blocking_lock();
            *pending = Some(crate::PendingVideoFlowReady {
                phase: if is_reconfigure {
                    common::api_bindings::VideoFlowPhase::RuntimeResize
                } else {
                    common::api_bindings::VideoFlowPhase::Start
                },
                width: setup.width,
                height: setup.height,
                fps: setup.redraw_rate,
                frames_remaining,
            });
        }

        result
    }

    fn start(&mut self) {}
    fn stop(&mut self) {}

    fn submit_decode_unit(&mut self, unit: VideoDecodeUnit<'_>) -> DecodeResult {
        let Some(stream) = self.stream.upgrade() else {
            warn!("Failed to send video decode unit because stream is deallocated");
            return DecodeResult::Ok;
        };

        stream.runtime.clone().block_on(async {
            let mut sender = stream.transport_sender.lock().await;

            if let Some(sender) = sender.as_mut() {
                let start = Instant::now();
                let result = match sender.send_video_unit(&unit).await {
                    Err(err) => {
                        warn!("Failed to send video decode unit: {err}");
                        DecodeResult::Ok
                    }
                    Ok(value) => value,
                };

                let frame_processing_time = Instant::now() - start;
                self.stats.analyze(&stream, &unit, frame_processing_time);

                if !matches!(result, DecodeResult::NeedIdr) {
                    stream.mark_video_flow_ready_if_pending().await;
                }

                result
            } else {
                debug!("Dropping video packet because of missing transport");

                DecodeResult::Ok
            }
        })
    }

    fn supported_formats(&self) -> SupportedVideoFormats {
        self.supported_formats
    }

    fn capabilities(&self) -> VideoCapabilities {
        VideoCapabilities::default()
    }
}

#[derive(Debug, Default)]
pub(crate) struct VideoStats {
    last_send: Option<Instant>,
    min_host_processing_latency: Duration,
    max_host_processing_latency: Duration,
    total_host_processing_latency: Duration,
    host_processing_frame_count: usize,
    min_streamer_processing_time: Duration,
    max_streamer_processing_time: Duration,
    total_streamer_processing_time: Duration,
    streamer_processing_time_frame_count: usize,
}

impl VideoStats {
    fn analyze(
        &mut self,
        stream: &Arc<StreamConnection>,
        unit: &VideoDecodeUnit,
        frame_processing_time: Duration,
    ) {
        if let Some(host_processing_latency) = unit.frame_processing_latency {
            self.min_host_processing_latency = self
                .min_host_processing_latency
                .min(host_processing_latency);
            self.max_host_processing_latency = self
                .max_host_processing_latency
                .max(host_processing_latency);
            self.total_host_processing_latency += host_processing_latency;
            self.host_processing_frame_count += 1;
        }

        self.min_streamer_processing_time =
            self.min_streamer_processing_time.min(frame_processing_time);
        self.max_streamer_processing_time =
            self.max_streamer_processing_time.max(frame_processing_time);
        self.total_streamer_processing_time += frame_processing_time;
        self.streamer_processing_time_frame_count += 1;

        // Send in 1 sec intervall
        if self
            .last_send
            .map(|last_send| last_send + Duration::from_secs(1) < Instant::now())
            .unwrap_or(true)
        {
            let now = Instant::now();
            let sample_window = self
                .last_send
                .map(|last_send| now.saturating_duration_since(last_send))
                .unwrap_or(Duration::from_secs(1));

            // Collect data
            let has_host_processing_latency = self.host_processing_frame_count > 0;
            let min_host_processing_latency = self.min_host_processing_latency;
            let max_host_processing_latency = self.max_host_processing_latency;
            let avg_host_processing_latency = self
                .total_host_processing_latency
                .checked_div(self.host_processing_frame_count as u32)
                .unwrap_or(Duration::ZERO);

            let min_streamer_processing_time = self.min_streamer_processing_time;
            let max_streamer_processing_time = self.max_streamer_processing_time;
            let avg_streamer_processing_time = self
                .total_streamer_processing_time
                .checked_div(self.streamer_processing_time_frame_count as u32)
                .unwrap_or(Duration::ZERO);
            let streamer_output_fps = if self.streamer_processing_time_frame_count > 0 {
                let window_secs = sample_window.as_secs_f64().max(0.001);
                self.streamer_processing_time_frame_count as f64 / window_secs
            } else {
                0.0
            };

            // Send data
            let runtime = stream.runtime.clone();

            let stream = stream.clone();
            runtime.spawn(async move {
                stream
                    .try_send_packet(
                        OutboundPacket::Stats(StreamerStatsUpdate::Video {
                            host_processing_latency: has_host_processing_latency.then_some(
                                StatsHostProcessingLatency {
                                    min_host_processing_latency_ms: min_host_processing_latency
                                        .as_secs_f64()
                                        * 1000.0,
                                    max_host_processing_latency_ms: max_host_processing_latency
                                        .as_secs_f64()
                                        * 1000.0,
                                    avg_host_processing_latency_ms: avg_host_processing_latency
                                        .as_secs_f64()
                                        * 1000.0,
                                },
                            ),
                            min_streamer_processing_time_ms: min_streamer_processing_time
                                .as_secs_f64()
                                * 1000.0,
                            max_streamer_processing_time_ms: max_streamer_processing_time
                                .as_secs_f64()
                                * 1000.0,
                            avg_streamer_processing_time_ms: avg_streamer_processing_time
                                .as_secs_f64()
                                * 1000.0,
                            streamer_output_fps,
                        }),
                        "host / streamer processing latency",
                        false,
                    )
                    .await;

                // Send RTT info
                let ml_stream_lock = stream.stream.read().await;
                if let Some(ml_stream) = ml_stream_lock.as_ref() {
                    let rtt = ml_stream.estimated_rtt_info();
                    drop(ml_stream_lock);

                    match rtt {
                        Ok(EstimatedRttInfo { rtt, rtt_variance }) => {
                            stream
                                .try_send_packet(
                                    OutboundPacket::Stats(StreamerStatsUpdate::Rtt {
                                        rtt_ms: rtt.as_secs_f64() * 1000.0,
                                        rtt_variance_ms: rtt_variance.as_secs_f64() * 1000.0,
                                    }),
                                    "estimated rtt info",
                                    false,
                                )
                                .await;
                        }
                        Err(err) => {
                            warn!("failed to get estimated rtt info: {err:?}");
                        }
                    };
                }
            });

            // Clear data
            self.min_host_processing_latency = Duration::MAX;
            self.max_host_processing_latency = Duration::ZERO;
            self.total_host_processing_latency = Duration::ZERO;
            self.host_processing_frame_count = 0;
            self.min_streamer_processing_time = Duration::MAX;
            self.max_streamer_processing_time = Duration::ZERO;
            self.total_streamer_processing_time = Duration::ZERO;
            self.streamer_processing_time_frame_count = 0;

            self.last_send = Some(now);
        }
    }
}
