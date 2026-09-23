use actix_web::{
    Error, FromRequest, HttpRequest, HttpResponse,
    body::MessageBody,
    cookie::{Cookie, Expiration, SameSite, time::OffsetDateTime},
    dev::{Payload, ServiceRequest, ServiceResponse},
    get,
    middleware::Next,
    post,
    web::{Data, Json},
};
use common::api_bindings::PostLoginRequest;
use futures::future::{Ready, ready};
use std::{pin::Pin, time::Duration};

use crate::app::{
    App, AppError,
    auth::{SessionToken, UserAuth},
    user::{Admin, AuthenticatedUser},
};

pub const COOKIE_SESSION_TOKEN_NAME: &str = "mlSession";
pub const COOKIE_RENTAL_STREAM_TICKET_NAME: &str = "mlRentalStream";

/// Cookie takes precedence over any legacy Host identity. Missing/invalid
/// ticket state is never converted into a general Host account session.
pub(crate) fn rental_stream_ticket(req: &HttpRequest) -> Result<Option<String>, AppError> {
    let cookies = req.cookies().map_err(|_| AppError::Unauthorized)?;
    let mut matches = cookies
        .iter()
        .filter(|c| c.name() == COOKIE_RENTAL_STREAM_TICKET_NAME);
    let Some(cookie) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(AppError::Unauthorized);
    }
    let value = cookie.value();
    let suffix = value
        .strip_prefix("mlnatstream_")
        .ok_or(AppError::Unauthorized)?;
    if suffix.len() != 32
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AppError::Unauthorized);
    }
    Ok(Some(value.to_string()))
}

/// Same scoped authority regardless of transport. Copying a cookie into a
/// Bearer header must not grant access to Host management APIs.
pub(crate) fn scoped_stream_ticket(req: &HttpRequest) -> Result<Option<String>, AppError> {
    let cookie = rental_stream_ticket(req)?;
    if req.headers().get_all("authorization").count() > 1 {
        return Err(AppError::Unauthorized);
    }
    if let Some(header) = req.headers().get("authorization") {
        let raw = header.to_str().map_err(|_| AppError::Unauthorized)?;
        let token = raw.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
        if token.is_empty() || token.chars().any(char::is_whitespace) {
            return Err(AppError::Unauthorized);
        }
        if let Some(ref value) = cookie {
            if value != token {
                return Err(AppError::Unauthorized);
            }
        }
        if token.starts_with("mlnatstream_") {
            let id = &token[12..];
            if id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(AppError::Unauthorized);
            }
            return Ok(Some(token.to_owned()));
        }
    }
    Ok(cookie)
}

pub(crate) fn rental_request_allowed(method: &str, route: &str) -> bool {
    (method == "GET" && matches!(route, "/authenticate" | "/host/stream" | "/host/mic"))
        // New explicit bearer/launch token must be independently authorized;
        // an old browser cookie must not block acquiring a new rental session.
        || (method == "POST" && matches!(route,
            "/android-native/bootstrap-web-session" | "/android-native/bootstrap-web-session-from-native-session"))
}

fn independently_authenticated_route(method: &str, route: &str) -> bool {
    method == "POST"
        && matches!(
            route,
            "/android-native/launch-token-from-tunnel"
                | "/android-native/revoke-unstarted-claim"
                | "/android-native/consume-launch"
                | "/android-native/bootstrap-web-session"
                | "/android-native/bootstrap-web-session-from-native-session"
        )
}

pub async fn rental_cookie_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let app = req.app_data::<Data<App>>().ok_or(AppError::AppDestroyed)?;
    let prefix = format!(
        "{}/api",
        app.config()
            .web_server
            .url_path_prefix
            .trim_end_matches('/')
    );
    let route = req
        .path()
        .strip_prefix(&prefix)
        .ok_or(AppError::Forbidden)?;
    let independent = independently_authenticated_route(req.method().as_str(), route);
    // These handlers validate a one-use launch token/current bearer themselves.
    // Stale browser cookies are ignored and cannot authorize the bootstrap.
    if independent {
        return next.call(req).await;
    }
    let ticket = scoped_stream_ticket(req.request())?;
    if app.config().web_server.rental_stream_ticket_required && ticket.is_none() {
        return Err(AppError::Unauthorized.into());
    }
    if let Some(ticket) = ticket {
        let allowed = rental_request_allowed(req.method().as_str(), route)
            || (req.method() == actix_web::http::Method::POST
                && matches!(
                    route,
                    "/android-native/refresh-stream-ticket"
                        | "/android-native/session-event"
                        | "/android-native/session-lifecycle"
                ));
        if !allowed {
            return Err(AppError::Forbidden.into());
        }
        // Terminal lifecycle action may reconcile an already revoking session;
        // the lifecycle handler validates that transition itself.
        if route != "/android-native/session-lifecycle" {
            app.peek_android_native_stream_ticket(&ticket).await?;
        }
    }
    next.call(req).await
}

impl FromRequest for UserAuth {
    type Error = AppError;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(extract_user_auth(req))
    }
}
fn extract_user_auth(req: &HttpRequest) -> Result<UserAuth, AppError> {
    if let Some(ticket) = scoped_stream_ticket(req)? {
        return Ok(UserAuth::AndroidNativeStreamTicket {
            stream_ticket: ticket,
        });
    }
    let app = match req.app_data::<Data<App>>() {
        None => return Err(AppError::AppDestroyed),
        Some(value) => value,
    };

    if let Some(header_auth) = &app.config().web_server.forwarded_header
        && let Some(username) = req.headers().get(&header_auth.username_header)
    {
        let Ok(username) = username.to_str() else {
            return Err(AppError::HeaderAuthMalformed);
        };

        Ok(UserAuth::ForwardedHeaders {
            username: username.to_string(),
        })
    } else if let Some(bearer) = req.headers().get("Authorization") {
        // Look for bearer
        let Ok(bearer) = bearer.to_str() else {
            return Err(AppError::BearerMalformed);
        };

        let token_str = bearer
            .strip_prefix("Bearer")
            .ok_or(AppError::AuthorizationNotBearer)?
            .trim();

        if token_str.starts_with("mlnatstream_") {
            return Ok(UserAuth::AndroidNativeStreamTicket {
                stream_ticket: token_str.to_string(),
            });
        }

        let token = SessionToken::decode(token_str)?;

        Ok(UserAuth::Session(token))
    } else if let Some(cookie) = req.cookie(COOKIE_SESSION_TOKEN_NAME) {
        // Look for cookie
        let token = SessionToken::decode(cookie.value())?;

        Ok(UserAuth::Session(token))
    } else {
        Ok(UserAuth::None)
    }
}

impl FromRequest for AuthenticatedUser {
    type Error = AppError;

    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let app = match req.app_data::<Data<App>>() {
            None => return Box::pin(ready(Err(AppError::AppDestroyed))),
            Some(value) => value,
        };

        let auth_future = UserAuth::from_request(req, payload);

        let app = app.clone();
        Box::pin(async move {
            let auth = auth_future.await?;

            let user = app.user_by_auth(auth).await?;

            Ok(user)
        })
    }
}

impl FromRequest for Admin {
    type Error = AppError;

    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let future = AuthenticatedUser::from_request(req, payload);

        Box::pin(async move {
            let user = future.await?;

            user.into_admin().await
        })
    }
}

#[post("/login")]
async fn login(
    app: Data<App>,
    Json(request): Json<PostLoginRequest>,
) -> Result<HttpResponse, Error> {
    let user = if app.config().web_server.first_login_create_admin {
        match app
            .try_add_first_login(request.name.clone(), request.password.clone())
            .await
        {
            Ok(user) => user,
            Err(AppError::FirstUserAlreadyExists) => {
                app.user_by_auth(UserAuth::UserPassword {
                    username: request.name,
                    password: request.password,
                })
                .await?
            }
            Err(err) => return Err(err.into()),
        }
    } else {
        app.user_by_auth(UserAuth::UserPassword {
            username: request.name,
            password: request.password,
        })
        .await?
    };

    let session_expiration = app.config().web_server.session_cookie_expiration;

    let session = user.new_session(session_expiration).await?;
    let mut session_bytes = [0; _];
    let session_str = session.encode(&mut session_bytes);

    Ok(HttpResponse::Ok()
        .cookie(build_cookie(&app, session_expiration, session_str))
        .finish())
}

#[post("/logout")]
async fn logout(app: Data<App>, auth: UserAuth, req: HttpRequest) -> Result<HttpResponse, Error> {
    let session = match auth {
        UserAuth::Session(session) => session,
        _ => return Ok(HttpResponse::BadRequest().finish()),
    };

    app.delete_session(session).await?;

    let mut response = HttpResponse::Ok().finish();

    if req.cookie(COOKIE_SESSION_TOKEN_NAME).is_some() {
        response.add_removal_cookie(&build_cookie(&app, Duration::ZERO, ""))?;
    }

    Ok(response)
}

pub async fn auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let Some(app) = req.app_data::<Data<App>>().cloned() else {
        return Err(AppError::AppDestroyed.into());
    };

    let mut response = next.call(req).await?;
    if let Some(err) = response.response().error()
        && let Some(AppError::SessionTokenNotFound) = err.as_error::<AppError>()
    {
        response
            .response_mut()
            .add_removal_cookie(&build_cookie(&app, Duration::ZERO, ""))?;
    }

    Ok(response)
}

pub fn build_cookie<'a>(app: &'a App, expiration: Duration, session_str: &'a str) -> Cookie<'a> {
    Cookie::build(COOKIE_SESSION_TOKEN_NAME, session_str)
        .path(&app.config().web_server.url_path_prefix)
        .same_site(SameSite::Strict)
        .http_only(true) // not accessible via js
        .secure(app.config().web_server.session_cookie_secure)
        .expires(Expiration::DateTime(OffsetDateTime::now_utc() + expiration))
        .finish()
}

pub fn build_rental_stream_cookie<'a>(
    app: &'a App,
    expiration: Duration,
    ticket: &'a str,
) -> Cookie<'a> {
    Cookie::build(COOKIE_RENTAL_STREAM_TICKET_NAME, ticket)
        .path(&app.config().web_server.url_path_prefix)
        .same_site(SameSite::Strict)
        .http_only(true)
        .secure(app.config().web_server.session_cookie_secure)
        .expires(Expiration::DateTime(OffsetDateTime::now_utc() + expiration))
        .finish()
}

#[get("/authenticate")]
async fn authenticate(_user: AuthenticatedUser) -> HttpResponse {
    HttpResponse::Ok().finish()
}

#[cfg(test)]
mod scoped_rental_tests {
    use super::*;
    use actix_web::{cookie::Cookie, test};

    fn ticket(ch: char) -> String {
        format!("mlnatstream_{}", ch.to_string().repeat(32))
    }

    #[actix_web::test]
    async fn scoped_ticket_precedes_legacy_cookie_and_rejects_conflicts() {
        let rental = ticket('a');
        let request = test::TestRequest::default()
            .cookie(Cookie::new(COOKIE_SESSION_TOKEN_NAME, "0".repeat(64)))
            .cookie(Cookie::new(
                COOKIE_RENTAL_STREAM_TICKET_NAME,
                rental.clone(),
            ))
            .to_http_request();
        assert_eq!(scoped_stream_ticket(&request).unwrap(), Some(rental));
        let conflicting = test::TestRequest::default()
            .cookie(Cookie::new(COOKIE_RENTAL_STREAM_TICKET_NAME, ticket('a')))
            .insert_header(("authorization", format!("Bearer {}", ticket('b'))))
            .to_http_request();
        assert!(scoped_stream_ticket(&conflicting).is_err());
        let duplicate = test::TestRequest::default()
            .append_header(("authorization", format!("Bearer {}", ticket('a'))))
            .append_header(("authorization", format!("Bearer {}", ticket('a'))))
            .to_http_request();
        assert!(scoped_stream_ticket(&duplicate).is_err());
    }

    #[actix_web::test]
    async fn malformed_cookie_never_falls_back_to_host_session() {
        for value in [
            "bad",
            "mlnatstream_ABCDEF00000000000000000000000000",
            "mlnatstream_0000000000000000000000000000000",
            "mlnatstream_000000000000000000000000000000000",
        ] {
            let request = test::TestRequest::default()
                .cookie(Cookie::new(COOKIE_RENTAL_STREAM_TICKET_NAME, value))
                .cookie(Cookie::new(COOKIE_SESSION_TOKEN_NAME, "0".repeat(64)))
                .to_http_request();
            assert!(scoped_stream_ticket(&request).is_err());
        }
    }

    #[actix_web::test]
    async fn rental_ticket_has_a_minimal_route_allowlist() {
        for (method, route) in [
            ("GET", "/authenticate"),
            ("GET", "/host/stream"),
            ("GET", "/host/mic"),
        ] {
            assert!(rental_request_allowed(method, route));
        }
        for (method, route) in [
            ("GET", "/user"),
            ("GET", "/hosts"),
            ("GET", "/apps"),
            ("GET", "/admin/host-capability"),
            ("POST", "/login"),
            ("POST", "/host/cancel"),
            ("POST", "/pair"),
            ("DELETE", "/host"),
        ] {
            assert!(!rental_request_allowed(method, route), "{method} {route}");
        }
    }

    #[actix_web::test]
    async fn rental_cookie_is_http_only_scoped_and_finite() {
        let mut config = common::config::Config::default();
        config.data_storage = common::config::StorageConfig::Json {
            path: std::env::temp_dir()
                .join(format!("cg-cookie-{}.json", uuid::Uuid::new_v4()))
                .to_string_lossy()
                .to_string(),
            session_expiration_check_interval: Duration::from_secs(300),
        };
        let app = App::new(config).await.unwrap();
        let value = ticket('a');
        let cookie = build_rental_stream_cookie(&app, Duration::from_secs(30), &value);
        assert_eq!(cookie.name(), COOKIE_RENTAL_STREAM_TICKET_NAME);
        assert_eq!(cookie.value(), value);
        assert!(cookie.http_only().unwrap());
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        assert!(cookie.expires().is_some());
    }
}
