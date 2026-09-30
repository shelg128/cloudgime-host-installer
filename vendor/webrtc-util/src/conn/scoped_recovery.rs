//! One-write, task-local authority at the encrypted UDP boundary. No global
//! endpoint registry; retransmissions/control tasks do not inherit this context.
use std::{future::Future,net::SocketAddr,sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU64,Ordering}},time::{Duration,Instant}};

#[derive(Debug,Default,Clone,Copy)]
pub struct Snapshot {pub originals:u64,pub original_bytes:u64,pub copies:u64,pub extra_bytes:u64,pub limited:u64,pub blocked:u64}
struct Bucket {last:Instant,bytes:f64,packets:f64,until:Instant,backoff:Instant,stats:Snapshot}
pub struct CopyBudget {deadline:Instant,byte_rate:f64,packet_rate:f64,burst_bytes:f64,burst_packets:f64,enabled:AtomicBool,generation:AtomicU64,state:Mutex<Bucket>}
impl CopyBudget {
 pub fn new(lifetime:Duration,kbps:u32,pps:u32)->Option<Arc<Self>>{
  if lifetime.is_zero()||!(1..=8000).contains(&kbps)||!(1..=2000).contains(&pps){return None}
  let now=Instant::now();let bytes=kbps as f64*125.;let packets=pps as f64;
  Some(Arc::new(Self{deadline:now.checked_add(lifetime)?,byte_rate:bytes,packet_rate:packets,burst_bytes:(bytes*0.1).clamp(1400.,32000.),burst_packets:(packets*0.1).clamp(1.,32.),enabled:AtomicBool::new(true),generation:AtomicU64::new(1),state:Mutex::new(Bucket{last:now,bytes:0.,packets:0.,until:now,backoff:now,stats:Snapshot::default()})}))
 }
 pub fn activate(&self,for_time:Duration){if !self.enabled.load(Ordering::Acquire){return}let now=Instant::now();if let Ok(mut s)=self.state.lock(){s.until=now+for_time.min(Duration::from_secs(20));}}
 pub fn suspend(&self){if let Ok(mut s)=self.state.lock(){s.until=Instant::now();s.bytes=0.;s.packets=0.;self.generation.fetch_add(1,Ordering::AcqRel);}}
 pub fn revoke(&self){self.enabled.store(false,Ordering::Release);self.suspend();}
 pub fn snapshot(&self)->Snapshot{self.state.lock().map(|s|s.stats).unwrap_or_default()}
 fn reserve(&self,wire_bytes:usize)->bool{self.reserve_generation(wire_bytes,self.generation.load(Ordering::Acquire))}
 fn reserve_generation(&self,wire_bytes:usize,generation:u64)->bool{let now=Instant::now();if !self.enabled.load(Ordering::Acquire)||now>=self.deadline{return false}let Ok(mut s)=self.state.lock()else{return false};if !self.enabled.load(Ordering::Acquire)||self.generation.load(Ordering::Acquire)!=generation{return false}s.stats.originals+=1;s.stats.original_bytes+=wire_bytes as u64;
  let elapsed=now.duration_since(s.last).as_secs_f64();s.last=now;s.bytes=(s.bytes+elapsed*self.byte_rate).min(self.burst_bytes);s.packets=(s.packets+elapsed*self.packet_rate).min(self.burst_packets);
  if now>=s.until||now<s.backoff{return false}if s.bytes<wire_bytes as f64||s.packets<1.{s.stats.limited+=1;return false}s.bytes-=wire_bytes as f64;s.packets-=1.;true
 }
 pub fn result(&self,wire_bytes:usize,ok:bool){if let Ok(mut s)=self.state.lock(){if ok{s.stats.copies+=1;s.stats.extra_bytes+=wire_bytes as u64}else{s.stats.blocked+=1;s.backoff=Instant::now()+Duration::from_secs(1)}}}
}
impl Drop for CopyBudget{fn drop(&mut self){self.enabled.store(false,Ordering::Release);}}

pub struct CopyScope {pub target:SocketAddr,pub local_port:u16,pub ssrc:u32,pub payload_type:u8,pub sequence:u16,pub budget:Arc<CopyBudget>,pub binding_live:Arc<AtomicBool>,generation:u64,created:Instant,used:AtomicBool}
impl CopyScope{pub fn new(target:SocketAddr,local_port:u16,ssrc:u32,pt:u8,sequence:u16,budget:Arc<CopyBudget>)->Option<Self>{Self::with_binding(target,local_port,ssrc,pt,sequence,budget,Arc::new(AtomicBool::new(true)))}
 pub fn with_binding(target:SocketAddr,local_port:u16,ssrc:u32,pt:u8,sequence:u16,budget:Arc<CopyBudget>,binding_live:Arc<AtomicBool>)->Option<Self>{
 if target.port()==0||target.ip().is_unspecified()||target.ip().is_multicast()||local_port==0||ssrc==0||pt>127||(64..=95).contains(&pt){return None}
 Some(Self{target,local_port,ssrc,payload_type:pt,sequence,generation:budget.generation.load(Ordering::Acquire),budget,binding_live,created:Instant::now(),used:AtomicBool::new(false)})
}}
tokio::task_local! {static CURRENT:CopyScope;}
pub async fn within<F:Future>(permission:CopyScope,send:F)->F::Output{CURRENT.scope(permission,send).await}
/// Only matches a single negotiated SRTP packet. TURN/DTLS/STUN/RTCP have
/// different framing and cannot match. No payload/cryptographic rewrite.
pub fn permit(buf:&[u8],target:SocketAddr,local_port:u16)->Option<(Arc<CopyBudget>,usize)>{CURRENT.try_with(|s|{
 if !s.binding_live.load(Ordering::Acquire)||target!=s.target||local_port!=s.local_port||s.created.elapsed()>Duration::from_millis(50)||buf.len()<22||buf.len()>1280||buf[0]>>6!=2||buf[1]&127!=s.payload_type{return None}
 if u16::from_be_bytes([buf[2],buf[3]])!=s.sequence||u32::from_be_bytes([buf[8],buf[9],buf[10],buf[11]])!=s.ssrc{return None}
 let mut header=12+(buf[0]&15)as usize*4;if buf.len()<header+10{return None}if buf[0]&0x10!=0{if buf.len()<header+4+10{return None}let words=u16::from_be_bytes([buf[header+2],buf[header+3]])as usize;header=header.checked_add(4+words*4)?;if buf.len()<header+10{return None}}
 if s.used.swap(true,Ordering::AcqRel){return None}
 // Include IP+UDP overhead in the extra-bandwidth budget (not link framing).
 let cost=buf.len()+if target.is_ipv4(){28}else{48};s.budget.reserve_generation(cost,s.generation).then(||(s.budget.clone(),cost))
 }).ok().flatten()}

#[cfg(test)]mod tests{
 use super::*;
 fn target()->SocketAddr{"127.0.0.1:34567".parse().unwrap()}
 fn data()->Vec<u8>{let mut b=vec![0;100];b[0]=0x80;b[1]=120;b[2..4].copy_from_slice(&4u16.to_be_bytes());b[8..12].copy_from_slice(&123u32.to_be_bytes());b}
 async fn active()->Arc<CopyBudget>{let b=CopyBudget::new(Duration::from_secs(30),2000,1200).unwrap();b.activate(Duration::from_secs(20));tokio::time::sleep(Duration::from_millis(20)).await;b}
 #[tokio::test]async fn no_context_or_wrong_binding_never_copies(){let b=active().await;let d=data();assert!(permit(&d,target(),99).is_none());within(CopyScope::new(target(),99,123,120,4,b).unwrap(),async{assert!(permit(&d,target(),100).is_none());let mut wrong=d.clone();wrong[8]=1;assert!(permit(&wrong,target(),99).is_none());let mut wrong=d.clone();wrong[1]=111;assert!(permit(&wrong,target(),99).is_none());assert!(permit(&d,"127.0.0.1:34568".parse().unwrap(),99).is_none());assert!(permit(&d,target(),99).is_some());assert!(permit(&d,target(),99).is_none());}).await;}
 #[tokio::test]async fn child_task_cannot_inherit_copy_authority(){let b=active().await;within(CopyScope::new(target(),99,123,120,4,b).unwrap(),async{assert!(tokio::spawn(async{permit(&data(),target(),99).is_none()}).await.unwrap());assert!(permit(&data(),target(),99).is_some());}).await;}
 #[tokio::test]async fn inactive_revoked_expired_and_backpressure_stop_copies(){let b=CopyBudget::new(Duration::from_secs(2),2000,1200).unwrap();assert!(!b.reserve(100));b.activate(Duration::from_secs(20));tokio::time::sleep(Duration::from_millis(20)).await;assert!(b.reserve(100));b.result(100,false);assert!(!b.reserve(100));b.revoke();assert!(!b.reserve(100));let x=CopyBudget::new(Duration::from_millis(1),2000,1200).unwrap();x.activate(Duration::from_secs(20));tokio::time::sleep(Duration::from_millis(3)).await;assert!(!x.reserve(100));}
 #[tokio::test]async fn budget_and_memory_are_bounded(){let b=active().await;let mut bytes=0;for _ in 0..1000{if b.reserve(1280){bytes+=1280;b.result(1280,true);}}assert!(bytes<=32000);assert!(b.snapshot().copies<=32);assert_eq!(b.snapshot().extra_bytes,bytes);assert!(b.snapshot().limited>0);}
 #[tokio::test]async fn old_scope_stays_invalid_after_reconnect(){let b=active().await;let scope=CopyScope::new(target(),99,123,120,4,b.clone()).unwrap();b.suspend();b.activate(Duration::from_secs(15));tokio::time::sleep(Duration::from_millis(20)).await;within(scope,async{assert!(permit(&data(),target(),99).is_none());}).await;within(CopyScope::new(target(),99,123,120,4,b).unwrap(),async{assert!(permit(&data(),target(),99).is_some());}).await;}
 #[tokio::test]async fn control_and_malformed_extensions_refused(){let b=active().await;within(CopyScope::new(target(),99,123,120,4,b).unwrap(),async{for x in[0,23,0x40]{let mut d=data();d[0]=x;assert!(permit(&d,target(),99).is_none());}let mut d=data();d[0]=0x90;d[14]=255;assert!(permit(&d,target(),99).is_none());assert!(permit(&data(),target(),99).is_some());}).await;}
}
