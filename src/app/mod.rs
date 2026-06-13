use std::{
    collections::HashMap,
    io,
    ops::Deref,
    path::PathBuf,
    sync::{Arc, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use actix_web::{ResponseError, http::StatusCode, web::Bytes};
use common::config::{Config, StorageConfig};
use hex::FromHexError;
use log::{error, info, warn};
use moonlight_common::{high::MoonlightClientError, http::client::tokio_hyper::TokioHyperClient};
use openssl::error::ErrorStack;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::app::{
    auth::{SessionToken, UserAuth},
    host::{AppId, HostId},
    password::StoragePassword,
    storage::{
        Either, Storage, StorageHostModify, StorageQueryHosts, StorageUserAdd, create_storage,
    },
    user::{Admin, AuthenticatedUser, Role, User, UserId},
};

pub mod auth;
pub mod host;
pub mod password;
pub mod storage;
pub mod user;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("the app got destroyed")]
    AppDestroyed,
    #[error("the user was not found")]
    UserNotFound,
    #[error("more than one user already exists")]
    FirstUserAlreadyExists,
    #[error("the config option first_login_create_admin is not true")]
    FirstLoginCreateAdminNotSet,
    #[error("the user already exists")]
    UserAlreadyExists,
    #[error("the host was not found")]
    HostNotFound,
    #[error("the host was already paired")]
    HostPaired,
    #[error("the host must be paired for this action")]
    HostNotPaired,
    // -- Unauthorized
    #[error("the credentials don't exists")]
    CredentialsWrong,
    #[error("the host was not found")]
    SessionTokenNotFound,
    #[error("the action is not allowed because the user is not authorized, 401")]
    Unauthorized,
    #[error("using a custom header for authorization is disabled")]
    HeaderAuthDisabled,
    // --
    #[error("the action is not allowed with the current privileges, 403")]
    Forbidden,
    // -- Bad Request
    #[error("the authorization header is not a bearer")]
    AuthorizationNotBearer,
    #[error("the custom header used to authorize is malformed")]
    HeaderAuthMalformed,
    #[error("the authorization header is not a bearer")]
    BearerMalformed,
    #[error("the password is empty")]
    PasswordEmpty,
    #[error("the password is empty")]
    NameEmpty,
    #[error("the authorization header is not a bearer")]
    BadRequest,
    #[error("the android native launch token was not found")]
    AndroidNativeLaunchTokenNotFound,
    #[error("the android native launch token already expired")]
    AndroidNativeLaunchTokenExpired,
    #[error("the android native launch token was already consumed")]
    AndroidNativeLaunchTokenConsumed,
    #[error("the android native stream ticket was not found")]
    AndroidNativeStreamTicketNotFound,
    #[error("the android native stream ticket already expired")]
    AndroidNativeStreamTicketExpired,
    #[error("the android native stream ticket was already consumed")]
    AndroidNativeStreamTicketConsumed,
    #[error("the android native stream ticket does not match the requested host/app")]
    AndroidNativeStreamTicketBindingMismatch,
    #[error("the android native session is no longer active")]
    AndroidNativeSessionInactive,
    #[error("there is no active android native owner session to share")]
    AndroidNativeSharedSessionActiveOwnerNotFound,
    #[error("the android native shared session invite was not found")]
    AndroidNativeSharedSessionInviteNotFound,
    #[error("the android native shared session invite already expired")]
    AndroidNativeSharedSessionInviteExpired,
    // --
    #[error("openssl error occured: {0}")]
    OpenSSL(#[from] ErrorStack),
    #[error("hex error occured: {0}")]
    Hex(#[from] FromHexError),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("moonlight error: {0}")]
    Moonlight(#[from] MoonlightClientError),
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::AppDestroyed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::FirstUserAlreadyExists => StatusCode::INTERNAL_SERVER_ERROR,
            Self::FirstLoginCreateAdminNotSet => StatusCode::INTERNAL_SERVER_ERROR,
            Self::HostNotFound => StatusCode::NOT_FOUND,
            Self::HostNotPaired => StatusCode::FORBIDDEN,
            Self::HostPaired => StatusCode::NOT_MODIFIED,
            Self::UserNotFound => StatusCode::NOT_FOUND,
            Self::UserAlreadyExists => StatusCode::CONFLICT,
            Self::CredentialsWrong => StatusCode::UNAUTHORIZED,
            Self::SessionTokenNotFound => StatusCode::UNAUTHORIZED,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::OpenSSL(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::HeaderAuthDisabled => StatusCode::UNAUTHORIZED,
            Self::Hex(_) => StatusCode::BAD_REQUEST,
            Self::AuthorizationNotBearer => StatusCode::BAD_REQUEST,
            Self::HeaderAuthMalformed => StatusCode::BAD_REQUEST,
            Self::BearerMalformed => StatusCode::BAD_REQUEST,
            Self::PasswordEmpty => StatusCode::BAD_REQUEST,
            Self::NameEmpty => StatusCode::BAD_REQUEST,
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::AndroidNativeLaunchTokenNotFound => StatusCode::NOT_FOUND,
            Self::AndroidNativeLaunchTokenExpired => StatusCode::UNAUTHORIZED,
            Self::AndroidNativeLaunchTokenConsumed => StatusCode::CONFLICT,
            Self::AndroidNativeStreamTicketNotFound => StatusCode::NOT_FOUND,
            Self::AndroidNativeStreamTicketExpired => StatusCode::UNAUTHORIZED,
            Self::AndroidNativeStreamTicketConsumed => StatusCode::CONFLICT,
            Self::AndroidNativeStreamTicketBindingMismatch => StatusCode::FORBIDDEN,
            Self::AndroidNativeSessionInactive => StatusCode::GONE,
            Self::AndroidNativeSharedSessionActiveOwnerNotFound => StatusCode::CONFLICT,
            Self::AndroidNativeSharedSessionInviteNotFound => StatusCode::NOT_FOUND,
            Self::AndroidNativeSharedSessionInviteExpired => StatusCode::UNAUTHORIZED,
            Self::Moonlight(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Clone)]
struct AppRef {
    inner: Weak<AppInner>,
}

impl AppRef {
    fn access(&self) -> Result<impl Deref<Target = AppInner> + 'static, AppError> {
        Weak::upgrade(&self.inner).ok_or(AppError::AppDestroyed)
    }
}

struct AppInner {
    config: Config,
    storage: Arc<dyn Storage + Send + Sync>,
    app_image_cache: RwLock<HashMap<(UserId, HostId, AppId), Bytes>>,
    android_native_launch_tokens: RwLock<HashMap<String, AndroidNativeLaunchTokenRecord>>,
    android_native_stream_tickets: RwLock<HashMap<String, AndroidNativeStreamTicketRecord>>,
    android_native_shared_session_invites:
        RwLock<HashMap<String, AndroidNativeSharedSessionInviteRecord>>,
    android_native_session_events: RwLock<HashMap<String, Vec<AndroidNativeSessionEventRecord>>>,
    android_native_session_lifecycle:
        RwLock<HashMap<String, AndroidNativeSessionLifecycleStateRecord>>,
}

pub type MoonlightClient = TokioHyperClient;

pub struct App {
    inner: Arc<AppInner>,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeHostBindingRecord {
    pub name: String,
    pub address: String,
    pub http_port: u16,
    pub https_port: Option<u16>,
    pub external_port: Option<u16>,
    pub unique_id: Option<String>,
    pub local_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeTrustBootstrapRecord {
    pub paired: bool,
    pub pair_mode: String,
    pub client_unique_id: Option<String>,
    pub client_certificate_pem: Option<String>,
    pub client_private_key_pem: Option<String>,
    pub server_certificate_pem: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeResolvedSession {
    pub token_id: String,
    pub session_id: String,
    pub user_id: UserId,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeLaunchTokenRecord {
    pub token_id: String,
    pub launch_token: String,
    pub session_id: String,
    pub user_id: UserId,
    pub host_id: HostId,
    pub app_id: AppId,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub consumed_at_unix_ms: Option<u64>,
    pub session_policy: common::api_bindings::AndroidNativeSessionPolicy,
    pub feature_profile: common::api_bindings::AndroidNativeFeatureProfile,
    pub host_binding: AndroidNativeHostBindingRecord,
    pub trust_bootstrap: AndroidNativeTrustBootstrapRecord,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeStreamTicketRecord {
    pub stream_ticket: String,
    pub token_id: String,
    pub session_id: String,
    pub user_id: UserId,
    pub host_id: HostId,
    pub app_id: AppId,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub consumed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeSharedSessionInviteRecord {
    pub invite_token: String,
    pub shared_session_id: String,
    pub owner_token_id: String,
    pub owner_session_id: String,
    pub user_id: UserId,
    pub host_id: HostId,
    pub app_id: AppId,
    pub role: common::api_bindings::AndroidNativeSharedSessionRole,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub consumed_at_unix_ms: Option<u64>,
    pub capabilities: common::api_bindings::AndroidNativeSharedSessionCapabilities,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeSessionEventRecord {
    pub token_id: String,
    pub session_id: String,
    pub user_id: UserId,
    pub sequence: u32,
    pub event_name: String,
    pub stage: String,
    pub detail: Option<String>,
    pub client_time_unix_ms: Option<u64>,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AndroidNativeSessionLifecycleStateRecord {
    pub token_id: String,
    pub session_id: String,
    pub user_id: UserId,
    pub trust_bootstrap_status: String,
    pub session_status: String,
    pub hidden_state_status: String,
    pub last_action: String,
    pub last_reason: Option<String>,
    pub last_updated_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct AndroidNativeActiveOwnerSessionRecord {
    token_id: String,
    session_id: String,
    user_id: UserId,
    host_id: HostId,
    app_id: AppId,
    last_updated_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct SharedPairStore {
    version: u8,
    hosts: Vec<SharedPairStoreHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SharedPairStoreHost {
    host_id: u32,
    address: String,
    http_port: u16,
    pair_info: storage::StorageHostPairInfo,
}

impl App {
    pub async fn new(config: Config) -> Result<Self, anyhow::Error> {
        let storage = create_storage(config.data_storage.clone()).await?;
        if let Err(err) = repair_shared_pair_store(&config, &storage).await {
            warn!("failed to repair shared pair store: {err}");
        }

        let app = AppInner {
            storage,
            config,
            app_image_cache: Default::default(),
            android_native_launch_tokens: Default::default(),
            android_native_stream_tickets: Default::default(),
            android_native_shared_session_invites: Default::default(),
            android_native_session_events: Default::default(),
            android_native_session_lifecycle: Default::default(),
        };

        Ok(Self {
            inner: Arc::new(app),
        })
    }

    fn new_ref(&self) -> AppRef {
        AppRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Handles all logic related to adding the first user:
    /// - Is this even currently allowed?
    /// - Moving hosts from global to first user
    pub async fn try_add_first_login(
        &self,
        username: String,
        password: String,
    ) -> Result<AuthenticatedUser, AppError> {
        if !self.config().web_server.first_login_create_admin {
            return Err(AppError::FirstLoginCreateAdminNotSet);
        }

        let any_user_exists = self.inner.storage.any_user_exists().await?;
        if any_user_exists {
            return Err(AppError::FirstUserAlreadyExists);
        }

        let mut user = self
            .add_user_no_auth(StorageUserAdd {
                name: username.clone(),
                password: Some(StoragePassword::new(&password)?),
                role: Role::Admin,
                client_unique_id: username,
            })
            .await?;

        if self.config().web_server.first_login_assign_global_hosts {
            // Note: only this user exists and all hosts are global, if migrated from v1 to v2
            // -> list_hosts will show just global hosts

            let hosts = user.hosts().await?;

            let user_id = user.id();
            for mut host in hosts {
                match host
                    .modify(
                        &mut user,
                        StorageHostModify {
                            owner: Some(Some(user_id)),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        warn!("failed to move global host to new user {user_id:?}: {err}");
                    }
                }
            }
        }

        Ok(user)
    }

    /// admin: The admin that tries to do this action
    pub async fn add_user(
        &self,
        _: &Admin,
        user: StorageUserAdd,
    ) -> Result<AuthenticatedUser, AppError> {
        self.add_user_no_auth(user).await
    }

    async fn add_user_no_auth(&self, user: StorageUserAdd) -> Result<AuthenticatedUser, AppError> {
        if user.name.is_empty() {
            return Err(AppError::NameEmpty);
        }

        let user = self.inner.storage.add_user(user).await?;

        Ok(AuthenticatedUser {
            inner: User {
                app: self.new_ref(),
                id: user.id,
                cache_storage: Some(user),
            },
        })
    }

    pub async fn user_by_auth(&self, auth: UserAuth) -> Result<AuthenticatedUser, AppError> {
        match auth {
            UserAuth::None => {
                let user_id = self.config().web_server.default_user_id.map(UserId);
                if let Some(user_id) = user_id {
                    let user = match self.user_by_id(user_id).await {
                        Ok(user) => user,
                        Err(AppError::UserNotFound) => {
                            error!("the default user {user_id:?} was not found!");
                            return Err(AppError::UserNotFound);
                        }
                        Err(err) => return Err(err),
                    };

                    user.authenticate(&UserAuth::None).await
                } else {
                    Err(AppError::Unauthorized)
                }
            }
            UserAuth::UserPassword { ref username, .. } => {
                let user = self.user_by_name(username).await?;

                user.authenticate(&auth).await
            }
            UserAuth::Session(session) => {
                let user = self.user_by_session(session).await?;

                Ok(user)
            }
            UserAuth::AndroidNativeStreamTicket { stream_ticket } => {
                let ticket = self
                    .peek_android_native_stream_ticket(&stream_ticket)
                    .await?;
                self.authenticated_user_by_id_trusted(ticket.user_id).await
            }
            UserAuth::ForwardedHeaders { ref username } => {
                let user = match self.user_by_name(username).await {
                    Ok(user) => user,
                    Err(AppError::UserNotFound) => {
                        let Some(config_forwarded_headers) =
                            &self.config().web_server.forwarded_header
                        else {
                            return Err(AppError::Unauthorized);
                        };

                        if !config_forwarded_headers.auto_create_missing_user {
                            return Err(AppError::Unauthorized);
                        }

                        let user = self
                            .add_user_no_auth(StorageUserAdd {
                                role: Role::User,
                                name: username.clone(),
                                password: None,
                                client_unique_id: username.clone(),
                            })
                            .await?;

                        return Ok(user);
                    }
                    Err(err) => return Err(err),
                };

                user.authenticate(&auth).await
            }
        }
    }

    pub async fn authenticated_user_by_id_trusted(
        &self,
        user_id: UserId,
    ) -> Result<AuthenticatedUser, AppError> {
        let user = self.user_by_id(user_id).await?;
        Ok(AuthenticatedUser { inner: user })
    }

    pub async fn user_by_id(&self, user_id: UserId) -> Result<User, AppError> {
        let user = self.inner.storage.get_user(user_id).await?;

        Ok(User {
            app: self.new_ref(),
            id: user_id,
            cache_storage: Some(user),
        })
    }
    pub async fn user_by_name(&self, name: &str) -> Result<User, AppError> {
        let (user_id, user) = self.inner.storage.get_user_by_name(name).await?;

        Ok(User {
            app: self.new_ref(),
            id: user_id,
            cache_storage: user,
        })
    }
    pub async fn user_by_session(
        &self,
        session: SessionToken,
    ) -> Result<AuthenticatedUser, AppError> {
        let (user_id, user) = self
            .inner
            .storage
            .get_user_by_session_token(session)
            .await?;

        Ok(AuthenticatedUser {
            inner: User {
                app: self.new_ref(),
                id: user_id,
                cache_storage: user,
            },
        })
    }

    pub async fn all_users(&self, _: Admin) -> Result<Vec<User>, AppError> {
        let users = self.inner.storage.list_users().await?;

        let users = match users {
            Either::Left(user_ids) => user_ids
                .into_iter()
                .map(|id| User {
                    app: self.new_ref(),
                    id,
                    cache_storage: None,
                })
                .collect::<Vec<_>>(),
            Either::Right(users) => users
                .into_iter()
                .map(|user| User {
                    app: self.new_ref(),
                    id: user.id,
                    cache_storage: Some(user),
                })
                .collect::<Vec<_>>(),
        };

        Ok(users)
    }

    pub async fn delete_session(&self, session: SessionToken) -> Result<(), AppError> {
        self.inner.storage.remove_session_token(session).await
    }

    pub async fn issue_android_native_launch_token(
        &self,
        user_id: UserId,
        host_id: HostId,
        app_id: AppId,
        host_binding: AndroidNativeHostBindingRecord,
        trust_bootstrap: AndroidNativeTrustBootstrapRecord,
    ) -> AndroidNativeLaunchTokenRecord {
        const TOKEN_TTL_MS: u64 = 300_000;

        let now = unix_time_ms_now();
        let token_id = format!("mlnat_{}", Uuid::new_v4().to_simple());
        let session_id = format!("android-native-{}", Uuid::new_v4().to_simple());
        let record = AndroidNativeLaunchTokenRecord {
            token_id: token_id.clone(),
            launch_token: token_id.clone(),
            session_id,
            user_id,
            host_id,
            app_id,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + TOKEN_TTL_MS,
            consumed_at_unix_ms: None,
            session_policy: common::api_bindings::AndroidNativeSessionPolicy {
                allow_add_host_ui: false,
                token_managed: true,
                billing_managed: true,
            },
            feature_profile: default_android_native_feature_profile(),
            host_binding,
            trust_bootstrap,
        };

        let mut tokens = self.inner.android_native_launch_tokens.write().await;
        tokens.retain(|_, value| {
            value.expires_at_unix_ms > now
                && value.consumed_at_unix_ms.is_none()
                && !(value.user_id == user_id && value.host_id == host_id && value.app_id == app_id)
        });
        tokens.insert(token_id, record.clone());

        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| {
            value.expires_at_unix_ms > now
                && !(value.user_id == user_id && value.host_id == host_id && value.app_id == app_id)
        });

        let mut lifecycle = self.inner.android_native_session_lifecycle.write().await;
        lifecycle.retain(|_, value| {
            value.completed_at_unix_ms.is_none() || value.last_updated_unix_ms + TOKEN_TTL_MS > now
        });
        lifecycle.insert(
            record.session_id.clone(),
            AndroidNativeSessionLifecycleStateRecord {
                token_id: record.token_id.clone(),
                session_id: record.session_id.clone(),
                user_id,
                trust_bootstrap_status: if record.trust_bootstrap.paired {
                    "paired_material_reused".to_string()
                } else {
                    "bootstrap_required".to_string()
                },
                session_status: "issued".to_string(),
                hidden_state_status: "not_staged".to_string(),
                last_action: "token_issued".to_string(),
                last_reason: None,
                last_updated_unix_ms: now,
                completed_at_unix_ms: None,
            },
        );
        record
    }

    async fn invalidate_android_native_session_artifacts(&self, session_id: &str) {
        if session_id.trim().is_empty() {
            return;
        }

        let now = unix_time_ms_now();
        {
            let mut tickets = self.inner.android_native_stream_tickets.write().await;
            tickets.retain(|_, value| {
                value.expires_at_unix_ms > now && value.session_id != session_id
            });
        }

        let mut tokens = self.inner.android_native_launch_tokens.write().await;
        tokens.retain(|_, value| value.expires_at_unix_ms > now && value.session_id != session_id);
    }

    async fn android_native_session_allows_stream_access(&self, session_id: &str) -> bool {
        if session_id.trim().is_empty() {
            return true;
        }

        let lifecycle = self.inner.android_native_session_lifecycle.read().await;
        lifecycle.get(session_id).is_none_or(|state| {
            state.completed_at_unix_ms.is_none()
                && !matches!(
                    state.session_status.as_str(),
                    "ended" | "expired" | "failed" | "abandoned"
                )
        })
    }

    pub async fn consume_android_native_launch_token(
        &self,
        launch_token: &str,
    ) -> Result<AndroidNativeLaunchTokenRecord, AppError> {
        let now = unix_time_ms_now();
        let mut tokens = self.inner.android_native_launch_tokens.write().await;
        let state = match tokens.get(launch_token) {
            Some(record) if record.expires_at_unix_ms <= now => 1_u8,
            Some(record) if record.consumed_at_unix_ms.is_some() => 2_u8,
            Some(_) => 0_u8,
            None => 3_u8,
        };

        match state {
            1 => {
                tokens.remove(launch_token);
                tokens.retain(|_, value| value.expires_at_unix_ms > now);
                Err(AppError::AndroidNativeLaunchTokenExpired)
            }
            2 => {
                tokens.retain(|_, value| value.expires_at_unix_ms > now);
                Err(AppError::AndroidNativeLaunchTokenConsumed)
            }
            3 => {
                tokens.retain(|_, value| value.expires_at_unix_ms > now);
                Err(AppError::AndroidNativeLaunchTokenNotFound)
            }
            _ => {
                let record = tokens
                    .get_mut(launch_token)
                    .ok_or(AppError::AndroidNativeLaunchTokenNotFound)?;
                record.consumed_at_unix_ms = Some(now);
                let clone = record.clone();
                tokens.retain(|_, value| value.expires_at_unix_ms > now);
                Ok(clone)
            }
        }
    }

    pub async fn consume_android_native_launch_token_with_stream_ticket(
        &self,
        launch_token: &str,
    ) -> Result<
        (
            AndroidNativeLaunchTokenRecord,
            AndroidNativeStreamTicketRecord,
        ),
        AppError,
    > {
        let record = self
            .consume_android_native_launch_token(launch_token)
            .await?;
        let stream_ticket = self.issue_android_native_stream_ticket(&record).await;
        Ok((record, stream_ticket))
    }

    pub async fn issue_android_native_stream_ticket(
        &self,
        launch_record: &AndroidNativeLaunchTokenRecord,
    ) -> AndroidNativeStreamTicketRecord {
        const STREAM_TICKET_TTL_MS: u64 = 8 * 60 * 60 * 1000;

        let now = unix_time_ms_now();
        let stream_ticket = format!("mlnatstream_{}", Uuid::new_v4().to_simple());
        let record = AndroidNativeStreamTicketRecord {
            stream_ticket: stream_ticket.clone(),
            token_id: launch_record.token_id.clone(),
            session_id: launch_record.session_id.clone(),
            user_id: launch_record.user_id,
            host_id: launch_record.host_id,
            app_id: launch_record.app_id,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + STREAM_TICKET_TTL_MS,
            consumed_at_unix_ms: None,
        };

        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        // A native direct session can have multiple authenticated channels
        // alive at once: main stream, microphone sidecar, and ticket refresh.
        // Keep same-session tickets until expiry so a refresh for one channel
        // cannot invalidate another channel in flight.
        tickets.retain(|_, value| value.expires_at_unix_ms > now);
        tickets.insert(stream_ticket, record.clone());
        record
    }

    pub async fn peek_android_native_stream_ticket(
        &self,
        stream_ticket: &str,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| value.expires_at_unix_ms > now);

        let record = tickets
            .get(stream_ticket)
            .cloned()
            .ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

        if record.expires_at_unix_ms <= now {
            return Err(AppError::AndroidNativeStreamTicketExpired);
        }
        if record.consumed_at_unix_ms.is_some() {
            return Err(AppError::AndroidNativeStreamTicketConsumed);
        }

        Ok(record)
    }

    pub async fn consume_android_native_stream_ticket(
        &self,
        stream_ticket: &str,
        host_id: HostId,
        app_id: AppId,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| value.expires_at_unix_ms > now);

        let record = tickets
            .get_mut(stream_ticket)
            .ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

        if record.expires_at_unix_ms <= now {
            return Err(AppError::AndroidNativeStreamTicketExpired);
        }
        if record.consumed_at_unix_ms.is_some() {
            return Err(AppError::AndroidNativeStreamTicketConsumed);
        }
        if record.host_id != host_id || record.app_id != app_id {
            return Err(AppError::AndroidNativeStreamTicketBindingMismatch);
        }

        record.consumed_at_unix_ms = Some(now);
        Ok(record.clone())
    }

    pub async fn authorize_android_native_stream_ticket_for_host(
        &self,
        stream_ticket: &str,
        host_id: HostId,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| value.expires_at_unix_ms > now);

        let record = tickets
            .get(stream_ticket)
            .cloned()
            .ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

        if record.expires_at_unix_ms <= now {
            return Err(AppError::AndroidNativeStreamTicketExpired);
        }
        if record.host_id != host_id {
            return Err(AppError::AndroidNativeStreamTicketBindingMismatch);
        }

        Ok(record)
    }

    pub async fn authorize_android_native_stream_ticket_for_stream(
        &self,
        stream_ticket: &str,
        host_id: HostId,
        app_id: AppId,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| value.expires_at_unix_ms > now);

        let record = tickets
            .get(stream_ticket)
            .cloned()
            .ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

        if record.expires_at_unix_ms <= now {
            return Err(AppError::AndroidNativeStreamTicketExpired);
        }
        if record.host_id != host_id || record.app_id != app_id {
            return Err(AppError::AndroidNativeStreamTicketBindingMismatch);
        }

        Ok(record)
    }

    pub async fn refresh_android_native_stream_ticket(
        &self,
        stream_ticket: &str,
        token_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let (source, reused_existing_ticket) = {
            let mut tickets = self.inner.android_native_stream_tickets.write().await;
            tickets.retain(|_, value| value.expires_at_unix_ms > now);

            let direct_source = tickets.get(stream_ticket).cloned();
            if direct_source.is_none()
                && let Some(active_ticket) = tickets
                    .values()
                    .find(|value| {
                        value.consumed_at_unix_ms.is_none()
                            && token_id.is_none_or(|requested| value.token_id == requested)
                            && session_id.is_none_or(|requested| value.session_id == requested)
                    })
                    .cloned()
            {
                (active_ticket, true)
            } else {
                let source = direct_source.ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

                if source.expires_at_unix_ms <= now {
                    return Err(AppError::AndroidNativeStreamTicketExpired);
                }
                if token_id.is_some_and(|value| value != source.token_id) {
                    return Err(AppError::AndroidNativeStreamTicketBindingMismatch);
                }
                if session_id.is_some_and(|value| value != source.session_id) {
                    return Err(AppError::AndroidNativeStreamTicketBindingMismatch);
                }

                (source, false)
            }
        };

        if !self
            .android_native_session_allows_stream_access(&source.session_id)
            .await
        {
            self.invalidate_android_native_session_artifacts(&source.session_id)
                .await;
            return Err(AppError::AndroidNativeSessionInactive);
        }

        if reused_existing_ticket {
            return Ok(source);
        }

        const STREAM_TICKET_TTL_MS: u64 = 8 * 60 * 60 * 1000;

        let refreshed_ticket = format!("mlnatstream_{}", Uuid::new_v4().to_simple());
        let refreshed_record = AndroidNativeStreamTicketRecord {
            stream_ticket: refreshed_ticket.clone(),
            token_id: source.token_id.clone(),
            session_id: source.session_id.clone(),
            user_id: source.user_id,
            host_id: source.host_id,
            app_id: source.app_id,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + STREAM_TICKET_TTL_MS,
            consumed_at_unix_ms: None,
        };

        let mut tickets = self.inner.android_native_stream_tickets.write().await;
        tickets.retain(|_, value| value.expires_at_unix_ms > now);
        tickets.insert(refreshed_ticket, refreshed_record.clone());

        Ok(refreshed_record)
    }

    pub async fn bootstrap_android_native_web_session_from_native_session(
        &self,
        token_id: Option<&str>,
        session_id: &str,
    ) -> Result<AndroidNativeStreamTicketRecord, AppError> {
        let now = unix_time_ms_now();
        let tickets = self.inner.android_native_stream_tickets.read().await;

        let record = tickets
            .values()
            .find(|value| {
                value.expires_at_unix_ms > now
                    && value.session_id == session_id
                    && token_id.is_none_or(|expected| value.token_id == expected)
            })
            .cloned()
            .ok_or(AppError::AndroidNativeStreamTicketNotFound)?;

        drop(tickets);

        if !self
            .android_native_session_allows_stream_access(&record.session_id)
            .await
        {
            self.invalidate_android_native_session_artifacts(&record.session_id)
                .await;
            return Err(AppError::AndroidNativeSessionInactive);
        }

        Ok(record)
    }

    pub async fn record_android_native_session_event(
        &self,
        launch_token: Option<&str>,
        token_id: Option<&str>,
        session_id: Option<&str>,
        event_name: String,
        stage: String,
        detail: Option<String>,
        client_time_unix_ms: Option<u64>,
    ) -> Result<(AndroidNativeSessionEventRecord, u32), AppError> {
        let matched = self
            .resolve_android_native_session(launch_token, token_id, session_id)
            .await?;

        let now = unix_time_ms_now();
        let mut session_events = self.inner.android_native_session_events.write().await;
        let entries = session_events
            .entry(matched.session_id.clone())
            .or_default();
        let sequence = entries.last().map_or(1, |entry| entry.sequence + 1);
        let event = AndroidNativeSessionEventRecord {
            token_id: matched.token_id.clone(),
            session_id: matched.session_id.clone(),
            user_id: matched.user_id,
            sequence,
            event_name,
            stage,
            detail,
            client_time_unix_ms,
            recorded_at_unix_ms: now,
        };
        entries.push(event.clone());
        if entries.len() > 64 {
            let drain_count = entries.len() - 64;
            entries.drain(0..drain_count);
        }
        let event_count = entries.len() as u32;

        info!(
            "android native session event recorded: session_id={} token_id={} user_id={} sequence={} event={} stage={} client_time_unix_ms={:?} detail={:?}",
            event.session_id,
            event.token_id,
            event.user_id.0,
            event.sequence,
            event.event_name,
            event.stage,
            event.client_time_unix_ms,
            event.detail
        );

        Ok((event, event_count))
    }

    pub async fn resolve_android_native_session(
        &self,
        launch_token: Option<&str>,
        token_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<AndroidNativeResolvedSession, AppError> {
        if launch_token.is_none() && session_id.is_none() {
            return Err(AppError::BadRequest);
        }

        let now = unix_time_ms_now();
        #[derive(Clone)]
        struct EventMatchedSession {
            token_id: String,
            session_id: String,
            user_id: UserId,
        }

        let mut tokens = self.inner.android_native_launch_tokens.write().await;
        tokens.retain(|_, value| value.expires_at_unix_ms > now);

        let matched_from_launch = if let Some(launch_token) = launch_token {
            tokens.get(launch_token).cloned()
        } else {
            tokens
                .values()
                .find(|value| {
                    session_id.is_some_and(|session_id| value.session_id == session_id)
                        && token_id.is_none_or(|token_id| value.token_id == token_id)
                })
                .cloned()
        };
        drop(tokens);
        let matched_from_launch = matched_from_launch.map(|record| EventMatchedSession {
            token_id: record.token_id,
            session_id: record.session_id,
            user_id: record.user_id,
        });

        let matched_from_ticket = if matched_from_launch.is_none()
            && let Some(session_id) = session_id
        {
            let mut tickets = self.inner.android_native_stream_tickets.write().await;
            tickets.retain(|_, value| value.expires_at_unix_ms > now);
            tickets
                .values()
                .find(|value| {
                    value.session_id == session_id
                        && token_id.is_none_or(|token_id| value.token_id == token_id)
                })
                .cloned()
                .map(|record| EventMatchedSession {
                    token_id: record.token_id,
                    session_id: record.session_id,
                    user_id: record.user_id,
                })
        } else {
            None
        };

        let matched_from_lifecycle = if matched_from_launch.is_none()
            && matched_from_ticket.is_none()
            && let Some(session_id) = session_id
        {
            let lifecycle = self.inner.android_native_session_lifecycle.read().await;
            lifecycle
                .get(session_id)
                .filter(|value| token_id.is_none_or(|token_id| value.token_id == token_id))
                .cloned()
                .map(|record| EventMatchedSession {
                    token_id: record.token_id,
                    session_id: record.session_id,
                    user_id: record.user_id,
                })
        } else {
            None
        };

        let matched = matched_from_launch
            .or(matched_from_ticket)
            .or(matched_from_lifecycle)
            .ok_or(AppError::AndroidNativeLaunchTokenNotFound)?;

        Ok(AndroidNativeResolvedSession {
            token_id: matched.token_id,
            session_id: matched.session_id,
            user_id: matched.user_id,
        })
    }

    pub async fn update_android_native_session_lifecycle(
        &self,
        launch_token: Option<&str>,
        token_id: Option<&str>,
        session_id: Option<&str>,
        action: String,
        detail: Option<String>,
        client_time_unix_ms: Option<u64>,
    ) -> Result<
        (
            AndroidNativeSessionEventRecord,
            AndroidNativeSessionLifecycleStateRecord,
        ),
        AppError,
    > {
        if launch_token.is_none() && session_id.is_none() {
            return Err(AppError::BadRequest);
        }

        let now = unix_time_ms_now();
        #[derive(Clone)]
        struct LifecycleMatchedSession {
            token_id: String,
            session_id: String,
            user_id: UserId,
            issued_at_unix_ms: u64,
            trust_bootstrap_paired: bool,
        }

        let mut tokens = self.inner.android_native_launch_tokens.write().await;
        tokens.retain(|_, value| value.expires_at_unix_ms > now);

        let matched_from_launch = if let Some(launch_token) = launch_token {
            tokens.get(launch_token).cloned()
        } else {
            tokens
                .values()
                .find(|value| {
                    session_id.is_some_and(|session_id| value.session_id == session_id)
                        && token_id.is_none_or(|token_id| value.token_id == token_id)
                })
                .cloned()
        };
        drop(tokens);

        let matched_from_launch = matched_from_launch.map(|record| LifecycleMatchedSession {
            token_id: record.token_id,
            session_id: record.session_id,
            user_id: record.user_id,
            issued_at_unix_ms: record.issued_at_unix_ms,
            trust_bootstrap_paired: record.trust_bootstrap.paired,
        });

        let matched_from_ticket = if matched_from_launch.is_none()
            && let Some(session_id) = session_id
        {
            let mut tickets = self.inner.android_native_stream_tickets.write().await;
            tickets.retain(|_, value| value.expires_at_unix_ms > now);
            tickets
                .values()
                .find(|value| {
                    value.session_id == session_id
                        && token_id.is_none_or(|token_id| value.token_id == token_id)
                })
                .cloned()
                .map(|record| LifecycleMatchedSession {
                    token_id: record.token_id,
                    session_id: record.session_id,
                    user_id: record.user_id,
                    issued_at_unix_ms: record.issued_at_unix_ms,
                    trust_bootstrap_paired: true,
                })
        } else {
            None
        };

        let matched_from_lifecycle = if matched_from_launch.is_none()
            && matched_from_ticket.is_none()
            && let Some(session_id) = session_id
        {
            let lifecycle = self.inner.android_native_session_lifecycle.read().await;
            lifecycle
                .get(session_id)
                .filter(|value| token_id.is_none_or(|token_id| value.token_id == token_id))
                .cloned()
                .map(|record| LifecycleMatchedSession {
                    token_id: record.token_id,
                    session_id: record.session_id,
                    user_id: record.user_id,
                    issued_at_unix_ms: record.last_updated_unix_ms,
                    trust_bootstrap_paired: record.trust_bootstrap_status != "bootstrap_required",
                })
        } else {
            None
        };

        let matched = matched_from_launch
            .or(matched_from_ticket)
            .or(matched_from_lifecycle)
            .ok_or(AppError::AndroidNativeLaunchTokenNotFound)?;

        let mut lifecycle = self.inner.android_native_session_lifecycle.write().await;
        let state = lifecycle
            .entry(matched.session_id.clone())
            .or_insert_with(|| AndroidNativeSessionLifecycleStateRecord {
                token_id: matched.token_id.clone(),
                session_id: matched.session_id.clone(),
                user_id: matched.user_id,
                trust_bootstrap_status: if matched.trust_bootstrap_paired {
                    "paired_material_reused".to_string()
                } else {
                    "bootstrap_required".to_string()
                },
                session_status: "issued".to_string(),
                hidden_state_status: "not_staged".to_string(),
                last_action: "token_issued".to_string(),
                last_reason: None,
                last_updated_unix_ms: matched.issued_at_unix_ms,
                completed_at_unix_ms: None,
            });

        match action.as_str() {
            "trust_bootstrap_staged" => {
                state.trust_bootstrap_status = if matched.trust_bootstrap_paired {
                    "staged_reused_pair".to_string()
                } else {
                    "staged_pending_pair".to_string()
                };
                state.hidden_state_status = "staged".to_string();
            }
            "trust_bootstrap_ready" => {
                state.trust_bootstrap_status = "ready".to_string();
                state.hidden_state_status = "staged".to_string();
            }
            "session_started" => {
                state.session_status = "started".to_string();
                state.hidden_state_status = "active".to_string();
            }
            "session_revoke_requested" => {
                state.hidden_state_status = "revoking".to_string();
            }
            "session_revoked" => {
                state.session_status = "ended".to_string();
                state.hidden_state_status = "cleared".to_string();
                state.completed_at_unix_ms = Some(now);
            }
            "session_expired_cleanup" => {
                state.session_status = "expired".to_string();
                state.hidden_state_status = "cleared".to_string();
                state.completed_at_unix_ms = Some(now);
            }
            "session_failed_cleanup" => {
                state.session_status = "failed".to_string();
                state.hidden_state_status = "cleared".to_string();
                state.completed_at_unix_ms = Some(now);
            }
            "session_abandoned_cleanup" => {
                state.session_status = "abandoned".to_string();
                state.hidden_state_status = "cleared".to_string();
                state.completed_at_unix_ms = Some(now);
            }
            _ => {}
        }

        state.last_action = action.clone();
        state.last_reason = detail.clone();
        state.last_updated_unix_ms = now;
        let lifecycle_snapshot = state.clone();
        drop(lifecycle);
        let should_invalidate_session_artifacts = matches!(
            action.as_str(),
            "session_revoked"
                | "session_expired_cleanup"
                | "session_failed_cleanup"
                | "session_abandoned_cleanup"
        );

        let event = {
            let mut session_events = self.inner.android_native_session_events.write().await;
            let entries = session_events
                .entry(matched.session_id.clone())
                .or_default();
            let sequence = entries.last().map_or(1, |entry| entry.sequence + 1);
            let event = AndroidNativeSessionEventRecord {
                token_id: matched.token_id.clone(),
                session_id: matched.session_id.clone(),
                user_id: matched.user_id,
                sequence,
                event_name: format!("lifecycle:{action}"),
                stage: "session_lifecycle".to_string(),
                detail,
                client_time_unix_ms,
                recorded_at_unix_ms: now,
            };
            entries.push(event.clone());
            if entries.len() > 64 {
                let drain_count = entries.len() - 64;
                entries.drain(0..drain_count);
            }
            event
        };

        info!(
            "android native session lifecycle updated: session_id={} token_id={} user_id={} action={} trust={} session={} hidden={} detail={:?}",
            lifecycle_snapshot.session_id,
            lifecycle_snapshot.token_id,
            lifecycle_snapshot.user_id.0,
            lifecycle_snapshot.last_action,
            lifecycle_snapshot.trust_bootstrap_status,
            lifecycle_snapshot.session_status,
            lifecycle_snapshot.hidden_state_status,
            lifecycle_snapshot.last_reason,
        );
        if should_invalidate_session_artifacts {
            self.invalidate_android_native_session_artifacts(&matched.session_id)
                .await;
        }

        Ok((event, lifecycle_snapshot))
    }

    async fn find_active_android_native_owner_session(
        &self,
        user_id: UserId,
        host_id: HostId,
        app_id: AppId,
    ) -> Option<AndroidNativeActiveOwnerSessionRecord> {
        let now = unix_time_ms_now();
        let lifecycle = self.inner.android_native_session_lifecycle.read().await;
        let tickets = self.inner.android_native_stream_tickets.read().await;

        lifecycle
            .values()
            .filter(|state| {
                state.user_id == user_id
                    && state.session_status == "started"
                    && state.completed_at_unix_ms.is_none()
            })
            .filter_map(|state| {
                tickets
                    .values()
                    .find(|ticket| {
                        ticket.expires_at_unix_ms > now
                            && ticket.user_id == user_id
                            && ticket.host_id == host_id
                            && ticket.app_id == app_id
                            && ticket.session_id == state.session_id
                    })
                    .map(|_ticket| AndroidNativeActiveOwnerSessionRecord {
                        token_id: state.token_id.clone(),
                        session_id: state.session_id.clone(),
                        user_id,
                        host_id,
                        app_id,
                        last_updated_unix_ms: state.last_updated_unix_ms,
                    })
            })
            .max_by_key(|session| session.last_updated_unix_ms)
    }

    async fn find_latest_active_android_native_owner_session(
        &self,
        host_id: Option<HostId>,
        app_id: Option<AppId>,
    ) -> Option<AndroidNativeActiveOwnerSessionRecord> {
        let now = unix_time_ms_now();
        let lifecycle = self.inner.android_native_session_lifecycle.read().await;
        let tickets = self.inner.android_native_stream_tickets.read().await;

        lifecycle
            .values()
            .filter(|state| {
                state.session_status == "started" && state.completed_at_unix_ms.is_none()
            })
            .filter_map(|state| {
                tickets
                    .values()
                    .find(|ticket| {
                        ticket.expires_at_unix_ms > now
                            && ticket.session_id == state.session_id
                            && host_id.is_none_or(|expected| ticket.host_id == expected)
                            && app_id.is_none_or(|expected| ticket.app_id == expected)
                    })
                    .map(|ticket| AndroidNativeActiveOwnerSessionRecord {
                        token_id: state.token_id.clone(),
                        session_id: state.session_id.clone(),
                        user_id: state.user_id,
                        host_id: ticket.host_id,
                        app_id: ticket.app_id,
                        last_updated_unix_ms: state.last_updated_unix_ms,
                    })
            })
            .max_by_key(|session| session.last_updated_unix_ms)
    }

    pub async fn android_native_owner_session_is_active(&self, session_id: &str) -> bool {
        if session_id.trim().is_empty() {
            return false;
        }

        let lifecycle = self.inner.android_native_session_lifecycle.read().await;
        lifecycle.get(session_id).is_some_and(|state| {
            state.session_status == "started" && state.completed_at_unix_ms.is_none()
        })
    }

    pub async fn issue_android_native_shared_session_invite(
        &self,
        user_id: UserId,
        host_id: HostId,
        app_id: AppId,
        role: common::api_bindings::AndroidNativeSharedSessionRole,
    ) -> Result<AndroidNativeSharedSessionInviteRecord, AppError> {
        if matches!(
            role,
            common::api_bindings::AndroidNativeSharedSessionRole::Owner
        ) {
            return Err(AppError::BadRequest);
        }

        let owner_session = self
            .find_active_android_native_owner_session(user_id, host_id, app_id)
            .await
            .ok_or(AppError::AndroidNativeSharedSessionActiveOwnerNotFound)?;

        const INVITE_TTL_MS: u64 = 15 * 60 * 1000;

        let now = unix_time_ms_now();
        let invite_token = format!("mlnatshare_{}", Uuid::new_v4().to_simple());
        let record = AndroidNativeSharedSessionInviteRecord {
            invite_token: invite_token.clone(),
            shared_session_id: format!("shared-{}", owner_session.session_id),
            owner_token_id: owner_session.token_id,
            owner_session_id: owner_session.session_id,
            user_id: owner_session.user_id,
            host_id: owner_session.host_id,
            app_id: owner_session.app_id,
            role,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + INVITE_TTL_MS,
            consumed_at_unix_ms: None,
            capabilities: shared_session_capabilities_for_role(role),
        };

        let mut invites = self
            .inner
            .android_native_shared_session_invites
            .write()
            .await;
        invites.retain(|_, value| value.expires_at_unix_ms > now);
        invites.insert(invite_token, record.clone());

        Ok(record)
    }

    pub async fn issue_android_native_shared_session_invite_for_active_owner(
        &self,
        host_id: Option<HostId>,
        app_id: Option<AppId>,
        role: common::api_bindings::AndroidNativeSharedSessionRole,
    ) -> Result<AndroidNativeSharedSessionInviteRecord, AppError> {
        if matches!(
            role,
            common::api_bindings::AndroidNativeSharedSessionRole::Owner
        ) {
            return Err(AppError::BadRequest);
        }

        let owner_session = self
            .find_latest_active_android_native_owner_session(host_id, app_id)
            .await
            .ok_or(AppError::AndroidNativeSharedSessionActiveOwnerNotFound)?;

        const INVITE_TTL_MS: u64 = 15 * 60 * 1000;

        let now = unix_time_ms_now();
        let invite_token = format!("mlnatshare_{}", Uuid::new_v4().to_simple());
        let record = AndroidNativeSharedSessionInviteRecord {
            invite_token: invite_token.clone(),
            shared_session_id: format!("shared-{}", owner_session.session_id),
            owner_token_id: owner_session.token_id,
            owner_session_id: owner_session.session_id,
            user_id: owner_session.user_id,
            host_id: owner_session.host_id,
            app_id: owner_session.app_id,
            role,
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + INVITE_TTL_MS,
            consumed_at_unix_ms: None,
            capabilities: shared_session_capabilities_for_role(role),
        };

        let mut invites = self
            .inner
            .android_native_shared_session_invites
            .write()
            .await;
        invites.retain(|_, value| value.expires_at_unix_ms > now);
        invites.insert(invite_token, record.clone());

        Ok(record)
    }

    pub async fn consume_android_native_shared_session_invite(
        &self,
        invite_token: &str,
    ) -> Result<AndroidNativeSharedSessionInviteRecord, AppError> {
        let now = unix_time_ms_now();
        let mut invites = self
            .inner
            .android_native_shared_session_invites
            .write()
            .await;
        invites.retain(|_, value| value.expires_at_unix_ms > now);

        let invite = invites
            .get(invite_token)
            .cloned()
            .ok_or(AppError::AndroidNativeSharedSessionInviteNotFound)?;

        if invite.expires_at_unix_ms <= now {
            invites.remove(invite_token);
            return Err(AppError::AndroidNativeSharedSessionInviteExpired);
        }

        Ok(invite)
    }
}

fn unix_time_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}

fn default_android_native_feature_profile() -> common::api_bindings::AndroidNativeFeatureProfile {
    common::api_bindings::AndroidNativeFeatureProfile {
        keyboard: true,
        mouse_relative: true,
        mouse_drag_drop: true,
        stats_overlay: true,
        gamepad: common::api_bindings::AndroidNativeGamepadFeatureProfile {
            supported: true,
            max_controllers: 4,
            custom_mapping: true,
            invert_ab: true,
            invert_xy: true,
            send_interval_override: true,
            rumble: true,
            touchpad_button: true,
        },
        microphone: common::api_bindings::AndroidNativeMicrophoneFeatureProfile {
            supported: true,
            selectable_devices: true,
            level_meter: true,
            diagnostics: true,
        },
        quality: common::api_bindings::AndroidNativeQualityFeatureProfile {
            adaptive_bitrate: true,
            adaptive_fps: true,
            fps_presets: vec![24, 30, 45, 60, 90, 120],
            default_fps: 60,
            bitrate_floor_kbps: 2_500,
            bitrate_ceiling_kbps: 30_000,
            bitrate_step_kbps: 500,
            default_bitrate_kbps: 4_000,
            latency_profiles: vec![
                "normal".to_string(),
                "low_latency".to_string(),
                "ultra_low".to_string(),
            ],
            default_latency_profile: "ultra_low".to_string(),
            canvas_vsync: false,
        },
    }
}

fn shared_session_capabilities_for_role(
    role: common::api_bindings::AndroidNativeSharedSessionRole,
) -> common::api_bindings::AndroidNativeSharedSessionCapabilities {
    use common::api_bindings::AndroidNativeSharedSessionRole as Role;

    match role {
        Role::Owner => common::api_bindings::AndroidNativeSharedSessionCapabilities {
            display_authority: true,
            allow_primary_input: true,
            allow_gamepad_slot: Some(0),
            can_end_session: true,
        },
        Role::Viewer => common::api_bindings::AndroidNativeSharedSessionCapabilities {
            display_authority: false,
            allow_primary_input: false,
            allow_gamepad_slot: None,
            can_end_session: false,
        },
        Role::Helper | Role::AdminAssist => {
            common::api_bindings::AndroidNativeSharedSessionCapabilities {
                display_authority: false,
                allow_primary_input: false,
                allow_gamepad_slot: None,
                can_end_session: false,
            }
        }
        Role::Player2 => common::api_bindings::AndroidNativeSharedSessionCapabilities {
            display_authority: false,
            allow_primary_input: false,
            allow_gamepad_slot: Some(1),
            can_end_session: false,
        },
    }
}

pub(crate) fn shared_session_attach_available_now(
    role: common::api_bindings::AndroidNativeSharedSessionRole,
    owner_session_active: bool,
) -> bool {
    if !owner_session_active {
        return false;
    }

    match role {
        common::api_bindings::AndroidNativeSharedSessionRole::Owner => true,
        common::api_bindings::AndroidNativeSharedSessionRole::Viewer
        | common::api_bindings::AndroidNativeSharedSessionRole::Helper
        | common::api_bindings::AndroidNativeSharedSessionRole::AdminAssist
        | common::api_bindings::AndroidNativeSharedSessionRole::Player2 => false,
    }
}

pub(crate) fn shared_session_status_message(
    role: common::api_bindings::AndroidNativeSharedSessionRole,
    owner_session_active: bool,
) -> String {
    use common::api_bindings::AndroidNativeSharedSessionRole as Role;

    if !owner_session_active {
        return "The owner session is no longer active, so this shared invite cannot attach right now."
            .to_string();
    }

    match role {
        Role::Owner => "The owner session is active.".to_string(),
        Role::Viewer => "Viewer share is issued and protected from display changes, but viewer attach is not live in this host build yet.".to_string(),
        Role::Helper => "Helper share is issued with display authority locked to the owner, but helper attach is not live in this host build yet.".to_string(),
        Role::AdminAssist => "Admin assist share is issued with owner-safe display authority, but assist attach is not live in this host build yet.".to_string(),
        Role::Player2 => "Player 2 share is issued with display authority locked to the owner, but the gamepad-only join lane is not live in this host build yet.".to_string(),
    }
}

pub(crate) fn resolve_shared_pair_store_path(config: &Config) -> Option<PathBuf> {
    let StorageConfig::Json { path, .. } = &config.data_storage;
    let data_path = PathBuf::from(path);
    let absolute_data_path = if data_path.is_absolute() {
        data_path
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join(&data_path)))
            .unwrap_or_else(|| PathBuf::from(path))
    };

    absolute_data_path
        .parent()
        .map(|directory| directory.join("shared_pair_info.json"))
}

async fn repair_shared_pair_store(
    config: &Config,
    storage: &Arc<dyn Storage + Send + Sync>,
) -> Result<(), anyhow::Error> {
    let Some(user_id) = config.web_server.default_user_id.map(UserId) else {
        return Ok(());
    };
    let Some(shared_path) = resolve_shared_pair_store_path(config) else {
        return Ok(());
    };

    let mut store = if tokio::fs::try_exists(&shared_path).await.unwrap_or(false) {
        let raw = tokio::fs::read_to_string(&shared_path).await?;
        serde_json::from_str::<SharedPairStore>(&raw).unwrap_or_default()
    } else {
        SharedPairStore::default()
    };
    if store.version == 0 {
        store.version = 1;
    }

    let listed_hosts = storage
        .list_user_hosts(StorageQueryHosts { user_id })
        .await?;
    let mut store_changed = false;
    let mut storage_repaired = 0usize;
    let expected_local_http_port = config.moonlight.default_http_port;

    for (host_id, maybe_host) in listed_hosts {
        let mut host = match maybe_host {
            Some(host) => host,
            None => storage.get_host(host_id).await?,
        };
        let local_loopback = matches!(host.address.as_str(), "127.0.0.1" | "localhost" | "::1");
        if local_loopback && host.http_port != expected_local_http_port {
            storage
                .modify_host(
                    host_id,
                    StorageHostModify {
                        http_port: Some(expected_local_http_port),
                        ..Default::default()
                    },
                )
                .await?;
            host.http_port = expected_local_http_port;
            storage_repaired += 1;
        }
        let existing_index = store
            .hosts
            .iter()
            .position(|entry| entry.host_id == host_id.0);
        match (host.pair_info.clone(), existing_index) {
            (Some(pair_info), Some(index)) => {
                let entry = &mut store.hosts[index];
                if entry.address != host.address
                    || entry.http_port != host.http_port
                    || entry.pair_info != pair_info
                {
                    entry.address = host.address;
                    entry.http_port = host.http_port;
                    entry.pair_info = pair_info;
                    store_changed = true;
                }
            }
            (Some(pair_info), None) => {
                store.hosts.push(SharedPairStoreHost {
                    host_id: host_id.0,
                    address: host.address,
                    http_port: host.http_port,
                    pair_info,
                });
                store_changed = true;
            }
            (None, Some(index)) => {
                storage
                    .modify_host(
                        host_id,
                        StorageHostModify {
                            pair_info: Some(Some(store.hosts[index].pair_info.clone())),
                            ..Default::default()
                        },
                    )
                    .await?;
                storage_repaired += 1;
            }
            (None, None) => {}
        }
    }

    if store_changed
        || storage_repaired > 0
        || !tokio::fs::try_exists(&shared_path).await.unwrap_or(false)
    {
        if let Some(parent) = shared_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(&store)?;
        tokio::fs::write(&shared_path, format!("{json}\n")).await?;
    }

    if storage_repaired > 0 {
        info!(
            "repaired {} host pair_info entries from shared pair store {}",
            storage_repaired,
            shared_path.display()
        );
    } else if store_changed {
        info!("updated shared pair store {}", shared_path.display());
    }

    Ok(())
}
