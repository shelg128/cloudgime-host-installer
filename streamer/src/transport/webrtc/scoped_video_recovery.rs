use std::{any::Any,collections::HashMap,net::{IpAddr,SocketAddr},sync::{Arc,Weak,atomic::{AtomicBool,Ordering}},time::{Duration,SystemTime,UNIX_EPOCH}};
use async_trait::async_trait;
use common::direct_recovery::DirectRecoveryGrant;
use tokio::sync::Mutex;
use webrtc::{ice_transport::{ice_candidate_type::RTCIceCandidateType as Kind,ice_protocol::RTCIceProtocol},peer_connection::{RTCPeerConnection,peer_connection_state::RTCPeerConnectionState},rtp_transceiver::rtp_codec::{RTPCodecType,RTCRtpCodecParameters},track::track_local::{TrackLocal,TrackLocalContext,track_local_static_rtp::TrackLocalStaticRTP}};
use webrtc_util::conn::scoped_recovery::{CopyBudget,CopyScope};
#[path="recovery_loss_policy.rs"]
mod recovery_loss_policy;
use recovery_loss_policy::{LossPolicy,Decision};

pub(super) struct Recovery {peer:Weak<RTCPeerConnection>,pub budget:Arc<CopyBudget>,policy:std::sync::Mutex<LossPolicy>}
impl Recovery {
 pub fn new(peer:Weak<RTCPeerConnection>,grant:&DirectRecoveryGrant)->Option<Arc<Self>>{
  let now=SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis()as u64;if !grant.valid_at(now){return None}
  let budget=CopyBudget::new(Duration::from_millis(grant.authority_expires_at_unix_ms-now),grant.max_extra_kbps,grant.max_extra_packets_per_second)?;
  Some(Arc::new(Self{peer,budget,policy:std::sync::Mutex::new(LossPolicy::default())}))
 }
 pub fn loss_window(&self,sent:u64,first_nacks:u64){
  // Actual peer RTCP processed by SRTP/authenticated interceptor pipeline.
  // Do not respond to client-supplied RouteTelemetry or arbitrary JSON loss.
  let decision=if let Ok(mut policy)=self.policy.lock(){policy.update(std::time::Instant::now(),sent,first_nacks)}else{Decision::Suspend};
  match decision {Decision::Renew=>self.budget.activate(Duration::from_secs(15)),Decision::Suspend=>self.budget.suspend(),Decision::Idle=>{}}
  let s=self.budget.snapshot();
  log::info!("CG_RECOVERY copies={} bytes={} limited={} blocked={}",s.copies,s.extra_bytes,s.limited,s.blocked);
  log::info!("CG_RECOVERY_LOSS sent={sent} nacks={first_nacks} action={decision:?}");
 }
 pub fn suspend(&self){if let Ok(mut p)=self.policy.lock(){p.reset();}self.budget.suspend();}
 async fn permission(&self,ssrc:u32,pt:u8,seq:u16,binding_live:Arc<AtomicBool>)->Option<CopyScope>{
  let peer=self.peer.upgrade()?;if peer.connection_state()!=RTCPeerConnectionState::Connected{return None}
  let dtls=peer.sctp().transport();let transport=dtls.ice_transport();let pair=transport.get_selected_candidate_pair().await?;
  if !direct_udp(pair.local.typ,pair.remote.typ,pair.local.protocol,pair.remote.protocol){return None}
  let ip:IpAddr=pair.remote.address.parse().ok()?;
  let local_port=if pair.local.related_port!=0{pair.local.related_port}else{pair.local.port};
  CopyScope::with_binding(SocketAddr::new(ip,pair.remote.port),local_port,ssrc,pt,seq,self.budget.clone(),binding_live)
 }
}
impl Drop for Recovery{fn drop(&mut self){self.budget.revoke();}}
fn direct_udp(a:Kind,b:Kind,pa:RTCIceProtocol,pb:RTCIceProtocol)->bool{
 matches!(a,Kind::Host|Kind::Srflx|Kind::Prflx)&&matches!(b,Kind::Host|Kind::Srflx|Kind::Prflx)&&pa==RTCIceProtocol::Udp&&pb==RTCIceProtocol::Udp
}
#[cfg(test)]
fn loss_activates(sent:u64,first:u64)->bool{sent>=50&&first>=3&&u128::from(first)*100>=u128::from(sent)*2&&u128::from(first)*4<=u128::from(sent)}

pub(super) struct RecoveryTrack {pub inner:Arc<TrackLocalStaticRTP>,recovery:Option<Arc<Recovery>>,bindings:Mutex<HashMap<String,(u32,u8,Arc<AtomicBool>)>>}
impl RecoveryTrack {
 pub fn new(inner:Arc<TrackLocalStaticRTP>,recovery:Option<Arc<Recovery>>)->Self{Self{inner,recovery,bindings:Mutex::new(HashMap::new())}}
 pub async fn permission(&self,sequence:u16)->Option<CopyScope>{
  let recovery=self.recovery.as_ref()?;
  let binding={let b=self.bindings.lock().await;if b.len()!=1{return None}b.values().next()?.clone()};
  recovery.permission(binding.0,binding.1,sequence,binding.2).await
 }
}
impl Drop for RecoveryTrack{fn drop(&mut self){for(_,_,live)in self.bindings.get_mut().values(){live.store(false,Ordering::Release);}}}
#[async_trait]
impl TrackLocal for RecoveryTrack {
 async fn bind(&self,c:&TrackLocalContext)->webrtc::error::Result<RTCRtpCodecParameters>{let codec=self.inner.bind(c).await?;if self.inner.kind()==RTPCodecType::Video&&codec.capability.mime_type.to_ascii_lowercase().starts_with("video/"){if let Some((_,_,live))=self.bindings.lock().await.insert(c.id(),(c.ssrc(),codec.payload_type,Arc::new(AtomicBool::new(true)))){live.store(false,Ordering::Release);}}Ok(codec)}
 async fn unbind(&self,c:&TrackLocalContext)->webrtc::error::Result<()>{if let Some((_,_,live))=self.bindings.lock().await.remove(&c.id()){live.store(false,Ordering::Release);}self.inner.unbind(c).await}
 fn id(&self)->&str{self.inner.id()}
 fn rid(&self)->Option<&str>{self.inner.rid()}
 fn stream_id(&self)->&str{self.inner.stream_id()}
 fn kind(&self)->RTPCodecType{self.inner.kind()}
 fn as_any(&self)->&dyn Any{self}
}
#[cfg(all(test,feature="cloudgime-scoped-direct-recovery"))]
#[path="scoped_recovery_peer_tests.rs"]
mod scoped_recovery_peer_tests;

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn relay_tcp_unknown_never_get_copy_authority(){assert!(direct_udp(Kind::Srflx,Kind::Host,RTCIceProtocol::Udp,RTCIceProtocol::Udp));for k in[Kind::Relay,Kind::Unspecified]{assert!(!direct_udp(k,Kind::Host,RTCIceProtocol::Udp,RTCIceProtocol::Udp));assert!(!direct_udp(Kind::Host,k,RTCIceProtocol::Udp,RTCIceProtocol::Udp));}assert!(!direct_udp(Kind::Host,Kind::Host,RTCIceProtocol::Tcp,RTCIceProtocol::Udp));}
 #[test]fn small_noisy_windows_and_extreme_loss_do_not_amplify(){assert!(!loss_activates(10,3));assert!(!loss_activates(1000,1));assert!(!loss_activates(100,80));assert!(loss_activates(500,50));assert!(!loss_activates(u64::MAX,1));}
 #[tokio::test]async fn extreme_loss_deactivates_existing_window(){let peer=Weak::new();let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;let recovery=Recovery::new(peer,&DirectRecoveryGrant{authority_expires_at_unix_ms:now+30000,max_extra_kbps:2000,max_extra_packets_per_second:1200}).unwrap();recovery.loss_window(100,10);tokio::time::sleep(Duration::from_millis(20)).await;let target="127.0.0.1:9999".parse().unwrap();let mut data=vec![0u8;100];data[0]=0x80;data[1]=120;data[2..4].copy_from_slice(&4u16.to_be_bytes());data[8..12].copy_from_slice(&123u32.to_be_bytes());recovery.loss_window(100,80);webrtc_util::conn::scoped_recovery::within(CopyScope::new(target,99,123,120,4,recovery.budget.clone()).unwrap(),async{assert!(webrtc_util::conn::scoped_recovery::permit(&data,target,99).is_none());}).await;}
 #[test]fn expired_or_absent_peer_never_authorizes(){let r=Recovery::new(Weak::new(),&DirectRecoveryGrant{authority_expires_at_unix_ms:1,max_extra_kbps:100,max_extra_packets_per_second:100});assert!(r.is_none());}
}
