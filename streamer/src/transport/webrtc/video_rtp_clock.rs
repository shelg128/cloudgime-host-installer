//! One timeline per retained WebRTC video track, independent of encoder setup.
#[derive(Default)]
pub(super) struct VideoRtpClock {
    previous_source: Option<u64>,
    previous_monotonic: u64,
    previous_output: u64,
}
impl VideoRtpClock {
    pub fn map(&mut self, source_ticks: u64, monotonic_ticks: u64) -> u32 {
        let output=match self.previous_source {
            None=>source_ticks,
            Some(previous) if source_ticks>previous=>self.previous_output.wrapping_add(source_ticks-previous),
            Some(_)=>self.previous_output.wrapping_add(monotonic_ticks.saturating_sub(self.previous_monotonic).max(1)),
        };
        self.previous_source=Some(source_ticks);
        self.previous_monotonic=monotonic_ticks;
        self.previous_output=output;
        output as u32
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn observed_restart_never_rewinds_retained_track(){let mut c=VideoRtpClock::default();let a=c.map(4_375_220,100_000);let b=c.map(255_946,370_000);assert_eq!(b.wrapping_sub(a),270_000);assert_eq!(c.map(258_946,373_000).wrapping_sub(b),3_000);}
    #[test] fn normal_capture_deltas_remain_exact(){let mut c=VideoRtpClock::default();assert_eq!(c.map(17_000,0),17_000);assert_eq!(c.map(20_000,8_000),20_000);assert_eq!(c.map(23_100,12_000),23_100);}
    #[test] fn rtp_u32_wrap_is_natural(){let mut c=VideoRtpClock::default();let a=c.map(u32::MAX as u64-100,1);let b=c.map(u32::MAX as u64+2_900,3_001);assert_eq!(b.wrapping_sub(a),3_000);}
    #[test] fn repeated_restarts_preserve_offsets(){let mut c=VideoRtpClock::default();c.map(5_000_000,0);let b=c.map(1_000,9_000);let d=c.map(4_000,12_000);let e=c.map(0,21_000);assert_eq!(d.wrapping_sub(b),3_000);assert_eq!(e.wrapping_sub(d),9_000);assert_eq!(c.map(3_000,24_000).wrapping_sub(e),3_000);}
    #[test] fn repeated_source_timestamp_still_advances(){let mut c=VideoRtpClock::default();let a=c.map(100,1);let b=c.map(100,1);assert_eq!(b.wrapping_sub(a),1);}
    #[test] fn new_tracks_have_independent_timeline(){let mut a=VideoRtpClock::default();a.map(8_000,0);a.map(0,3_000);let mut b=VideoRtpClock::default();assert_eq!(b.map(100,0),100);}
}
