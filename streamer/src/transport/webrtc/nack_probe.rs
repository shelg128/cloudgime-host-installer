//! Bounded diagnostics: no payload, endpoint, credential or absolute timestamp stored.
use std::time::{Duration, Instant};
const SLOTS: usize = 4096;
const MAX_AGE: Duration = Duration::from_secs(10);
#[derive(Clone)]
struct Sent { sequence: u16, at: Instant, bucket: usize, nacked: bool }
#[derive(Default, Debug)]
pub(super) struct Window {
    pub sent: [u64; 3], pub first_nacks: [u64; 3], pub repeats: u64,
    pub unknown: u64, pub max_age_ms: u64,
}
pub(super) struct NackProbe { slots: Vec<Option<Sent>>, window: Window }
impl Default for NackProbe {
    fn default() -> Self { Self { slots: vec![None; SLOTS], window: Window::default() } }
}
impl NackProbe {
    pub fn sent(&mut self, sequence: u16, bytes: usize, now: Instant) {
        let bucket = if bytes <= 300 { 0 } else if bytes <= 800 { 1 } else { 2 };
        self.slots[sequence as usize % SLOTS] = Some(Sent { sequence, at: now, bucket, nacked: false });
        self.window.sent[bucket] += 1;
    }
    pub fn nack(&mut self, sequence: u16, now: Instant) {
        let Some(entry) = self.slots[sequence as usize % SLOTS].as_mut() else { self.window.unknown += 1; return; };
        if entry.sequence != sequence || now.saturating_duration_since(entry.at) > MAX_AGE {
            self.window.unknown += 1; return;
        }
        if entry.nacked { self.window.repeats += 1; }
        else { self.window.first_nacks[entry.bucket] += 1; entry.nacked = true; }
        self.window.max_age_ms = self.window.max_age_ms.max(now.saturating_duration_since(entry.at).as_millis() as u64);
    }
    pub fn take_window(&mut self) -> Window { std::mem::take(&mut self.window) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn bins_track_first_loss_not_repeated_feedback() {
        let mut p = NackProbe::default(); let now = Instant::now();
        for (seq,bytes) in [(1,100),(2,700),(3,1188)] { p.sent(seq,bytes,now); p.nack(seq,now+Duration::from_millis(100)); }
        p.nack(3,now+Duration::from_millis(180)); let w=p.take_window();
        assert_eq!(w.sent,[1,1,1]); assert_eq!(w.first_nacks,[1,1,1]); assert_eq!(w.repeats,1); assert_eq!(w.max_age_ms,180);
    }
    #[test] fn collision_and_expired_feedback_cannot_attribute_to_another_packet() {
        let mut p=NackProbe::default(); let now=Instant::now();
        p.sent(1,100,now);p.sent(4097,1188,now);p.nack(1,now);p.nack(4097,now+Duration::from_secs(11));
        let w=p.take_window();assert_eq!(w.unknown,2);assert_eq!(w.first_nacks,[0,0,0]);
    }
    #[test] fn window_reset_does_not_recount_old_nacks() {
        let mut p=NackProbe::default();let now=Instant::now();p.sent(65535,10,now);p.nack(65535,now);p.take_window();p.nack(65535,now);
        let w=p.take_window();assert_eq!(w.sent,[0,0,0]);assert_eq!(w.first_nacks,[0,0,0]);assert_eq!(w.repeats,1);
        p.sent(65535,1188,now);p.nack(65535,now);assert_eq!(p.take_window().first_nacks,[0,0,1]);
    }
}
