//! Extra traffic credit earned only by a successfully sent eligible original.
//! No wall-clock refill or idle burst: numerator units are hundredths of a byte.
#[derive(Debug)]
pub(super) struct ShareBudget { percent:u8, credit:u64 }
impl ShareBudget {
 pub fn new(percent:u8)->Option<Self>{if !(1..=25).contains(&percent){return None;}Some(Self{percent,credit:0})}
 pub fn original(&mut self,wire_bytes:usize){
  // At most one maximum eligible IPv6 UDP datagram of carried credit.
  self.credit=self.credit.saturating_add((wire_bytes as u64).saturating_mul(self.percent as u64)).min(1328*100);
 }
 pub fn allows(&self,wire_bytes:usize)->bool{self.credit>=(wire_bytes as u64).saturating_mul(100)}
 pub fn debit(&mut self,wire_bytes:usize){self.credit=self.credit.saturating_sub((wire_bytes as u64).saturating_mul(100));}
 pub fn reset(&mut self){self.credit=0;}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn ratio_is_capped_without_startup_or_idle_credit(){let mut b=ShareBudget::new(25).unwrap();let(mut originals,mut copies)=(0u64,0u64);for n in 1..10000 {let size=if n%5==0{128}else{1200};b.original(size);originals+=size as u64;if b.allows(size){b.debit(size);copies+=size as u64;}assert!(copies*4<=originals);}assert!(copies>0);}
 #[test]fn single_original_cannot_earn_its_own_copy(){let mut b=ShareBudget::new(25).unwrap();for _ in 0..3{b.original(1200);assert!(!b.allows(1200));}b.original(1200);assert!(b.allows(1200));b.debit(1200);assert!(!b.allows(1200));}
 #[test]fn reset_and_credit_bound_survive_large_input(){let mut b=ShareBudget::new(25).unwrap();b.original(usize::MAX);assert_eq!(b.credit,1328*100);b.reset();assert!(!b.allows(1));}
 #[test]fn ten_percent_caps_every_prefix_for_mixed_wire_sizes(){let mut b=ShareBudget::new(10).unwrap();let(mut bytes,mut copies)=(0u64,0u64);for n in 0..10000{let size=[128,1200,1308,80,700][n%5];b.original(size);bytes+=size as u64;if b.allows(size){b.debit(size);copies+=size as u64;}assert!(copies*10<=bytes);}assert!(copies>0);b.reset();for _ in 0..9{b.original(1200);assert!(!b.allows(1200));}b.original(1200);assert!(b.allows(1200));}
 #[test]fn raw_share_cap5_respects_every_prefix(){let mut b=ShareBudget::new(5).unwrap();let(mut total,mut extra)=(0u64,0u64);for n in 1..10000{let size=[80,128,900,1200,1328][n%5];b.original(size);total+=size as u64;if b.allows(size){b.debit(size);extra+=size as u64;}assert!(extra*20<=total);}assert!(extra>0);}
 #[test]fn rejects_unsupported_ratio_and_tracks_peers_independently(){assert!(ShareBudget::new(0).is_none());assert!(ShareBudget::new(26).is_none());let mut a=ShareBudget::new(25).unwrap();let b=ShareBudget::new(25).unwrap();a.original(4800);assert!(a.allows(1200));assert!(!b.allows(1200));}
}
