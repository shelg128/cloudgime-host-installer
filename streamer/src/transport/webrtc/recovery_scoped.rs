//! Deferred local QA activation. No public endpoint, no media/recovery changes.
use std::{collections::HashMap,fs::{File,OpenOptions},io::{Read,Write},path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,atomic::{AtomicBool,AtomicU64,Ordering}},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use common::qa_trace_scope::{TraceContext,TraceRequest,rental_matches};
use super::recorder::{Tap,Data};
use webrtc::interceptor::stream_info::StreamInfo;
const WAIT_LIMIT:Duration=Duration::from_secs(900);
#[derive(Clone)]struct Binding{epoch:u64,ssrc:u32,pt:u8,rate:u32,last_rtp:Option<u32>,frames:u64,last_progress:Option<Instant>}
#[derive(Default)]struct Progress{bindings:HashMap<u32,Binding>,ambiguous:bool}
pub(super) struct ScopedCapture{tap:OnceLock<Tap>,progress:Mutex<Progress>,packets_sent:AtomicU64,closed:AtomicBool}
fn now_ms()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()as u64}
fn read_bounded(path:&Path)->Option<serde_json::Value>{let f=File::open(path).ok()?;if f.metadata().ok()?.len()>4096{return None;}let mut text=String::new();f.take(4097).read_to_string(&mut text).ok()?;if text.len()>4096{return None;}serde_json::from_str(&text).ok()}
fn write_new(path:&Path,value:&serde_json::Value)->std::io::Result<()>{let mut f=OpenOptions::new().write(true).create_new(true).open(path)?;serde_json::to_writer(&mut f,value)?;f.flush()}
impl ScopedCapture{
 pub(super) fn environment()->Option<Arc<Self>>{
  let raw=std::env::var("CG_QA_TRACE_PARENT_CONTEXT").ok()?;if raw.len()>4096{return None;}let ctx:TraceContext=serde_json::from_str(&raw).ok()?;if !ctx.valid(now_ms())||ctx.rental_session_id.is_none(){return None;}
  let dir=PathBuf::from(std::env::var_os("CG_QA_SCOPED_TRACE_DIR")?);if !dir.is_absolute(){return None;}let dir=dir.canonicalize().ok()?;if !dir.is_dir()||dir.file_name()?.to_str()?!=ctx.connection_nonce{return None;}
  let rental=PathBuf::from("C:\\Users\\CloudGimeRenter\\AppData\\Roaming\\SentinelLauncher\\state\\session.json");
  Self::start(ctx,dir,rental)
 }
 pub(super) fn start(ctx:TraceContext,dir:PathBuf,rental:PathBuf)->Option<Arc<Self>>{
  if !ctx.valid(now_ms())||ctx.rental_session_id.is_none(){return None;}
  let obj=Arc::new(Self{tap:OnceLock::new(),progress:Mutex::new(Progress::default()),packets_sent:AtomicU64::new(0),closed:AtomicBool::new(false)});let weak=Arc::downgrade(&obj);
  write_new(&dir.join("peer.json"),&serde_json::json!({"schema":1,"context":ctx,"process":std::process::id(),"readiness":"SDK-send progress only; not renderer or billing readiness","mode":"waiting"})).ok()?;
  std::thread::Builder::new().name("cg-qa-trace-gate".into()).spawn(move||{
   let began=Instant::now();let mut sampled_at=began;let mut sampled_packets=0u64;let mut activated=false;let mut announced=false;let mut rejection_written=false;
   loop{
    std::thread::sleep(Duration::from_millis(100));let Some(obj)=weak.upgrade()else{break;};
    let valid=ctx.valid(now_ms())&&read_bounded(&rental).is_some_and(|s|rental_matches(&ctx,&s));
    if obj.closed.load(Ordering::Acquire)||began.elapsed()>=WAIT_LIMIT||!ctx.valid(now_ms())||dir.join("stop.json").exists(){obj.close();break;}
    if activated{let tap=obj.acquire();if !valid||tap.as_ref().is_none_or(|t|!t.active()){obj.close();break;}continue;}
    if !valid{continue;}
    let ready=obj.ready_binding();
    if sampled_at.elapsed()>=Duration::from_millis(500){
     let elapsed=sampled_at.elapsed();let packets=obj.packets_sent.load(Ordering::Acquire);
     if let Some(binding)=ready.as_ref(){let snapshot=serde_json::json!({"schema":1,"context":ctx,"process":std::process::id(),"epoch":binding.epoch,"ssrc":binding.ssrc,"pt":binding.pt,"observed_at_ms":now_ms(),"sample_ms":elapsed.as_millis(),"packets_sent":packets,"packets_per_second":(packets.saturating_sub(sampled_packets))as f64/elapsed.as_secs_f64(),"progress_frames":binding.frames,"active_trace":false});if std::fs::write(dir.join("progress.json"),snapshot.to_string()).is_err(){obj.close();break;}}
     sampled_packets=packets;sampled_at=Instant::now();
    }
    if let Some(binding)=ready.as_ref(){if !announced{
     if write_new(&dir.join("ready.json"),&serde_json::json!({"schema":1,"context":ctx,"process":std::process::id(),"epoch":binding.epoch,"ssrc":binding.ssrc,"pt":binding.pt,"frames_sent":binding.frames,"readiness":"at least3 distinct marked RTP sends, fresh within1s; no renderer claim"})).is_err(){obj.close();break;}announced=true;
    }}
    let Some(value)=read_bounded(&dir.join("request.json"))else{continue;};let request=serde_json::from_value::<TraceRequest>(value);
    let approved=request.as_ref().ok().is_some_and(|r|r.matches(&ctx,now_ms()));
    if !approved||ready.is_none(){if !rejection_written{let _=write_new(&dir.join("rejected.json"),&serde_json::json!({"reason":"scope_expiry_or_readiness_rejected"}));rejection_written=true;}continue;}
    // Revalidate on worker immediately before publication. Parent-owned process stops on native revoke.
    if !read_bounded(&rental).is_some_and(|s|rental_matches(&ctx,&s)){continue;}
    let request=request.unwrap();let Some(binding)=obj.ready_binding()else{continue;};
    let path=dir.join(format!("capture-{}.jsonl",request.request_nonce));let Ok(file)=OpenOptions::new().write(true).create_new(true).open(&path)else{obj.close();break;};
    let scope=serde_json::json!({"context":ctx,"request_nonce":request.request_nonce,"rental_state_matched":true,"binding_epoch":binding.epoch,"binding_ssrc":binding.ssrc,"trust":"authorized-parent context plus trusted local rental state/operator; not independent billing attestation"});
    let Ok(tap)=Tap::start_scoped(file,request.request_nonce.clone(),binding.epoch,Duration::from_millis(request.duration_ms),scope)else{obj.close();break;};
    tap.record(0,0,Data::Bind{epoch:binding.epoch,ssrc:binding.ssrc,pt:binding.pt,clock_rate:binding.rate});
    if obj.closed.load(Ordering::Acquire){tap.close();break;}
    if obj.tap.set(tap.clone()).is_err(){tap.close();obj.close();break;}if obj.closed.load(Ordering::Acquire){tap.close();break;}
    let _=write_new(&dir.join("armed.json"),&serde_json::json!({"request_nonce":request.request_nonce,"file":path.file_name().and_then(|x|x.to_str()),"duration_ms":request.duration_ms,"at_ms":now_ms()}));activated=true;
   }
   let _=write_new(&dir.join("gate-stopped.json"),&serde_json::json!({"activated":activated,"at_ms":now_ms()}));
  }).ok()?;Some(obj)
 }
 pub(super) fn acquire(&self)->Option<Tap>{if self.closed.load(Ordering::Acquire){return None;}self.tap.get().filter(|t|t.active()).cloned()}
 pub(super) fn bind(&self,info:&StreamInfo,epoch:u64){if self.closed.load(Ordering::Acquire){return;}if let Ok(mut p)=self.progress.try_lock(){if !p.bindings.is_empty(){p.ambiguous=true;drop(p);self.close();return;}if p.bindings.len()<2{p.bindings.insert(info.ssrc,Binding{epoch,ssrc:info.ssrc,pt:info.payload_type,rate:info.clock_rate,last_rtp:None,frames:0,last_progress:None});}}else{self.close();}}
 pub(super) fn sent(&self,ssrc:u32,epoch:u64,rtp:u32,marker:bool){if self.closed.load(Ordering::Relaxed){return;}self.packets_sent.fetch_add(1,Ordering::Relaxed);if !marker{return;}if let Ok(mut p)=self.progress.try_lock(){if let Some(b)=p.bindings.get_mut(&ssrc){if b.epoch==epoch&&b.last_rtp.is_none_or(|last|{let delta=rtp.wrapping_sub(last);delta>0&&delta<0x8000_0000}){b.last_rtp=Some(rtp);b.frames=b.frames.saturating_add(1);b.last_progress=Some(Instant::now());}}}}
 fn ready_binding(&self)->Option<Binding>{let p=self.progress.try_lock().ok()?;if p.ambiguous||p.bindings.len()!=1{return None;}let b=p.bindings.values().next()?;if b.frames<3||b.last_progress.is_none_or(|t|t.elapsed()>Duration::from_secs(1)){return None;}Some(b.clone())}
 pub(super) fn close(&self){self.closed.store(true,Ordering::Release);if let Some(tap)=self.tap.get(){tap.close();}}
}
impl Drop for ScopedCapture{fn drop(&mut self){self.close();}}
