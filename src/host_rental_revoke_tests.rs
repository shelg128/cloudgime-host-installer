use super::*;
use crate::app::{
    AndroidNativeHostBindingRecord, AndroidNativeRentalBindingRecord,
    AndroidNativeTrustBootstrapRecord,
    host::{AppId, HostId},
    user::UserId,
};
use serde_json::json;
use std::{process::Stdio, sync::Arc};
use tokio::{process::Command, sync::Mutex};

const SECRET: &str = "fixture-host-credential";
fn command() -> RevokeCommand {
    RevokeCommand {
        schema_version: 1,
        source: "HOST".into(),
        operation_id: "11111111-1111-4111-8111-111111111111".into(),
        session_id: "rental-session".into(),
        prepare_operation_id: "prepare_fixture_0001".into(),
        handoff_binding_id: "22222222-2222-4222-8222-222222222222".into(),
        native_session_id: "native-session".into(),
        claim_id: "claim".into(),
        pc_id: "pc".into(),
        user_id: "renter".into(),
        application_activation_id: "host-activation".into(),
        command_id: "33333333-3333-4333-8333-333333333333".into(),
        challenge: "06930de9024c8a0a338b6913adc24e264a13438cbc530ebe9fb6fe7843129bb8".into(),
        requested_at: "2026-01-01T00:00:00.000Z".into(),
    }
}
fn signed(mut c: RevokeCommand) -> VerifiedRevokeCommand {
    c.challenge = c.expected_challenge(SECRET).unwrap();
    parse_commands(
        &serde_json::to_vec(&json!({"ok":true,"commands":[c]})).unwrap(),
        "pc",
        "host-activation",
        SECRET,
    )
    .unwrap()
    .remove(0)
}
#[test]
fn backend_node_hmac_fixture_and_strict_parser_match() {
    let c = command();
    assert_eq!(c.expected_challenge(SECRET).unwrap(), c.challenge);
    c.validate("pc", "host-activation", SECRET, Utc::now())
        .unwrap();
    assert!(
        c.validate("wrong", "host-activation", SECRET, Utc::now())
            .is_err()
    );
    assert!(c.validate("pc", "wrong", SECRET, Utc::now()).is_err());
    assert!(
        c.validate("pc", "host-activation", "rotated", Utc::now())
            .is_err()
    );
    let base = json!({"ok":true,"commands":[c]});
    assert_eq!(
        parse_commands(
            &serde_json::to_vec(&base).unwrap(),
            "pc",
            "host-activation",
            SECRET
        )
        .unwrap()
        .len(),
        1
    );
    for key in [
        "schemaVersion",
        "source",
        "operationId",
        "sessionId",
        "prepareOperationId",
        "handoffBindingId",
        "nativeSessionId",
        "claimId",
        "pcId",
        "userId",
        "applicationActivationId",
        "commandId",
        "challenge",
        "requestedAt",
    ] {
        let mut bad = base.clone();
        bad["commands"][0][key] = json!("tampered");
        assert!(
            parse_commands(
                &serde_json::to_vec(&bad).unwrap(),
                "pc",
                "host-activation",
                SECRET
            )
            .is_err(),
            "field {key}"
        );
        let mut bad = base.clone();
        bad["commands"][0].as_object_mut().unwrap().remove(key);
        assert!(
            parse_commands(
                &serde_json::to_vec(&bad).unwrap(),
                "pc",
                "host-activation",
                SECRET
            )
            .is_err(),
            "missing {key}"
        );
    }
    for extra in ["pid", "path", "shell", "credential"] {
        let mut bad = base.clone();
        bad["commands"][0][extra] = json!("bad");
        assert!(
            parse_commands(
                &serde_json::to_vec(&bad).unwrap(),
                "pc",
                "host-activation",
                SECRET
            )
            .is_err()
        );
    }
    assert!(parse_commands(&vec![b' '; MAX_BODY + 1], "pc", "host-activation", SECRET).is_err());
    assert!(
        parse_commands(
            br#"{"ok":true,"ok":true,"commands":[]}"#,
            "pc",
            "host-activation",
            SECRET
        )
        .is_err()
    );
    let mut dup = base.clone();
    dup["commands"]
        .as_array_mut()
        .unwrap()
        .push(base["commands"][0].clone());
    assert!(
        parse_commands(
            &serde_json::to_vec(&dup).unwrap(),
            "pc",
            "host-activation",
            SECRET
        )
        .is_err()
    );
    let mut bad = base.clone();
    bad["ok"] = json!(false);
    assert!(
        parse_commands(
            &serde_json::to_vec(&bad).unwrap(),
            "pc",
            "host-activation",
            SECRET
        )
        .is_err()
    );
    let mut future = command();
    future.requested_at = "2999-01-01T00:00:00.000Z".into();
    assert!(
        future
            .validate("pc", "host-activation", SECRET, Utc::now())
            .is_err()
    );
}
async fn child(
    scope: &crate::native_stream_scope::NativeStreamScope,
) -> Arc<Mutex<tokio::process::Child>> {
    let mut permit = scope.launch_permit().await.unwrap();
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd.exe");
        c.args(["/D", "/C", "pause"]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = Command::new("cat");
    let p = Arc::new(Mutex::new(
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap(),
    ));
    assert!(permit.attach(p.clone()));
    p
}
#[actix_web::test]
async fn backend_command_revokes_prelogin_without_browser_and_replay_cannot_target_other_session() {
    let root = std::env::temp_dir().join(format!("cg-backend-revoke-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut config = common::config::Config::default();
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().into(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    let app = App::new(config).await.unwrap();
    let issue = |binding| {
        app.issue_android_native_launch_token(
            UserId(3),
            HostId(1),
            AppId(1379015545),
            AndroidNativeHostBindingRecord {
                name: "test".into(),
                address: "127.0.0.1".into(),
                http_port: 47989,
                https_port: None,
                external_port: None,
                unique_id: None,
                local_ip: None,
            },
            AndroidNativeTrustBootstrapRecord {
                paired: false,
                pair_mode: "test".into(),
                client_unique_id: None,
                client_certificate_pem: None,
                client_private_key_pem: None,
                server_certificate_pem: None,
            },
            Some(binding),
        )
    };
    let base = command();
    let launch = issue(AndroidNativeRentalBindingRecord {
        handoff_binding_id: base.handoff_binding_id.clone(),
        claim_id: base.claim_id.clone(),
        prepare_operation_id: None,
        rental_session_id: None,
    })
    .await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&launch.launch_token)
        .await
        .unwrap();
    let scope = crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .begin(&launch.session_id, 1)
        .await
        .unwrap();
    let sibling = crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .begin(&launch.session_id, 2)
        .await
        .unwrap();
    let own = child(&scope).await;
    let sibling_child = child(&sibling).await;
    let other_id = format!("unrelated-{}", uuid::Uuid::new_v4());
    let unrelated = crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .begin(&other_id, 2)
        .await
        .unwrap();
    let other = child(&unrelated).await;
    let mut c = base;
    c.native_session_id = launch.session_id.clone();
    for field in ["claim", "handoff", "native"] {
        let mut wrong = c.clone();
        match field {
            "claim" => wrong.claim_id = "another-claim".into(),
            "handoff" => wrong.handoff_binding_id = "44444444-4444-4444-8444-444444444444".into(),
            _ => wrong.native_session_id = other_id.clone(),
        }
        assert!(
            app.revoke_backend_rental_stream(&signed(wrong))
                .await
                .is_err()
        );
        assert!(!scope.is_cancelled());
        assert!(!sibling.is_cancelled());
        assert!(!unrelated.is_cancelled());
        assert!(other.lock().await.try_wait().unwrap().is_none());
        assert!(own.lock().await.try_wait().unwrap().is_none());
        assert!(sibling_child.lock().await.try_wait().unwrap().is_none());
    }
    let verified = signed(c.clone());
    assert_eq!(
        app.revoke_backend_rental_stream(&verified).await.unwrap(),
        2
    );
    assert!(scope.is_cancelled());
    assert!(sibling.is_cancelled());
    assert!(own.lock().await.try_wait().unwrap().is_some());
    assert!(sibling_child.lock().await.try_wait().unwrap().is_some());
    assert!(other.lock().await.try_wait().unwrap().is_none());
    assert!(
        app.peek_android_native_stream_ticket(&ticket.stream_ticket)
            .await
            .is_err()
    );
    assert!(
        crate::native_stream_scope::NATIVE_STREAM_SCOPES
            .begin(&launch.session_id, 3)
            .await
            .is_err()
    );
    assert_eq!(
        app.revoke_backend_rental_stream(&verified).await.unwrap(),
        2
    );
    c.operation_id = "55555555-5555-4555-8555-555555555555".into();
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(c)).await.err(),
        Some("HOST_REVOKE_OPERATION_CONFLICT")
    );
    crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .stop(&other_id)
        .await
        .unwrap();
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
#[actix_web::test]
async fn backend_command_after_restart_creates_only_exact_deny_tombstone() {
    let root = std::env::temp_dir().join(format!("cg-revoke-restart-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut config = common::config::Config::default();
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().into(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    let app = App::new(config).await.unwrap();
    let mut old = command();
    old.native_session_id = format!("old-native-{}", uuid::Uuid::new_v4());
    let unrelated_id = format!("unrelated-{}", uuid::Uuid::new_v4());
    let unrelated = crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .begin(&unrelated_id, 91)
        .await
        .unwrap();
    let other = child(&unrelated).await;
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(old.clone()))
            .await
            .unwrap(),
        0
    );
    assert!(
        crate::native_stream_scope::NATIVE_STREAM_SCOPES
            .begin(&old.native_session_id, 92)
            .await
            .is_err()
    );
    assert!(!unrelated.is_cancelled());
    assert!(other.lock().await.try_wait().unwrap().is_none());
    // Exact replay remains idempotent and still is not a cleanup receipt.
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(old))
            .await
            .unwrap(),
        0
    );
    crate::native_stream_scope::NATIVE_STREAM_SCOPES
        .stop(&unrelated_id)
        .await
        .unwrap();
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_is_opt_in_and_emits_only_authenticated_exact_bound_receipt() {
    let s = include_str!("host_rental_revoke.rs");
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(compact.contains("if!enabled{returnRevokeWorker(None);}"));
    let config: common::config::Config = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(!config.web_server.rental_access_revocation_enabled);
    assert!(!config.web_server.rental_stream_ticket_required);
    let main: String = include_str!("main.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(main.contains(
        "rental_access_revocation_enabled&&app.config().web_server.rental_stream_ticket_required"
    ));
    assert!(s.contains("https_only(true)"));
    assert!(s.contains("redirect(reqwest::redirect::Policy::none())"));
    assert!(s.contains("/rental-cleanup-receipt"));
    assert!(s.contains("streamTerminated"));
    assert!(s.contains("write_dpapi_secret"));
    assert!(!s.contains("taskkill"));
    assert!(!s.contains("PowerShell"));
}

#[test]
fn poll_and_hmac_validation_use_host_activation_not_component_activation() {
    let binding = UniversalHostBinding {
        host_id: "host-activation".into(),
        pc_id: "pc".into(),
        device_id: "PC-fixture".into(),
        keeper_entry_id: "keeper-fixture".into(),
        machine_identity: "machine-fixture".into(),
        install_instance_id: "installation-fixture".into(),
        token_kind: "always_on_host".into(),
        application_activation_id: "component-activation-different".into(),
    };
    assert_ne!(binding.host_id, binding.application_activation_id);
    assert_eq!(
        poll_identity(&binding),
        json!({"pcId":"pc","applicationActivationId":"host-activation"})
    );
    let body = serde_json::to_vec(&json!({"ok":true,"commands":[command()]})).unwrap();
    assert_eq!(
        parse_commands(
            &body,
            &binding.pc_id,
            cleanup_activation_id(&binding),
            SECRET
        )
        .unwrap()
        .len(),
        1
    );
    assert!(
        parse_commands(
            &body,
            &binding.pc_id,
            &binding.application_activation_id,
            SECRET
        )
        .is_err()
    );
    let mut wrong = command();
    wrong.application_activation_id = binding.application_activation_id.clone();
    wrong.challenge = wrong.expected_challenge(SECRET).unwrap();
    let wrong_body = serde_json::to_vec(&json!({"ok":true,"commands":[wrong]})).unwrap();
    assert!(
        parse_commands(
            &wrong_body,
            &binding.pc_id,
            cleanup_activation_id(&binding),
            SECRET
        )
        .is_err()
    );
}
