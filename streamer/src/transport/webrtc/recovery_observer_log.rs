//! Default-off, bounded metadata-only SDK boundary observer. No recovery policy.
use std::{fs::{File,OpenOptions},io::{BufWriter,Write},path::PathBuf,sync::{Arc,atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering},mpsc::{self,SyncSender,TrySendError}},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use serde::Serialize;

const QUEUE:usize=512;
const MAX_EVENTS:u64=50_000;
const MAX_BYTES:usize=16*1024*1024;
const MAX_SECONDS:u64=60;
static PEERS:AtomicU64=AtomicU64::new(1);
// One environment-armed collector per process; peer recreation cannot multiply quotas.
static ENV_USED:AtomicBool=AtomicBool::new(false);

#[derive(Clone,Debug,serde::Deserialize,Serialize)]
#[serde(tag="kind")]
pub(super) enum Data {
 Bind{epoch:u64,ssrc:u32,pt:u8,clock_rate:u32},
 Unbind{ssrc:u32},
 Rtp{epoch:u64,ssrc:u32,pt:u8,seq:u16,rtp:u32,marker:bool,payload_bytes:usize,returned_bytes:usize,ok:bool},
 Sr{ssrc:u32,ntp_seconds:u32,ntp_fraction:u32,compact_ntp:u32,rtp:u32,returned_bytes:usize,ok:bool},
 Rr{sender:u32,ssrc:u32,fraction_lost:u8,total_lost_raw:u32,highest_sequence:u32,jitter:u32,lsr:u32,dlsr:u32},
 Nack{sender:u32,ssrc:u32,pid:u16,blp:u16},
 Remb{sender:u32,ssrc:u32,bitrate:f32},
 Pli{sender:u32,ssrc:u32},
 FramePacket{epoch:u64,ssrc:u32,seq:u16,rtp:u32,frame_id:u64,index:u32,count:u32,reference:u8},
 ReadError,
}
#[derive(Clone,Debug,serde::Deserialize,Serialize)]
pub(super) struct Event{pub id:u64,pub started_us:u64,pub completed_us:u64,pub data:Data}
#[derive(Default)]
struct State{#[cfg(test)]rtp_writes:AtomicU64,#[cfg(test)]rr_with_lsr:AtomicU64,active:AtomicBool,in_flight:AtomicUsize,outstanding_calls:AtomicUsize,cancelled_calls:AtomicU64,attempted:AtomicU64,accepted:AtomicU64,dropped:AtomicU64,omitted:AtomicU64,quota:AtomicBool}
#[derive(Clone)]
pub(super) struct Tap{tx:SyncSender<Event>,state:Arc<State>,began:Instant,deadline:Instant}
pub(super) struct PendingCall{state:Arc<State>,finished:bool}
impl PendingCall{pub(super) fn finish(mut self,within_window:bool){self.finished=true;if !within_window{self.state.omitted.fetch_add(1,Ordering::Relaxed);}}}
impl Drop for PendingCall{fn drop(&mut self){if !self.finished{self.state.cancelled_calls.fetch_add(1,Ordering::AcqRel);}self.state.outstanding_calls.fetch_sub(1,Ordering::Release);}}
#[derive(Debug)]
struct Config{dir:PathBuf,nonce:String,expires_ms:u64}
fn wall_ms()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()as u64}
impl Config{
 fn parse(dir:PathBuf,nonce:String,expires_ms:u64,now:u64)->Option<Self>{
  if !dir.is_absolute()||nonce.len()!=32||!nonce.bytes().all(|x|x.is_ascii_hexdigit())||expires_ms<=now||expires_ms-now>MAX_SECONDS*1000{return None;}
  let dir=dir.canonicalize().ok()?;if !dir.is_dir(){return None;}Some(Self{dir,nonce,expires_ms})
 }
 fn environment()->Option<Self>{Self::parse(std::env::var_os("CG_QA_RECOVERY_TRACE_DIR")?.into(),std::env::var("CG_QA_RECOVERY_TRACE_NONCE").ok()?,std::env::var("CG_QA_RECOVERY_TRACE_EXPIRES_MS").ok()?.parse().ok()?,wall_ms())}
}
impl Tap{
 pub(super) fn from_environment()->Option<Self>{
  let config=Config::environment()?;if ENV_USED.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return None;}let peer=PEERS.fetch_add(1,Ordering::Relaxed);
  let path=config.dir.join(format!("recovery-{}-{}-{}.jsonl",config.nonce,std::process::id(),peer));
  let file=OpenOptions::new().write(true).create_new(true).open(path).ok()?;
  let duration=Duration::from_millis(config.expires_ms.saturating_sub(wall_ms()).min(MAX_SECONDS*1000));
  Self::start(file,config.nonce,peer,duration).ok()
 }
 fn start(file:File,nonce:String,peer:u64,duration:Duration)->std::io::Result<Self>{Self::start_with_scope(file,nonce,peer,duration,None)}
 #[cfg(feature="cloudgime-scoped-recovery-metadata")]
 pub(super) fn start_scoped(file:File,nonce:String,peer:u64,duration:Duration,scope:serde_json::Value)->std::io::Result<Self>{Self::start_with_scope(file,nonce,peer,duration,Some(scope))}
 fn start_with_scope(file:File,nonce:String,peer:u64,duration:Duration,scope:Option<serde_json::Value>)->std::io::Result<Self>{
  let(mut writer,began)=(BufWriter::new(file),Instant::now());
  serde_json::to_writer(&mut writer,&serde_json::json!({"type":"header","schema":1,"nonce":nonce,"process":std::process::id(),"peer":peer,"clock":"peer-local Instant microseconds","scope":"SDK peer only; see optional parent_binding for operator-authorized mapping","parent_binding":scope,"rtp_boundary":"innermost SDK RTP writer before SRTP; begin/end of awaited call, not NIC egress","rtcp_boundary":"SR write begin/end; RTCP read completion after SRTP decrypt before default interceptors","payload_retained":false,"max_events":MAX_EVENTS,"max_bytes":MAX_BYTES,"max_duration_ms":duration.as_millis()}))?;writer.write_all(b"\n")?;writer.flush()?;
  let(tx,rx)=mpsc::sync_channel(QUEUE);let state=Arc::new(State::default());state.active.store(true,Ordering::Release);let status=state.clone();let deadline=began+duration;
  std::thread::Builder::new().name("cg-qa-meta-writer".into()).spawn(move||{
   let(mut written,mut bytes,mut io_error,mut reason)=(0u64,0usize,false,"closed");
   loop{
    if Instant::now()>=deadline{status.active.store(false,Ordering::Release);reason="expired";}
    match rx.recv_timeout(Duration::from_millis(20)){
     Ok(event)=>{if io_error{continue;}match serde_json::to_vec(&event){Ok(line)=>{if bytes+line.len()+1>MAX_BYTES-4096{status.active.store(false,Ordering::Release);status.quota.store(true,Ordering::Release);reason="byte_quota";status.omitted.fetch_add(1,Ordering::Relaxed);continue;}if writer.write_all(&line).and_then(|_|writer.write_all(b"\n")).is_err(){io_error=true;status.active.store(false,Ordering::Release);reason="write_error";}else{written+=1;bytes+=line.len()+1;}},Err(_)=>{io_error=true;status.active.store(false,Ordering::Release);reason="serialize_error";}}},
     Err(mpsc::RecvTimeoutError::Disconnected)=>break,
     Err(mpsc::RecvTimeoutError::Timeout)=>{if !status.active.load(Ordering::Acquire)&&status.in_flight.load(Ordering::Acquire)==0{break;}}
    }
   }
   let accepted=status.accepted.load(Ordering::Acquire);let dropped=status.dropped.load(Ordering::Acquire);let omitted=status.omitted.load(Ordering::Acquire);let quota=status.quota.load(Ordering::Acquire);
   let outstanding=status.outstanding_calls.load(Ordering::Acquire);let cancelled=status.cancelled_calls.load(Ordering::Acquire);
   let complete=!io_error&&dropped==0&&omitted==0&&!quota&&written==accepted&&outstanding==0&&cancelled==0;
   let footer=serde_json::json!({"type":"footer","reason":reason,"accepted":accepted,"written":written,"dropped_queue":dropped,"omitted":omitted,"quota":quota,"io_error":io_error,"outstanding_calls":outstanding,"cancelled_calls":cancelled,"observation_complete":complete,"scope":"observed SDK window only; expiry is not whole-session completeness","elapsed_us":began.elapsed().as_micros()});
   let _=serde_json::to_writer(&mut writer,&footer);let _=writer.write_all(b"\n");let _=writer.flush();
  })?;
  Ok(Self{tx,state,began,deadline})
 }
 pub(super) fn begin_call(&self)->PendingCall{self.state.outstanding_calls.fetch_add(1,Ordering::AcqRel);PendingCall{state:self.state.clone(),finished:false}}
 pub(super) fn now_us(&self)->u64{self.began.elapsed().as_micros()as u64}
 pub(super) fn active(&self)->bool{self.state.active.load(Ordering::Relaxed)&&Instant::now()<self.deadline}
 pub(super) fn omit(&self,n:usize){if self.active(){self.state.omitted.fetch_add(n as u64,Ordering::Relaxed);}}
 pub(super) fn record(&self,started_us:u64,completed_us:u64,data:Data){
  #[cfg(test)]if matches!(&data,Data::Rtp{ok:true,..}){self.state.rtp_writes.fetch_add(1,Ordering::Release);}
  #[cfg(test)]if matches!(&data,Data::Rr{lsr,..}if *lsr!=0){self.state.rr_with_lsr.fetch_add(1,Ordering::Release);}
  if !self.active(){return;}
  self.state.in_flight.fetch_add(1,Ordering::AcqRel);
  if !self.state.active.load(Ordering::Acquire){self.state.in_flight.fetch_sub(1,Ordering::Release);return;}
  let id=self.state.attempted.fetch_add(1,Ordering::Relaxed);
  if id>=MAX_EVENTS{self.state.quota.store(true,Ordering::Release);self.state.active.store(false,Ordering::Release);self.state.in_flight.fetch_sub(1,Ordering::Release);return;}
  match self.tx.try_send(Event{id,started_us,completed_us,data}){Ok(())=>{self.state.accepted.fetch_add(1,Ordering::Release);},Err(TrySendError::Full(_))=>{self.state.dropped.fetch_add(1,Ordering::Relaxed);},Err(TrySendError::Disconnected(_))=>{self.state.omitted.fetch_add(1,Ordering::Relaxed);self.state.active.store(false,Ordering::Release);}}
  self.state.in_flight.fetch_sub(1,Ordering::Release);
 }
 #[cfg(test)]pub(super) fn rtp_writes(&self)->u64{self.state.rtp_writes.load(Ordering::Acquire)}
 #[cfg(test)]pub(super) fn has_rr_lsr(&self)->bool{self.state.rr_with_lsr.load(Ordering::Acquire)>0}
 pub(super) fn close(&self){self.state.active.store(false,Ordering::Release);}
 #[cfg(test)] pub(super) fn test_file()->(Self,PathBuf){let dir=std::env::var_os("CG_QA_TEST_EVIDENCE_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);let peer=PEERS.fetch_add(1,Ordering::Relaxed);let path=dir.join(format!("sdk-trace-{}-{}.jsonl",std::process::id(),peer));let file=OpenOptions::new().write(true).create_new(true).open(&path).unwrap();(Self::start(file,"a".repeat(32),peer,Duration::from_secs(15)).unwrap(),path)}
 #[cfg(test)] pub(super) fn memory(capacity:usize)->(Self,mpsc::Receiver<Event>){let(tx,rx)=mpsc::sync_channel(capacity);let state=Arc::new(State::default());state.active.store(true,Ordering::Release);let began=Instant::now();(Self{tx,state,began,deadline:began+Duration::from_secs(60)},rx)}
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn config_rejects_invalid_scope_and_lifetime(){let dir=std::env::temp_dir();assert!(Config::parse(dir.clone(),"f".repeat(32),2000,1000).is_some());assert!(Config::parse(dir.clone(),"../a".into(),2000,1000).is_none());assert!(Config::parse(dir.clone(),"f".repeat(32),1000,1000).is_none());assert!(Config::parse(dir,"f".repeat(32),62000,1000).is_none());assert!(Config::parse("relative".into(),"f".repeat(32),2000,1000).is_none());}
 #[test]fn queue_full_never_blocks(){let(t,rx)=Tap::memory(1);for _ in 0..10000{t.record(0,1,Data::ReadError);}assert_eq!(t.state.accepted.load(Ordering::Acquire),1);assert_eq!(t.state.dropped.load(Ordering::Acquire),9999);assert!(rx.try_recv().is_ok());}
 #[test]fn closed_and_expired_ignore_new_events(){let(mut t,rx)=Tap::memory(1);t.deadline=Instant::now();t.record(0,0,Data::ReadError);assert!(rx.try_recv().is_err());t.close();assert!(!t.active());}
 #[test]fn bounded_event_quota_stops_collector(){let(t,_rx)=Tap::memory(1);t.state.attempted.store(MAX_EVENTS,Ordering::Release);t.record(0,0,Data::ReadError);assert!(!t.active());assert!(t.state.quota.load(Ordering::Acquire));}
 #[test]fn metadata_serializes_no_payload(){let(t,rx)=Tap::memory(1);t.record(2,4,Data::Rtp{epoch:1,ssrc:9,pt:98,seq:42,rtp:123,marker:true,payload_bytes:1200,returned_bytes:1212,ok:true});let e=rx.try_recv().unwrap();let text=serde_json::to_string(&e).unwrap();assert!(text.contains("payload_bytes"));assert!(!text.contains("payload\""));}
 #[test]fn writer_expires_and_footer_is_present(){let path=std::env::temp_dir().join(format!("cg-meta-test-{}-{}.jsonl",std::process::id(),PEERS.fetch_add(1,Ordering::Relaxed)));let file=OpenOptions::new().write(true).create_new(true).open(&path).unwrap();let t=Tap::start(file,"f".repeat(32),1,Duration::from_millis(40)).unwrap();t.record(0,1,Data::ReadError);std::thread::sleep(Duration::from_millis(120));let text=std::fs::read_to_string(&path).unwrap();let f:serde_json::Value=serde_json::from_str(text.lines().last().unwrap()).unwrap();assert_eq!(f["type"],"footer");assert_eq!(f["written"],1);assert_eq!(f["observation_complete"],true);assert!(!t.active());}
}
