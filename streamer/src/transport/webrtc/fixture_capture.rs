//! Opt-in QA encoded fixture capture. Not compiled into normal builds.
use std::{fs::{self, OpenOptions},io::Write,path::PathBuf,sync::mpsc::{self, SyncSender},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use serde::Deserialize;
const MAX_FRAMES:usize=180;
const MAX_BYTES:usize=16*1024*1024;
#[derive(Deserialize)]
struct Request{session_id:String,claim_id:String,expires_ms:u64,fixture_state:PathBuf,rental_state:PathBuf,output_directory:PathBuf}
struct Packet{rtp:u32,micros:u64,idr:bool,data:Vec<u8>}
pub(super) struct FixtureCapture{began:Instant,last_check:Instant,request:Option<Request>,sender:Option<SyncSender<Packet>>,frames:usize,bytes:usize,armed:bool,started:bool,done:bool}
impl Default for FixtureCapture{fn default()->Self{Self{began:Instant::now(),last_check:Instant::now()-Duration::from_secs(1),request:None,sender:None,frames:0,bytes:0,armed:false,started:false,done:false}}}
fn valid_ids(session:&str,claim:&str)->bool{session.len()==36&&session.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-')&&claim.starts_with("claim_")&&claim.len()>=20&&claim.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')}
fn request_file()->PathBuf{PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(||"C:\\ProgramData".into())).join("Cloudgime/qa-encoded-fixture/request.json")}
fn valid_state(f:&serde_json::Value,s:&serde_json::Value,r:&Request)->bool{
 f["sessionId"].as_str()==Some(&r.session_id)&&f["claimId"].as_str()==Some(&r.claim_id)&&f["foreground"].as_bool()==Some(true)&&f["stress"].as_bool()==Some(true)&&s["RentalSessionId"].as_str()==Some(&r.session_id)&&s["ClaimId"].as_str()==Some(&r.claim_id)
}
fn valid(r:&Request)->bool{
 let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()as u64;
 if !valid_ids(&r.session_id,&r.claim_id)||r.expires_ms<now||r.expires_ms>now+60_000{return false;}
 let expected=request_file().parent().unwrap().join(&r.session_id);
 if r.output_directory!=expected{return false;}
 let fixture_root=PathBuf::from("C:\\Users\\CloudGimeRenter\\AppData\\Local\\CgLoadQA");
 if r.fixture_state!=fixture_root.join(&r.session_id).join("state.json")||r.rental_state!=PathBuf::from("C:\\Users\\CloudGimeRenter\\AppData\\Roaming\\SentinelLauncher\\state\\session.json"){return false;}
 let Ok(meta)=fs::metadata(&r.fixture_state)else{return false;};if meta.modified().ok().and_then(|t|t.elapsed().ok()).is_none_or(|age|age>Duration::from_secs(3)){return false;}
 let Ok(f)=fs::read(&r.fixture_state)else{return false;};let Ok(f)=serde_json::from_slice::<serde_json::Value>(&f)else{return false;};
 if f["sessionId"].as_str()!=Some(&r.session_id)||f["claimId"].as_str()!=Some(&r.claim_id)||f["foreground"].as_bool()!=Some(true)||f["stress"].as_bool()!=Some(true){return false;}
 let Ok(s)=fs::read(&r.rental_state)else{return false;};let Ok(s)=serde_json::from_slice::<serde_json::Value>(&s)else{return false;};
 if !valid_state(&f,&s,r){return false;}
 // The stream bridge may be in session 0. Foreground proof is produced by
 // the exact interactive fixture UI thread, not the service desktop.
 true
}
impl FixtureCapture{
 pub(super) fn observe(&mut self,data:&[u8],rtp:u32,idr:bool)->bool{
  if self.done{return false;}
  if self.last_check.elapsed()>=Duration::from_millis(250){self.last_check=Instant::now();if !self.armed{let Ok(raw)=fs::read(request_file())else{return false;};let Ok(r)=serde_json::from_slice::<Request>(&raw)else{return false;};if !valid(&r){return false;}self.request=Some(r);self.armed=true;self.began=Instant::now();return true;}if self.request.as_ref().is_none_or(|r|!valid(r)) {self.stop("scope_or_fixture_changed");return false;}}
  if !self.armed{return false;}
  if self.began.elapsed()>Duration::from_secs(12){self.stop("deadline");return false;}
  if !self.started{if !idr{return false;}let Some(r)=self.request.as_ref()else{return false;};let path=r.output_directory.join("frames.cgr");let Ok(mut file)=OpenOptions::new().write(true).create_new(true).open(&path)else{self.done=true;return false;};let(tx,rx)=mpsc::sync_channel::<Packet>(4);let status=r.output_directory.join("writer.json");std::thread::spawn(move||{let mut n=0;let mut bytes=0;let mut error=None;while let Ok(p)=rx.recv(){let result=(||->std::io::Result<()>{file.write_all(&(p.data.len()as u32).to_le_bytes())?;file.write_all(&p.rtp.to_le_bytes())?;file.write_all(&p.micros.to_le_bytes())?;file.write_all(&(u32::from(p.idr)).to_le_bytes())?;file.write_all(&p.data)?;Ok(())})();if result.is_err(){error=Some("write_failed");break;}n+=1;bytes+=p.data.len();}if file.flush().is_err(){error=Some("flush_failed");}let _=fs::write(status,serde_json::json!({"frames":n,"bytes":bytes,"error":error}).to_string());});self.sender=Some(tx);self.started=true;}
  if data.len()>MAX_BYTES.saturating_sub(self.bytes){self.stop("byte_limit");return false;}
  let p=Packet{rtp,micros:self.began.elapsed().as_micros()as u64,idr,data:data.to_vec()};
  if self.sender.as_ref().is_none_or(|s|s.try_send(p).is_err()){self.stop("writer_backpressure");return false;}
  self.frames+=1;self.bytes+=data.len();if self.frames>=MAX_FRAMES{self.stop("frame_limit");}false
 }
 fn stop(&mut self,reason:&str){self.done=true;self.sender.take();if let Some(r)=&self.request{let _=fs::write(r.output_directory.join("capture.json"),serde_json::json!({"frames":self.frames,"bytes":self.bytes,"reason":reason,"elapsed_ms":self.began.elapsed().as_millis(),"complete":reason=="frame_limit"}).to_string());}}
}
impl Drop for FixtureCapture{fn drop(&mut self){if self.armed&&!self.done{self.stop("dropped");}}}
#[cfg(test)]mod tests{
 use super::*;
 fn request()->Request{Request{session_id:"088db39c-5e5e-4d2a-814c-f3b9a5e2f5ba".into(),claim_id:"claim_b47a9da1b7203efb97168a6d98ead476".into(),expires_ms:0,fixture_state:PathBuf::new(),rental_state:PathBuf::new(),output_directory:PathBuf::new()}}
 fn states(r:&Request)->(serde_json::Value,serde_json::Value){(serde_json::json!({"sessionId":r.session_id,"claimId":r.claim_id,"foreground":true,"stress":true}),serde_json::json!({"RentalSessionId":r.session_id,"ClaimId":r.claim_id}))}
 #[test]fn ids_are_scoped(){let r=request();assert!(valid_ids(&r.session_id,&r.claim_id));assert!(!valid_ids("../../other",&r.claim_id));assert!(!valid_ids(&r.session_id,"../claim_1234567890123456789"));}
 #[test]fn bound_foreground_fixture_is_accepted(){let r=request();let(f,s)=states(&r);assert!(valid_state(&f,&s,&r));}
 #[test]fn foreground_and_stress_are_mandatory(){let r=request();for key in ["foreground","stress"]{let(mut f,s)=states(&r);f[key]=false.into();assert!(!valid_state(&f,&s,&r));f.as_object_mut().unwrap().remove(key);assert!(!valid_state(&f,&s,&r));}}
 #[test]fn claim_and_session_must_match_on_both_sides(){let r=request();for key in ["sessionId","claimId"]{let(mut f,s)=states(&r);f[key]="other".into();assert!(!valid_state(&f,&s,&r));}for key in ["RentalSessionId","ClaimId"]{let(f,mut s)=states(&r);s[key]="other".into();assert!(!valid_state(&f,&s,&r));}}
 #[test]fn expired_request_is_rejected_before_files(){assert!(!valid(&request()));}
 #[test]fn done_capture_never_rearms(){let mut c=FixtureCapture::default();c.done=true;assert!(!c.observe(&[0,0,1,0x65],0,true));assert!(c.sender.is_none());assert_eq!(c.frames,0);}
}
