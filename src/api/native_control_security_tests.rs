use super::*;
use actix_web::{App as HttpApp, test};

#[actix_web::test]
async fn tunnel_requires_exact_host_runtime_bearer_and_strict_rental_binding() {
    let valid = || Ok("host-runtime-fixture".to_string());
    fn valid_fixture() -> Result<String, AppError> {
        Ok("host-runtime-fixture".to_string())
    }
    fn missing_fixture() -> Result<String, AppError> {
        Err(AppError::Unauthorized)
    }
    let absent = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .to_http_request();
    assert_eq!(
        classify_host_tunnel(&absent, valid_fixture),
        TunnelAuthCategory::HeaderMissing
    );
    let basic = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .insert_header(("authorization", "Basic fixture-only"))
        .to_http_request();
    assert_eq!(
        classify_host_tunnel(&basic, missing_fixture),
        TunnelAuthCategory::HeaderMalformed
    );
    let remote = test::TestRequest::default()
        .peer_addr("203.0.113.9:10000".parse().unwrap())
        .insert_header(("authorization", "Bearer host-runtime-fixture"))
        .to_http_request();
    assert_eq!(
        classify_host_tunnel(&remote, valid_fixture),
        TunnelAuthCategory::PeerNonLoopback
    );
    let bearer = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .insert_header(("authorization", "Bearer host-runtime-fixture"))
        .to_http_request();
    assert_eq!(
        classify_host_tunnel(&bearer, missing_fixture),
        TunnelAuthCategory::CredentialLoadFailed
    );
    assert_eq!(
        classify_host_tunnel(&bearer, || Ok("other-fixture".to_string())),
        TunnelAuthCategory::CredentialMismatch
    );
    assert_eq!(
        classify_host_tunnel(&bearer, || Ok("fixture token-with-space".to_string())),
        TunnelAuthCategory::CredentialShapeInvalid
    );
    assert_eq!(
        classify_host_tunnel(&bearer, valid_fixture),
        TunnelAuthCategory::Accepted
    );
    let duplicate = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .append_header(("authorization", "Bearer fixture-one"))
        .append_header(("authorization", "Bearer fixture-two"))
        .to_http_request();
    assert_eq!(
        classify_host_tunnel(&duplicate, valid_fixture),
        TunnelAuthCategory::HeaderDuplicate
    );
    for request in [
        test::TestRequest::default()
            .peer_addr("127.0.0.1:10000".parse().unwrap())
            .to_http_request(),
        test::TestRequest::default()
            .peer_addr("127.0.0.1:10000".parse().unwrap())
            .insert_header(("authorization", "Basic host-runtime-fixture"))
            .to_http_request(),
        test::TestRequest::default()
            .peer_addr("127.0.0.1:10000".parse().unwrap())
            .insert_header(("authorization", "Bearer wrong"))
            .to_http_request(),
        test::TestRequest::default()
            .peer_addr("203.0.113.9:10000".parse().unwrap())
            .insert_header(("authorization", "Bearer host-runtime-fixture"))
            .to_http_request(),
    ] {
        assert!(authorize_host_tunnel(&request, valid).is_err());
    }
    let duplicate = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .append_header(("authorization", "Bearer host-runtime-fixture"))
        .append_header(("authorization", "Bearer host-runtime-fixture"))
        .to_http_request();
    assert!(authorize_host_tunnel(&duplicate, valid).is_err());
    let accepted = test::TestRequest::default()
        .peer_addr("127.0.0.1:10000".parse().unwrap())
        .insert_header(("authorization", "Bearer host-runtime-fixture"))
        .to_http_request();
    authorize_host_tunnel(&accepted, valid).unwrap();
    assert!(authorize_host_tunnel(&accepted, || Err(AppError::Unauthorized)).is_err());

    let id = uuid::Uuid::new_v4().to_string();
    let parsed:AndroidNativeRentalBindingRequest=serde_json::from_value(serde_json::json!({
        "handoffBindingId":id,"claimId":"claim-1","prepareOperationId":"prepare_fixture_0001","rentalSessionId":"session-1"
    })).unwrap();
    let binding = validate_tunnel_rental_binding(Some("sentinel_prelogin"), None, Some(parsed))
        .unwrap()
        .unwrap();
    assert_eq!(binding.handoff_binding_id, id);
    assert_eq!(binding.claim_id, "claim-1");
    let invalid =
        |value| serde_json::from_value::<AndroidNativeRentalBindingRequest>(value).unwrap();
    assert!(validate_tunnel_rental_binding(None,None,Some(invalid(serde_json::json!({"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"c"})))).is_err());
    assert!(validate_tunnel_rental_binding(Some("sentinel_prelogin"),Some(881448767),Some(invalid(serde_json::json!({"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"c"})))).is_err());
    assert!(
        validate_tunnel_rental_binding(
            Some("sentinel_prelogin"),
            None,
            Some(invalid(
                serde_json::json!({"handoffBindingId":"bad","claimId":"c"})
            ))
        )
        .is_err()
    );
    assert!(validate_tunnel_rental_binding(Some("sentinel_prelogin"),None,Some(invalid(serde_json::json!({"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"c","prepareOperationId":"prepare_fixture_0001"})))).is_err());
    assert!(serde_json::from_value::<AndroidNativeRentalBindingRequest>(serde_json::json!({"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"c","extra":true})).is_err());

    let root_payload:PostAndroidNativeTunnelLaunchTokenRequest=serde_json::from_value(serde_json::json!({
        "streamProfile":"sentinel_prelogin","rootNativeSessionId":"android-native-root",
        "rentalBinding":{"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"claim-1"}
    })).unwrap();
    assert_eq!(
        root_payload.root_native_session_id.as_deref(),
        Some("android-native-root")
    );
    assert!(serde_json::from_value::<PostAndroidNativeTunnelLaunchTokenRequest>(serde_json::json!({
        "streamProfile":"sentinel_prelogin","rootNativeSessionId":"root","rentalBinding":{"handoffBindingId":uuid::Uuid::new_v4().to_string(),"claimId":"c"},"extra":true
    })).is_err());
}

#[actix_web::test]
async fn invalid_header_never_falls_back_to_body_secret() {
    let request = test::TestRequest::default()
        .insert_header(("authorization", "Basic public-id"))
        .to_http_request();
    assert!(native_control_credential(&request, Some("otherwise-valid-body-secret")).is_err());
    let request = test::TestRequest::default().to_http_request();
    assert!(native_control_credential(&request, None).is_err());
    assert_eq!(
        native_control_credential(&request, Some("launch-secret")).unwrap(),
        "launch-secret"
    );
    let request = test::TestRequest::default()
        .insert_header(("authorization", "Bearer stream-secret"))
        .to_http_request();
    assert_eq!(
        native_control_credential(&request, Some("different-body-secret")).unwrap(),
        "stream-secret"
    );
}

#[actix_web::test]
async fn control_http_routes_require_credential_before_side_effects() {
    let root = std::env::temp_dir().join(format!("cg-native-http-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = common::config::Config::default();
    config.web_server.default_user_id = None;
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().to_string(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    let app = App::new(config).await.unwrap();
    let service = test::init_service(
        HttpApp::new()
            .app_data(Data::new(app))
            .service(post_android_native_bootstrap_web_session_from_native_session)
            .service(post_android_native_session_event)
            .service(post_android_native_session_lifecycle)
            .service(post_android_native_display_control),
    )
    .await;
    for (path, payload) in [
        (
            "/android-native/bootstrap-web-session-from-native-session",
            serde_json::json!({"session_id":"public-session","token_id":"public-token"}),
        ),
        (
            "/android-native/session-event",
            serde_json::json!({"session_id":"public-session","token_id":"public-token","event_name":"test","stage":"test"}),
        ),
        (
            "/android-native/session-lifecycle",
            serde_json::json!({"session_id":"public-session","token_id":"public-token","action":"session_started"}),
        ),
        (
            "/android-native/display-control",
            serde_json::json!({"session_id":"public-session","token_id":"public-token","action":"repair"}),
        ),
    ] {
        let req = test::TestRequest::post()
            .uri(path)
            .set_json(&payload)
            .to_request();
        let response = test::call_service(&service, req).await;
        assert_eq!(
            response.status(),
            actix_web::http::StatusCode::UNAUTHORIZED,
            "{path}"
        );
        assert!(response.headers().get("set-cookie").is_none());
    }
}

#[actix_web::test]
async fn tunnel_payload_from_backend_preserves_sentinel_profile() {
    let payload: PostAndroidNativeTunnelLaunchTokenRequest =
        serde_json::from_value(serde_json::json!({"streamProfile":"sentinel_prelogin"})).unwrap();
    let id = crate::stream_controller::resolve_tunnel_launch_app(
        payload.stream_profile.as_deref(),
        payload.app_id,
    )
    .unwrap();
    assert_eq!(id, crate::stream_controller::SENTINEL_PRELOGIN_APP_ID);
}
