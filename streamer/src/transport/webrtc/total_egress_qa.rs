//! Innermost QA interceptor accounts exact protected video originals/retries.
//! It grants no copy permission and never mutates or delays RTP/RTCP.
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use webrtc::interceptor::{
    self, Attributes, Interceptor, InterceptorBuilder, RTCPReader, RTCPWriter, RTPReader,
    RTPWriter, stream_info::StreamInfo,
};
use webrtc::rtp::packet::Packet;
use webrtc_util::conn::media_egress_qa::{MediaLedger, WriteKind, WriteScope, within};
type Result<T> = std::result::Result<T, interceptor::Error>;
pub(super) struct Builder;
impl InterceptorBuilder for Builder {
    fn build(&self, _: &str) -> Result<Arc<dyn Interceptor + Send + Sync>> {
        Ok(Arc::new(Observer {
            tracks: Mutex::new(HashMap::new()),
        }))
    }
}
#[derive(Default)]
struct Classifier {
    slots: Vec<Option<(u16, u32)>>,
}
impl Classifier {
    fn classify(&mut self, p: &Packet, scoped_original: bool) -> WriteKind {
        if self.slots.is_empty() {
            self.slots.resize(8192, None);
        }
        let i = p.header.sequence_number as usize % 8192;
        let key = (p.header.sequence_number, p.header.timestamp);
        if scoped_original {
            self.slots[i] = Some(key);
            WriteKind::Original
        } else if self.slots[i] == Some(key) {
            WriteKind::Retry
        } else {
            WriteKind::Unknown
        }
    }
}
struct Track {
    classify: Mutex<Classifier>,
    ledger: Arc<MediaLedger>,
    live: Arc<AtomicBool>,
    report: Mutex<Instant>,
    ssrc: u32,
    pt: u8,
}
struct Observer {
    tracks: Mutex<HashMap<u32, Arc<Track>>>,
}
struct Writer {
    next: Arc<dyn RTPWriter + Send + Sync>,
    track: Arc<Track>,
}
#[async_trait]
impl RTPWriter for Writer {
    async fn write(&self, p: &Packet, a: &Attributes) -> Result<usize> {
        if !self.track.live.load(Ordering::Acquire) {
            return self.next.write(p, a).await;
        }
        let original =
            super::original_write_qa::matches(p.header.sequence_number, p.header.timestamp);
        let kind = self
            .track
            .classify
            .lock()
            .map(|mut c| c.classify(p, original))
            .unwrap_or(WriteKind::Unknown);
        let scope = WriteScope::classified(
            self.track.ledger.clone(),
            self.track.live.clone(),
            self.track.ssrc,
            self.track.pt,
            p.header.sequence_number,
            kind,
        );
        let r = within(scope, self.next.write(p, a)).await;
        if let Ok(mut last) = self.track.report.lock() {
            if last.elapsed() >= Duration::from_secs(5) {
                let s = self.track.ledger.snapshot();
                log::info!(
                    "CG_EG3 o={} r={} u={} c={} d={} x={}",
                    s.original_bytes,
                    s.retry_bytes,
                    s.unknown_bytes,
                    s.copy_bytes,
                    s.denied,
                    s.epochs
                );
                *last = Instant::now();
            }
        }
        r
    }
}
#[async_trait]
impl Interceptor for Observer {
    async fn bind_local_stream(
        &self,
        s: &StreamInfo,
        next: Arc<dyn RTPWriter + Send + Sync>,
    ) -> Arc<dyn RTPWriter + Send + Sync> {
        if !s.mime_type.eq_ignore_ascii_case("video/h264") {
            return next;
        }
        let Ok(mut tracks) = self.tracks.lock() else {
            return next;
        };
        if tracks.len() >= 2 && !tracks.contains_key(&s.ssrc) {
            return next;
        }
        let t = Arc::new(Track {
            classify: Mutex::new(Classifier::default()),
            ledger: MediaLedger::new(),
            live: Arc::new(AtomicBool::new(true)),
            report: Mutex::new(Instant::now()),
            ssrc: s.ssrc,
            pt: s.payload_type,
        });
        if let Some(old) = tracks.insert(s.ssrc, t.clone()) {
            old.live.store(false, Ordering::Release);
            old.ledger.close();
        }
        Arc::new(Writer { next, track: t })
    }
    async fn unbind_local_stream(&self, s: &StreamInfo) {
        if let Ok(mut tracks) = self.tracks.lock() {
            if let Some(t) = tracks.remove(&s.ssrc) {
                t.live.store(false, Ordering::Release);
                t.ledger.close();
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
    async fn bind_rtcp_reader(
        &self,
        r: Arc<dyn RTCPReader + Send + Sync>,
    ) -> Arc<dyn RTCPReader + Send + Sync> {
        r
    }
    async fn bind_rtcp_writer(
        &self,
        w: Arc<dyn RTCPWriter + Send + Sync>,
    ) -> Arc<dyn RTCPWriter + Send + Sync> {
        w
    }
    async fn close(&self) -> Result<()> {
        if let Ok(mut tracks) = self.tracks.lock() {
            for t in tracks.values() {
                t.live.store(false, Ordering::Release);
                t.ledger.close();
            }
            tracks.clear();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Sink {
        count: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl RTPWriter for Sink {
        async fn write(&self, _: &Packet, _: &Attributes) -> Result<usize> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(10)
        }
    }
    #[test]
    fn only_matching_scoped_original_can_label_retry() {
        let mut c = Classifier::default();
        let mut p = Packet::default();
        p.header.sequence_number = 3;
        p.header.timestamp = 900;
        assert_eq!(c.classify(&p, false), WriteKind::Unknown);
        assert_eq!(c.classify(&p, true), WriteKind::Original);
        assert_eq!(c.classify(&p, false), WriteKind::Retry);
        p.header.timestamp += 90;
        assert_eq!(c.classify(&p, false), WriteKind::Unknown);
        p.header.sequence_number += 8192;
        assert_eq!(c.classify(&p, false), WriteKind::Unknown);
        assert_eq!(c.slots.len(), 8192);
    }
    #[tokio::test]
    async fn actual_default_responder_retries_debit_same_video_ledger() {
        use webrtc::interceptor::{chain::Chain, stream_info::RTCPFeedback};
        use webrtc::rtcp::{
            packet::Packet as Control,
            transport_feedbacks::transport_layer_nack::{NackPair, TransportLayerNack},
        };
        use webrtc_util::conn::{
            Conn,
            scoped_recovery::{CopyBudget, CopyScope},
        };
        struct UdpSink {
            socket: tokio::net::UdpSocket,
            target: std::net::SocketAddr,
        }
        #[async_trait]
        impl RTPWriter for UdpSink {
            async fn write(&self, p: &Packet, _: &Attributes) -> Result<usize> {
                let mut protected = vec![0u8; 100];
                protected[0] = 0x80;
                protected[1] = 98;
                protected[2..4].copy_from_slice(&p.header.sequence_number.to_be_bytes());
                protected[4..8].copy_from_slice(&p.header.timestamp.to_be_bytes());
                protected[8..12].copy_from_slice(&p.header.ssrc.to_be_bytes());
                Conn::send_to(&self.socket, &protected, self.target)
                    .await
                    .map_err(|e| interceptor::Error::Other(e.to_string()))
            }
        }
        struct Feedback;
        #[async_trait]
        impl RTCPReader for Feedback {
            async fn read(
                &self,
                _: &mut [u8],
                _: &Attributes,
            ) -> Result<(Vec<Box<dyn Control + Send + Sync>>, Attributes)> {
                Ok((
                    vec![Box::new(TransportLayerNack {
                        sender_ssrc: 7,
                        media_ssrc: 99,
                        nacks: vec![NackPair {
                            packet_id: 3,
                            lost_packets: 0,
                        }],
                    })],
                    Attributes::new(),
                ))
            }
        }
        let rx = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let tx = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = rx.local_addr().unwrap();
        let port = tx.local_addr().unwrap().port();
        let sink = Arc::new(UdpSink { socket: tx, target });
        let observer = Arc::new(Observer {
            tracks: Mutex::new(HashMap::new()),
        });
        let responder = webrtc::interceptor::nack::responder::Responder::builder()
            .build("egress-test")
            .unwrap();
        let chain = Chain::new(vec![observer.clone(), responder]);
        let info = StreamInfo {
            ssrc: 99,
            payload_type: 98,
            mime_type: "video/H264".into(),
            rtcp_feedback: vec![RTCPFeedback {
                typ: "nack".into(),
                parameter: "".into(),
            }],
            ..Default::default()
        };
        let writer = chain.bind_local_stream(&info, sink).await;
        let mut unscoped = Packet::default();
        unscoped.header.version = 2;
        unscoped.header.ssrc = 99;
        unscoped.header.payload_type = 98;
        unscoped.header.timestamp = 9000;
        for seq in 90..110 {
            unscoped.header.sequence_number = seq;
            super::super::original_write_qa::within(
                seq,
                9000,
                writer.write(&unscoped, &Attributes::new()),
            )
            .await
            .unwrap();
        }
        let track = observer.tracks.lock().unwrap().get(&99).unwrap().clone();
        assert_eq!(track.ledger.snapshot().originals, 20);
        assert_eq!(track.ledger.snapshot().unknown, 0);
        assert_eq!(
            track.ledger.snapshot().copies,
            0,
            "enough accounting credit never creates copy authority"
        );
        let budget = CopyBudget::new_with_share(Duration::from_secs(10), 2000, 1200, 25).unwrap();
        budget.activate(Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
        for seq in 0..4 {
            let mut p = Packet::default();
            p.header.version = 2;
            p.header.ssrc = 99;
            p.header.payload_type = 98;
            p.header.sequence_number = seq;
            p.header.timestamp = 9000;
            super::super::original_write_qa::within(
                seq,
                9000,
                webrtc_util::conn::scoped_recovery::within(
                    CopyScope::new(target, port, 99, 98, seq, budget.clone()).unwrap(),
                    writer.write(&p, &Attributes::new()),
                ),
            )
            .await
            .unwrap();
        }
        let reader = chain.bind_rtcp_reader(Arc::new(Feedback)).await;
        reader
            .read(&mut [0; 1500], &Attributes::new())
            .await
            .unwrap();
        let track = observer.tracks.lock().unwrap().get(&99).unwrap().clone();
        for _ in 0..100 {
            if track.ledger.snapshot().retries == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let s = track.ledger.snapshot();
        assert_eq!(s.originals, 24);
        assert_eq!(s.retries, 1);
        assert_eq!(s.unknown, 0);
        assert_eq!(s.copies, 1);
        assert_eq!(s.retry_bytes, 128);
        assert_eq!(budget.snapshot().copies, 1);
        chain.close().await.unwrap();
        assert!(!track.live.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn writes_remain_once_and_audio_has_no_accounting_track() {
        let o = Observer {
            tracks: Mutex::new(HashMap::new()),
        };
        let sink = Arc::new(Sink { count: 0.into() });
        let audio = StreamInfo {
            ssrc: 1,
            mime_type: "audio/opus".into(),
            ..Default::default()
        };
        let a = o.bind_local_stream(&audio, sink.clone()).await;
        a.write(&Packet::default(), &Attributes::new())
            .await
            .unwrap();
        assert!(o.tracks.lock().unwrap().is_empty());
        let v = StreamInfo {
            ssrc: 2,
            payload_type: 98,
            mime_type: "video/H264".into(),
            ..Default::default()
        };
        let w = o.bind_local_stream(&v, sink.clone()).await;
        w.write(&Packet::default(), &Attributes::new())
            .await
            .unwrap();
        assert_eq!(sink.count.load(Ordering::Relaxed), 2);
        let t = o.tracks.lock().unwrap().get(&2).unwrap().clone();
        o.unbind_local_stream(&v).await;
        assert!(!t.live.load(Ordering::Acquire));
        assert!(
            !t.ledger
                .reserve_copy("127.0.0.1:9".parse().unwrap(), 1, 100)
        );
    }
}
