//! Passive pre-responder metadata observer; never changes packets, tasks or timers.
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use webrtc::{
    interceptor::{
        self, Attributes, Interceptor, InterceptorBuilder, RTCPReader, RTCPWriter, RTPReader,
        RTPWriter, stream_info::StreamInfo,
    },
    rtcp::{
        packet::Packet as RtcpPacket, transport_feedbacks::transport_layer_nack::TransportLayerNack,
    },
    rtp::packet::Packet as RtpPacket,
};
type Result<T> = std::result::Result<T, interceptor::Error>;
const SLOTS: usize = 8192;
const MAX_TRACKS: usize = 4;
const MAX_EVENTS: u32 = 1800;
#[derive(Clone)]
struct Entry {
    seq: u16,
    ts: u32,
    first: Instant,
    last_nack: Option<Instant>,
    nacks: u32,
    writes: u32,
}
struct Window {
    slots: Vec<Option<Entry>>,
    logs: u32,
    began: Instant,
    nacks: u64,
    repeats: u64,
}
struct Probe {
    ssrc: u32,
    alive: AtomicBool,
    inner: Mutex<Window>,
}
impl Probe {
    fn new(ssrc: u32) -> Self {
        Self {
            ssrc,
            alive: AtomicBool::new(true),
            inner: Mutex::new(Window {
                slots: vec![None; SLOTS],
                logs: 0,
                began: Instant::now(),
                nacks: 0,
                repeats: 0,
            }),
        }
    }
    fn nack(&self, seq: u16, now: Instant) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut w) = self.inner.lock() else {
            return;
        };
        w.nacks += 1;
        let idx = seq as usize % SLOTS;
        let mut detail = None;
        if let Some(e) = w.slots[idx].as_mut() {
            if e.seq == seq && now.saturating_duration_since(e.first) < Duration::from_secs(10) {
                e.last_nack = Some(now);
                e.nacks += 1;
                detail = Some((e.ts, e.nacks));
            }
        }
        if let Some((ts, count)) = detail {
            Self::emit(&mut w, self.ssrc, "n", seq, ts, count, 0, 0);
        }
    }
    fn before_write(&self, p: &RtpPacket, now: Instant) -> Option<(u32, u64)> {
        if !self.alive.load(Ordering::Acquire) {
            return None;
        }
        let Ok(mut w) = self.inner.lock() else {
            return None;
        };
        let idx = p.header.sequence_number as usize % SLOTS;
        let old = w.slots[idx].as_mut();
        if let Some(e) = old {
            if e.seq == p.header.sequence_number && e.ts == p.header.timestamp {
                e.writes += 1;
                let n = e.nacks;
                let age = e
                    .last_nack
                    .map(|t| now.saturating_duration_since(t).as_micros() as u64);
                w.repeats += 1;
                return age.map(|a| (n, a));
            }
        }
        w.slots[idx] = Some(Entry {
            seq: p.header.sequence_number,
            ts: p.header.timestamp,
            first: now,
            last_nack: None,
            nacks: 0,
            writes: 1,
        });
        None
    }
    fn complete(&self, p: &RtpPacket, meta: Option<(u32, u64)>, elapsed: Duration, ok: bool) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        if let Some((n, age)) = meta {
            if let Ok(mut w) = self.inner.lock() {
                Self::emit(
                    &mut w,
                    self.ssrc,
                    if ok { "w" } else { "e" },
                    p.header.sequence_number,
                    p.header.timestamp,
                    n,
                    age,
                    elapsed.as_micros() as u64,
                );
            }
        }
    }
    fn emit(
        w: &mut Window,
        ssrc: u32,
        event: &str,
        seq: u16,
        ts: u32,
        n: u32,
        age: u64,
        write: u64,
    ) {
        if seq % 8 != 0 || w.logs >= MAX_EVENTS || w.began.elapsed() > Duration::from_secs(600) {
            return;
        }
        w.logs += 1;
        log::info!("CG_NQ e={event} s={ssrc} q={seq} t={ts} n={n} a={age} w={write}");
    }
}
#[derive(Default)]
pub(super) struct Builder;
impl InterceptorBuilder for Builder {
    fn build(&self, _: &str) -> Result<Arc<dyn Interceptor + Send + Sync>> {
        Ok(Arc::new(Observer {
            tracks: Arc::new(Mutex::new(HashMap::new())),
        }))
    }
}
struct Observer {
    tracks: Arc<Mutex<HashMap<u32, Arc<Probe>>>>,
}
struct Reader {
    next: Arc<dyn RTCPReader + Send + Sync>,
    tracks: Arc<Mutex<HashMap<u32, Arc<Probe>>>>,
}
#[async_trait]
impl RTCPReader for Reader {
    async fn read(
        &self,
        b: &mut [u8],
        a: &Attributes,
    ) -> Result<(Vec<Box<dyn RtcpPacket + Send + Sync>>, Attributes)> {
        let (p, attr) = self.next.read(b, a).await?;
        for packet in &p {
            if let Some(n) = packet.as_any().downcast_ref::<TransportLayerNack>() {
                let track = self
                    .tracks
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&n.media_ssrc).cloned());
                if let Some(t) = track {
                    for pair in &n.nacks {
                        for seq in *pair {
                            t.nack(seq, Instant::now());
                        }
                    }
                }
            }
        }
        Ok((p, attr))
    }
}
struct Writer {
    next: Arc<dyn RTPWriter + Send + Sync>,
    probe: Arc<Probe>,
}
#[async_trait]
impl RTPWriter for Writer {
    async fn write(&self, p: &RtpPacket, a: &Attributes) -> Result<usize> {
        let start = Instant::now();
        let meta = self.probe.before_write(p, start);
        let out = self.next.write(p, a).await;
        self.probe.complete(p, meta, start.elapsed(), out.is_ok());
        out
    }
}
#[async_trait]
impl Interceptor for Observer {
    async fn bind_rtcp_reader(
        &self,
        r: Arc<dyn RTCPReader + Send + Sync>,
    ) -> Arc<dyn RTCPReader + Send + Sync> {
        Arc::new(Reader {
            next: r,
            tracks: self.tracks.clone(),
        })
    }
    async fn bind_rtcp_writer(
        &self,
        w: Arc<dyn RTCPWriter + Send + Sync>,
    ) -> Arc<dyn RTCPWriter + Send + Sync> {
        w
    }
    async fn bind_local_stream(
        &self,
        s: &StreamInfo,
        w: Arc<dyn RTPWriter + Send + Sync>,
    ) -> Arc<dyn RTPWriter + Send + Sync> {
        if !s.mime_type.to_ascii_lowercase().starts_with("video/") {
            return w;
        }
        let Ok(mut m) = self.tracks.lock() else {
            return w;
        };
        if m.len() >= MAX_TRACKS && !m.contains_key(&s.ssrc) {
            return w;
        }
        let p = Arc::new(Probe::new(s.ssrc));
        if let Some(old) = m.insert(s.ssrc, p.clone()) {
            old.alive.store(false, Ordering::Release);
        }
        Arc::new(Writer { next: w, probe: p })
    }
    async fn unbind_local_stream(&self, s: &StreamInfo) {
        if let Ok(mut m) = self.tracks.lock() {
            if let Some(p) = m.remove(&s.ssrc) {
                p.alive.store(false, Ordering::Release);
            }
        }
    }
    async fn bind_remote_stream(
        &self,
        _: &StreamInfo,
        r: Arc<dyn RTPReader + Send + Sync>,
    ) -> Arc<dyn RTPReader + Send + Sync> {
        r
    }
    async fn unbind_remote_stream(&self, _: &StreamInfo) {}
    async fn close(&self) -> Result<()> {
        if let Ok(mut m) = self.tracks.lock() {
            for p in m.values() {
                p.alive.store(false, Ordering::Release);
            }
            m.clear();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct Sink {
        count: AtomicUsize,
        last: Mutex<Option<RtpPacket>>,
    }
    #[async_trait]
    impl RTPWriter for Sink {
        async fn write(&self, p: &RtpPacket, _: &Attributes) -> Result<usize> {
            self.count.fetch_add(1, Ordering::Relaxed);
            *self.last.lock().unwrap() = Some(p.clone());
            Ok(99)
        }
    }
    fn packet(seq: u16) -> RtpPacket {
        let mut p = RtpPacket::default();
        p.header.sequence_number = seq;
        p.header.timestamp = 777;
        p.header.ssrc = 12;
        p
    }
    #[tokio::test]
    async fn writer_is_byte_preserving_and_single_call() {
        let sink = Arc::new(Sink {
            count: AtomicUsize::new(0),
            last: Mutex::new(None),
        });
        let probe = Arc::new(Probe::new(12));
        let w = Writer {
            next: sink.clone(),
            probe: probe.clone(),
        };
        let p = packet(5);
        assert_eq!(w.write(&p, &Attributes::new()).await.unwrap(), 99);
        probe.nack(5, Instant::now());
        assert_eq!(w.write(&p, &Attributes::new()).await.unwrap(), 99);
        assert_eq!(sink.count.load(Ordering::Relaxed), 2);
        assert_eq!(*sink.last.lock().unwrap(), Some(p));
        assert_eq!(probe.inner.lock().unwrap().repeats, 1);
    }
    #[tokio::test]
    async fn default_responder_dispatch_is_observed_before_resend() {
        struct Feedback;
        #[async_trait]
        impl RTCPReader for Feedback {
            async fn read(
                &self,
                _: &mut [u8],
                _: &Attributes,
            ) -> Result<(Vec<Box<dyn RtcpPacket + Send + Sync>>, Attributes)> {
                use webrtc::rtcp::transport_feedbacks::transport_layer_nack::NackPair;
                Ok((
                    vec![Box::new(TransportLayerNack {
                        sender_ssrc: 99,
                        media_ssrc: 12,
                        nacks: vec![NackPair {
                            packet_id: 8,
                            lost_packets: 0,
                        }],
                    })],
                    Attributes::new(),
                ))
            }
        }
        let observer = Arc::new(Observer {
            tracks: Arc::new(Mutex::new(HashMap::new())),
        });
        let responder = webrtc::interceptor::nack::responder::Responder::builder()
            .build("qa-test")
            .unwrap();
        let chain = webrtc::interceptor::chain::Chain::new(vec![observer.clone(), responder]);
        let sink = Arc::new(Sink {
            count: AtomicUsize::new(0),
            last: Mutex::new(None),
        });
        let info = StreamInfo {
            ssrc: 12,
            mime_type: "video/H264".into(),
            rtcp_feedback: vec![webrtc::interceptor::stream_info::RTCPFeedback {
                typ: "nack".into(),
                parameter: "".into(),
            }],
            ..Default::default()
        };
        let writer = chain.bind_local_stream(&info, sink.clone()).await;
        writer.write(&packet(8), &Attributes::new()).await.unwrap();
        let reader = chain.bind_rtcp_reader(Arc::new(Feedback)).await;
        reader
            .read(&mut [0; 1500], &Attributes::new())
            .await
            .unwrap();
        for _ in 0..100 {
            if sink.count.load(Ordering::Relaxed) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(sink.count.load(Ordering::Relaxed), 2);
        let probe = observer.tracks.lock().unwrap().get(&12).unwrap().clone();
        assert_eq!(probe.inner.lock().unwrap().nacks, 1);
        assert_eq!(probe.inner.lock().unwrap().repeats, 1);
        assert_eq!(probe.inner.lock().unwrap().logs, 2);
        chain.unbind_local_stream(&info).await;
        assert!(!probe.alive.load(Ordering::Acquire));
        assert!(observer.tracks.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn audio_passthrough_and_track_count_are_bounded() {
        let observer = Observer {
            tracks: Arc::new(Mutex::new(HashMap::new())),
        };
        let sink = Arc::new(Sink {
            count: AtomicUsize::new(0),
            last: Mutex::new(None),
        });
        let audio = StreamInfo {
            ssrc: 7,
            mime_type: "audio/opus".into(),
            ..Default::default()
        };
        observer
            .bind_local_stream(&audio, sink.clone())
            .await
            .write(&packet(7), &Attributes::new())
            .await
            .unwrap();
        assert!(observer.tracks.lock().unwrap().is_empty());
        for ssrc in 1..=8 {
            observer
                .bind_local_stream(
                    &StreamInfo {
                        ssrc,
                        mime_type: "video/H264".into(),
                        ..Default::default()
                    },
                    sink.clone(),
                )
                .await;
        }
        assert_eq!(observer.tracks.lock().unwrap().len(), MAX_TRACKS);
        observer.close().await.unwrap();
        assert!(observer.tracks.lock().unwrap().is_empty());
    }
    #[test]
    fn timestamp_change_with_same_sequence_is_a_new_original() {
        let probe = Probe::new(12);
        let p = packet(8);
        probe.before_write(&p, Instant::now());
        probe.nack(8, Instant::now());
        let mut next = p.clone();
        next.header.timestamp += 90;
        assert!(probe.before_write(&next, Instant::now()).is_none());
        assert_eq!(
            probe.inner.lock().unwrap().slots[8].as_ref().unwrap().nacks,
            0
        );
    }
    #[test]
    fn bounded_collision_and_expiry_do_not_match_other_packet() {
        let p = Probe::new(12);
        let now = Instant::now();
        p.before_write(&packet(1), now);
        p.before_write(&packet(8193), now);
        p.nack(1, now);
        assert_eq!(p.inner.lock().unwrap().slots[1].as_ref().unwrap().nacks, 0);
        p.nack(8193, now + Duration::from_secs(11));
        assert_eq!(p.inner.lock().unwrap().slots[1].as_ref().unwrap().nacks, 0);
    }
    #[test]
    fn close_stops_diagnostics_without_affecting_underlying_write() {
        let p = Probe::new(12);
        p.alive.store(false, Ordering::Release);
        p.nack(1, Instant::now());
        assert!(p.before_write(&packet(1), Instant::now()).is_none());
        assert_eq!(p.inner.lock().unwrap().nacks, 0);
    }
    #[test]
    fn log_count_has_hard_bound() {
        let p = Probe::new(12);
        p.before_write(&packet(8), Instant::now());
        for _ in 0..5000 {
            p.nack(8, Instant::now());
        }
        assert_eq!(p.inner.lock().unwrap().logs, MAX_EVENTS);
    }
}
