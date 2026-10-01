//! Flush bytes before replacing a disposable metadata cache. Not a credential writer.
use std::{fs,io::{self,Write},path::Path};
pub(super) fn replace(path:&Path,bytes:&[u8],secure:impl FnOnce(&Path)->io::Result<()>)->io::Result<()> {
    let temp=path.with_extension(format!("{}.next",uuid::Uuid::new_v4()));
    let result=(||{
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        secure(&temp)?;
        file.write_all(bytes)?;file.sync_all()?;drop(file);
        replace_file(&temp,path)?;
        Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(&temp);}result
}
#[cfg(windows)]
fn replace_file(from:&Path,to:&Path)->io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_REPLACE_EXISTING,MOVEFILE_WRITE_THROUGH};
    let from:Vec<u16>=from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to:Vec<u16>=to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe{MoveFileExW(from.as_ptr(),to.as_ptr(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)}==0 {Err(io::Error::last_os_error())}else{Ok(())}
}
#[cfg(not(windows))]
fn replace_file(from:&Path,to:&Path)->io::Result<()> {fs::rename(from,to)?;if let Some(parent)=to.parent(){fs::File::open(parent)?.sync_all()?;}Ok(())}
#[cfg(test)] mod tests {
    use super::*;
    fn root()->std::path::PathBuf{let p=std::env::temp_dir().join(format!("cg-cache-test-{}",uuid::Uuid::new_v4()));fs::create_dir(&p).unwrap();p}
    #[test] fn creates_and_replaces_complete_file(){let d=root();let p=d.join("cache.json");replace(&p,b"first",|_|Ok(())).unwrap();replace(&p,b"second-full-value",|_|Ok(())).unwrap();assert_eq!(fs::read(&p).unwrap(),b"second-full-value");assert_eq!(fs::read_dir(&d).unwrap().count(),1);fs::remove_dir_all(d).unwrap();}
    #[test] fn failed_acl_never_overwrites_valid_destination(){let d=root();let p=d.join("cache.json");fs::write(&p,b"before").unwrap();assert!(replace(&p,b"after",|_|Err(io::Error::new(io::ErrorKind::PermissionDenied,"fixture"))).is_err());assert_eq!(fs::read(&p).unwrap(),b"before");assert_eq!(fs::read_dir(&d).unwrap().count(),1);fs::remove_dir_all(d).unwrap();}
    #[test] fn acl_step_runs_before_any_payload_write(){let d=root();let p=d.join("cache.json");replace(&p,b"metadata",|temp|{assert_eq!(fs::metadata(temp)?.len(),0);Ok(())}).unwrap();assert_eq!(fs::read(&p).unwrap(),b"metadata");fs::remove_dir_all(d).unwrap();}
}
