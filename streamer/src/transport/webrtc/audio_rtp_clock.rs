//! Track-owned Opus RTP timeline. Source setup must not rewind a retained SSRC.
//! RTP Opus uses 48kHz irrespective of the PCM input sample rate (RFC7587).
#[derive(Debug, Default)]
pub(super) struct AudioRtpClock {
    packet_ticks: Option<u32>,
    next: u32,
    previous: Option<(u32, u64)>,
    restart_pending: bool,
}

fn packet_ticks(sample_rate: u32, samples: u32) -> Result<u32, &'static str> {
    if !matches!(sample_rate, 8000 | 12000 | 16000 | 24000 | 48000) {
        return Err("unsupported_opus_sample_rate");
    }
    let numerator = u64::from(samples) * 48_000;
    if numerator == 0 || numerator % u64::from(sample_rate) != 0 {
        return Err("non_integral_opus_duration");
    }
    let ticks = numerator / u64::from(sample_rate);
    // Opus packet durations: integral 2.5ms, no longer than 120ms.
    if ticks > 5_760 || ticks % 120 != 0 {
        return Err("invalid_opus_packet_duration");
    }
    Ok(ticks as u32)
}

impl AudioRtpClock {
    pub fn configure(&mut self, rate: u32, samples: u32) -> Result<(), &'static str> {
        // Invalid setup must stop delivery until a valid setup succeeds.
        self.packet_ticks = None;
        self.restart_pending = true;
        self.packet_ticks = Some(packet_ticks(rate, samples)?);
        Ok(())
    }

    /// `monotonic_ticks` is track-relative elapsed time scaled to 48kHz.
    /// Consult arrival time only for the first packet after explicit setup.
    pub fn stamp(&mut self, monotonic_ticks: u64) -> Option<u32> {
        let step = self.packet_ticks?;
        if self.restart_pending {
            if let Some((previous, at)) = self.previous {
                let elapsed = monotonic_ticks.saturating_sub(at);
                let normal_delta = self.next.wrapping_sub(previous) as u64;
                // Avoid backwards/ambiguous (> half a u32 cycle) RTP jumps.
                if elapsed < (1u64 << 31) && elapsed > normal_delta {
                    self.next = previous.wrapping_add(elapsed as u32);
                }
            }
            self.restart_pending = false;
        }
        let stamp = self.next;
        self.next = self.next.wrapping_add(step);
        self.previous = Some((stamp, monotonic_ticks));
        Some(stamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_track_reconfiguration_does_not_restart_at_zero() {
        let mut c=AudioRtpClock::default(); c.configure(48000,240).unwrap();
        assert_eq!(c.stamp(0),Some(0));assert_eq!(c.stamp(240),Some(240));
        c.configure(48000,240).unwrap();assert_eq!(c.stamp(480),Some(480));
    }
    #[test]
    fn restart_preserves_real_gap_but_normal_jitter_does_not_retime_samples() {
        let mut c=AudioRtpClock::default();c.configure(48000,240).unwrap();
        c.stamp(0);let a=c.stamp(240).unwrap();c.configure(48000,240).unwrap();
        let b=c.stamp(96240).unwrap();assert_eq!(b.wrapping_sub(a),96000);
        assert_eq!(c.stamp(200000).unwrap().wrapping_sub(b),240);
    }
    #[test]
    fn rates_use_the_same_48khz_clock() {
        for rate in [8000,12000,16000,24000,48000] {
            let mut c=AudioRtpClock::default();c.configure(rate,rate/200).unwrap();
            assert_eq!(c.stamp(0),Some(0));assert_eq!(c.stamp(1000),Some(240));
        }
    }
    #[test]
    fn wrap_is_natural_and_setup_preserves_it() {
        let mut c=AudioRtpClock::default();c.configure(48000,240).unwrap();
        c.next=u32::MAX-100;let a=c.stamp(0).unwrap();c.configure(48000,240).unwrap();
        assert_eq!(c.stamp(240).unwrap().wrapping_sub(a),240);
    }
    #[test]
    fn packet_duration_change_does_not_move_prior_boundary() {
        let mut c=AudioRtpClock::default();c.configure(48000,240).unwrap();c.stamp(0);
        c.configure(48000,960).unwrap();assert_eq!(c.stamp(240),Some(240));
        assert_eq!(c.stamp(500),Some(1200));
    }
    #[test]
    fn invalid_setup_stops_audio_without_resetting_track() {
        let mut c=AudioRtpClock::default();c.configure(48000,240).unwrap();c.stamp(0);
        for (rate,samples) in [(0,240),(80000,400),(48000,0),(48000,119),(48000,6000),(12000,31)] {
            assert!(c.configure(rate,samples).is_err());assert_eq!(c.stamp(100),None);
        }
        c.configure(48000,240).unwrap();assert_eq!(c.stamp(240),Some(240));
    }
    #[test]
    fn new_tracks_start_independently() {
        let mut a=AudioRtpClock::default();a.configure(48000,240).unwrap();a.stamp(0);a.stamp(240);
        let mut b=AudioRtpClock::default();b.configure(48000,240).unwrap();assert_eq!(b.stamp(9000),Some(0));
    }
    #[test]
    fn repeated_setup_without_packet_does_not_add_synthetic_delay() {
        let mut c=AudioRtpClock::default();c.configure(48000,240).unwrap();c.stamp(0);
        c.configure(48000,240).unwrap();c.configure(48000,240).unwrap();assert_eq!(c.stamp(240),Some(240));
    }
}
