//! Hysteresis for authenticated NACK feedback. Recovery suppresses observed
//! loss itself, so using the entry threshold again for retention oscillates.
use std::time::{Duration, Instant};
const HOLD: Duration = Duration::from_secs(15);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision { Idle, Renew, Suspend }
#[derive(Default)]
pub(super) struct LossPolicy { until: Option<Instant> }
impl LossPolicy {
    pub fn reset(&mut self) { self.until = None; }
    pub fn update(&mut self, now: Instant, sent: u64, nacks: u64) -> Decision {
        if self.until.is_some_and(|end| now >= end) { self.until = None; }
        // Delayed feedback / tiny windows cannot classify path quality.
        if sent < 50 { return Decision::Idle; }
        if u128::from(nacks) * 4 > u128::from(sent) {
            self.reset();
            return Decision::Suspend;
        }
        let enter = nacks >= 3 && u128::from(nacks) * 100 >= u128::from(sent) * 2;
        let retain = self.until.is_some() && nacks > 0;
        if enter || retain {
            self.until = now.checked_add(HOLD);
            Decision::Renew
        } else {
            // Do not extend on zero-loss samples. The budget's independent
            // monotonic expiry stops copies even if all feedback ceases.
            Decision::Idle
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn residual_nacks_keep_protected_path_from_oscillating() {
        let t=Instant::now();let mut p=LossPolicy::default();
        assert_eq!(p.update(t,500,30),Decision::Renew);
        for sec in [5,10,15,20,25,30] { assert_eq!(p.update(t+Duration::from_secs(sec),500,1),Decision::Renew); }
    }
    #[test] fn clean_or_missing_feedback_expires_without_extension() {
        let t=Instant::now();let mut p=LossPolicy::default();p.update(t,500,30);
        assert_eq!(p.update(t+Duration::from_secs(5),500,0),Decision::Idle);
        assert_eq!(p.update(t+Duration::from_secs(10),500,0),Decision::Idle);
        assert_eq!(p.update(t+Duration::from_secs(15),500,1),Decision::Idle);
        p.update(t+Duration::from_secs(20),500,30);
        assert_eq!(p.update(t+Duration::from_secs(50),500,1),Decision::Idle);
    }
    #[test] fn small_window_and_single_old_nack_cannot_enable_or_renew() {
        let t=Instant::now();let mut p=LossPolicy::default();
        assert_eq!(p.update(t,500,1),Decision::Idle);assert_eq!(p.update(t,10,5),Decision::Idle);
        p.update(t,500,30);assert_eq!(p.update(t+Duration::from_secs(10),10,5),Decision::Idle);
        assert_eq!(p.update(t+Duration::from_secs(16),500,1),Decision::Idle);
    }
    #[test] fn extreme_loss_and_reconnect_require_entry_threshold_again() {
        let t=Instant::now();let mut p=LossPolicy::default();p.update(t,500,30);
        assert_eq!(p.update(t,500,200),Decision::Suspend);assert_eq!(p.update(t,500,1),Decision::Idle);
        p.update(t,500,30);p.reset();assert_eq!(p.update(t,500,1),Decision::Idle);
    }
    #[test] fn independent_peers_do_not_share_hysteresis() {
        let t=Instant::now();let mut a=LossPolicy::default();let mut b=LossPolicy::default();a.update(t,500,30);
        assert_eq!(b.update(t,500,1),Decision::Idle);assert_eq!(a.update(t,500,1),Decision::Renew);
    }
    #[test] fn thresholds_and_large_counts_are_bounded() {
        let t=Instant::now();let mut p=LossPolicy::default();assert_eq!(p.update(t,100,2),Decision::Idle);
        assert_eq!(p.update(t,100,3),Decision::Renew);assert_eq!(p.update(t,100,26),Decision::Suspend);
        assert_eq!(p.update(t,u64::MAX,1),Decision::Idle);assert_eq!(p.update(t,u64::MAX,u64::MAX),Decision::Suspend);
    }
}
