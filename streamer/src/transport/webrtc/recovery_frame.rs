//! Numeric-only metadata transported in SDK Attributes, never on the RTP wire.
use std::sync::atomic::{AtomicU64,Ordering};
use webrtc::{interceptor::Attributes,rtp::packet::Packet};
static NEXT_FRAME:AtomicU64=AtomicU64::new(1);
pub(super) const FRAME:usize=0x43474601;
pub(super) const INDEX:usize=0x43474602;
pub(super) const COUNT:usize=0x43474603;
pub(super) const REFERENCE:usize=0x43474604;
pub struct VideoSample{pub packet:Packet,pub frame_id:u64,pub index:u32,pub count:u32,pub reference:u8}
// 0 unknown, 1 key, 2 reference, 3 non-reference header only. Never a drop certificate.
pub(super) fn classify_h264(samples:&[bytes::BytesMut])->u8{let(mut vcl,mut refs,mut idrs,mut unsupported)=(0,0,0,false);for nal in samples{if nal.is_empty()||nal[0]&128!=0{return 0;}let t=nal[0]&31;if !matches!(t,1|5|6|7|8|9|10|11|12){unsupported=true;}if matches!(t,1|5){if nal.len()<2{return 0;}vcl+=1;if nal[0]&0x60!=0{refs+=1;}if t==5{idrs+=1;}}}if unsupported||vcl==0{return 0;}if idrs>0{return if idrs==vcl&&refs==vcl{1}else{0};}if refs==vcl{2}else if refs==0{3}else{0}}
pub(super) fn attach(packets:Vec<Packet>,reference:u8)->Vec<VideoSample>{let frame_id=NEXT_FRAME.fetch_add(1,Ordering::Relaxed);let count=packets.len()as u32;packets.into_iter().enumerate().map(|(index,packet)|VideoSample{packet,frame_id,index:index as u32,count,reference}).collect()}
impl VideoSample{pub(super) fn attributes(&self)->Attributes{if self.frame_id==0{return Attributes::new();}Attributes::from([(FRAME,self.frame_id as usize),(INDEX,self.index as usize),(COUNT,self.count as usize),(REFERENCE,self.reference as usize)])}}
#[cfg(test)]mod tests{use super::*;
#[test]fn only_internal_attributes_change(){let p=Packet::default();let a=attach(vec![p.clone(),p.clone()],2);assert_eq!(a[0].packet,p);assert_eq!(a[0].frame_id,a[1].frame_id);assert_eq!(a[1].index,1);assert_eq!(a[1].attributes()[&COUNT],2);assert_ne!(a[0].frame_id,attach(vec![p],2)[0].frame_id);}
#[test]fn classification_conservative(){use bytes::BytesMut as B;assert_eq!(classify_h264(&[B::from(&b"\x65\x88"[..])]),1);assert_eq!(classify_h264(&[B::from(&b"\x61\x88"[..])]),2);assert_eq!(classify_h264(&[B::from(&b"\x01\x88"[..])]),3);assert_eq!(classify_h264(&[B::from(&b"\x62\x88"[..])]),0);assert_eq!(classify_h264(&[]),0);}
}
