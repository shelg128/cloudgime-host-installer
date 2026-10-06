//! Innermost SDK interceptor. Observe metadata only; never change data or recovery.
use std::sync::{Arc,atomic::{AtomicBool,AtomicU64,Ordering}};
use async_trait::async_trait;
use webrtc::{interceptor::{Attributes,Interceptor,InterceptorBuilder,RTPReader,RTPWriter,RTCPReader,RTCPWriter,stream_info::StreamInfo},rtcp::{packet::Packet as RtcpPacket,sender_report::SenderReport,receiver_report::ReceiverReport,transport_feedbacks::transport_layer_nack::TransportLayerNack,payload_feedbacks::{receiver_estimated_maximum_bitrate::ReceiverEstimatedMaximumBitrate,picture_loss_indication::PictureLossIndication}}};
#[path="recovery_observer_log.rs"]mod recorder;
use recorder::{Tap,Data};
#[cfg(feature="cloudgime-scoped-recovery-metadata")]
#[path="recovery_scoped.rs"]mod scoped;
#[derive(Clone)]enum Capture{Immediate(Tap),#[cfg(feature="cloudgime-scoped-recovery-metadata")]Scoped(Arc<scoped::ScopedCapture>)}
impl From<Tap> for Capture{fn from(t:Tap)->Self{Self::Immediate(t)}}
impl Capture{
 fn acquire(&self)->Option<Tap>{match self{Self::Immediate(t)=>t.active().then(||t.clone()),#[cfg(feature="cloudgime-scoped-recovery-metadata")]Self::Scoped(s)=>s.acquire()}}
 fn close(&self){match self{Self::Immediate(t)=>t.close(),#[cfg(feature="cloudgime-scoped-recovery-metadata")]Self::Scoped(s)=>s.close()}}
 fn bind(&self,info:&StreamInfo,epoch:u64){#[cfg(feature="cloudgime-scoped-recovery-metadata")]if let Self::Scoped(s)=self{s.bind(info,epoch);}if let Some(t)=self.acquire(){let n=t.now_us();t.record(n,n,Data::Bind{epoch,ssrc:info.ssrc,pt:info.payload_type,clock_rate:info.clock_rate});}}
 fn unbind(&self,ssrc:u32){if let Some(t)=self.acquire(){let n=t.now_us();t.record(n,n,Data::Unbind{ssrc});}#[cfg(feature="cloudgime-scoped-recovery-metadata")]if let Self::Scoped(s)=self{s.close();}}
 fn sent(&self,p:&webrtc::rtp::packet::Packet,epoch:u64){#[cfg(feature="cloudgime-scoped-recovery-metadata")]if let Self::Scoped(s)=self{s.sent(p.header.ssrc,epoch,p.header.timestamp,p.header.marker);}let _=(p,epoch);}
 fn omit(&self,n:usize){if let Some(t)=self.acquire(){t.omit(n);}}
}
// Re-export via tests only; no raw RTCP/payload is persisted.
pub(super) struct ObserverBuilder{tap:Capture,consumed:AtomicBool}
impl ObserverBuilder{
 pub(super) fn from_environment()->Option<Self>{
  #[cfg(feature="cloudgime-scoped-recovery-metadata")]
  {return scoped::ScopedCapture::environment().map(|s|Self{tap:Capture::Scoped(s),consumed:AtomicBool::new(false)});}
  #[cfg(not(feature="cloudgime-scoped-recovery-metadata"))]
  Tap::from_environment().map(|tap|Self{tap:tap.into(),consumed:AtomicBool::new(false)})
 }
}
impl InterceptorBuilder for ObserverBuilder{
 fn build(&self,_id:&str)->Result<Arc<dyn Interceptor+Send+Sync>,webrtc::interceptor::Error>{if self.consumed.swap(true,Ordering::AcqRel){return Ok(Arc::new(webrtc::interceptor::noop::NoOp{}));}Ok(Arc::new(Observer{tap:self.tap.clone(),epoch:AtomicU64::new(1),streams:std::sync::Mutex::new(Vec::new())}))}
}
struct Observer{tap:Capture,epoch:AtomicU64,streams:std::sync::Mutex<Vec<(u32,Arc<AtomicBool>)>>}
#[async_trait]impl Interceptor for Observer{
 async fn bind_rtcp_reader(&self,reader:Arc<dyn RTCPReader+Send+Sync>)->Arc<dyn RTCPReader+Send+Sync>{Arc::new(RtcpIn{inner:reader,tap:self.tap.clone()})}
 async fn bind_rtcp_writer(&self,writer:Arc<dyn RTCPWriter+Send+Sync>)->Arc<dyn RTCPWriter+Send+Sync>{Arc::new(RtcpOut{inner:writer,tap:self.tap.clone()})}
 async fn bind_local_stream(&self,info:&StreamInfo,writer:Arc<dyn RTPWriter+Send+Sync>)->Arc<dyn RTPWriter+Send+Sync>{
  if !info.mime_type.to_ascii_lowercase().starts_with("video/"){return writer;}
  let epoch=self.epoch.fetch_add(1,Ordering::Relaxed);let active=Arc::new(AtomicBool::new(true));
  if let Ok(mut streams)=self.streams.lock(){if streams.len()>=32{self.tap.omit(1);return writer;}for(ssrc,flag)in streams.iter(){if *ssrc==info.ssrc{flag.store(false,Ordering::Release);}}streams.push((info.ssrc,active.clone()));}else{self.tap.omit(1);return writer;}
  self.tap.bind(info,epoch);
  Arc::new(RtpOut{inner:writer,tap:self.tap.clone(),epoch,active})
 }
 async fn unbind_local_stream(&self,info:&StreamInfo){if let Ok(mut streams)=self.streams.lock(){for(ssrc,flag)in streams.iter(){if *ssrc==info.ssrc{flag.store(false,Ordering::Release);}}streams.retain(|(ssrc,_)|*ssrc!=info.ssrc);}self.tap.unbind(info.ssrc);}
 async fn bind_remote_stream(&self,_info:&StreamInfo,reader:Arc<dyn RTPReader+Send+Sync>)->Arc<dyn RTPReader+Send+Sync>{reader}
 async fn unbind_remote_stream(&self,_info:&StreamInfo){}
 async fn close(&self)->Result<(),webrtc::interceptor::Error>{self.tap.close();if let Ok(streams)=self.streams.lock(){for(_,flag)in streams.iter(){flag.store(false,Ordering::Release);}}Ok(())}
}
struct RtpOut{inner:Arc<dyn RTPWriter+Send+Sync>,tap:Capture,epoch:u64,active:Arc<AtomicBool>}
#[async_trait]impl RTPWriter for RtpOut{
 async fn write(&self,p:&webrtc::rtp::packet::Packet,a:&Attributes)->Result<usize,webrtc::interceptor::Error>{
  let tap=if self.active.load(Ordering::Acquire){self.tap.acquire()}else{None};
  let call=tap.as_ref().map(|t|t.begin_call());let start=tap.as_ref().map_or(0,|t|t.now_us());let result=self.inner.write(p,a).await;
  if result.as_ref().is_ok_and(|n|*n>0)&&self.active.load(Ordering::Acquire){self.tap.sent(p,self.epoch);}
  if let Some(t)=tap{let end=t.now_us();if self.active.load(Ordering::Acquire){t.record(start,end,Data::Rtp{epoch:self.epoch,ssrc:p.header.ssrc,pt:p.header.payload_type,seq:p.header.sequence_number,rtp:p.header.timestamp,marker:p.header.marker,payload_bytes:p.payload.len(),returned_bytes:result.as_ref().copied().unwrap_or(0),ok:result.is_ok()});
   #[cfg(feature="cloudgime-scoped-recovery-metadata")] {
    use super::recovery_frame::{FRAME,INDEX,COUNT,REFERENCE};
    if let (Some(&frame),Some(&index),Some(&count),Some(&reference))=(a.get(&FRAME),a.get(&INDEX),a.get(&COUNT),a.get(&REFERENCE)){
     if frame>0&&count>0&&index<count&&count<=65536&&reference<=3{t.record(start,end,Data::FramePacket{epoch:self.epoch,ssrc:p.header.ssrc,seq:p.header.sequence_number,rtp:p.header.timestamp,frame_id:frame as u64,index:index as u32,count:count as u32,reference:reference as u8});}else{t.omit(1);}
    }
   }
  }if let Some(call)=call{call.finish(t.active()&&self.active.load(Ordering::Acquire));}}
  result
 }
}
struct RtcpOut{inner:Arc<dyn RTCPWriter+Send+Sync>,tap:Capture}
#[async_trait]impl RTCPWriter for RtcpOut{
 async fn write(&self,p:&[Box<dyn RtcpPacket+Send+Sync>],a:&Attributes)->Result<usize,webrtc::interceptor::Error>{
  let Some(tap)=self.tap.acquire()else{return self.inner.write(p,a).await;};
  let call=tap.begin_call();let start=tap.now_us();let result=self.inner.write(p,a).await;let end=tap.now_us();
  for packet in p.iter().take(32){if let Some(sr)=packet.as_any().downcast_ref::<SenderReport>(){tap.record(start,end,Data::Sr{ssrc:sr.ssrc,ntp_seconds:(sr.ntp_time>>32)as u32,ntp_fraction:sr.ntp_time as u32,compact_ntp:(sr.ntp_time>>16)as u32,rtp:sr.rtp_time,returned_bytes:result.as_ref().copied().unwrap_or(0),ok:result.is_ok()});}}
  if p.len()>32{tap.omit(p.len()-32);}call.finish(tap.active());result
 }
}
struct RtcpIn{inner:Arc<dyn RTCPReader+Send+Sync>,tap:Capture}
impl RtcpIn{
 fn record(tap:&Tap,packets:&[Box<dyn RtcpPacket+Send+Sync>],at:u64){
  for p in packets.iter().take(32){let any=p.as_any();
   if let Some(rr)=any.downcast_ref::<ReceiverReport>(){for r in rr.reports.iter().take(31){tap.record(at,at,Data::Rr{sender:rr.ssrc,ssrc:r.ssrc,fraction_lost:r.fraction_lost,total_lost_raw:r.total_lost,highest_sequence:r.last_sequence_number,jitter:r.jitter,lsr:r.last_sender_report,dlsr:r.delay});}if rr.reports.len()>31{tap.omit(rr.reports.len()-31);}}
   else if let Some(nack)=any.downcast_ref::<TransportLayerNack>(){for n in nack.nacks.iter().take(64){tap.record(at,at,Data::Nack{sender:nack.sender_ssrc,ssrc:nack.media_ssrc,pid:n.packet_id,blp:n.lost_packets});}if nack.nacks.len()>64{tap.omit(nack.nacks.len()-64);}}
   else if let Some(remb)=any.downcast_ref::<ReceiverEstimatedMaximumBitrate>(){if remb.bitrate.is_finite()&&remb.bitrate>=0.0{for ssrc in remb.ssrcs.iter().take(32){tap.record(at,at,Data::Remb{sender:remb.sender_ssrc,ssrc:*ssrc,bitrate:remb.bitrate});}if remb.ssrcs.len()>32{tap.omit(remb.ssrcs.len()-32);}}else{tap.omit(1);}}
   else if let Some(pli)=any.downcast_ref::<PictureLossIndication>(){tap.record(at,at,Data::Pli{sender:pli.sender_ssrc,ssrc:pli.media_ssrc});}
  }if packets.len()>32{tap.omit(packets.len()-32);}
 }
}
#[async_trait]impl RTCPReader for RtcpIn{
 async fn read(&self,b:&mut[u8],a:&Attributes)->Result<(Vec<Box<dyn RtcpPacket+Send+Sync>>,Attributes),webrtc::interceptor::Error>{
  let result=self.inner.read(b,a).await;if let Some(tap)=self.tap.acquire(){let at=tap.now_us();match &result{Ok((packets,_))=>Self::record(&tap,packets,at),Err(_)=>tap.record(at,at,Data::ReadError)}}result
 }
}
#[cfg(test)]#[path="recovery_observer_tests.rs"]mod tests;
