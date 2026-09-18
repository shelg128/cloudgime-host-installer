//! Handle-based, bounded Windows reader. Files must be produced by trusted
//! machine code; this is not a signature check or a runtime desktop attestation.
use super::{MAX_BYTES, TargetError};
use std::path::Path;

#[cfg(not(windows))]
pub(super) fn read_machine_owned(_: &Path) -> Result<Vec<u8>, TargetError> {
    Err(TargetError::Path)
}

#[cfg(windows)]
pub(super) fn read_machine_owned(path: &Path) -> Result<Vec<u8>, TargetError> {
    windows_acl::read_machine_owned(path)
}

#[cfg(windows)]
mod windows_acl {
    use super::*;
    use std::{
        ffi::c_void,
        fs::{File, OpenOptions},
        io::Read,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
        path::Component,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            ACE_HEADER, ACL,
            Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GetAce, GetLengthSid, IsValidAcl, IsValidSid,
            OWNER_SECURITY_INFORMATION,
        },
    };
    // Constants are Windows file access masks; no GENERIC mapping guesses.
    const READ_CONTROL: u32 = 0x0002_0000;
    const FULL_MUTATION: u32 = 0x1000_0000 | 0x4000_0000 | 0x000D_0156;
    const ANCESTOR_MUTATION: u32 = 0x1000_0000 | 0x4000_0000 | 0x000C_0040;
    const REPARSE: u32 = 0x400;
    struct Descriptor(*mut c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }

    pub(crate) fn read_machine_owned(path: &Path) -> Result<Vec<u8>, TargetError> {
        valid_local_path(path)?;
        let parents: Vec<_> = path.ancestors().skip(1).collect();
        let mut held_parents = Vec::new();
        for (index, parent) in parents.iter().enumerate().rev() {
            let file = open(parent, true)?;
            // Immediate directory must not permit replacement/creation. Higher
            // ancestors may allow creating other directories, but not DELETE_CHILD
            // or ACL takeover of this already-existing path.
            let mask = if index == 0 {
                FULL_MUTATION
            } else {
                ANCESTOR_MUTATION
            };
            verify_acl(&file, mask)?;
            held_parents.push((file, mask));
        }
        let mut file = open(path, false)?;
        verify_acl(&file, FULL_MUTATION)?;
        let length = file.metadata().map_err(|_| TargetError::File)?.len();
        if length == 0 || length > MAX_BYTES {
            return Err(TargetError::File);
        }
        let mut bytes = Vec::with_capacity(length as usize);
        (&mut file)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TargetError::File)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES || bytes.len() as u64 != length {
            return Err(TargetError::File);
        }
        // File sharing disallows writers/deletion while read is in progress.
        // Parent handles disallow deletion/rename until all checks are complete.
        verify_acl(&file, FULL_MUTATION)?;
        for (parent, mask) in &held_parents {
            verify_acl(parent, *mask)?;
        }
        Ok(bytes)
    }
    fn valid_local_path(path: &Path) -> Result<(), TargetError> {
        let text = path.to_str().ok_or(TargetError::Path)?;
        // Ordinary local drive only: no UNC/device prefixes, ADS or ambiguous
        // trailing-dot/space names. Reject traversal rather than normalizing it.
        if !path.is_absolute()
            || text.len() < 4
            || !text.as_bytes()[0].is_ascii_alphabetic()
            || &text[1..2] != ":"
            || !matches!(text.as_bytes()[2], b'\\' | b'/')
            || text[2..].contains(':')
            || text.chars().any(char::is_control)
        {
            return Err(TargetError::Path);
        }
        for component in path.components() {
            match component {
                Component::ParentDir | Component::CurDir => return Err(TargetError::Path),
                Component::Normal(value) => {
                    let value = value.to_str().ok_or(TargetError::Path)?;
                    if value.ends_with(['.', ' ']) {
                        return Err(TargetError::Path);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn open(path: &Path, directory: bool) -> Result<File, TargetError> {
        let mut options = OpenOptions::new();
        options
            .access_mode(READ_CONTROL | 0x80 | if directory { 0 } else { 1 })
            .share_mode(if directory { 3 } else { 1 }) // no DELETE sharing; file also excludes WRITE
            .custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 });
        let file = options.open(path).map_err(|_| TargetError::File)?;
        let meta = file.metadata().map_err(|_| TargetError::File)?;
        if meta.file_attributes() & REPARSE != 0 {
            return Err(TargetError::Path);
        }
        if meta.is_dir() != directory || (!directory && !meta.is_file()) {
            return Err(TargetError::File);
        }
        Ok(file)
    }
    fn trusted(sid: &str) -> bool {
        matches!(
            sid,
            "S-1-5-18"
                | "S-1-5-32-544"
                | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
        )
    }
    fn verify_acl(file: &File, mutation: u32) -> Result<(), TargetError> {
        let mut owner: *mut c_void = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut sd = ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut sd,
            )
        };
        let _descriptor = Descriptor(sd);
        if status != 0
            || sd.is_null()
            || owner.is_null()
            || dacl.is_null()
            || unsafe { IsValidAcl(dacl) } == 0
        {
            return Err(TargetError::Acl);
        }
        if !trusted(&sid_string(owner)?) {
            return Err(TargetError::Acl);
        }
        let acl = unsafe { &*dacl };
        if acl.AceCount > 1024 {
            return Err(TargetError::Acl);
        }
        let start = dacl as usize;
        let end = start
            .checked_add(acl.AclSize as usize)
            .ok_or(TargetError::Acl)?;
        for index in 0..acl.AceCount {
            let mut raw: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(dacl, index as u32, &mut raw) } == 0 || raw.is_null() {
                return Err(TargetError::Acl);
            }
            let address = raw as usize;
            if address < start || address.checked_add(4).ok_or(TargetError::Acl)? > end {
                return Err(TargetError::Acl);
            }
            let header = unsafe { &*(raw as *const ACE_HEADER) };
            if header.AceSize < 4
                || address
                    .checked_add(header.AceSize as usize)
                    .ok_or(TargetError::Acl)?
                    > end
            {
                return Err(TargetError::Acl);
            }
            if header.AceFlags & 8 != 0 {
                continue;
            } // INHERIT_ONLY does not apply to this object
            match header.AceType {
                1 => continue, // deny cannot add rights; allow rules evaluated conservatively
                0 => {}
                _ => return Err(TargetError::Acl), // unknown/object/callback ACE: fail closed
            }
            // Basic allow ACE = header(4), mask(4), SID(8+4*n).
            if header.AceSize < 16 {
                return Err(TargetError::Acl);
            }
            let bytes =
                unsafe { std::slice::from_raw_parts(raw.cast::<u8>(), header.AceSize as usize) };
            let sid_len = 8 + 4 * bytes[9] as usize;
            if 8 + sid_len > bytes.len() {
                return Err(TargetError::Acl);
            }
            let sid = unsafe { raw.cast::<u8>().add(8).cast::<c_void>() };
            if unsafe { IsValidSid(sid) } == 0 || unsafe { GetLengthSid(sid) } as usize != sid_len {
                return Err(TargetError::Acl);
            }
            let mask = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| TargetError::Acl)?);
            if mask & mutation != 0 && !trusted(&sid_string(sid)?) {
                return Err(TargetError::Acl);
            }
        }
        Ok(())
    }
    fn sid_string(sid: *mut c_void) -> Result<String, TargetError> {
        if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
            return Err(TargetError::Acl);
        }
        let mut text: *mut u16 = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return Err(TargetError::Acl);
        }
        let _text = Descriptor(text.cast());
        let mut n = 0usize;
        while unsafe { *text.add(n) } != 0 {
            n += 1;
            if n > 256 {
                return Err(TargetError::Acl);
            }
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(text, n) })
            .map_err(|_| TargetError::Acl)
    }
}
