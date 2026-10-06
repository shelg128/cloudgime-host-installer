//! Local operator QA context; parent obtains identity from an authorized native ticket.
use serde::{Deserialize,Serialize};
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct TraceContext{pub native_session_id:String,pub claim_id:String,pub handoff_binding_id:String,pub rental_session_id:Option<String>,pub connection_nonce:String,pub generation:u64,pub authority_expires_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceRequest{pub context:TraceContext,pub request_nonce:String,pub expires_ms:u64,pub duration_ms:u64}
fn id(s:&str)->bool{!s.is_empty()&&s.len()<=256&&s.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'||c==b'_')}
pub fn nonce(s:&str)->bool{s.len()==32&&s.bytes().all(|x|x.is_ascii_hexdigit())}
impl TraceContext{pub fn valid(&self,now:u64)->bool{id(&self.native_session_id)&&id(&self.claim_id)&&self.claim_id.starts_with("claim_")&&id(&self.handoff_binding_id)&&self.rental_session_id.as_ref().is_none_or(|x|id(x))&&nonce(&self.connection_nonce)&&self.generation>0&&self.authority_expires_ms>now}}
impl TraceRequest{pub fn matches(&self,ctx:&TraceContext,now:u64)->bool{self.context==*ctx&&ctx.valid(now)&&nonce(&self.request_nonce)&&self.expires_ms>now&&self.expires_ms-now<=30_000&&self.duration_ms>0&&self.duration_ms<=60_000&&now.checked_add(self.duration_ms).is_some_and(|end|end<=ctx.authority_expires_ms)}}
pub fn rental_matches(ctx:&TraceContext,s:&serde_json::Value)->bool{s["ClaimId"].as_str()==Some(ctx.claim_id.as_str())&&s["PcId"].as_str()==Some("2")&&s["RentalSessionId"].as_str().is_some_and(|id|!id.is_empty()&&ctx.rental_session_id.as_deref().is_some_and(|r|r==id))}
#[cfg(test)]mod tests{use super::*;fn c()->TraceContext{TraceContext{native_session_id:"native_1".into(),claim_id:"claim_123".into(),handoff_binding_id:"h_1".into(),rental_session_id:Some("r_1".into()),connection_nonce:"a".repeat(32),generation:1,authority_expires_ms:100000}}
#[test]fn request_exact_and_bounded(){let c=c();let mut r=TraceRequest{context:c.clone(),request_nonce:"b".repeat(32),expires_ms:2000,duration_ms:1000};assert!(r.matches(&c,1000));r.context.generation=2;assert!(!r.matches(&c,1000));r.context=c.clone();r.duration_ms=61000;assert!(!r.matches(&c,1000));}
#[test]fn missing_rental_never_invented(){let mut c=c();let state=serde_json::json!({"PcId":"2","ClaimId":"claim_123","RentalSessionId":"r_1"});assert!(rental_matches(&c,&state));c.rental_session_id=None;assert!(!rental_matches(&c,&state));}
#[test]fn pathlike_ids_and_expiry_denied(){let mut x=c();x.native_session_id="../other".into();assert!(!x.valid(1));let c=c();assert!(!c.valid(100000));}
}
