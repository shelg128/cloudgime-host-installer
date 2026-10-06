//! Local-only startup receipt: proves feature/env/directory reached this web process.
//! It does NOT attest a ticket/rental or grant capture permission.
use std::{fs::OpenOptions,io::Write,path::Path,time::{SystemTime,UNIX_EPOCH}};
fn write_receipt(root:&Path,now_ms:u64)->std::io::Result<std::path::PathBuf>{
 if !root.is_absolute(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"absolute root required"));}
 let root=root.canonicalize()?;if !root.is_dir(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"directory required"));}
 let path=root.join(format!("parent-activation-{}.json",std::process::id()));let mut f=OpenOptions::new().write(true).create_new(true).open(&path)?;
 let value=serde_json::json!({"schema":1,"process":std::process::id(),"created_at_ms":now_ms,"scoped_feature":true,"operator_enabled":true,"root_writable":true,"scope":"startup-only; ticket/rental authority NOT attested"});serde_json::to_writer(&mut f,&value)?;f.write_all(b"\n")?;f.flush()?;Ok(path)
}
pub(crate) fn attest_from_environment(){
 if std::env::var("CG_QA_ENABLE_SCOPED_TRACE").ok().as_deref()!=Some("1"){return;}
 let Some(dir)=std::env::var_os("CG_QA_SCOPED_TRACE_ROOT")else{tracing::warn!("QA_SCOPED_STARTUP_ROOT_MISSING");return;};let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()as u64;
 match write_receipt(Path::new(&dir),now){Ok(_)=>tracing::info!("QA_SCOPED_STARTUP_ATTESTED"),Err(_)=>tracing::warn!("QA_SCOPED_STARTUP_ATTESTATION_FAILED")}
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn startup_receipt_is_minimal_and_never_overwrites(){let root=std::env::temp_dir().join(format!("cg-qa-startup-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let path=write_receipt(&root,1234).unwrap();let raw=std::fs::read(&path).unwrap();let v:serde_json::Value=serde_json::from_slice(&raw).unwrap();assert_eq!(v["process"],std::process::id());assert_eq!(v["created_at_ms"],1234);assert_eq!(v["operator_enabled"],true);assert_eq!(v.as_object().unwrap().len(),7);assert!(write_receipt(&root,2222).is_err());assert_eq!(std::fs::read(path).unwrap(),raw);}
 #[test]fn startup_receipt_requires_existing_absolute_directory(){assert!(write_receipt(Path::new("relative"),1).is_err());let root=std::env::temp_dir().join(format!("cg-missing-{}",uuid::Uuid::new_v4()));assert!(write_receipt(&root,1).is_err());}
}
