use super::*;
use std::{
    fs,
    sync::{Arc, mpsc},
    thread,
    time::UNIX_EPOCH,
};

const NOW: u64 = 1_800_000_000_000;
fn instant(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}
fn valid(now: u64, session: &str, generation: u64) -> String {
    format!(
        r#"{{"schemaVersion":1,"provisioningBindingFingerprint":"{}","sessionId":"{}","prepareOperationId":"prepare_desktop_0001","handoffBindingId":"11111111-1111-4111-8111-111111111111","nativeSessionId":"native","claimId":"claim","pcId":"pc","userId":"user","applicationActivationId":"activation","windowsSessionId":1,"logonSid":"S-1-5-5-1-2","desktopPath":"CloudGimeAgent-{}\\Default","generation":{},"observedAtUnixMs":{}}}"#,
        "a".repeat(64),
        session,
        "b".repeat(32),
        generation,
        now
    )
}
struct Fixture {
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new(body: &str) -> Self {
        Self::in_parent(&std::env::temp_dir(), body)
    }
    fn in_parent(parent: &Path, body: &str) -> Self {
        let root = parent.join(format!("cg-target-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("target.json");
        fs::write(&path, body).unwrap();
        Self { root, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn reader() -> Arc<ReadTarget> {
    Arc::new(|p| fs::read(p).map_err(|_| TargetError::Revoked))
}
fn authority(v: &RentalDesktopTarget) -> RentalDesktopAuthority {
    RentalDesktopAuthority {
        provisioning_binding_fingerprint: v.provisioning_binding_fingerprint.clone(),
        session_id: v.session_id.clone(),
        prepare_operation_id: v.prepare_operation_id.clone(),
        handoff_binding_id: v.handoff_binding_id.clone(),
        native_session_id: v.native_session_id.clone(),
        claim_id: v.claim_id.clone(),
        pc_id: v.pc_id.clone(),
        user_id: v.user_id.clone(),
        application_activation_id: v.application_activation_id.clone(),
        windows_session_id: v.windows_session_id,
        logon_sid: v.logon_sid.clone(),
        desktop_path: v.desktop_path.clone(),
        generation: v.generation,
    }
}
fn fixture_authority() -> RentalDesktopAuthority {
    authority(&parse(valid(NOW, "s", 7).as_bytes(), instant(NOW)).unwrap())
}

#[test]
fn strict_target_and_authority() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let registry = RentalDesktopTargetRegistry::default();
    let permit = registry
        .register_using(&f.path, fixture_authority(), instant(NOW), reader())
        .unwrap();
    assert!(
        permit
            .with_target(instant(NOW + 29_999), |p| p.starts_with("CloudGimeAgent-"))
            .unwrap()
    );
    assert_eq!(
        permit.with_target(instant(NOW + 30_000), |_| ()),
        Err(TargetError::Stale)
    );
    assert_eq!(
        permit.with_target(instant(NOW), |_| ()),
        Err(TargetError::Revoked)
    );
}
#[test]
fn mutation_delete_restore_and_unknown_fields_fail_closed() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let registry = RentalDesktopTargetRegistry::default();
    let permit = registry
        .register_using(&f.path, fixture_authority(), instant(NOW), reader())
        .unwrap();
    fs::write(&f.path, valid(NOW + 1, "s", 7)).unwrap();
    assert_eq!(
        permit.with_target(instant(NOW), |_| ()),
        Err(TargetError::Changed)
    );
    fs::write(&f.path, valid(NOW, "s", 7)).unwrap();
    assert_eq!(
        permit.with_target(instant(NOW), |_| ()),
        Err(TargetError::Revoked)
    );
    assert_eq!(
        registry
            .register_using(&f.path, fixture_authority(), instant(NOW), reader())
            .err(),
        Some(TargetError::Revoked)
    );
    let lease = TargetLease::read(&f.path, instant(NOW), reader()).unwrap();
    fs::remove_file(&f.path).unwrap();
    assert_eq!(
        lease.require_current(instant(NOW)),
        Err(TargetError::Revoked)
    );
    let extra = valid(NOW, "s", 7).replace("{", "{\"desktopPassword\":\"x\",");
    assert_eq!(
        parse(extra.as_bytes(), instant(NOW)).err(),
        Some(TargetError::Json)
    );
}
#[test]
fn malformed_duplicate_missing_null_and_oversized_json_fail() {
    let source = valid(NOW, "s", 7);
    for body in [
        source.replace("CloudGimeAgent-", "winsta0\\"),
        source.replace("S-1-5-5-1-2", "S-1-5-18"),
        source.replace("\"generation\":7", "\"generation\":0"),
        source.replace(
            "\"sessionId\":\"s\"",
            "\"sessionId\":\"s\",\"sessionId\":\"x\"",
        ),
        source.replace(
            "\"sessionId\":\"s\"",
            "\"sessionId\":\"s\",\"session\\u0049d\":\"x\"",
        ),
        source.replace("\"claimId\":\"claim\",", ""),
        source.replace("\"claimId\":\"claim\"", "\"claimId\":null"),
        source.replace("\"generation\":7", "\"generation\":\"7\""),
        source.replace("S-1-5-5-1-2", "S-1-5-5-4294967296-2"),
        source.replace("S-1-5-5-1-2", "S-1-5-5-01-2"),
        source.replace(
            "11111111-1111-4111-8111-111111111111",
            "00000000-0000-0000-0000-000000000000",
        ),
        "null".into(),
        "[]".into(),
        format!("{source}{{}}"),
    ] {
        assert!(
            parse(body.as_bytes(), instant(NOW)).is_err(),
            "unexpectedly parsed test fixture"
        );
    }
    assert_eq!(
        parse(&vec![b' '; MAX_BYTES as usize + 1], instant(NOW)).err(),
        Some(TargetError::File)
    );
    assert_eq!(
        parse(source.as_bytes(), instant(NOW - 1)).err(),
        Some(TargetError::Stale)
    );
}
#[test]
fn every_authority_field_and_desktop_are_exact_bound() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let original = fixture_authority();
    let mut changed = Vec::new();
    macro_rules! alter {
        ($field:ident,$value:expr) => {{
            let mut a = original.clone();
            a.$field = $value;
            changed.push(a);
        }};
    }
    alter!(provisioning_binding_fingerprint, "c".repeat(64));
    alter!(session_id, "other".into());
    alter!(prepare_operation_id, "prepare_other_0001".into());
    alter!(
        handoff_binding_id,
        "22222222-2222-4222-8222-222222222222".into()
    );
    alter!(native_session_id, "other".into());
    alter!(claim_id, "other".into());
    alter!(pc_id, "other".into());
    alter!(user_id, "other".into());
    alter!(application_activation_id, "other".into());
    alter!(windows_session_id, 2);
    alter!(logon_sid, "S-1-5-5-9-9".into());
    alter!(
        desktop_path,
        format!("CloudGimeAgent-{}\\Default", "c".repeat(32))
    );
    alter!(generation, 8);
    for a in changed {
        assert_eq!(
            RentalDesktopTargetRegistry::default()
                .register_using(&f.path, a, instant(NOW), reader())
                .err(),
            Some(TargetError::Binding)
        );
    }
}
#[test]
fn revoke_waits_for_active_operation_and_blocks_reconnect() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let a = fixture_authority();
    let registry = Arc::new(RentalDesktopTargetRegistry::default());
    let permit = registry
        .register_using(&f.path, a.clone(), instant(NOW), reader())
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let work = thread::spawn(move || {
        permit.with_target(instant(NOW), |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let r = registry.clone();
    let a2 = a.clone();
    let (starting_tx, starting_rx) = mpsc::sync_channel(1);
    let revoking = thread::spawn(move || {
        starting_tx.send(()).unwrap();
        r.revoke(&a2)
    });
    starting_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    thread::sleep(Duration::from_millis(30));
    assert!(!revoking.is_finished());
    release_tx.send(()).unwrap();
    work.join().unwrap().unwrap();
    revoking.join().unwrap().unwrap();
    assert_eq!(
        registry
            .register_using(&f.path, a, instant(NOW), reader())
            .err(),
        Some(TargetError::Revoked)
    );
}
#[test]
fn revoke_before_register_and_cross_session_isolation() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let old = fixture_authority();
    let registry = RentalDesktopTargetRegistry::default();
    registry.revoke(&old).unwrap();
    registry.revoke(&old).unwrap();
    assert_eq!(
        registry
            .register_using(&f.path, old, instant(NOW), reader())
            .err(),
        Some(TargetError::Revoked)
    );
    let n = Fixture::new(&valid(NOW, "new", 8));
    let new = authority(&parse(valid(NOW, "new", 8).as_bytes(), instant(NOW)).unwrap());
    let permit = registry
        .register_using(&n.path, new.clone(), instant(NOW), reader())
        .unwrap();
    let mut wrong = new;
    wrong.generation += 1;
    assert_eq!(registry.revoke(&wrong), Err(TargetError::Changed));
    assert!(permit.with_target(instant(NOW), |_| true).unwrap());
}
#[test]
fn clock_rollback_failure_poison_and_capacity_bound() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    let registry = RentalDesktopTargetRegistry::default();
    let p = registry
        .register_using(&f.path, fixture_authority(), instant(NOW), reader())
        .unwrap();
    p.with_target(instant(NOW + 5), |_| ()).unwrap();
    assert_eq!(
        p.with_target(instant(NOW + 4), |_| ()),
        Err(TargetError::Stale)
    );
    assert_eq!(
        p.with_target(instant(NOW + 6), |_| ()),
        Err(TargetError::Revoked)
    );
    let r = RentalDesktopTargetRegistry::default();
    for i in 0..CAPACITY {
        let mut a = fixture_authority();
        a.session_id = format!("session-{i}");
        r.revoke(&a).unwrap();
    }
    assert_eq!(r.revoke(&fixture_authority()), Err(TargetError::Capacity));
}
#[test]
fn production_reader_rejects_temp_acl() {
    let f = Fixture::new(&valid(NOW, "s", 7));
    assert!(matches!(
        RentalDesktopTargetRegistry::default().register(&f.path, fixture_authority(), instant(NOW)),
        Err(TargetError::Acl) | Err(TargetError::Path)
    ));
}

#[cfg(windows)]
fn set_test_acl(path: &Path, sddl: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
        },
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut sd = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut sd,
                std::ptr::null_mut(),
            )
        },
        0
    );
    let result = unsafe { SetFileSecurityW(path.as_ptr(), 0x8000_0005, sd) };
    unsafe {
        LocalFree(sd);
    }
    assert_ne!(result, 0, "setting fixture ACL failed");
}
#[cfg(windows)]
#[test]
fn production_reader_accepts_machine_file_and_rejects_acl_drift() {
    let parent = PathBuf::from(std::env::var_os("ProgramData").unwrap());
    let f = Fixture::in_parent(&parent, &valid(NOW, "s", 7));
    const DIR: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)";
    const FILE: &str = "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)";
    set_test_acl(&f.root, DIR);
    set_test_acl(&f.path, FILE);
    let registry = RentalDesktopTargetRegistry::default();
    let permit = registry
        .register(&f.path, fixture_authority(), instant(NOW))
        .unwrap();
    assert!(
        permit
            .with_target(instant(NOW), |p| p.ends_with("\\Default"))
            .unwrap()
    );
    set_test_acl(&f.path, "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FW;;;BU)");
    assert_eq!(
        permit.with_target(instant(NOW), |_| ()),
        Err(TargetError::Acl)
    );
    set_test_acl(&f.path, FILE);
    assert_eq!(
        permit.with_target(instant(NOW), |_| ()),
        Err(TargetError::Revoked)
    );
    assert_eq!(
        registry
            .register(&f.path, fixture_authority(), instant(NOW))
            .err(),
        Some(TargetError::Revoked)
    );
    // A different registry can validate the restored file, but must replay all
    // durable revokes before admission. Process-local tombstones alone are NOT recovery.
    set_test_acl(
        &f.root,
        "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FW;;;BU)",
    );
    assert_eq!(
        RentalDesktopTargetRegistry::default()
            .register(&f.path, fixture_authority(), instant(NOW))
            .err(),
        Some(TargetError::Acl)
    );
    set_test_acl(&f.root, DIR);
    set_test_acl(&f.path, "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GA;;;BU)");
    assert_eq!(
        RentalDesktopTargetRegistry::default()
            .register(&f.path, fixture_authority(), instant(NOW))
            .err(),
        Some(TargetError::Acl)
    );
}
