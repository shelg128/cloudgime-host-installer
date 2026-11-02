//! Video egress control: the host-side answer to "encoder produced 6 FPS, so the
//! client saw 1.5 s of empty screen".
//!
//! Three observables drive this module, all measured on the host egress path:
//!   1. egress FPS  — access units actually *started* on RTP egress (not requested FPS)
//!   2. egress backlog — time the oldest queued access unit has been waiting
//!   3. queue rejects — access units refused because the queue was full
//!
//! Policy (deliberately conservative, no billing/route/codec authority):
//!   * Backpressure never asks for a keyframe. A queue reject means the transport
//!     is behind, so a fresh IDR would compete with the frames already waiting and
//!     re-enter the drop -> IDR -> drop spiral that produced ~1 frame/second.
//!     Keyframes remain owned by the reliability path (PLI/NACK/packetizer errors).
//!   * When egress stays far below the requested rate, issue ONE guarded keyframe
//!     request. The request carries a rate limiter so a stalled encoder cannot turn
//!     into a keyframe storm.
//!
//! Everything here is pure state plus a clock. No I/O, so it is unit testable
//! without a GPU, a network, or a running host.

use std::time::{Duration, Instant};

/// Egress FPS measurement window. Multiple independent (video/audio) track worker
/// loops assert this; only one should log per window.
const SAMPLE_WINDOW: Duration = Duration::from_secs(5);

/// Below this share of requested FPS the egress is treated as degraded.
const DEGRADED_FPS_FRACTION: f64 = 0.60;

/// Backlog that already costs the client visible latency.
const BACKLOG_WARN: Duration = Duration::from_millis(500);

/// A guarded keyframe request needs this much consecutive degraded evidence.
const KEYFRAME_CONFIRMATION_WINDOWS: u32 = 2;

const KEYFRAME_MIN_INTERVAL: Duration = Duration::from_secs(5);
const KEYFRAME_MAX_INTERVAL: Duration = Duration::from_secs(60);

/// Rejection streaks longer than this are treated as structural, not transient.
const MAX_STALLED_STREAK_LOGGED: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EgressVerdict {
    /// Throughput matches the requested rate.
    Healthy,
    /// Window not representative (no target, nothing attempted, too short).
    NoData,
    /// Frames are being attempted but the transport is behind them.
    Backpressure,
    /// The encoder is not producing at the requested rate at all.
    EncoderStall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyframeDecision {
    /// Do not touch the keyframe path.
    None,
    /// Request exactly one IDR now.
    RequestIdr,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EgressAssessment {
    pub verdict: EgressVerdict,
    pub egress_fps: f64,
    pub target_fps: Option<f64>,
    pub queue_rejects: u64,
    pub max_backlog: Duration,
    pub keyframe: KeyframeDecision,
    /// Rejection streak length after this window (0 when the window was clean).
    pub reject_streak: u32,
}

/// Host-side egress control state. One instance per video track.
pub(crate) struct VideoEgressControl {
    target_fps: Option<f64>,
    window_started: Instant,
    frames_attempted: u64,
    frames_sent: u64,
    queue_rejects: u64,
    total_queue_rejects: u64,
    max_backlog: Duration,
    reject_streak: u32,
    degraded_windows: u32,
    last_keyframe_request: Option<Instant>,
    keyframe_interval: Duration,
}

impl Default for VideoEgressControl {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoEgressControl {
    pub(crate) fn new() -> Self {
        Self {
            target_fps: None,
            window_started: Instant::now(),
            frames_attempted: 0,
            frames_sent: 0,
            queue_rejects: 0,
            total_queue_rejects: 0,
            max_backlog: Duration::ZERO,
            reject_streak: 0,
            degraded_windows: 0,
            last_keyframe_request: None,
            keyframe_interval: KEYFRAME_MIN_INTERVAL,
        }
    }

    /// Requested output rate from the host stream setup. `None`/0 disables FPS
    /// based decisions so a missing redraw rate cannot trigger spurious keyframes.
    pub(crate) fn set_target_fps(&mut self, redraw_rate: u32) {
        self.target_fps = (redraw_rate > 0).then_some(redraw_rate as f64);
    }

    /// A new access unit was accepted and is now waiting for RTP egress.
    pub(crate) fn on_frame_queued(&mut self) {
        self.frames_attempted = self.frames_attempted.saturating_add(1);
    }

    /// A queued access unit finished its RTP egress.
    pub(crate) fn on_frame_sent(&mut self) {
        self.frames_sent = self.frames_sent.saturating_add(1);
    }

    /// The queue refused the access unit because the sender is behind.
    ///
    /// Deliberately does **not** set `needs_idr`: see the module docs.
    pub(crate) fn on_queue_reject(&mut self) {
        self.queue_rejects = self.queue_rejects.saturating_add(1);
        self.total_queue_rejects = self.total_queue_rejects.saturating_add(1);
        self.reject_streak = self.reject_streak.saturating_add(1);
    }

    /// Record how long the just-dequeued access unit waited before egress.
    pub(crate) fn on_backlog(&mut self, waited: Duration) {
        if waited > self.max_backlog {
            self.max_backlog = waited;
        }
    }

    pub(crate) fn total_queue_rejects(&self) -> u64 {
        self.total_queue_rejects
    }

    /// Close the current window and decide whether to ask for a keyframe.
    ///
    /// `keyframe_rate_limited` is true when the reliability path already asked for
    /// an IDR recently, in which case the control will not stack another request.
    pub(crate) fn finish_window(
        &mut self,
        now: Instant,
        keyframe_rate_limited: bool,
    ) -> Option<EgressAssessment> {
        let window = now.saturating_duration_since(self.window_started);
        if window < SAMPLE_WINDOW {
            return None;
        }

        let attempted = std::mem::take(&mut self.frames_attempted);
        let sent = std::mem::take(&mut self.frames_sent);
        let rejects = std::mem::take(&mut self.queue_rejects);
        let max_backlog = std::mem::take(&mut self.max_backlog);
        self.window_started = now;

        let egress_fps = sent as f64 / window.as_secs_f64();
        let target = self.target_fps;

        let verdict = if attempted == 0 {
            EgressVerdict::NoData
        } else if target.is_none() {
            EgressVerdict::NoData
        } else if rejects > 0 || max_backlog >= BACKLOG_WARN {
            EgressVerdict::Backpressure
        } else if egress_fps < target.unwrap_or_default() * DEGRADED_FPS_FRACTION {
            EgressVerdict::EncoderStall
        } else {
            EgressVerdict::Healthy
        };

        if rejects == 0 {
            self.reject_streak = 0;
        }

        let keyframe = match verdict {
            EgressVerdict::EncoderStall => {
                self.degraded_windows = self.degraded_windows.saturating_add(1);
                self.maybe_request_keyframe(now, keyframe_rate_limited)
            }
            _ => {
                self.degraded_windows = 0;
                KeyframeDecision::None
            }
        };

        Some(EgressAssessment {
            verdict,
            egress_fps,
            target_fps: target,
            queue_rejects: rejects,
            max_backlog,
            keyframe,
            reject_streak: self.reject_streak.min(MAX_STALLED_STREAK_LOGGED),
        })
    }

    fn maybe_request_keyframe(
        &mut self,
        now: Instant,
        keyframe_rate_limited: bool,
    ) -> KeyframeDecision {
        if keyframe_rate_limited || self.degraded_windows < KEYFRAME_CONFIRMATION_WINDOWS {
            return KeyframeDecision::None;
        }
        if let Some(last) = self.last_keyframe_request {
            if now.saturating_duration_since(last) < self.keyframe_interval {
                return KeyframeDecision::None;
            }
        }
        self.last_keyframe_request = Some(now);
        // Exponential backoff keeps a wedged encoder from being flooded by IDRs,
        // which are the most expensive frames to produce.
        self.keyframe_interval = (self.keyframe_interval * 2).min(KEYFRAME_MAX_INTERVAL);
        KeyframeDecision::RequestIdr
    }

    /// Called when egress recovered so a later stall starts from a short interval.
    pub(crate) fn on_recovered(&mut self) {
        self.degraded_windows = 0;
        self.keyframe_interval = KEYFRAME_MIN_INTERVAL;
    }
}

/// Emit one line per window so host-side blame does not depend on client logs.
/// Backpressure and encoder stall are warnings; healthy windows stay at info.
pub(crate) fn log_assessment(target: &str, a: &EgressAssessment) {
    let target_fps = a
        .target_fps
        .map(|f| format!("{f:.1}"))
        .unwrap_or_else(|| "-".to_string());
    match a.verdict {
        EgressVerdict::Backpressure => log::warn!(
            "[Stream]: egress backpressure {target} fps={:.1}/{target_fps} rejects={} backlog_ms={} streak={} keyframe={:?}",
            a.egress_fps,
            a.queue_rejects,
            a.max_backlog.as_millis(),
            a.reject_streak,
            a.keyframe
        ),
        EgressVerdict::EncoderStall => log::warn!(
            "[Stream]: egress encoder_stall {target} fps={:.1}/{target_fps} rejects={} backlog_ms={} keyframe={:?}",
            a.egress_fps,
            a.queue_rejects,
            a.max_backlog.as_millis(),
            a.keyframe
        ),
        EgressVerdict::Healthy | EgressVerdict::NoData => log::info!(
            "[Stream]: egress {target} verdict={:?} fps={:.1}/{target_fps} rejects={} backlog_ms={}",
            a.verdict,
            a.egress_fps,
            a.queue_rejects,
            a.max_backlog.as_millis()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(target: f64) -> VideoEgressControl {
        let mut c = VideoEgressControl::new();
        c.set_target_fps(target as u32);
        c.window_started = Instant::now();
        c
    }

    fn advance(c: &mut VideoEgressControl, window: Duration) -> Instant {
        c.window_started + window
    }

    fn sent(c: &mut VideoEgressControl, n: u64) {
        for _ in 0..n {
            c.on_frame_queued();
            c.on_frame_sent();
        }
    }

    /// `finish_window(advance(&mut c, W), ..)` borrows `c` twice; advance first.
    macro_rules! close_window {
        ($c:expr, $rate_limited:expr) => {{
            let now = advance(&mut $c, SAMPLE_WINDOW);
            $c.finish_window(now, $rate_limited)
        }};
    }

    #[test]
    fn healthy_throughput_does_not_request_keyframes() {
        let mut c = control(30.0);
        sent(&mut c, 150); // 30 fps over 5 s
        let a = close_window!(c, false).expect("window closed");
        assert_eq!(a.verdict, EgressVerdict::Healthy);
        assert_eq!(a.keyframe, KeyframeDecision::None);
        assert!((a.egress_fps - 30.0).abs() < 0.01);
    }

    #[test]
    fn stalled_encoder_requests_exactly_one_guarded_keyframe() {
        let mut c = control(30.0);
        sent(&mut c, 35); // ~7 fps, observed production shape
        let first = close_window!(c, false).unwrap();
        assert_eq!(first.verdict, EgressVerdict::EncoderStall);
        assert_eq!(
            first.keyframe,
            KeyframeDecision::None,
            "one degraded window is not enough evidence"
        );

        sent(&mut c, 35);
        let second = close_window!(c, false).unwrap();
        assert_eq!(second.keyframe, KeyframeDecision::RequestIdr);

        // Immediate third window must be blocked by the interval, not stack IDRs.
        sent(&mut c, 35);
        let third = close_window!(c, false).unwrap();
        assert_eq!(third.keyframe, KeyframeDecision::None);
    }

    #[test]
    fn keyframe_backoff_grows_and_caps() {
        let mut c = control(30.0);
        // Intervals double from 5 s and cap at 60 s: 10, 20, 40, 60.
        let mut observed = Vec::new();
        for _ in 0..4 {
            // Two consecutive degraded windows to clear the confirmation gate and
            // the current interval, which triggers one keyframe request.
            sent(&mut c, 35);
            let _ = close_window!(c, false);
            sent(&mut c, 35);
            let first = close_window!(c, false).unwrap();
            assert_eq!(first.keyframe, KeyframeDecision::RequestIdr);
            observed.push(c.keyframe_interval);

            // Spend exactly the granted interval so the next request is allowed.
            let wait = c.keyframe_interval;
            let jump = c.window_started + wait;
            let _ = c.finish_window(jump, false);
        }
        assert_eq!(
            observed,
            vec![
                KEYFRAME_MIN_INTERVAL * 2,
                KEYFRAME_MIN_INTERVAL * 4,
                KEYFRAME_MIN_INTERVAL * 8,
                KEYFRAME_MAX_INTERVAL
            ]
        );
        assert_eq!(c.keyframe_interval, KEYFRAME_MAX_INTERVAL);
    }

    #[test]
    fn queue_reject_is_backpressure_and_never_forces_idr() {
        let mut c = control(30.0);
        c.on_frame_queued();
        c.on_frame_sent();
        c.on_queue_reject();
        let a = close_window!(c, false).unwrap();
        assert_eq!(a.verdict, EgressVerdict::Backpressure);
        assert_eq!(a.reject_streak, 1);
        assert_eq!(
            a.keyframe,
            KeyframeDecision::None,
            "a full queue must not add encoder work"
        );
    }

    #[test]
    fn external_keyframe_in_flight_blocks_stacked_request() {
        let mut c = control(30.0);
        sent(&mut c, 35);
        let _ = close_window!(c, false);
        // Second degraded window, but the reliability path already asked for an
        // IDR (PLI/NACK/packetizer error), so the control must stay silent.
        sent(&mut c, 35);
        let a = close_window!(c, true).unwrap();
        assert_eq!(a.verdict, EgressVerdict::EncoderStall);
        assert_eq!(a.keyframe, KeyframeDecision::None);
    }

    #[test]
    fn backlog_alone_flags_backpressure() {
        let mut c = control(30.0);
        sent(&mut c, 150);
        c.on_backlog(Duration::from_millis(900));
        let a = close_window!(c, false).unwrap();
        assert_eq!(a.verdict, EgressVerdict::Backpressure);
        assert_eq!(a.keyframe, KeyframeDecision::None);
    }

    #[test]
    fn missing_target_never_invents_a_keyframe() {
        let mut c = VideoEgressControl::new();
        c.set_target_fps(0);
        sent(&mut c, 3);
        let a = close_window!(c, false).unwrap();
        assert_eq!(a.verdict, EgressVerdict::NoData);
        assert_eq!(a.keyframe, KeyframeDecision::None);
    }

    #[test]
    fn short_window_is_ignored_and_counters_survive() {
        let mut c = control(30.0);
        sent(&mut c, 10);
        let short = advance(&mut c, Duration::from_secs(1));
        assert!(c.finish_window(short, false).is_none());
        // The counters survive the ignored window; 10 frames over the full 5 s
        // window is ~2 fps, which is below target by design and therefore reads
        // as an encoder stall, not a healthy stream.
        let a = close_window!(c, false).unwrap();
        assert_eq!(a.verdict, EgressVerdict::EncoderStall);
        assert!((a.egress_fps - 2.0).abs() < 0.01);
    }

    #[test]
    fn reject_streak_is_bounded_and_resets_after_recovery() {
        let mut c = control(30.0);
        for _ in 0..20 {
            c.on_queue_reject();
        }
        let a = close_window!(c, false).unwrap();
        assert_eq!(a.reject_streak, MAX_STALLED_STREAK_LOGGED);
        sent(&mut c, 150);
        let b = close_window!(c, false).unwrap();
        assert_eq!(b.reject_streak, 0);
        assert_eq!(c.total_queue_rejects(), 20);
    }

    #[test]
    fn recovery_resets_backoff() {
        let mut c = control(30.0);
        sent(&mut c, 35);
        let _ = close_window!(c, false);
        sent(&mut c, 35);
        let _ = close_window!(c, false);
        assert_eq!(c.keyframe_interval, KEYFRAME_MIN_INTERVAL * 2);
        c.on_recovered();
        assert_eq!(c.keyframe_interval, KEYFRAME_MIN_INTERVAL);
        assert_eq!(c.degraded_windows, 0);
    }
}
