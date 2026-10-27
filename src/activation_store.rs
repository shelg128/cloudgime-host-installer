//! Handle-based validation reuses the production machine-file boundary. DPAPI
//! LocalMachine still needs a PRIVATE DACL: untrusted readers are rejected too.
use anyhow::{Result, bail};
use std::path::Path;
const MAX_BYTES: u64 = 524288;
#[derive(Debug)]
enum TargetError {
    Path,
    File,
    Acl,
}
#[path = "rental_target_file.rs"]
mod reader;
#[cfg(windows)]
pub(super) fn read_private(path: &Path) -> Result<Vec<u8>> {
    use std::{
        ffi::c_void,
        fs::OpenOptions,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            ACE_HEADER, ACL,
            Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GetAce, IsValidAcl,
        },
    };
    let file = OpenOptions::new()
        .access_mode(0x20080)
        .share_mode(1)
        .open(path)?;
    let mut acl: *mut ACL = ptr::null_mut();
    let mut sd = ptr::null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut sd,
        )
    };
    struct Guard(*mut c_void);
    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }
    let _guard = Guard(sd);
    if code != 0 || acl.is_null() || unsafe { IsValidAcl(acl) } == 0 {
        bail!("ACTIVATION_PRIVATE_ACL_INVALID")
    }
    let value = unsafe { &*acl };
    if value.AceCount > 1024 {
        bail!("ACTIVATION_PRIVATE_ACL_INVALID")
    }
    for index in 0..value.AceCount {
        let mut ace = ptr::null_mut();
        if unsafe { GetAce(acl, index as u32, &mut ace) } == 0 || ace.is_null() {
            bail!("ACTIVATION_PRIVATE_ACL_INVALID")
        }
        let h = unsafe { &*(ace as *const ACE_HEADER) };
        if h.AceFlags & 8 != 0 || h.AceType == 1 {
            continue;
        }
        if h.AceType != 0 || h.AceSize < 16 {
            bail!("ACTIVATION_PRIVATE_ACL_INVALID")
        }
        let mut sid = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW((ace as *mut u8).add(8).cast(), &mut sid) } == 0 {
            bail!("ACTIVATION_PRIVATE_ACL_INVALID")
        }
        let _sid = Guard(sid.cast());
        let mut len = 0;
        while unsafe { *sid.add(len) } != 0 && len < 256 {
            len += 1;
        }
        if len == 256 {
            bail!("ACTIVATION_PRIVATE_ACL_INVALID")
        }
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid, len) });
        if text != "S-1-5-18" && text != "S-1-5-32-544" {
            bail!("ACTIVATION_PRIVATE_ACL_REQUIRED")
        }
    }
    reader::read_machine_owned(path)
        .map_err(|_| anyhow::anyhow!("ACTIVATION_STORAGE_BOUNDARY_INVALID"))
}
#[cfg(not(windows))]
pub(super) fn read_private(_: &Path) -> Result<Vec<u8>> {
    bail!("ACTIVATION_WINDOWS_REQUIRED")
}
