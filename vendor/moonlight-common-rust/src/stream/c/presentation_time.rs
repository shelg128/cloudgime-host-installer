//! Unit boundary for Limelight DECODE_UNIT::presentationTimeUs.
//!
//! Limelight has already converted the original 90 kHz RTP clock to
//! microseconds before constructing the decode unit. Applying a second
//! 90 kHz conversion here stretches media time by 1000/90 (~11.11x).
//! Keep this module free of FFI/dependencies so its arithmetic can be tested
//! with rustc directly as well as through the crate's normal test build.
use std::time::Duration;

pub(super) fn from_microseconds(presentation_time_us: u64) -> Duration {
    Duration::from_micros(presentation_time_us)
}

#[cfg(test)]
mod tests {
    use super::from_microseconds;
    use std::time::Duration;

    // Same clock conversion as streamer/src/transport/webrtc/video.rs.
    fn outbound_rtp_ticks(time: Duration) -> u32 {
        (time.as_nanos() * 90_000 / 1_000_000_000) as u32
    }

    #[test]
    fn zero_timestamp_stays_zero() {
        assert_eq!(from_microseconds(0), Duration::ZERO);
    }

    #[test]
    fn one_second_is_not_eleven_seconds() {
        assert_eq!(from_microseconds(1_000_000), Duration::from_secs(1));
        assert_eq!(outbound_rtp_ticks(from_microseconds(1_000_000)), 90_000);
    }

    #[test]
    fn fractional_microseconds_are_preserved() {
        assert_eq!(from_microseconds(16_667).as_nanos(), 16_667_000);
        assert_eq!(from_microseconds(33_333).as_nanos(), 33_333_000);
    }

    #[test]
    fn timestamp_is_presentation_time_not_rtp_field() {
        assert_eq!(from_microseconds(90_000), Duration::from_millis(90));
        assert_eq!(outbound_rtp_ticks(from_microseconds(90_000)), 8_100);
    }

    #[test]
    fn thirty_fps_keeps_thirty_frames_per_media_second() {
        let pts: Vec<_> = (0..=30).map(|n| from_microseconds(n * 1_000_000 / 30)).collect();
        assert_eq!(pts[30] - pts[0], Duration::from_secs(1));
        for pair in pts.windows(2) {
            let step = outbound_rtp_ticks(pair[1]).wrapping_sub(outbound_rtp_ticks(pair[0]));
            assert!((2_999..=3_001).contains(&step), "invalid 30fps RTP step: {step}");
        }
    }

    #[test]
    fn sixty_fps_keeps_sixty_frames_per_media_second() {
        let pts: Vec<_> = (0..=60).map(|n| from_microseconds(n * 1_000_000 / 60)).collect();
        assert_eq!(pts[60] - pts[0], Duration::from_secs(1));
        for pair in pts.windows(2) {
            let step = outbound_rtp_ticks(pair[1]).wrapping_sub(outbound_rtp_ticks(pair[0]));
            assert!((1_499..=1_501).contains(&step), "invalid 60fps RTP step: {step}");
        }
    }

    #[test]
    fn native_c_to_rust_to_rtp_roundtrip_loses_at_most_one_tick() {
        // RtpVideoQueue.c: presentationTimeUs = (timestamp * 1000) / 90.
        for ticks in [0u32, 1, 90, 1_500, 3_000, 90_000, 4_000_000_000, u32::MAX] {
            let time_us = (u64::from(ticks) * 1000) / 90;
            let back = outbound_rtp_ticks(from_microseconds(time_us));
            assert!(back <= ticks);
            assert!(ticks - back <= 1, "lost more than precision rounding for {ticks}");
        }
    }

    #[test]
    fn long_rental_avoids_old_u64_nanosecond_multiplication_overflow() {
        let six_hours_us = 6 * 3600 * 1_000_000;
        assert_eq!(from_microseconds(six_hours_us), Duration::from_secs(6 * 3600));
        assert_eq!(from_microseconds(u64::MAX).as_micros(), u128::from(u64::MAX));
    }

    #[test]
    fn output_wraps_only_at_rtp_packetizer_boundary() {
        let before_us = (u64::from(u32::MAX) * 1000) / 90;
        let before = from_microseconds(before_us);
        let after = from_microseconds(before_us + 1_000_000);
        assert!(after > before);
        assert_eq!(after - before, Duration::from_secs(1));
        assert_eq!(outbound_rtp_ticks(after).wrapping_sub(outbound_rtp_ticks(before)), 90_000);
    }

    #[test]
    fn repeated_and_reset_source_timestamps_are_not_artificially_retimed() {
        assert_eq!(from_microseconds(123_456), from_microseconds(123_456));
        assert_eq!(from_microseconds(0), Duration::ZERO);
        assert!(from_microseconds(1) < from_microseconds(123_456));
    }
}
