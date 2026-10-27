//! Metadata-only QA correlation. No payload copies, timestamps/packet mutation or recovery actions.
use bytes::BytesMut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct AccessUnitMeta {
    pub nals: usize,
    pub bytes: usize,
    pub idr: usize,
    pub sps: usize,
    pub pps: usize,
    pub slices: usize,
    pub sei: usize,
    pub aud: usize,
}
pub(super) fn summarize_h264(samples: &[BytesMut]) -> AccessUnitMeta {
    let mut m = AccessUnitMeta::default();
    for s in samples {
        m.nals += 1;
        m.bytes += s.len();
        match s.first().map(|b| b & 31) {
            Some(5) => {
                m.idr += 1;
                m.slices += 1;
            }
            Some(1..=4) => m.slices += 1,
            Some(7) => m.sps += 1,
            Some(8) => m.pps += 1,
            Some(6) => m.sei += 1,
            Some(9) => m.aud += 1,
            _ => {}
        }
    }
    m
}
struct Window {
    began: Instant,
    last: Option<Instant>,
    events: u32,
    units: u64,
    idrs: u64,
    rejects: u64,
}
pub(super) struct FrameProbe {
    pli: AtomicU64,
    window: Mutex<Window>,
}
impl FrameProbe {
    pub fn new() -> Self {
        Self {
            pli: AtomicU64::new(0),
            window: Mutex::new(Window {
                began: Instant::now(),
                last: None,
                events: 0,
                units: 0,
                idrs: 0,
                rejects: 0,
            }),
        }
    }
    pub fn pli(&self) {
        self.pli.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record(
        &self,
        meta: AccessUnitMeta,
        timestamp: u32,
        important: bool,
        packets: usize,
        markers: usize,
        outcome: &'static str,
    ) {
        let now = Instant::now();
        let Ok(mut w) = self.window.lock() else {
            return;
        };
        w.units += 1;
        if meta.idr > 0 {
            w.idrs += 1;
        }
        if outcome != "queued" {
            w.rejects += 1;
        }
        let key = meta.idr > 0 || outcome != "queued";
        if !should_log(
            w.events,
            now.duration_since(w.began),
            w.last.map(|t| now.duration_since(t)),
            key,
        ) {
            return;
        }
        w.events += 1;
        w.last = Some(now);
        tracing::info!(
            "CG_FRAME_QA au={} idrTotal={} rejectTotal={} pliTotal={} ts={} important={} nals={} bytes={} idr={} sps={} pps={} slices={} sei={} aud={} packets={} markers={} outcome={}",
            w.units,
            w.idrs,
            w.rejects,
            self.pli.load(Ordering::Relaxed),
            timestamp,
            important,
            meta.nals,
            meta.bytes,
            meta.idr,
            meta.sps,
            meta.pps,
            meta.slices,
            meta.sei,
            meta.aud,
            packets,
            markers,
            outcome
        );
    }
}
fn should_log(events: u32, elapsed: Duration, since: Option<Duration>, key: bool) -> bool {
    events < 400
        && elapsed < Duration::from_secs(600)
        && since.is_none_or(|s| s >= Duration::from_millis(if key { 100 } else { 5000 }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_metadata_without_mutating_payload() {
        let s = vec![
            BytesMut::from(&[0x67, 1][..]),
            BytesMut::from(&[0x68, 2][..]),
            BytesMut::from(&[0x65, 3][..]),
            BytesMut::from(&[0x65, 4][..]),
            BytesMut::from(&[0x09, 0xf0][..]),
        ];
        let before = s.clone();
        let m = summarize_h264(&s);
        assert_eq!(
            (m.nals, m.bytes, m.idr, m.sps, m.pps, m.slices, m.aud),
            (5, 10, 2, 1, 1, 2, 1)
        );
        assert_eq!(s, before);
    }
    #[test]
    fn handles_empty_delta_and_non_video_nals() {
        let m = summarize_h264(&[
            BytesMut::new(),
            BytesMut::from(&[0x61][..]),
            BytesMut::from(&[0x06][..]),
        ]);
        assert_eq!((m.nals, m.idr, m.slices, m.sei), (3, 0, 1, 1));
    }
    #[test]
    fn bounded_in_time_count_and_frequency() {
        assert!(should_log(0, Duration::ZERO, None, false));
        assert!(!should_log(400, Duration::ZERO, None, true));
        assert!(!should_log(0, Duration::from_secs(600), None, true));
        assert!(!should_log(
            1,
            Duration::from_secs(1),
            Some(Duration::from_millis(99)),
            true
        ));
        assert!(should_log(
            1,
            Duration::from_secs(1),
            Some(Duration::from_millis(100)),
            true
        ));
        assert!(!should_log(
            1,
            Duration::from_secs(1),
            Some(Duration::from_millis(100)),
            false
        ));
    }
    #[test]
    fn pli_is_passive_counter() {
        let p = FrameProbe::new();
        p.pli();
        p.pli();
        assert_eq!(p.pli.load(Ordering::Relaxed), 2);
        assert_eq!(p.window.lock().unwrap().events, 0);
    }
}
