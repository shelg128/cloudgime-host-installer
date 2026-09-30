//! QA-only datagram duplication. Inspect only the clear SRTP header; never
//! decrypt, rewrite, re-encrypt or allocate another RTP sequence number.
use std::sync::atomic::{AtomicU64,Ordering};
use std::time::{Duration,Instant};

pub(crate) fn eligible(buf:&[u8])->bool {
    // Installed experiment uses negotiated video PT 96..=103. Do not treat
    // audio PT111, RTCP, STUN, DTLS or TURN ChannelData as eligible video.
    (22..=1280).contains(&buf.len()) && (buf[0]>>6)==2 && (96..=103).contains(&(buf[1]&127))
}

pub(crate) struct Stats {
    pub originals:AtomicU64,pub original_bytes:AtomicU64,
    pub copies:AtomicU64,pub copy_bytes:AtomicU64,pub skipped:AtomicU64,
    last_log:AtomicU64,start:Instant,
}
impl Default for Stats {
 fn default()->Self{Self{originals:AtomicU64::new(0),original_bytes:AtomicU64::new(0),copies:AtomicU64::new(0),copy_bytes:AtomicU64::new(0),skipped:AtomicU64::new(0),last_log:AtomicU64::new(0),start:Instant::now()}}
}
impl Stats {
 pub(crate) fn record(&self,bytes:usize,copied:bool)->Option<String>{
  self.originals.fetch_add(1,Ordering::Relaxed);self.original_bytes.fetch_add(bytes as u64,Ordering::Relaxed);
  if copied{self.copies.fetch_add(1,Ordering::Relaxed);self.copy_bytes.fetch_add(bytes as u64,Ordering::Relaxed);}else{self.skipped.fetch_add(1,Ordering::Relaxed);}
  let bucket=(self.start.elapsed().as_secs()/5)+1;let previous=self.last_log.load(Ordering::Relaxed);
  if bucket>previous && self.last_log.compare_exchange(previous,bucket,Ordering::Relaxed,Ordering::Relaxed).is_ok(){Some(format!("CG_DUP_QA original={} copy={} skipped={} extraBytes={}",self.originals.load(Ordering::Relaxed),self.copies.load(Ordering::Relaxed),self.skipped.load(Ordering::Relaxed),self.copy_bytes.load(Ordering::Relaxed)))}else{None}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn packet(first:u8,second:u8,len:usize)->Vec<u8>{let mut b=vec![0;len];if len>0{b[0]=first;}if len>1{b[1]=second;}b}
 #[test]fn video_and_marker_are_eligible_without_mutation(){for pt in 96..=103{let b=packet(0x90,pt|0x80,1222);let copy=b.clone();assert!(eligible(&b));assert_eq!(b,copy);}}
 #[test]fn no_audio_control_dtls_stun_turn(){for (a,b)in[(0x80,111),(0x80,200),(0x80,205),(0x00,0x01),(23,0xfe),(0x40,98),(0x7f,98)]{assert!(!eligible(&packet(a,b,100)));}}
 #[test]fn size_boundaries_are_checked_before_indexing(){for len in 0..22{assert!(!eligible(&packet(0x80,98,len)));}assert!(!eligible(&packet(0x80,98,1281)));assert!(eligible(&packet(0x80,98,1280)));}
 #[test]fn extra_bytes_and_skipped_are_separate(){let s=Stats::default();s.record(100,true);s.record(200,false);assert_eq!(s.original_bytes.load(Ordering::Relaxed),300);assert_eq!(s.copy_bytes.load(Ordering::Relaxed),100);assert_eq!(s.copies.load(Ordering::Relaxed),1);assert_eq!(s.skipped.load(Ordering::Relaxed),1);}
}
