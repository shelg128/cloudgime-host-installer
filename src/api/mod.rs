use actix_web::{
    HttpRequest, HttpResponse, delete,
    dev::HttpServiceFactory,
    get,
    http::header,
    middleware::from_fn,
    patch, post, services,
    web::{self, Data, Json, Query},
};
use futures::future::try_join_all;
use log::warn;
use moonlight_common::{crypto::openssl::OpenSSLCryptoBackend, http::pair::PairPin};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::spawn;
use url::Url;

use crate::{
    api::{
        admin::{
            add_user, delete_user, get_host_capability_profile, get_host_operations_status,
            list_users, patch_user, refresh_host_capability_profile,
        },
        android_native::{
            issue_android_native_launch_response, post_android_native_bootstrap_web_session,
            post_android_native_bootstrap_web_session_from_native_session,
            post_android_native_consume_launch, post_android_native_display_control,
            post_android_native_launch_token, post_android_native_launch_token_from_tunnel,
            post_android_native_refresh_stream_ticket, post_android_native_session_event,
            post_android_native_session_lifecycle,
            post_android_native_shared_session_consume_invite,
            post_android_native_shared_session_invite,
            post_android_native_shared_session_invite_loopback,
        },
        auth::auth_middleware,
        response_streaming::StreamedResponse,
    },
    app::{
        App, AppError,
        host::{AppId, HostId},
        storage::StorageHostModify,
        user::{AuthenticatedUser, Role, UserId},
    },
};
use common::api_bindings::{
    self, DeleteHostQuery, DetailedUser, GetAppImageQuery, GetAppsQuery, GetAppsResponse,
    GetHostQuery, GetHostResponse, GetHostsResponse, GetUserQuery, PatchHostRequest,
    PostHostRequest, PostHostResponse, PostPairRequest, PostPairResponse1, PostPairResponse2,
    PostWakeUpRequest, UndetailedHost,
};

pub mod admin;
pub mod android_native;
pub mod auth;
pub mod stream;

pub mod response_streaming;

#[derive(Serialize)]
struct QuickLaunchTargetResponse {
    host_id: u32,
    host_name: String,
    app_id: u32,
    app_title: String,
    is_hdr_supported: bool,
    native_scheme_url: String,
    open_native_path: String,
    web_stream_path: String,
}

#[derive(Deserialize)]
struct QuickLaunchTargetQuery {
    #[serde(rename = "hostId")]
    host_id_camel: Option<u32>,
    host_id: Option<u32>,
}

fn prefixed_path(path_prefix: &str, suffix: &str) -> String {
    let prefix = path_prefix.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    if prefix.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{prefix}/{suffix}")
    }
}

fn request_query_param(req: &HttpRequest, key: &str) -> Option<String> {
    url::form_urlencoded::parse(req.query_string().as_bytes())
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

fn append_query_param_to_url(raw_url: &str, key: &str, value: &str) -> String {
    if value.trim().is_empty() {
        return raw_url.to_string();
    }

    let Ok(mut url) = Url::parse(raw_url) else {
        return raw_url.to_string();
    };

    let already_present = url.query_pairs().any(|(candidate, _)| candidate == key);
    if !already_present {
        url.query_pairs_mut().append_pair(key, value);
    }

    url.into()
}

fn append_query_param_to_path(raw_path: &str, key: &str, value: &str) -> String {
    if value.trim().is_empty() || raw_path.contains(&format!("{key}=")) {
        return raw_path.to_string();
    }

    let encoded_query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair(key, value)
        .finish();
    let separator = if raw_path.contains('?') { '&' } else { '?' };
    format!("{raw_path}{separator}{encoded_query}")
}

fn host_launch_rank(host: &UndetailedHost) -> u8 {
    match (host.server_state, &host.paired) {
        (Some(api_bindings::HostState::Free), common::api_bindings::PairStatus::Paired) => 0,
        (Some(api_bindings::HostState::Free), _) => 1,
        (Some(api_bindings::HostState::Busy), common::api_bindings::PairStatus::Paired) => 2,
        (Some(api_bindings::HostState::Busy), _) => 3,
        (None, common::api_bindings::PairStatus::Paired) => 4,
        _ => 5,
    }
}

fn pick_default_app(apps: Vec<crate::app::host::App>) -> Option<crate::app::host::App> {
    let mut iter = apps.into_iter();
    let mut fallback = None;
    while let Some(app) = iter.next() {
        if fallback.is_none() {
            fallback = Some(crate::app::host::App {
                id: app.id,
                title: app.title.clone(),
                is_hdr_supported: app.is_hdr_supported,
            });
        }
        if app.title.eq_ignore_ascii_case("Desktop") {
            return Some(app);
        }
    }
    fallback
}

#[get("/user")]
async fn get_user(
    app: Data<App>,
    mut user: AuthenticatedUser,
    Query(query): Query<GetUserQuery>,
) -> Result<Json<DetailedUser>, AppError> {
    match (query.name, query.user_id) {
        (None, None) => {
            let detailed_user = user.detailed_user().await?;

            Ok(Json(detailed_user))
        }
        (None, Some(user_id)) => {
            let target_user_id = UserId(user_id);

            let mut target_user = app.user_by_id(target_user_id).await?;

            let detailed_user = target_user.detailed_user(&mut user).await?;

            Ok(Json(detailed_user))
        }
        (Some(name), None) => {
            let mut target_user = app.user_by_name(&name).await?;

            let detailed_user = target_user.detailed_user(&mut user).await?;

            Ok(Json(detailed_user))
        }
        (Some(_), Some(_)) => Err(AppError::BadRequest),
    }
}

#[get("/hosts")]
async fn list_hosts(
    mut user: AuthenticatedUser,
) -> Result<StreamedResponse<GetHostsResponse, UndetailedHost>, AppError> {
    let (mut stream_response, stream_sender) =
        StreamedResponse::new(GetHostsResponse { hosts: Vec::new() });

    let hosts = user.hosts().await?;

    // Try join all because storage should always work, the actual host info will be send using response streaming
    let undetailed_hosts = try_join_all(hosts.into_iter().map(move |mut host| {
        let mut user = user.clone();
        let stream_sender = stream_sender.clone();

        async move {
            // First query db
            let undetailed_cache = host.undetailed_host_cached(&mut user).await;

            // Then send http request now
            let mut user = user.clone();

            spawn(async move {
                let undetailed = match host.undetailed_host(&mut user).await {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("Failed to get undetailed host of {host:?}: {err}");
                        return;
                    }
                };

                if let Err(err) = stream_sender.send(undetailed).await {
                    warn!(
                        "Failed to send back undetailed host data using response streaming: {err}"
                    );
                }
            });

            undetailed_cache
        }
    }))
    .await?;

    stream_response.set_initial(GetHostsResponse {
        hosts: undetailed_hosts,
    });

    Ok(stream_response)
}

#[get("/host")]
async fn get_host(
    mut user: AuthenticatedUser,
    Query(query): Query<GetHostQuery>,
) -> Result<Json<GetHostResponse>, AppError> {
    let host_id = HostId(query.host_id);

    let mut host = user.host(host_id).await?;

    let detailed = host.detailed_host(&mut user).await?;

    Ok(Json(GetHostResponse { host: detailed }))
}

#[post("/host")]
async fn post_host(
    app: Data<App>,
    mut user: AuthenticatedUser,
    Json(request): Json<PostHostRequest>,
) -> Result<Json<PostHostResponse>, AppError> {
    let mut host = user
        .host_add(
            request.address,
            request
                .http_port
                .unwrap_or(app.config().moonlight.default_http_port),
        )
        .await?;

    Ok(Json(PostHostResponse {
        host: host.detailed_host(&mut user).await?,
    }))
}

#[patch("/host")]
async fn patch_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PatchHostRequest>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    let mut modify = StorageHostModify::default();

    let role = user.role().await?;
    if request.change_owner {
        match role {
            Role::Admin => {
                modify.owner = Some(request.owner.map(UserId));
            }
            Role::User => {
                return Err(AppError::Forbidden);
            }
        }
    }

    host.modify(&mut user, modify).await?;

    Ok(HttpResponse::Ok().finish())
}

#[delete("/host")]
async fn delete_host(
    mut user: AuthenticatedUser,
    Query(query): Query<DeleteHostQuery>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(query.host_id);

    user.host_delete(host_id).await?;

    Ok(HttpResponse::Ok().finish())
}

#[post("/pair")]
async fn pair_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostPairRequest>,
) -> Result<StreamedResponse<PostPairResponse1, PostPairResponse2>, AppError> {
    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    let pin = PairPin::new_random(&OpenSSLCryptoBackend)?;

    let (stream_response, stream_sender) =
        StreamedResponse::new(PostPairResponse1::Pin(pin.to_string()));

    spawn(async move {
        let result = host.pair(&mut user, pin).await;

        let result = match result {
            Ok(()) => host.detailed_host(&mut user).await,
            Err(err) => Err(err),
        };

        match result {
            Ok(detailed_host) => {
                if let Err(err) = stream_sender
                    .send(PostPairResponse2::Paired(detailed_host))
                    .await
                {
                    warn!("Failed to send pair success: {err}");
                }
            }
            Err(err) => {
                warn!("Failed to pair host: {err}");
                if let Err(err) = stream_sender.send(PostPairResponse2::PairError).await {
                    warn!("Failed to send pair failure: {err}");
                }
            }
        }
    });

    Ok(stream_response)
}

#[post("/host/wake")]
async fn wake_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostWakeUpRequest>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(request.host_id);

    let host = user.host(host_id).await?;

    host.wake(&mut user).await?;

    Ok(HttpResponse::Ok().finish())
}

#[get("/apps")]
async fn get_apps(
    mut user: AuthenticatedUser,
    Query(query): Query<GetAppsQuery>,
) -> Result<Json<GetAppsResponse>, AppError> {
    let host_id = HostId(query.host_id);

    let mut host = user.host(host_id).await?;

    let apps = host.list_apps(&mut user).await?;

    Ok(Json(GetAppsResponse {
        apps: apps
            .into_iter()
            .map(|app| api_bindings::App {
                app_id: app.id.0,
                title: app.title,
                is_hdr_supported: app.is_hdr_supported,
            })
            .collect(),
    }))
}

#[get("/quick-launch-target")]
async fn get_quick_launch_target(
    app: Data<App>,
    req: HttpRequest,
    mut user: AuthenticatedUser,
    query: Query<QuickLaunchTargetQuery>,
) -> Result<Json<QuickLaunchTargetResponse>, AppError> {
    let requested_host_id = query.host_id_camel.or(query.host_id);

    let (mut host, summary) = if let Some(host_id) = requested_host_id {
        let mut host = user.host(HostId(host_id)).await?;
        let summary = host.undetailed_host(&mut user).await?;
        (host, summary)
    } else {
        let mut selected: Option<(crate::app::host::Host, UndetailedHost, u8)> = None;

        for mut host in user.hosts().await? {
            let summary = host.undetailed_host(&mut user).await?;
            let rank = host_launch_rank(&summary);

            let should_replace = match &selected {
                Some((_, _, current_rank)) => rank < *current_rank,
                None => true,
            };

            if should_replace {
                selected = Some((host, summary, rank));
                if rank == 0 {
                    break;
                }
            }
        }

        let (host, summary, _) = selected.ok_or(AppError::HostNotFound)?;
        (host, summary)
    };

    let selected_app =
        pick_default_app(host.list_apps(&mut user).await?).ok_or(AppError::BadRequest)?;

    let launch_response = issue_android_native_launch_response(
        &app,
        &req,
        &mut user,
        &mut host,
        HostId(summary.host_id),
        selected_app.id,
        "windows",
    )
    .await?;

    Ok(Json(QuickLaunchTargetResponse {
        host_id: summary.host_id,
        host_name: summary.name,
        app_id: launch_response.app_id,
        app_title: selected_app.title,
        is_hdr_supported: selected_app.is_hdr_supported,
        native_scheme_url: launch_response.native_scheme_url,
        open_native_path: launch_response.open_native_path,
        web_stream_path: launch_response.web_stream_path,
    }))
}

#[get("/app/image")]
async fn get_app_image(
    mut user: AuthenticatedUser,
    Query(query): Query<GetAppImageQuery>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(query.host_id);
    let app_id = AppId(query.app_id);

    let mut host = user.host(host_id).await?;

    let image = host
        .app_image(&mut user, app_id, query.force_refresh)
        .await?;

    let mut hasher = Sha256::new();
    hasher.update(&image);
    let etag = format!("\"{:x}\"", hasher.finalize());

    let cache_control = "private, no-cache, must-revalidate";

    if let Some(if_none_match) = req.headers().get(header::IF_NONE_MATCH) {
        if if_none_match.to_str().ok() == Some(&etag) && query.force_refresh == false {
            return Ok(HttpResponse::NotModified()
                .insert_header((header::ETAG, etag))
                .insert_header((header::CACHE_CONTROL, cache_control))
                .finish());
        }
    }

    Ok(HttpResponse::Ok()
        .insert_header((header::ETAG, etag))
        .insert_header((header::CACHE_CONTROL, cache_control))
        .body(image))
}

pub fn api_service() -> impl HttpServiceFactory {
    web::scope("/api")
        .service(services![
            post_android_native_shared_session_consume_invite,
            post_android_native_shared_session_invite_loopback,
            stream::shared_session_player2_ws
        ])
        .service(
            web::scope("")
                .wrap(from_fn(auth_middleware))
                .service(services![
                    // -- Auth
                    auth::login,
                    auth::logout,
                    auth::authenticate
                ])
                .service(services![
                    // -- Host
                    get_user,
                    list_hosts,
                    get_host,
                    post_host,
                    patch_host,
                    wake_host,
                    delete_host,
                    pair_host,
                    get_quick_launch_target,
                    get_apps,
                    get_app_image,
                ])
                .service(services![
                    // -- Stream
                    stream::start_host,
                    stream::start_host_mic,
                    stream::cancel_host,
                ])
                .service(services![
                    // -- Android Native
                    post_android_native_launch_token,
                    post_android_native_launch_token_from_tunnel,
                    post_android_native_bootstrap_web_session,
                    post_android_native_bootstrap_web_session_from_native_session,
                    post_android_native_consume_launch,
                    post_android_native_display_control,
                    post_android_native_refresh_stream_ticket,
                    post_android_native_session_event,
                    post_android_native_session_lifecycle,
                    post_android_native_shared_session_invite,
                ])
                .service(services![
                    // -- Admin
                    add_user,
                    patch_user,
                    delete_user,
                    list_users,
                    get_host_operations_status,
                    get_host_capability_profile,
                    refresh_host_capability_profile
                ]),
        )
}
