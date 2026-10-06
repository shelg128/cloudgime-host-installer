//! Operator-only QA: context is derived from an already-authorized ticket, never request JSON.
use crate::app::AndroidNativeStreamTicketRecord;
use common::qa_trace_scope::TraceContext;
use tokio::process::Command;
fn context(record:&AndroidNativeStreamTicketRecord,generation:u64,nonce:String,now:u64)->Option<TraceContext>{let b=record.rental_binding.as_ref()?;let rental=b.rental_session_id.clone()?;let ctx=TraceContext{native_session_id:record.session_id.clone(),claim_id:b.claim_id.clone(),handoff_binding_id:b.handoff_binding_id.clone(),rental_session_id:Some(rental),connection_nonce:nonce,generation,authority_expires_ms:record.authority_expires_at_unix_ms.unwrap_or(record.expires_at_unix_ms).min(record.expires_at_unix_ms)};ctx.valid(now).then_some(ctx)}
pub(super) fn configure(command:&mut Command,record:Option<&AndroidNativeStreamTicketRecord>,generation:u64){
 // Child must never inherit another connection's diagnostic identity or old eager gate.
 for name in ["CG_QA_TRACE_PARENT_CONTEXT","CG_QA_SCOPED_TRACE_DIR","CG_QA_RECOVERY_TRACE_DIR","CG_QA_RECOVERY_TRACE_NONCE","CG_QA_RECOVERY_TRACE_EXPIRES_MS"]{command.env_remove(name);}
 if std::env::var("CG_QA_ENABLE_SCOPED_TRACE").ok().as_deref()!=Some("1"){return;}
 let Some(record)=record else{return;};let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()as u64;
 let Some(ctx)=context(record,generation,uuid::Uuid::new_v4().to_simple().to_string(),now)else{return;};
 let Some(root)=std::env::var_os("CG_QA_SCOPED_TRACE_ROOT").map(std::path::PathBuf::from)else{return;};if !root.is_absolute(){return;}let Ok(root)=root.canonicalize()else{return;};if !root.is_dir(){return;}
 // Prepared trusted operator root; create a unique connection directory, never reuse.
 let dir=root.join(&ctx.connection_nonce);if std::fs::create_dir(&dir).is_err(){return;}
 let Ok(json)=serde_json::to_string(&ctx)else{return;};
 command.env("CG_QA_TRACE_PARENT_CONTEXT",json).env("CG_QA_SCOPED_TRACE_DIR",dir);
}
#[cfg(test)]mod tests{use super::*;use crate::app::{AndroidNativeRentalBindingRecord,host::{HostId,AppId},user::UserId};
fn record()->AndroidNativeStreamTicketRecord{AndroidNativeStreamTicketRecord{stream_ticket:"secret-not-exported".into(),authority_expires_at_unix_ms:Some(9000),token_id:"token-not-exported".into(),session_id:"native_a".into(),user_id:UserId(1),host_id:HostId(2),app_id:AppId(3),issued_at_unix_ms:0,expires_at_unix_ms:10000,consumed_at_unix_ms:None,rental_binding:Some(AndroidNativeRentalBindingRecord{handoff_binding_id:"handoff_a".into(),claim_id:"claim_abc".into(),prepare_operation_id:None,rental_session_id:Some("rental_a".into())})}}
#[test]fn parent_context_omits_tickets_and_respects_authority(){let c=context(&record(),7,"a".repeat(32),1).unwrap();assert_eq!(c.authority_expires_ms,9000);let json=serde_json::to_string(&c).unwrap();assert!(!json.contains("secret"));assert!(!json.contains("token-not"));assert_eq!(c.generation,7);}
#[test]fn missing_binding_or_rental_never_arms(){let mut r=record();r.rental_binding.as_mut().unwrap().rental_session_id=None;assert!(context(&r,1,"a".repeat(32),1).is_none());r.rental_binding=None;assert!(context(&r,1,"a".repeat(32),1).is_none());assert!(context(&record(),1,"a".repeat(32),9000).is_none());}
#[test]fn no_record_still_scrubs_inherited_diagnostic_context(){let mut c=Command::new("not-executed");configure(&mut c,None,1);let vars:std::collections::HashMap<_,_>=c.as_std().get_envs().map(|(k,v)|(k.to_string_lossy().to_string(),v.map(|x|x.to_string_lossy().to_string()))).collect();for name in ["CG_QA_TRACE_PARENT_CONTEXT","CG_QA_SCOPED_TRACE_DIR","CG_QA_RECOVERY_TRACE_DIR","CG_QA_RECOVERY_TRACE_NONCE","CG_QA_RECOVERY_TRACE_EXPIRES_MS"]{assert_eq!(vars.get(name),Some(&None));}}

}
