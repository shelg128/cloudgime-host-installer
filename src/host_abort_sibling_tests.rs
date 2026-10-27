use super::*;
#[actix_web::test]
async fn aborted_sibling_before_stream_start_revokes_only_issued_root() {
    let root = std::env::temp_dir().join(format!("cg-abort-sibling-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut config = common::config::Config::default();
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().into(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    let app = App::new(config).await.unwrap();
    let host = AndroidNativeHostBindingRecord {
        name: "test".into(),
        address: "127.0.0.1".into(),
        http_port: 47989,
        https_port: None,
        external_port: None,
        unique_id: None,
        local_ip: None,
    };
    let trust = AndroidNativeTrustBootstrapRecord {
        paired: false,
        pair_mode: "test".into(),
        client_unique_id: None,
        client_certificate_pem: None,
        client_private_key_pem: None,
        server_certificate_pem: None,
    };
    let mut c = command();
    let binding = AndroidNativeRentalBindingRecord {
        handoff_binding_id: c.handoff_binding_id.clone(),
        claim_id: c.claim_id.clone(),
        prepare_operation_id: None,
        rental_session_id: None,
    };
    let launch = app
        .issue_android_native_launch_token(
            UserId(3),
            HostId(1),
            AppId(1379015545),
            host.clone(),
            trust.clone(),
            Some(binding.clone()),
        )
        .await;
    let mut sibling_binding = binding.clone();
    sibling_binding.handoff_binding_id = uuid::Uuid::new_v4().to_string();
    let sibling = app
        .issue_android_native_sibling_launch_token(
            &launch.session_id,
            UserId(3),
            HostId(1),
            AppId(1379015545),
            host,
            trust,
            sibling_binding.clone(),
        )
        .await
        .unwrap();
    c.native_session_id = launch.session_id.clone();
    let mut wrong = c.clone();
    wrong.handoff_binding_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(wrong)).await.err(),
        Some("HOST_REVOKE_RENTAL_BINDING_MISMATCH")
    );
    c.handoff_binding_id = sibling_binding.handoff_binding_id;
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(c.clone()))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        app.revoke_backend_rental_stream(&signed(c)).await.unwrap(),
        0
    );
    assert!(
        app.consume_android_native_launch_token_with_stream_ticket(&launch.launch_token)
            .await
            .is_err()
    );
    assert!(
        app.consume_android_native_launch_token_with_stream_ticket(&sibling.launch_token)
            .await
            .is_err()
    );
    assert!(
        crate::native_stream_scope::NATIVE_STREAM_SCOPES
            .begin(&launch.session_id, 93)
            .await
            .is_err()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
