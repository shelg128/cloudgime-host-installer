use std::time::Duration;

/// Limelight DECODE_UNIT.presentationTimeUs is already in microseconds.
/// Do not scale it as the separate 90 kHz rtpTimestamp field: doing so makes
/// the outgoing WebRTC clock run at 1 MHz instead of 90 kHz.
pub(super) fn presentation_timestamp(presentation_time_us: u64) -> Duration {
    Duration::from_micros(presentation_time_us)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn outgoing_rtp(us: u64) -> u32 {
        (presentation_timestamp(us).as_nanos() * 90_000 / 1_000_000_000) as u32
    }
    #[test]
    fn one_second_is_ninety_thousand_rtp_ticks() {
        assert_eq!(presentation_timestamp(1_000_000), Duration::from_secs(1));
        assert_eq!(outgoing_rtp(1_000_000), 90_000);
    }
    #[test]
    fn frame_intervals_preserve_wall_clock_scale() {
        assert_eq!(outgoing_rtp(33_333), 2_999);
        assert_eq!(outgoing_rtp(16_667), 1_500);
        assert_eq!(outgoing_rtp(5_000_000), 450_000);
    }
    #[test]
    fn zero_is_zero() { assert_eq!(outgoing_rtp(0), 0); }
    #[test]
    fn long_sessions_do_not_overflow_intermediate_arithmetic() {
        let us = 48 * 60 * 60 * 1_000_000_u64;
        assert_eq!(presentation_timestamp(us), Duration::from_secs(172_800));
        assert_eq!(outgoing_rtp(us), (172_800_u64 * 90_000) as u32);
    }
    #[test]
    fn full_ffi_range_is_representable_without_multiplication_overflow() {
        assert_eq!(presentation_timestamp(u64::MAX).as_micros(), u64::MAX as u128);
    }
}
