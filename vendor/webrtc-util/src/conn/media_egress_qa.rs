//! QA media accounting only, not permission to send. Actual protected-video
//! originals and retransmissions debit the same recovery credit as extra copies.
use std::{
    future::Future,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const SCALE: i128 = 100;
const SHARE: i128 = 25;
const MAX_CREDIT: i128 = 1328 * SCALE;
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub originals: u64,
    pub original_bytes: u64,
    pub retries: u64,
    pub retry_bytes: u64,
    pub unknown: u64,
    pub unknown_bytes: u64,
    pub copies: u64,
    pub copy_bytes: u64,
    pub denied: u64,
    pub failed: u64,
    pub epochs: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    Original,
    Retry,
    Unknown,
}
struct State {
    endpoint: Option<(SocketAddr, u16)>,
    last: Instant,
    credit: i128,
    stats: Snapshot,
}
pub struct MediaLedger {
    live: AtomicBool,
    inner: Mutex<State>,
}
impl MediaLedger {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            live: AtomicBool::new(true),
            inner: Mutex::new(State {
                endpoint: None,
                last: Instant::now(),
                credit: 0,
                stats: Snapshot::default(),
            }),
        })
    }
    pub fn close(&self) {
        self.live.store(false, Ordering::Release);
        if let Ok(mut s) = self.inner.lock() {
            s.credit = 0;
            s.endpoint = None;
        }
    }
    pub fn reset(&self) {
        if let Ok(mut s) = self.inner.lock() {
            s.credit = 0;
            s.endpoint = None;
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().map(|s| s.stats).unwrap_or_default()
    }
    fn record(&self, target: SocketAddr, port: u16, bytes: usize, kind: WriteKind, now: Instant) {
        if !self.live.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut s) = self.inner.lock() {
            if !self.live.load(Ordering::Acquire) {
                return;
            }
            if s.endpoint != Some((target, port))
                || now.saturating_duration_since(s.last) > Duration::from_secs(1)
            {
                s.credit = 0;
                s.stats.epochs += 1;
                s.endpoint = Some((target, port));
            }
            s.last = now;
            if kind == WriteKind::Original {
                s.stats.originals += 1;
                s.stats.original_bytes += bytes as u64;
                s.credit = s
                    .credit
                    .saturating_add(bytes as i128 * SHARE)
                    .min(MAX_CREDIT);
            } else {
                if kind == WriteKind::Retry {
                    s.stats.retries += 1;
                    s.stats.retry_bytes += bytes as u64;
                } else {
                    s.stats.unknown += 1;
                    s.stats.unknown_bytes += bytes as u64;
                }
                s.credit = s.credit.saturating_sub(bytes as i128 * SCALE);
            }
        }
    }
    pub fn reserve_copy(&self, target: SocketAddr, port: u16, bytes: usize) -> bool {
        if !self.live.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut s) = self.inner.lock() else {
            return false;
        };
        if !self.live.load(Ordering::Acquire)
            || s.endpoint != Some((target, port))
            || s.last.elapsed() > Duration::from_millis(50)
            || s.credit < bytes as i128 * SCALE
        {
            s.stats.denied += 1;
            return false;
        }
        s.credit -= bytes as i128 * SCALE;
        true
    }
    pub fn copy_result(&self, bytes: usize, ok: bool) {
        if let Ok(mut s) = self.inner.lock() {
            if ok {
                s.stats.copies += 1;
                s.stats.copy_bytes += bytes as u64;
            } else {
                s.stats.failed += 1;
                s.credit = s.credit.min(0);
            }
        }
    }
}
/// Writer scope comes only from a negotiated video track, including NACK writes.
/// It does not create or transfer CopyScope/grant authority.
pub struct WriteScope {
    pub ledger: Arc<MediaLedger>,
    pub binding: Arc<AtomicBool>,
    pub ssrc: u32,
    pub pt: u8,
    pub seq: u16,
    pub kind: WriteKind,
    created: Instant,
    used: AtomicBool,
}
impl WriteScope {
    pub fn new(
        ledger: Arc<MediaLedger>,
        binding: Arc<AtomicBool>,
        ssrc: u32,
        pt: u8,
        seq: u16,
        original: bool,
    ) -> Self {
        Self::classified(
            ledger,
            binding,
            ssrc,
            pt,
            seq,
            if original {
                WriteKind::Original
            } else {
                WriteKind::Retry
            },
        )
    }
    pub fn classified(
        ledger: Arc<MediaLedger>,
        binding: Arc<AtomicBool>,
        ssrc: u32,
        pt: u8,
        seq: u16,
        kind: WriteKind,
    ) -> Self {
        Self {
            ledger,
            binding,
            ssrc,
            pt,
            seq,
            kind,
            created: Instant::now(),
            used: AtomicBool::new(false),
        }
    }
}
tokio::task_local! {static MEDIA:WriteScope;}
pub async fn within<F: Future>(scope: WriteScope, f: F) -> F::Output {
    MEDIA.scope(scope, f).await
}
pub fn observe_success(buf: &[u8], target: SocketAddr, port: u16) -> Option<Arc<MediaLedger>> {
    MEDIA
        .try_with(|s| {
            if !s.binding.load(Ordering::Acquire)
                || s.created.elapsed() > Duration::from_millis(50)
                || target.port() == 0
                || port == 0
                || buf.len() < 22
                || buf.len() > 1280
                || buf[0] >> 6 != 2
                || buf[1] & 127 != s.pt
                || (64..=95).contains(&s.pt)
                || u16::from_be_bytes([buf[2], buf[3]]) != s.seq
                || u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) != s.ssrc
            {
                return None;
            }
            let mut header = 12 + 4 * (buf[0] & 15) as usize;
            if buf.len() < header + 10 {
                return None;
            }
            if buf[0] & 0x10 != 0 {
                if buf.len() < header + 14 {
                    return None;
                }
                header += 4 + 4 * u16::from_be_bytes([buf[header + 2], buf[header + 3]]) as usize;
                if buf.len() < header + 10 {
                    return None;
                }
            }
            if s.used.swap(true, Ordering::AcqRel) {
                return None;
            }
            let bytes = buf.len() + if target.is_ipv4() { 28 } else { 48 };
            s.ledger.record(target, port, bytes, s.kind, Instant::now());
            Some(s.ledger.clone())
        })
        .ok()
        .flatten()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ep() -> SocketAddr {
        "127.0.0.1:4567".parse().unwrap()
    }
    #[test]
    fn retries_have_priority_and_cannot_be_credited_as_originals() {
        let b = MediaLedger::new();
        let now = Instant::now();
        for _ in 0..4 {
            b.record(ep(), 1, 1200, WriteKind::Original, now);
        }
        b.record(ep(), 1, 1200, WriteKind::Retry, now);
        assert!(!b.reserve_copy(ep(), 1, 1200));
        for _ in 0..4 {
            b.record(ep(), 1, 1200, WriteKind::Original, now);
        }
        assert!(b.reserve_copy(ep(), 1, 1200));
        b.copy_result(1200, true);
        let s = b.snapshot();
        assert_eq!(s.original_bytes, 9600);
        assert_eq!(s.retry_bytes, 1200);
        assert_eq!(s.copy_bytes, 1200);
    }
    #[test]
    fn existing_retransmission_debt_stops_optional_copies() {
        let b = MediaLedger::new();
        let now = Instant::now();
        b.record(ep(), 1, 1200, WriteKind::Original, now);
        for _ in 0..3 {
            b.record(ep(), 1, 1200, WriteKind::Retry, now);
        }
        for _ in 0..11 {
            b.record(ep(), 1, 1200, WriteKind::Original, now);
        }
        assert!(!b.reserve_copy(ep(), 1, 1));
        assert_eq!(b.snapshot().retries, 3);
    }
    #[test]
    fn no_idle_credit_endpoint_isolation_and_close() {
        let b = MediaLedger::new();
        let now = Instant::now();
        for _ in 0..10 {
            b.record(ep(), 1, 1200, WriteKind::Original, now);
        }
        assert!(!b.reserve_copy(ep(), 2, 1200));
        b.record(ep(), 2, 1200, WriteKind::Original, now);
        assert!(!b.reserve_copy(ep(), 2, 1200));
        b.close();
        b.record(ep(), 2, 10000, WriteKind::Original, now);
        assert!(!b.reserve_copy(ep(), 2, 1));
    }
    #[test]
    fn unclassified_writes_consume_headroom_but_are_not_called_retries() {
        let b = MediaLedger::new();
        for _ in 0..4 {
            b.record(ep(), 1, 1200, WriteKind::Original, Instant::now());
        }
        b.record(ep(), 1, 1200, WriteKind::Unknown, Instant::now());
        assert!(!b.reserve_copy(ep(), 1, 1));
        let s = b.snapshot();
        assert_eq!(s.retry_bytes, 0);
        assert_eq!(s.unknown_bytes, 1200);
    }
    #[test]
    fn reset_and_failed_send_do_not_refund_credit() {
        let b = MediaLedger::new();
        for _ in 0..4 {
            b.record(ep(), 1, 1200, WriteKind::Original, Instant::now());
        }
        assert!(b.reserve_copy(ep(), 1, 1200));
        b.copy_result(1200, false);
        assert!(!b.reserve_copy(ep(), 1, 1));
        b.reset();
        assert!(!b.reserve_copy(ep(), 1, 1));
    }
    #[tokio::test]
    async fn context_cannot_leak_to_child_or_survive_unbind() {
        let b = MediaLedger::new();
        let live = Arc::new(AtomicBool::new(true));
        let mut p = vec![0u8; 100];
        p[0] = 0x80;
        p[1] = 120;
        p[2..4].copy_from_slice(&1u16.to_be_bytes());
        p[8..12].copy_from_slice(&99u32.to_be_bytes());
        let cp = p.clone();
        within(
            WriteScope::new(b.clone(), live.clone(), 99, 120, 1, true),
            async {
                assert!(
                    tokio::spawn(async move { observe_success(&cp, ep(), 1).is_none() })
                        .await
                        .unwrap()
                );
                assert!(observe_success(&p, ep(), 1).is_some());
                assert!(observe_success(&p, ep(), 1).is_none());
            },
        )
        .await;
        live.store(false, Ordering::Release);
        within(WriteScope::new(b.clone(), live, 99, 120, 1, true), async {
            assert!(observe_success(&p, ep(), 1).is_none());
        })
        .await;
        assert_eq!(b.snapshot().originals, 1);
    }
}
