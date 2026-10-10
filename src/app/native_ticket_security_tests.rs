use super::*;
#[path = "claim_revoke_tests.rs"]
mod claim_revoke_tests;

async fn fixture() -> (App, PathBuf) {
    let root = std::env::temp_dir().join(format!("cg-native-auth-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = Config::default();
    config.data_storage = StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().to_string(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    config.web_server.default_user_id = None;
    (App::new(config).await.unwrap(), root)
}

async fn launch(app: &App) -> AndroidNativeLaunchTokenRecord {
    app.issue_android_native_launch_token(
        UserId(9001),
        HostId(1),
        AppId(1379015545),
        AndroidNativeHostBindingRecord {
            name: "fixture".into(),
            address: "127.0.0.1".into(),
            http_port: 47989,
            https_port: None,
            external_port: None,
            unique_id: None,
            local_ip: None,
        },
        AndroidNativeTrustBootstrapRecord {
            paired: false,
            pair_mode: "fixture".into(),
            client_unique_id: None,
            client_certificate_pem: None,
            client_private_key_pem: None,
            server_certificate_pem: None,
        },
        None,
    )
    .await
}

#[actix_web::test]
async fn unknown_ticket_cannot_recover_someone_elses_session() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    for (token_id, session_id) in [
        (None, None),
        (
            Some(issued.token_id.as_str()),
            Some(issued.session_id.as_str()),
        ),
    ] {
        let result = app
            .refresh_android_native_stream_ticket("invalid-ticket", token_id, session_id)
            .await;
        assert!(
            matches!(result, Err(AppError::AndroidNativeStreamTicketNotFound)),
            "an unknown ticket must never select another valid ticket, even with guessed IDs"
        );
    }
    assert!(
        app.authorize_android_native_stream_ticket_for_stream(
            &ticket.stream_ticket,
            ticket.host_id,
            ticket.app_id
        )
        .await
        .is_ok()
    );
}

#[actix_web::test]
async fn missing_or_terminal_lifecycle_cannot_authorize_or_refresh() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    assert!(!app.android_native_session_allows_stream_access("").await);
    app.inner
        .android_native_session_lifecycle
        .write()
        .await
        .remove(&issued.session_id);
    assert!(
        !app.android_native_session_allows_stream_access(&issued.session_id)
            .await
    );
    assert!(matches!(
        app.authorize_android_native_stream_ticket_for_host(&ticket.stream_ticket, ticket.host_id)
            .await,
        Err(AppError::AndroidNativeSessionInactive)
    ));
    assert!(matches!(
        app.authorize_android_native_stream_ticket_for_stream(
            &ticket.stream_ticket,
            ticket.host_id,
            ticket.app_id
        )
        .await,
        Err(AppError::AndroidNativeSessionInactive)
    ));
    assert!(matches!(
        app.refresh_android_native_stream_ticket(&ticket.stream_ticket, None, None)
            .await,
        Err(AppError::AndroidNativeSessionInactive)
    ));
}

#[actix_web::test]
async fn revoke_status_cannot_authorize_during_artifact_cleanup_gap() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    // Model the window after lifecycle revocation but before ticket-map invalidation.
    for status in ["ended", "expired", "failed", "abandoned"] {
        let mut states = app.inner.android_native_session_lifecycle.write().await;
        let state = states.get_mut(&issued.session_id).unwrap();
        state.session_status = status.into();
        state.completed_at_unix_ms = Some(unix_time_ms_now());
        drop(states);
        assert!(
            matches!(
                app.authorize_android_native_stream_ticket_for_stream(
                    &ticket.stream_ticket,
                    ticket.host_id,
                    ticket.app_id
                )
                .await,
                Err(AppError::AndroidNativeSessionInactive)
            ),
            "revoked ticket authorized"
        );
    }
}

#[actix_web::test]
async fn revoking_unknown_and_mismatched_lifecycle_block_new_channels() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    let original = app
        .inner
        .android_native_session_lifecycle
        .read()
        .await
        .get(&issued.session_id)
        .unwrap()
        .clone();
    for case in 0..5 {
        let mut changed = original.clone();
        match case {
            0 => changed.hidden_state_status = "revoking".into(),
            1 => changed.session_status = "unknown".into(),
            2 => changed.user_id = UserId(9999),
            3 => changed.token_id = "other-token".into(),
            _ => changed.session_id = "other-session".into(),
        }
        app.inner
            .android_native_session_lifecycle
            .write()
            .await
            .insert(issued.session_id.clone(), changed);
        assert!(matches!(
            app.peek_android_native_stream_ticket(&ticket.stream_ticket)
                .await,
            Err(AppError::AndroidNativeSessionInactive)
        ));
        assert!(matches!(
            app.refresh_android_native_stream_ticket(&ticket.stream_ticket, None, None)
                .await,
            Err(AppError::AndroidNativeSessionInactive)
        ));
        assert!(matches!(
            app.issue_android_native_stream_ticket(&issued).await,
            Err(AppError::AndroidNativeSessionInactive)
        ));
        assert!(matches!(
            app.consume_android_native_stream_ticket(
                &ticket.stream_ticket,
                ticket.host_id,
                ticket.app_id
            )
            .await,
            Err(AppError::AndroidNativeSessionInactive)
        ));
    }
    assert!(
        app.inner
            .android_native_stream_tickets
            .read()
            .await
            .get(&ticket.stream_ticket)
            .unwrap()
            .consumed_at_unix_ms
            .is_none()
    );
}

#[actix_web::test]
async fn unconsumed_launch_cannot_mint_a_ticket_without_live_lifecycle() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    app.inner
        .android_native_session_lifecycle
        .write()
        .await
        .remove(&issued.session_id);
    assert!(matches!(
        app.consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
            .await,
        Err(AppError::AndroidNativeSessionInactive)
    ));
    assert!(
        app.inner
            .android_native_stream_tickets
            .read()
            .await
            .is_empty()
    );
    assert!(
        app.inner
            .android_native_launch_tokens
            .read()
            .await
            .get(&issued.launch_token)
            .unwrap()
            .consumed_at_unix_ms
            .is_none()
    );
}

#[actix_web::test]
async fn revocation_is_monotonic_and_late_start_cannot_restore_access() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    app.update_android_native_session_lifecycle(
        Some(&issued.launch_token),
        Some(&issued.token_id),
        Some(&issued.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    for action in [
        "session_started",
        "trust_bootstrap_staged",
        "trust_bootstrap_ready",
    ] {
        let before = app
            .inner
            .android_native_session_lifecycle
            .read()
            .await
            .get(&issued.session_id)
            .unwrap()
            .clone();
        assert!(
            matches!(
                app.update_android_native_session_lifecycle(
                    Some(&issued.launch_token),
                    Some(&issued.token_id),
                    Some(&issued.session_id),
                    action.into(),
                    None,
                    None
                )
                .await,
                Err(AppError::AndroidNativeSessionInactive)
            ),
            "late event reopened revoking lifecycle"
        );
        let after = app
            .inner
            .android_native_session_lifecycle
            .read()
            .await
            .get(&issued.session_id)
            .unwrap()
            .clone();
        assert_eq!(after.hidden_state_status, before.hidden_state_status);
        assert_eq!(after.last_updated_unix_ms, before.last_updated_unix_ms);
        assert!(
            app.authorize_android_native_stream_ticket_for_stream(
                &ticket.stream_ticket,
                ticket.host_id,
                ticket.app_id
            )
            .await
            .is_err()
        );
    }
    app.update_android_native_session_lifecycle(
        Some(&issued.launch_token),
        Some(&issued.token_id),
        Some(&issued.session_id),
        "session_revoked".into(),
        None,
        None,
    )
    .await
    .unwrap();
    let before = app
        .inner
        .android_native_session_lifecycle
        .read()
        .await
        .get(&issued.session_id)
        .unwrap()
        .clone();
    for action in [
        "session_started",
        "session_revoked",
        "session_revoke_requested",
        "session_failed_cleanup",
    ] {
        assert!(
            matches!(
                app.update_android_native_session_lifecycle(
                    None,
                    Some(&issued.token_id),
                    Some(&issued.session_id),
                    action.into(),
                    None,
                    None
                )
                .await,
                Err(AppError::Unauthorized)
            ),
            "terminal replay with IDs only must not authenticate"
        );
        assert!(
            app.update_android_native_session_lifecycle(
                Some(&issued.launch_token),
                Some(&issued.token_id),
                Some(&issued.session_id),
                action.into(),
                None,
                None
            )
            .await
            .is_err(),
            "revoked bearer must not schedule cleanup twice"
        );
    }
    let after = app
        .inner
        .android_native_session_lifecycle
        .read()
        .await
        .get(&issued.session_id)
        .unwrap()
        .clone();
    assert_eq!(after.completed_at_unix_ms, before.completed_at_unix_ms);
    assert_eq!(after.last_action, before.last_action);
}

#[actix_web::test]
async fn lifecycle_without_identity_cannot_recreate_missing_state() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    app.inner
        .android_native_session_lifecycle
        .write()
        .await
        .remove(&issued.session_id);
    assert!(matches!(
        app.update_android_native_session_lifecycle(
            Some(&issued.launch_token),
            Some(&issued.token_id),
            Some(&issued.session_id),
            "session_started".into(),
            None,
            None
        )
        .await,
        Err(AppError::AndroidNativeSessionInactive)
    ));
    assert!(
        !app.android_native_session_allows_stream_access(&issued.session_id)
            .await
    );
}

#[actix_web::test]
async fn rental_binding_propagates_launch_ticket_lifecycle_and_refresh() {
    let (app, _root) = fixture().await;
    let binding = AndroidNativeRentalBindingRecord {
        handoff_binding_id: Uuid::new_v4().to_string(),
        claim_id: "claim".into(),
        prepare_operation_id: None,
        rental_session_id: None,
    };
    let issued = app
        .issue_android_native_launch_token(
            UserId(9001),
            HostId(1),
            AppId(crate::stream_controller::SENTINEL_PRELOGIN_APP_ID),
            AndroidNativeHostBindingRecord {
                name: "fixture".into(),
                address: "127.0.0.1".into(),
                http_port: 47989,
                https_port: None,
                external_port: None,
                unique_id: None,
                local_ip: None,
            },
            AndroidNativeTrustBootstrapRecord {
                paired: false,
                pair_mode: "fixture".into(),
                client_unique_id: None,
                client_certificate_pem: None,
                client_private_key_pem: None,
                server_certificate_pem: None,
            },
            Some(binding.clone()),
        )
        .await;
    assert_eq!(issued.rental_binding, Some(binding.clone()));
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    assert_eq!(ticket.rental_binding, Some(binding.clone()));
    let refreshed = app
        .refresh_android_native_stream_ticket(
            &ticket.stream_ticket,
            Some(&issued.token_id),
            Some(&issued.session_id),
        )
        .await
        .unwrap();
    assert_eq!(refreshed.rental_binding, Some(binding.clone()));
    let lifecycle = app
        .inner
        .android_native_session_lifecycle
        .read()
        .await
        .get(&issued.session_id)
        .unwrap()
        .clone();
    assert_eq!(lifecycle.rental_binding, Some(binding));
}

#[actix_web::test]
async fn public_ids_are_not_launch_credentials() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    assert_ne!(
        issued.token_id, issued.launch_token,
        "token identifier must not disclose bearer secret"
    );
    assert!(
        app.consume_android_native_launch_token(&issued.token_id)
            .await
            .is_err()
    );
}

#[actix_web::test]
async fn session_ids_or_invalid_credentials_cannot_resolve_or_update_lifecycle() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    for credential in [None, Some("not-a-valid-credential")] {
        assert!(
            app.resolve_android_native_session(
                credential,
                Some(&issued.token_id),
                Some(&issued.session_id)
            )
            .await
            .is_err(),
            "IDs or bad bearer must not fall back to scanning session records"
        );
        assert!(
            app.update_android_native_session_lifecycle(
                credential,
                Some(&issued.token_id),
                Some(&issued.session_id),
                "session_revoke_requested".into(),
                None,
                None
            )
            .await
            .is_err(),
            "IDs must not authorize lifecycle mutation"
        );
    }
    assert!(
        app.authorize_android_native_stream_ticket_for_stream(
            &ticket.stream_ticket,
            ticket.host_id,
            ticket.app_id
        )
        .await
        .is_ok()
    );
    assert!(
        app.resolve_android_native_session(
            Some(&issued.launch_token),
            Some("wrong-token-id"),
            Some(&issued.session_id)
        )
        .await
        .is_err(),
        "supplied IDs must constrain even a valid credential"
    );
}

#[actix_web::test]
async fn valid_bearer_controls_require_exact_ids_and_live_lifecycle() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    let resolved = app
        .resolve_android_native_session(
            Some(&ticket.stream_ticket),
            Some(&issued.token_id),
            Some(&issued.session_id),
        )
        .await
        .unwrap();
    assert_eq!(resolved.session_id, issued.session_id);
    assert_eq!(resolved.user_id, issued.user_id);
    assert!(
        app.bootstrap_android_native_web_session_from_native_session(
            "",
            Some(&issued.token_id),
            &issued.session_id
        )
        .await
        .is_err()
    );
    assert!(
        app.bootstrap_android_native_web_session_from_native_session(
            &issued.token_id,
            Some(&issued.token_id),
            &issued.session_id
        )
        .await
        .is_err()
    );
    assert!(
        app.bootstrap_android_native_web_session_from_native_session(
            &ticket.stream_ticket,
            None,
            "other-session"
        )
        .await
        .is_err()
    );
    let web = app
        .bootstrap_android_native_web_session_from_native_session(
            &ticket.stream_ticket,
            Some(&issued.token_id),
            &issued.session_id,
        )
        .await
        .unwrap();
    assert_eq!(web.session_id, issued.session_id);
    // Long-running clients use the ticket, not a stale launch token or public IDs.
    app.inner.android_native_launch_tokens.write().await.clear();
    app.update_android_native_session_lifecycle(
        Some(&ticket.stream_ticket),
        Some(&issued.token_id),
        Some(&issued.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(
        app.bootstrap_android_native_web_session_from_native_session(
            &ticket.stream_ticket,
            Some(&issued.token_id),
            &issued.session_id
        )
        .await
        .is_err()
    );
    app.update_android_native_session_lifecycle(
        Some(&ticket.stream_ticket),
        Some(&issued.token_id),
        Some(&issued.session_id),
        "session_revoked".into(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(
        app.resolve_android_native_session(
            Some(&ticket.stream_ticket),
            Some(&issued.token_id),
            Some(&issued.session_id)
        )
        .await
        .is_err()
    );
}

async fn rental_device_ticket_fixture() -> (App, PathBuf, AndroidNativeStreamTicketRecord) {
    let (app, root) = fixture().await;
    let binding = AndroidNativeRentalBindingRecord {
        handoff_binding_id: Uuid::new_v4().to_string(),
        claim_id: "claim-root".into(),
        prepare_operation_id: None,
        rental_session_id: None,
    };
    let issued = app
        .issue_android_native_launch_token(
            UserId(9001),
            HostId(1),
            AppId(1379015545),
            AndroidNativeHostBindingRecord {
                name: "fixture".into(),
                address: "127.0.0.1".into(),
                http_port: 47989,
                https_port: None,
                external_port: None,
                unique_id: None,
                local_ip: None,
            },
            AndroidNativeTrustBootstrapRecord {
                paired: false,
                pair_mode: "fixture".into(),
                client_unique_id: None,
                client_certificate_pem: None,
                client_private_key_pem: None,
                server_certificate_pem: None,
            },
            Some(binding),
        )
        .await;
    let (_, mut ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    // A short remaining lease makes accidental extension by sibling issuance visible.
    ticket.expires_at_unix_ms = unix_time_ms_now() + 60_000;
    app.inner
        .android_native_stream_tickets
        .write()
        .await
        .insert(ticket.stream_ticket.clone(), ticket.clone());
    (app, root, ticket)
}

#[actix_web::test]
async fn same_claim_sibling_tickets_coexist_without_extending_authority() {
    let (app, root, first) = rental_device_ticket_fixture().await;
    let second = app
        .issue_sibling_rental_stream_ticket(
            &first.stream_ticket,
            "claim-root",
            first.host_id,
            first.app_id,
        )
        .await
        .unwrap();
    let third = app
        .issue_sibling_rental_stream_ticket(
            &second.stream_ticket,
            "claim-root",
            first.host_id,
            first.app_id,
        )
        .await
        .unwrap();
    assert_ne!(first.stream_ticket, second.stream_ticket);
    assert_ne!(second.stream_ticket, third.stream_ticket);
    for ticket in [&first, &second, &third] {
        assert_eq!(ticket.session_id, first.session_id);
        assert_eq!(
            ticket.token_id, first.token_id,
            "identifier remains bound to the root lifecycle"
        );
        assert_eq!(ticket.user_id, first.user_id);
        assert_eq!(ticket.host_id, first.host_id);
        assert_eq!(ticket.app_id, first.app_id);
        assert_eq!(ticket.rental_binding, first.rental_binding);
        assert_eq!(ticket.expires_at_unix_ms, first.expires_at_unix_ms);
        assert!(
            app.authorize_android_native_stream_ticket_for_stream(
                &ticket.stream_ticket,
                ticket.host_id,
                ticket.app_id,
            )
            .await
            .is_ok()
        );
    }
    let before = app.inner.android_native_stream_tickets.read().await.len();
    for (credential, claim, host, application) in [
        ("bad-bearer", "claim-root", first.host_id, first.app_id),
        (
            first.token_id.as_str(),
            "claim-root",
            first.host_id,
            first.app_id,
        ),
        (
            first.stream_ticket.as_str(),
            "other-claim",
            first.host_id,
            first.app_id,
        ),
        (
            first.stream_ticket.as_str(),
            " claim-root",
            first.host_id,
            first.app_id,
        ),
        (
            first.stream_ticket.as_str(),
            "",
            first.host_id,
            first.app_id,
        ),
        (
            first.stream_ticket.as_str(),
            "claim-root",
            HostId(2),
            first.app_id,
        ),
        (
            first.stream_ticket.as_str(),
            "claim-root",
            first.host_id,
            AppId(881448767),
        ),
    ] {
        assert!(
            app.issue_sibling_rental_stream_ticket(credential, claim, host, application)
                .await
                .is_err()
        );
        assert_eq!(
            app.inner.android_native_stream_tickets.read().await.len(),
            before
        );
    }
    // This is the same terminal lifecycle guard used by authorization, not just map removal.
    app.update_android_native_session_lifecycle(
        Some(&first.stream_ticket),
        Some(&first.token_id),
        Some(&first.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    for ticket in [&first, &second, &third] {
        assert!(
            app.peek_android_native_stream_ticket(&ticket.stream_ticket)
                .await
                .is_err()
        );
        assert!(
            app.issue_sibling_rental_stream_ticket(
                &ticket.stream_ticket,
                "claim-root",
                ticket.host_id,
                ticket.app_id,
            )
            .await
            .is_err()
        );
        assert!(
            app.refresh_android_native_stream_ticket(
                &ticket.stream_ticket,
                Some(&ticket.token_id),
                Some(&ticket.session_id),
            )
            .await
            .is_err()
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn sibling_ticket_requires_exact_live_root_binding() {
    let (app, root, parent) = rental_device_ticket_fixture().await;
    let baseline =
        app.inner.android_native_session_lifecycle.read().await[&parent.session_id].clone();
    for case in 0..8 {
        let mut state = baseline.clone();
        match case {
            0 => state.user_id = UserId(555),
            1 => state.token_id = "different-root-token-id".into(),
            2 => state.rental_binding.as_mut().unwrap().claim_id = "other-claim".into(),
            3 => {
                state.rental_binding.as_mut().unwrap().handoff_binding_id =
                    Uuid::new_v4().to_string()
            }
            4 => state.rental_binding = None,
            5 => state.hidden_state_status = "revoking".into(),
            6 => state.completed_at_unix_ms = Some(unix_time_ms_now()),
            _ => state.session_status = "ended".into(),
        }
        app.inner
            .android_native_session_lifecycle
            .write()
            .await
            .insert(parent.session_id.clone(), state);
        assert!(
            app.issue_sibling_rental_stream_ticket(
                &parent.stream_ticket,
                "claim-root",
                parent.host_id,
                parent.app_id,
            )
            .await
            .is_err(),
            "case {case} must fail without minting a bearer"
        );
        assert_eq!(
            app.inner.android_native_stream_tickets.read().await.len(),
            1
        );
    }
    app.inner
        .android_native_session_lifecycle
        .write()
        .await
        .insert(parent.session_id.clone(), baseline);
    let mut consumed = parent.clone();
    consumed.consumed_at_unix_ms = Some(unix_time_ms_now());
    app.inner
        .android_native_stream_tickets
        .write()
        .await
        .insert(parent.stream_ticket.clone(), consumed);
    assert!(matches!(
        app.issue_sibling_rental_stream_ticket(
            &parent.stream_ticket,
            "claim-root",
            parent.host_id,
            parent.app_id,
        )
        .await,
        Err(AppError::AndroidNativeStreamTicketConsumed)
    ));
    let mut expired = parent.clone();
    expired.expires_at_unix_ms = unix_time_ms_now().saturating_sub(1);
    app.inner
        .android_native_stream_tickets
        .write()
        .await
        .insert(parent.stream_ticket.clone(), expired);
    assert!(
        app.issue_sibling_rental_stream_ticket(
            &parent.stream_ticket,
            "claim-root",
            parent.host_id,
            parent.app_id,
        )
        .await
        .is_err()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn sibling_capability_limit_is_bounded_and_does_not_evict_other_bearers() {
    let (app, root, parent) = rental_device_ticket_fixture().await;
    // Capability capacity is not a device count. Parent + 63 siblings = 64.
    for _ in 1..64 {
        app.issue_sibling_rental_stream_ticket(
            &parent.stream_ticket,
            "claim-root",
            parent.host_id,
            parent.app_id,
        )
        .await
        .unwrap();
    }
    assert_eq!(
        app.inner.android_native_stream_tickets.read().await.len(),
        64
    );
    assert!(matches!(
        app.issue_sibling_rental_stream_ticket(
            &parent.stream_ticket,
            "claim-root",
            parent.host_id,
            parent.app_id,
        )
        .await,
        Err(AppError::AndroidNativeTicketCapacity)
    ));
    let tickets = app
        .inner
        .android_native_stream_tickets
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for ticket in tickets {
        assert!(
            app.peek_android_native_stream_ticket(&ticket.stream_ticket)
                .await
                .is_ok()
        );
        assert_eq!(ticket.expires_at_unix_ms, parent.expires_at_unix_ms);
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn explicit_same_claim_root_launch_is_separate_bearer_and_never_extends_authority() {
    let (app, root) = fixture().await;
    let binding = AndroidNativeRentalBindingRecord {
        handoff_binding_id: Uuid::new_v4().to_string(),
        claim_id: "claim-root".into(),
        prepare_operation_id: None,
        rental_session_id: None,
    };
    let first = app
        .issue_android_native_launch_token(
            UserId(9001),
            HostId(1),
            AppId(1379015545),
            AndroidNativeHostBindingRecord {
                name: "a".into(),
                address: "127.0.0.1".into(),
                http_port: 47989,
                https_port: None,
                external_port: None,
                unique_id: None,
                local_ip: None,
            },
            AndroidNativeTrustBootstrapRecord {
                paired: false,
                pair_mode: "a".into(),
                client_unique_id: None,
                client_certificate_pem: None,
                client_private_key_pem: None,
                server_certificate_pem: None,
            },
            Some(binding.clone()),
        )
        .await;
    let (_, mut ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&first.launch_token)
        .await
        .unwrap();
    ticket.expires_at_unix_ms = unix_time_ms_now() + 60_000;
    app.inner
        .android_native_stream_tickets
        .write()
        .await
        .insert(ticket.stream_ticket.clone(), ticket.clone());
    let second = app
        .issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await
        .unwrap();
    assert_eq!(second.session_id, first.session_id);
    assert_eq!(second.token_id, first.token_id);
    assert_ne!(second.launch_token, first.launch_token);
    assert!(second.expires_at_unix_ms <= ticket.expires_at_unix_ms);
    let (_, sibling_ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&second.launch_token)
        .await
        .unwrap();
    assert_eq!(sibling_ticket.session_id, ticket.session_id);
    assert_eq!(sibling_ticket.expires_at_unix_ms, ticket.expires_at_unix_ms);
    let renewed = app
        .refresh_android_native_stream_ticket(&sibling_ticket.stream_ticket, None, None)
        .await
        .unwrap();
    assert_eq!(renewed.expires_at_unix_ms, ticket.expires_at_unix_ms);
    assert_eq!(sibling_ticket.rental_binding, ticket.rental_binding);
    assert!(
        app.peek_android_native_stream_ticket(&ticket.stream_ticket)
            .await
            .is_ok()
    );
    assert!(
        app.peek_android_native_stream_ticket(&sibling_ticket.stream_ticket)
            .await
            .is_ok()
    );

    let token_count = app.inner.android_native_launch_tokens.read().await.len();
    let mut other_binding = binding.clone();
    other_binding.claim_id = "other".into();
    for result in [
        app.issue_android_native_sibling_launch_token(
            "missing",
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            UserId(7),
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            HostId(2),
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            AppId(2),
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            other_binding,
        )
        .await,
    ] {
        assert!(result.is_err());
    }
    assert_eq!(
        app.inner.android_native_launch_tokens.read().await.len(),
        token_count
    );
    app.update_android_native_session_lifecycle(
        Some(&ticket.stream_ticket),
        Some(&first.token_id),
        Some(&first.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding
        )
        .await
        .is_err()
    );
    assert!(
        app.peek_android_native_stream_ticket(&ticket.stream_ticket)
            .await
            .is_err()
    );
    assert!(
        app.peek_android_native_stream_ticket(&sibling_ticket.stream_ticket)
            .await
            .is_err()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn same_claim_can_join_before_first_device_consumes_launch() {
    let (app, root) = fixture().await;
    let binding = AndroidNativeRentalBindingRecord {
        handoff_binding_id: Uuid::new_v4().to_string(),
        claim_id: "claim-pending-root".into(),
        prepare_operation_id: None,
        rental_session_id: None,
    };
    let first = app
        .issue_android_native_launch_token(
            UserId(9001),
            HostId(1),
            AppId(1379015545),
            AndroidNativeHostBindingRecord {
                name: "a".into(),
                address: "127.0.0.1".into(),
                http_port: 47989,
                https_port: None,
                external_port: None,
                unique_id: None,
                local_ip: None,
            },
            AndroidNativeTrustBootstrapRecord {
                paired: false,
                pair_mode: "a".into(),
                client_unique_id: None,
                client_certificate_pem: None,
                client_private_key_pem: None,
                server_certificate_pem: None,
            },
            Some(binding.clone()),
        )
        .await;
    assert!(
        app.inner
            .android_native_stream_tickets
            .read()
            .await
            .is_empty()
    );
    let pending_count = app.inner.android_native_launch_tokens.read().await.len();
    let mut wrong_claim = binding.clone();
    wrong_claim.claim_id = "other-claim".into();
    for rejected in [
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            wrong_claim,
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            HostId(2),
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            AppId(2),
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            binding.clone(),
        )
        .await,
    ] {
        assert!(matches!(
            rejected,
            Err(AppError::AndroidNativeStreamTicketBindingMismatch)
        ));
    }
    assert_eq!(
        app.inner.android_native_launch_tokens.read().await.len(),
        pending_count
    );
    let mut second_binding = binding.clone();
    second_binding.handoff_binding_id = Uuid::new_v4().to_string();
    let second = app
        .issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            second_binding.clone(),
        )
        .await
        .expect("parallel devices must join a pending root, not require first playback");
    assert_eq!(second.session_id, first.session_id);
    assert_ne!(second.launch_token, first.launch_token);
    let mut third_binding = binding.clone();
    third_binding.handoff_binding_id = Uuid::new_v4().to_string();
    let third = app
        .issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            third_binding,
        )
        .await
        .unwrap();
    assert_eq!(third.session_id, first.session_id);
    let mut fourth_binding = binding.clone();
    fourth_binding.handoff_binding_id = Uuid::new_v4().to_string();
    assert!(matches!(
        app.issue_android_native_sibling_launch_token(
            &first.session_id,
            first.user_id,
            first.host_id,
            first.app_id,
            first.host_binding.clone(),
            first.trust_bootstrap.clone(),
            fourth_binding,
        )
        .await,
        Err(AppError::AndroidNativeDeviceLimit)
    ));
    // Either device may reach the player first. Neither consumes the other's bearer.
    let (_, second_ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&second.launch_token)
        .await
        .unwrap();
    let (_, first_ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&first.launch_token)
        .await
        .unwrap();
    assert_eq!(second_ticket.rental_binding, Some(second_binding));
    assert_eq!(first_ticket.rental_binding, Some(binding));
    assert!(second_ticket.expires_at_unix_ms <= first_ticket.expires_at_unix_ms);
    assert!(
        app.peek_android_native_stream_ticket(&first_ticket.stream_ticket)
            .await
            .is_ok()
    );
    assert!(
        app.peek_android_native_stream_ticket(&second_ticket.stream_ticket)
            .await
            .is_ok()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn valid_refresh_requires_possession_and_keeps_host_app_binding() {
    let (app, _root) = fixture().await;
    let issued = launch(&app).await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&issued.launch_token)
        .await
        .unwrap();
    assert!(matches!(
        app.refresh_android_native_stream_ticket(&ticket.stream_ticket, Some("wrong-token"), None)
            .await,
        Err(AppError::AndroidNativeStreamTicketBindingMismatch)
    ));
    let refreshed = app
        .refresh_android_native_stream_ticket(
            &ticket.stream_ticket,
            Some(&issued.token_id),
            Some(&issued.session_id),
        )
        .await
        .unwrap();
    assert_eq!(refreshed.session_id, ticket.session_id);
    assert_eq!(refreshed.host_id, ticket.host_id);
    assert_eq!(refreshed.app_id, ticket.app_id);
    assert_ne!(refreshed.stream_ticket, ticket.stream_ticket);
    assert!(matches!(
        app.authorize_android_native_stream_ticket_for_stream(
            &refreshed.stream_ticket,
            HostId(2),
            ticket.app_id
        )
        .await,
        Err(AppError::AndroidNativeStreamTicketBindingMismatch)
    ));
}
