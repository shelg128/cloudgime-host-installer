//! Bounded QA original-frame schedule using a conservative 25% recovery allowance.
//! Actual original/retry/copy accounting remains in the separate UDP ledger.
//! Not a hard whole-connection shaper; retries are never delayed here.
use std::time::Duration;
#[derive(Default)]pub(super) struct FramePlan{wire:u64,group:u64}
impl FramePlan{
 pub fn record(&mut self,wire_bytes:u64){self.wire=self.wire.saturating_add(wire_bytes);self.group=self.group.saturating_add(wire_bytes);}
 pub fn next_due(&mut self,elapsed:Duration)->Option<Duration>{
  if self.group<4800||elapsed>=Duration::from_millis(6){return None;}
  self.group=0;
  // 24Mbps burst service ceiling; serialize at most 6ms per frame. First group immediate.
  let target=Duration::from_micros(self.wire.saturating_mul(8)/24).min(Duration::from_millis(6));
  (target>elapsed).then_some(target)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn four_packet_quantum_then_absolute_deadline(){let mut p=FramePlan::default();for _ in 0..3{p.record(1200);assert!(p.next_due(Duration::ZERO).is_none());}p.record(1200);assert_eq!(p.next_due(Duration::ZERO),Some(Duration::from_micros(1600)));}
 #[test]fn network_write_delay_consumes_schedule_not_added_to_it(){let mut p=FramePlan::default();p.record(4800);assert_eq!(p.next_due(Duration::from_millis(2)),None);p.record(4800);assert_eq!(p.next_due(Duration::from_millis(2)),Some(Duration::from_micros(3200)));}
 #[test]fn keyframe_and_overflow_cannot_grow_delay_unbounded(){let mut p=FramePlan::default();p.record(u64::MAX);assert_eq!(p.next_due(Duration::ZERO),Some(Duration::from_millis(6)));p.record(4800);assert_eq!(p.next_due(Duration::from_millis(6)),None);}
 #[test]fn no_trailing_wait_and_new_frame_has_no_credit_debt(){let mut a=FramePlan::default();a.record(10000);let mut b=FramePlan::default();b.record(1000);assert!(b.next_due(Duration::ZERO).is_none());}
}
