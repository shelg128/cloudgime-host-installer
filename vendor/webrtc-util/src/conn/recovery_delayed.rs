//! QA-only temporal separation of an already protected copy. One slot per
//! budget, no worker/timer, no wait on original send. Drained by later originals.
use super::*;
const MIN_DELAY: Duration = Duration::from_millis(3);
const MAX_AGE: Duration = Duration::from_millis(45);
struct Pending {
    bytes: Vec<u8>,
    target: SocketAddr,
    port: u16,
    ssrc: u32,
    pt: u8,
    generation: u64,
    binding: Arc<AtomicBool>,
    at: Instant,
    cost: usize,
}
#[derive(Default)]
pub(super) struct DelayedSlot {
    pending: Option<Pending>,
}
impl DelayedSlot {
    fn step(
        &mut self,
        s: &CopyScope,
        buf: &[u8],
        cost: usize,
        now: Instant,
        queue: bool,
    ) -> Option<(Vec<u8>, usize)> {
        let mut ready = None;
        if let Some(p) = self.pending.as_ref() {
            let age = now.saturating_duration_since(p.at);
            if p.generation != s.generation
                || p.target != s.target
                || p.port != s.local_port
                || p.ssrc != s.ssrc
                || p.pt != s.payload_type
                || !p.binding.load(Ordering::Acquire)
                || !s.binding_live.load(Ordering::Acquire)
                || age > MAX_AGE
            {
                self.pending = None;
            } else if age >= MIN_DELAY {
                let p = self.pending.take().unwrap();
                ready = Some((p.bytes, p.cost));
            }
        }
        if queue && self.pending.is_none() {
            self.pending = Some(Pending {
                bytes: buf.to_vec(),
                target: s.target,
                port: s.local_port,
                ssrc: s.ssrc,
                pt: s.payload_type,
                generation: s.generation,
                binding: s.binding_live.clone(),
                at: now,
                cost,
            });
        }
        ready
    }
    pub(super) fn reset(&mut self) {
        self.pending = None;
    }
}
pub fn delayed_copy(
    buf: &[u8],
    target: SocketAddr,
    port: u16,
) -> Option<(Arc<CopyBudget>, Vec<u8>, usize)> {
    let capped = CURRENT
        .try_with(|s| {
            s.budget
                .state
                .lock()
                .ok()
                .map(|state| state.share.is_some())
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if !capped {
        return permit(buf, target, port).map(|(b, c)| (b, buf.to_vec(), c));
    }
    CURRENT
        .try_with(|s| {
            let cost = eligible_cost(s, buf, target, port)?;
            if s.used.swap(true, Ordering::AcqRel) {
                return None;
            }
            let now = Instant::now();
            let b = &s.budget;
            if !b.enabled.load(Ordering::Acquire)
                || now >= b.deadline
                || b.generation.load(Ordering::Acquire) != s.generation
            {
                return None;
            }
            let mut state = b.state.lock().ok()?;
            if !b.enabled.load(Ordering::Acquire)
                || now >= b.deadline
                || b.generation.load(Ordering::Acquire) != s.generation
            {
                return None;
            }
            state.stats.originals += 1;
            state.stats.original_bytes += cost as u64;
            let elapsed = now.duration_since(state.last).as_secs_f64();
            state.last = now;
            state.bytes = (state.bytes + elapsed * b.byte_rate).min(b.burst_bytes);
            state.packets = (state.packets + elapsed * b.packet_rate).min(b.burst_packets);
            if now >= state.until || now < state.backoff {
                state.delayed.reset();
                if let Some(share) = &mut state.share {
                    share.reset();
                }
                return None;
            }
            let queue = if let Some(share) = &mut state.share {
                share.original(cost);
                share.allows(cost)
            } else {
                false
            };
            let free = state.delayed.pending.is_none();
            // Drain prior slot first. Credit for a newly queued copy is spent once only.
            let ready = state.delayed.step(s, buf, cost, now, false);
            if queue && state.delayed.pending.is_none() {
                if let Some(share) = &mut state.share {
                    share.debit(cost);
                }
                state.delayed.step(s, buf, cost, now, true);
            } else if !queue || !free {
                state.stats.limited += 1;
            }
            let (bytes, send_cost) = ready?;
            // Rate/packet tokens are checked and spent at actual dispatch, not enqueue.
            if state.bytes < send_cost as f64 || state.packets < 1. {
                state.stats.limited += 1;
                return None;
            }
            state.bytes -= send_cost as f64;
            state.packets -= 1.;
            Some((b.clone(), bytes, send_cost))
        })
        .ok()
        .flatten()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (CopyScope, Vec<u8>) {
        let b = CopyBudget::new_with_share(Duration::from_secs(30), 2000, 1200, 25).unwrap();
        b.activate(Duration::from_secs(20));
        let s = CopyScope::new("127.0.0.1:2345".parse().unwrap(), 99, 123, 120, 4, b).unwrap();
        let mut buf = vec![0u8; 100];
        buf[0] = 0x80;
        buf[1] = 120;
        buf[2..4].copy_from_slice(&4u16.to_be_bytes());
        buf[8..12].copy_from_slice(&123u32.to_be_bytes());
        (s, buf)
    }
    #[test]
    fn exact_ciphertext_after_min_delay_no_queue_growth() {
        let (s, b) = setup();
        let mut q = DelayedSlot::default();
        let t = Instant::now();
        assert!(q.step(&s, &b, 128, t, true).is_none());
        assert!(
            q.step(&s, &vec![9; 100], 128, t + Duration::from_millis(2), true)
                .is_none()
        );
        let (data, cost) = q.step(&s, &b, 128, t + MIN_DELAY, false).unwrap();
        assert_eq!(data, b);
        assert_eq!(cost, 128);
        assert!(q.pending.is_none());
    }
    #[test]
    fn stale_or_unbound_or_different_peer_discards() {
        for mode in 0..6 {
            let (mut s, b) = setup();
            let mut q = DelayedSlot::default();
            let t = Instant::now();
            q.step(&s, &b, 128, t, true);
            if mode == 0 {
                s.binding_live.store(false, Ordering::Release);
            }
            if mode == 1 {
                s.generation += 1;
            }
            if mode == 2 {
                s.target = "127.0.0.1:6789".parse().unwrap();
            }
            if mode == 4 {
                s.ssrc += 1;
            }
            if mode == 5 {
                s.payload_type += 1;
            }
            let dt = if mode == 3 {
                Duration::from_millis(46)
            } else {
                MIN_DELAY
            };
            assert!(q.step(&s, &b, 128, t + dt, false).is_none());
            assert!(q.pending.is_none());
        }
    }
    #[tokio::test]
    async fn delayed_slot_never_crosses_suspend_or_expiry() {
        for revoke in [false, true] {
            let (s, buf) = setup();
            let budget = s.budget.clone();
            let now = Instant::now();
            {
                let mut b = budget.state.lock().unwrap();
                b.delayed.step(&s, &buf, 128, now, true);
            }
            if revoke {
                budget.revoke();
            } else {
                budget.suspend();
                budget.activate(Duration::from_secs(1));
            }
            assert!(budget.state.lock().unwrap().delayed.pending.is_none());
            within(s, async {
                assert!(delayed_copy(&buf, "127.0.0.1:2345".parse().unwrap(), 99).is_none());
            })
            .await;
        }
        let mut s = setup().0;
        s.budget = CopyBudget::new_with_share(Duration::from_millis(1), 2000, 1200, 25).unwrap();
        s.budget.activate(Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(3)).await;
        let buf = setup().1;
        within(s, async {
            assert!(delayed_copy(&buf, "127.0.0.1:2345".parse().unwrap(), 99).is_none());
        })
        .await;
    }
    #[tokio::test]
    async fn delayed_dispatch_cannot_exceed_cumulative_share_or_rate_budget() {
        let (s, mut buf) = setup();
        let b = s.budget.clone();
        tokio::time::sleep(Duration::from_millis(20)).await;
        for seq in 0..200u16 {
            buf[2..4].copy_from_slice(&seq.to_be_bytes());
            let scope = CopyScope::new(s.target, 99, 123, 120, seq, b.clone()).unwrap();
            within(scope, async {
                if let Some((budget, bytes, cost)) = delayed_copy(&buf, s.target, 99) {
                    assert!(bytes.len() <= 1280);
                    budget.result(cost, true);
                }
            })
            .await;
            let stats = b.snapshot();
            assert!(stats.extra_bytes * 4 <= stats.original_bytes);
            if seq % 20 == 19 {
                tokio::time::sleep(Duration::from_millis(4)).await;
            }
        }
        assert!(b.snapshot().copies > 0);
        let stats = b.snapshot();
        assert!(stats.copies <= 32);
        b.revoke();
    }
    #[tokio::test]
    async fn no_context_or_revoke_can_dispatch() {
        let (s, b) = setup();
        let budget = s.budget.clone();
        budget.revoke();
        within(s, async {
            assert!(delayed_copy(&b, "127.0.0.1:2345".parse().unwrap(), 99).is_none());
        })
        .await;
        assert!(delayed_copy(&b, "127.0.0.1:2345".parse().unwrap(), 99).is_none());
    }
}
