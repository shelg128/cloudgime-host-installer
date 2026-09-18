use super::*;
use crate::api::auth::{COOKIE_RENTAL_STREAM_TICKET_NAME, COOKIE_SESSION_TOKEN_NAME};
use crate::app::auth::SessionToken;
use crate::app::{
    AndroidNativeHostBindingRecord, AndroidNativeLaunchTokenRecord,
    AndroidNativeRentalBindingRecord, AndroidNativeTrustBootstrapRecord,
    host::{AppId, HostId},
};
use actix_web::{App as HttpApp, cookie::Cookie, http::StatusCode, test, web};
use std::time::Duration;

async fn fixture() -> (
    Data<App>,
    std::path::PathBuf,
    common::config::Config,
    AndroidNativeLaunchTokenRecord,
) {
    let root = std::env::temp_dir().join(format!("cg-cookie-http-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut config = common::config::Config::default();
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().to_string(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    config.web_server.default_user_id = None;
    config.web_server.first_login_create_admin = true;
    config.web_server.url_path_prefix = "/pilot".into();
    config.web_server.session_cookie_secure = true;
    let app = Data::new(App::new(config.clone()).await.unwrap());
    let user = app
        .try_add_first_login("fixture-owner".into(), "fixture-only-password".into())
        .await
        .unwrap();
    let launch = app
        .issue_android_native_launch_token(
            user.id(),
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
            Some(AndroidNativeRentalBindingRecord {
                handoff_binding_id: uuid::Uuid::new_v4().to_string(),
                claim_id: "claim".into(),
                prepare_operation_id: None,
                rental_session_id: None,
            }),
        )
        .await;
    (app, root, config, launch)
}

#[actix_web::test]
async fn bootstrap_cookie_is_scoped_and_revoked_in_http_even_for_host_admin_owner() {
    let (app, root, config, launch) = fixture().await;
    let service = test::init_service(
        HttpApp::new()
            .app_data(app.clone())
            .service(web::scope("/pilot").service(crate::api::api_service())),
    )
    .await;
    let response = test::call_service(
        &service,
        test::TestRequest::post()
            .uri("/pilot/api/android-native/bootstrap-web-session")
            .set_json(serde_json::json!({"launch_token":launch.launch_token}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let cookie = response
        .response()
        .cookies()
        .find(|c| c.name() == COOKIE_RENTAL_STREAM_TICKET_NAME)
        .unwrap()
        .into_owned();
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.path(), Some("/pilot"));
    assert!(
        SessionToken::decode(cookie.value()).is_err(),
        "not a general Host session token"
    );
    let old = response
        .response()
        .cookies()
        .find(|c| c.name() == COOKIE_SESSION_TOKEN_NAME)
        .unwrap();
    assert_eq!(old.value(), "");
    let body = test::read_body(response).await;
    assert!(
        !String::from_utf8_lossy(&body).contains(cookie.value()),
        "ticket stays HttpOnly, not response JSON"
    );
    let record = app
        .peek_android_native_stream_ticket(cookie.value())
        .await
        .unwrap();
    assert_eq!(record.session_id, launch.session_id);
    assert_eq!(record.rental_binding, launch.rental_binding);
    let authenticated = test::call_service(
        &service,
        test::TestRequest::get()
            .uri("/pilot/api/authenticate")
            .cookie(cookie.clone())
            .to_request(),
    )
    .await;
    assert_eq!(authenticated.status(), StatusCode::OK);
    for path in [
        "/user",
        "/users",
        "/hosts",
        "/apps",
        "/admin/host-capability",
        "/quick-launch-target",
    ] {
        for bearer in [false, true] {
            let mut request = test::TestRequest::get().uri(&format!("/pilot/api{path}"));
            request = if bearer {
                request.insert_header(("authorization", format!("Bearer {}", cookie.value())))
            } else {
                request.cookie(cookie.clone())
            };
            let denied = test::try_call_service(&service, request.to_request()).await;
            match denied {
                Ok(r) => assert_eq!(r.status(), StatusCode::FORBIDDEN),
                Err(e) => assert_eq!(e.as_response_error().status_code(), StatusCode::FORBIDDEN),
            }
        }
    }
    for path in [
        "/login",
        "/pair",
        "/host",
        "/host/cancel",
        "/android-native/launch-token-from-tunnel",
    ] {
        let denied = test::try_call_service(
            &service,
            test::TestRequest::post()
                .uri(&format!("/pilot/api{path}"))
                .cookie(cookie.clone())
                .set_json(serde_json::json!({}))
                .to_request(),
        )
        .await;
        let expected = if path == "/android-native/launch-token-from-tunnel" {
            StatusCode::UNAUTHORIZED // independently verifies Host runtime bearer
        } else {
            StatusCode::FORBIDDEN
        };
        match denied {
            Ok(r) => assert_eq!(r.status(), expected),
            Err(e) => assert_eq!(e.as_response_error().status_code(), expected),
        }
    }
    assert!(
        app.authorize_android_native_stream_ticket_for_stream(
            cookie.value(),
            HostId(2),
            record.app_id
        )
        .await
        .is_err()
    );
    assert!(
        app.authorize_android_native_stream_ticket_for_stream(
            cookie.value(),
            record.host_id,
            AppId(0)
        )
        .await
        .is_err()
    );
    app.update_android_native_session_lifecycle(
        Some(cookie.value()),
        Some(&launch.token_id),
        Some(&launch.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    let denied = test::try_call_service(
        &service,
        test::TestRequest::get()
            .uri("/pilot/api/authenticate")
            .cookie(cookie.clone())
            .to_request(),
    )
    .await;
    match denied {
        Ok(r) => assert!(!r.status().is_success()),
        Err(e) => assert_eq!(e.as_response_error().status_code(), StatusCode::GONE),
    }
    // New App instance uses the same storage, but in-memory ticket authority is
    // absent. The old cookie cannot become a persisted Host-account login.
    let restarted = Data::new(App::new(config).await.unwrap());
    let service2 = test::init_service(
        HttpApp::new()
            .app_data(restarted)
            .service(web::scope("/pilot").service(crate::api::api_service())),
    )
    .await;
    let denied = test::try_call_service(
        &service2,
        test::TestRequest::get()
            .uri("/pilot/api/authenticate")
            .cookie(cookie)
            .to_request(),
    )
    .await;
    match denied {
        Ok(r) => assert!(!r.status().is_success()),
        Err(e) => assert_eq!(e.as_response_error().status_code(), StatusCode::NOT_FOUND),
    }
    drop(service2);
    drop(service);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn native_to_web_bootstrap_does_not_mint_general_host_cookie_or_extend_ticket() {
    let (app, root, _, launch) = fixture().await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&launch.launch_token)
        .await
        .unwrap();
    app.consume_android_native_stream_ticket(&ticket.stream_ticket, ticket.host_id, ticket.app_id)
        .await
        .unwrap();
    let service = test::init_service(
        HttpApp::new()
            .app_data(app.clone())
            .service(web::scope("/pilot").service(crate::api::api_service())),
    )
    .await;
    let request = test::TestRequest::post()
        .uri("/pilot/api/android-native/bootstrap-web-session-from-native-session")
        .insert_header(("authorization", format!("Bearer {}", ticket.stream_ticket)))
        .cookie(Cookie::new(
            COOKIE_RENTAL_STREAM_TICKET_NAME,
            "expired-cookie",
        ))
        .set_json(serde_json::json!({"session_id":launch.session_id,"token_id":launch.token_id}))
        .to_request();
    let response = test::call_service(&service, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .response()
        .cookies()
        .find(|c| c.name() == COOKIE_RENTAL_STREAM_TICKET_NAME)
        .unwrap();
    assert_ne!(cookie.value(), ticket.stream_ticket);
    let refreshed = app
        .peek_android_native_stream_ticket(cookie.value())
        .await
        .unwrap();
    assert_eq!(refreshed.session_id, ticket.session_id);
    assert_eq!(refreshed.token_id, ticket.token_id);
    assert_eq!(refreshed.host_id, ticket.host_id);
    assert_eq!(refreshed.app_id, ticket.app_id);
    assert_eq!(refreshed.rental_binding, ticket.rental_binding);
    assert_eq!(refreshed.expires_at_unix_ms, ticket.expires_at_unix_ms);
    assert!(refreshed.consumed_at_unix_ms.is_none());
    drop(response);
    drop(service);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn revoked_ticket_stops_idle_video_and_mic_children_but_not_another_session() {
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::{process::Command, sync::Mutex, time::timeout};
    let (app, root, _, launch) = fixture().await;
    let (_, ticket) = app
        .consume_android_native_launch_token_with_stream_ticket(&launch.launch_token)
        .await
        .unwrap();
    let registry = crate::native_stream_scope::NativeStreamRegistry::default();
    let video = registry.begin(&launch.session_id, 1).await.unwrap();
    let mic = registry.begin(&launch.session_id, 2).await.unwrap();
    let unrelated = registry.begin("unrelated-session", 3).await.unwrap();
    async fn worker(
        scope: &crate::native_stream_scope::NativeStreamScope,
    ) -> Arc<Mutex<tokio::process::Child>> {
        let mut permit = scope.launch_permit().await.unwrap();
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd.exe");
            c.args(["/D", "/C", "pause"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = Command::new("cat");
        let child = Arc::new(Mutex::new(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        ));
        assert!(permit.attach(child.clone()));
        child
    }
    let a = worker(&video).await;
    let b = worker(&mic).await;
    let other = worker(&unrelated).await;
    let video_watch =
        crate::api::stream::monitor_ticket_scope(app.clone(), ticket.clone(), video.clone());
    let mic_watch =
        crate::api::stream::monitor_ticket_scope(app.clone(), ticket.clone(), mic.clone());
    app.update_android_native_session_lifecycle(
        Some(&ticket.stream_ticket),
        Some(&launch.token_id),
        Some(&launch.session_id),
        "session_revoke_requested".into(),
        None,
        None,
    )
    .await
    .unwrap();
    timeout(Duration::from_secs(12), video_watch)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(12), mic_watch)
        .await
        .unwrap()
        .unwrap();
    assert!(video.is_cancelled() && mic.is_cancelled());
    assert!(a.lock().await.try_wait().unwrap().is_some());
    assert!(b.lock().await.try_wait().unwrap().is_some());
    assert!(other.lock().await.try_wait().unwrap().is_none());
    assert!(video.launch_permit().await.is_err());
    assert!(mic.launch_permit().await.is_err());
    assert!(
        app.authorize_android_native_stream_ticket_for_stream(
            &ticket.stream_ticket,
            ticket.host_id,
            ticket.app_id
        )
        .await
        .is_err()
    );
    registry.stop("unrelated-session").await.unwrap();
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn cookie_runtime_wiring_uses_scoped_authority_before_launch_and_cancels_both_directions() {
    let source = include_str!("stream.rs");
    let start = source
        .split("pub async fn start_host(")
        .nth(1)
        .unwrap()
        .split("#[get(\"/android-native/shared-session/player2/ws\")]")
        .next()
        .unwrap();
    assert!(
        start.contains("let scoped_ticket = crate::api::auth::scoped_stream_ticket(&request)?;")
    );
    assert!(start.contains("if let Some(stream_ticket) = scoped_ticket.as_ref()"));
    assert!(start.contains("monitor_ticket_scope(web_app.clone(), record.clone(), scope.clone())"));
    assert!(start.matches("scope_cancelled(&native_scope)").count() >= 2);
    let mic = source.split("pub async fn start_host_mic(").nth(1).unwrap();
    assert!(mic.contains("scoped_stream_ticket(&request)?"));
    assert!(
        mic.find("scope.launch_permit().await").unwrap()
            < mic.find("Command::new(&staged_mic_sidecar_path)").unwrap()
    );
    assert!(mic.matches("scope_cancelled(&native_scope)").count() >= 2);
    assert!(mic.contains("permit.attach(child.clone())"));
    assert!(mic.contains("if native_scope.is_none()"));
}

#[actix_web::test]
async fn required_mode_blocks_default_user_host_cookie_and_management_apis() {
    let root = std::env::temp_dir().join(format!("cg-ticket-required-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut config = common::config::Config::default();
    config.data_storage = common::config::StorageConfig::Json {
        path: root.join("data.json").to_string_lossy().into(),
        session_expiration_check_interval: Duration::from_secs(300),
    };
    // A configured default user must not bypass rental mode even if it does not
    // resolve: the middleware rejects before AuthenticatedUser is consulted.
    config.web_server.default_user_id = Some(1);
    config.web_server.rental_stream_ticket_required = true;
    config.web_server.url_path_prefix = "/pilot".into();
    let guarded = Data::new(App::new(config).await.unwrap());
    let service = test::init_service(
        HttpApp::new()
            .app_data(guarded.clone())
            .service(web::scope("/pilot").service(crate::api::api_service())),
    )
    .await;
    for path in [
        "/authenticate",
        "/host/stream",
        "/host/mic",
        "/user",
        "/hosts",
        "/apps",
        "/admin/host-capability",
    ] {
        let result = test::try_call_service(
            &service,
            test::TestRequest::get()
                .uri(&format!("/pilot/api{path}"))
                .to_request(),
        )
        .await;
        match result {
            Ok(r) => assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{path}"),
            Err(e) => assert_eq!(
                e.as_response_error().status_code(),
                StatusCode::UNAUTHORIZED,
                "{path}"
            ),
        }
    }
    // Independently authenticated launch bootstrap is still reachable; its own
    // one-use credential decides the result, not default_user_id.
    let bootstrap = test::call_service(
        &service,
        test::TestRequest::post()
            .uri("/pilot/api/android-native/bootstrap-web-session")
            .set_json(serde_json::json!({"launch_token":"invalid-fixture-token"}))
            .to_request(),
    )
    .await;
    assert_eq!(
        bootstrap.status(),
        StatusCode::NOT_FOUND,
        "handler independently rejects invalid launch token"
    );
    drop(bootstrap);
    drop(service);
    drop(guarded);
    std::fs::remove_dir_all(root).unwrap();
}

#[actix_web::test]
async fn required_mode_defaults_off_for_legacy_configs() {
    let config: common::config::Config = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(!config.web_server.rental_stream_ticket_required);
    assert!(!common::config::WebServerConfig::default().rental_stream_ticket_required);
}
