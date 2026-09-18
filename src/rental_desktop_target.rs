//! Source-only admission/revocation contract, NOT a capture backend or a receipt.
//! Caller supplies a backend-authorized tuple independently of the target file.
//! The file's signature/authority is NOT inferred from its contents or existence.
//! An immutable target expires after 30s; renewal, durable revoke replay, and
//! binding to actual stream/input handles remain integration work.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

#[path = "rental_target_file.rs"]
mod target_file;
const MAX_BYTES: u64 = 32 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30);
const CAPACITY: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RentalDesktopTarget {
    pub schema_version: u32,
    pub provisioning_binding_fingerprint: String,
    pub session_id: String,
    pub prepare_operation_id: String,
    pub handoff_binding_id: String,
    pub native_session_id: String,
    pub claim_id: String,
    pub pc_id: String,
    pub user_id: String,
    pub application_activation_id: String,
    pub windows_session_id: u32,
    pub logon_sid: String,
    pub desktop_path: String,
    pub generation: u64,
    pub observed_at_unix_ms: u64,
}

/// Must originate at authenticated setup, never constructed from the target file
/// by a production caller. No API in this module authenticates the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RentalDesktopAuthority {
    pub provisioning_binding_fingerprint: String,
    pub session_id: String,
    pub prepare_operation_id: String,
    pub handoff_binding_id: String,
    pub native_session_id: String,
    pub claim_id: String,
    pub pc_id: String,
    pub user_id: String,
    pub application_activation_id: String,
    pub windows_session_id: u32,
    pub logon_sid: String,
    pub desktop_path: String,
    pub generation: u64,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum TargetError {
    #[error("target_path_invalid")]
    Path,
    #[error("target_file_invalid")]
    File,
    #[error("target_acl_untrusted")]
    Acl,
    #[error("target_json_invalid")]
    Json,
    #[error("target_binding_invalid")]
    Binding,
    #[error("target_stale")]
    Stale,
    #[error("target_changed")]
    Changed,
    #[error("target_revoked")]
    Revoked,
    #[error("target_capacity")]
    Capacity,
}

// Immutable reader: production always uses the handle/ACL verifier. Tests inject
// deterministic bytes ONLY through a cfg(test) private constructor below.
type ReadTarget = dyn Fn(&Path) -> Result<Vec<u8>, TargetError> + Send + Sync;
struct TargetLease {
    path: PathBuf,
    digest: [u8; 32],
    target: RentalDesktopTarget,
    read: Arc<ReadTarget>,
    last_observed: Mutex<SystemTime>,
}
impl TargetLease {
    fn read(path: &Path, now: SystemTime, read: Arc<ReadTarget>) -> Result<Self, TargetError> {
        let bytes = read(path)?;
        let target = parse(&bytes, now)?;
        Ok(Self {
            path: path.into(),
            digest: Sha256::digest(&bytes).into(),
            target,
            read,
            last_observed: Mutex::new(now),
        })
    }
    fn require_current(&self, now: SystemTime) -> Result<(), TargetError> {
        let mut last = self
            .last_observed
            .lock()
            .map_err(|_| TargetError::Revoked)?;
        if now < *last {
            return Err(TargetError::Stale);
        }
        let bytes = (self.read)(&self.path)?;
        if bytes.len() as u64 > MAX_BYTES || <[u8; 32]>::from(Sha256::digest(&bytes)) != self.digest
        {
            return Err(TargetError::Changed);
        }
        validate(&self.target, now)?;
        *last = now;
        Ok(())
    }
}
struct Entry {
    authority: RentalDesktopAuthority,
    digest: Option<[u8; 32]>,
    // Held across a synchronous operation. Do NOT re-enter this registry or
    // schedule deferred OS work in the operation closure; that is not gated.
    revoked: Mutex<bool>,
}
#[derive(Default)]
pub(crate) struct RentalDesktopTargetRegistry {
    sessions: Mutex<HashMap<String, Arc<Entry>>>,
}
pub(crate) struct RentalDesktopTargetPermit {
    entry: Arc<Entry>,
    lease: TargetLease,
}
impl RentalDesktopTargetRegistry {
    pub(crate) fn register(
        &self,
        path: &Path,
        authority: RentalDesktopAuthority,
        now: SystemTime,
    ) -> Result<RentalDesktopTargetPermit, TargetError> {
        self.register_using(
            path,
            authority,
            now,
            Arc::new(target_file::read_machine_owned),
        )
    }
    fn register_using(
        &self,
        path: &Path,
        authority: RentalDesktopAuthority,
        now: SystemTime,
        read: Arc<ReadTarget>,
    ) -> Result<RentalDesktopTargetPermit, TargetError> {
        validate_authority(&authority)?;
        let lease = TargetLease::read(path, now, read)?;
        if !matches_authority(&lease.target, &authority) {
            return Err(TargetError::Binding);
        }
        let mut sessions = self.sessions.lock().map_err(|_| TargetError::Revoked)?;
        if !sessions.contains_key(&authority.session_id) && sessions.len() >= CAPACITY {
            return Err(TargetError::Capacity);
        }
        let entry = sessions
            .entry(authority.session_id.clone())
            .or_insert_with(|| {
                Arc::new(Entry {
                    authority: authority.clone(),
                    digest: Some(lease.digest),
                    revoked: Mutex::new(false),
                })
            })
            .clone();
        if *entry.revoked.lock().map_err(|_| TargetError::Revoked)? {
            return Err(TargetError::Revoked);
        }
        if entry.authority != authority || entry.digest != Some(lease.digest) {
            return Err(TargetError::Changed);
        }
        Ok(RentalDesktopTargetPermit { entry, lease })
    }
    /// Process-local tombstone only. An authenticated revoke preceding registration
    /// still prevents it. No eviction while old operations can reference the entry.
    /// Production restart must replay backend/durable revokes before accepting work.
    pub(crate) fn revoke(&self, authority: &RentalDesktopAuthority) -> Result<(), TargetError> {
        validate_authority(authority)?;
        let entry = {
            let mut sessions = self.sessions.lock().map_err(|_| TargetError::Revoked)?;
            if !sessions.contains_key(&authority.session_id) && sessions.len() >= CAPACITY {
                return Err(TargetError::Capacity);
            }
            sessions
                .entry(authority.session_id.clone())
                .or_insert_with(|| {
                    Arc::new(Entry {
                        authority: authority.clone(),
                        digest: None,
                        revoked: Mutex::new(true),
                    })
                })
                .clone()
        };
        if entry.authority != *authority {
            return Err(TargetError::Changed);
        }
        *entry.revoked.lock().map_err(|_| TargetError::Revoked)? = true;
        Ok(())
    }
}
impl RentalDesktopTargetPermit {
    /// Execute bounded, synchronous work under the revoke barrier. Returning a
    /// desktop path or spawning async work from here is NOT an enforcement proof.
    /// There are intentionally no OS capture/input implementations in this module.
    pub(crate) fn with_target<T>(
        &self,
        now: SystemTime,
        operation: impl FnOnce(&str) -> T,
    ) -> Result<T, TargetError> {
        let mut revoked = self
            .entry
            .revoked
            .lock()
            .map_err(|_| TargetError::Revoked)?;
        if *revoked {
            return Err(TargetError::Revoked);
        }
        if let Err(error) = self.lease.require_current(now) {
            *revoked = true; // observed expiry/missing/change cannot revive on file restoration
            return Err(error);
        }
        Ok(operation(&self.lease.target.desktop_path))
    }
}
fn matches_authority(v: &RentalDesktopTarget, a: &RentalDesktopAuthority) -> bool {
    v.provisioning_binding_fingerprint == a.provisioning_binding_fingerprint
        && v.session_id == a.session_id
        && v.prepare_operation_id == a.prepare_operation_id
        && v.handoff_binding_id == a.handoff_binding_id
        && v.native_session_id == a.native_session_id
        && v.claim_id == a.claim_id
        && v.pc_id == a.pc_id
        && v.user_id == a.user_id
        && v.application_activation_id == a.application_activation_id
        && v.windows_session_id == a.windows_session_id
        && v.logon_sid == a.logon_sid
        && v.desktop_path == a.desktop_path
        && v.generation == a.generation
}
fn parse(bytes: &[u8], now: SystemTime) -> Result<RentalDesktopTarget, TargetError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return Err(TargetError::File);
    }
    // Deserialize directly into a struct: duplicate/unknown/absent fields fail.
    let value: RentalDesktopTarget =
        serde_json::from_slice(bytes).map_err(|_| TargetError::Json)?;
    validate(&value, now)?;
    Ok(value)
}
fn validate(v: &RentalDesktopTarget, now: SystemTime) -> Result<(), TargetError> {
    if v.schema_version != 1
        || !valid_fields(
            &v.provisioning_binding_fingerprint,
            &v.prepare_operation_id,
            &v.handoff_binding_id,
            v.windows_session_id,
            &v.logon_sid,
            &v.desktop_path,
            v.generation,
            &[
                &v.session_id,
                &v.native_session_id,
                &v.claim_id,
                &v.pc_id,
                &v.user_id,
                &v.application_activation_id,
            ],
        )
    {
        return Err(TargetError::Binding);
    }
    let observed = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(v.observed_at_unix_ms))
        .ok_or(TargetError::Stale)?;
    if now
        .duration_since(observed)
        .map_err(|_| TargetError::Stale)?
        >= MAX_AGE
    {
        return Err(TargetError::Stale);
    }
    Ok(())
}
fn validate_authority(a: &RentalDesktopAuthority) -> Result<(), TargetError> {
    if !valid_fields(
        &a.provisioning_binding_fingerprint,
        &a.prepare_operation_id,
        &a.handoff_binding_id,
        a.windows_session_id,
        &a.logon_sid,
        &a.desktop_path,
        a.generation,
        &[
            &a.session_id,
            &a.native_session_id,
            &a.claim_id,
            &a.pc_id,
            &a.user_id,
            &a.application_activation_id,
        ],
    ) {
        return Err(TargetError::Binding);
    }
    Ok(())
}
fn valid_fields(
    hash: &str,
    operation: &str,
    handoff: &str,
    session: u32,
    sid: &str,
    desktop: &str,
    generation: u64,
    ids: &[&str],
) -> bool {
    hash.len() == 64
        && lower_hex(hash)
        && (16..=128).contains(&operation.len())
        && operation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && uuid::Uuid::parse_str(handoff)
            .is_ok_and(|id| !id.is_nil() && id.to_hyphenated().to_string() == handoff)
        && session > 0
        && session <= i32::MAX as u32
        && generation > 0
        && ids.iter().all(|s| valid_text(s))
        && logon_sid(sid)
        && desktop_path(desktop)
}
fn valid_text(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && s.trim() == s && !s.chars().any(char::is_control)
}
fn lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn logon_sid(s: &str) -> bool {
    let Some(s) = s.strip_prefix("S-1-5-5-") else {
        return false;
    };
    let p: Vec<_> = s.split('-').collect();
    p.len() == 2
        && p.iter()
            .all(|s| s.parse::<u32>().is_ok_and(|n| n.to_string() == *s))
}
fn desktop_path(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("CloudGimeAgent-") else {
        return false;
    };
    let Some(id) = rest.strip_suffix("\\Default") else {
        return false;
    };
    id.len() == 32 && lower_hex(id) && id.bytes().any(|b| b != b'0')
}

#[cfg(test)]
#[path = "rental_desktop_target_tests.rs"]
mod tests;
