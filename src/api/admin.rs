use actix_web::{
    HttpResponse, delete, get, patch, post,
    web::{Data, Json},
};
use common::api_bindings::{
    DeleteUserRequest, DetailedUser, GetUsersResponse, HostCapabilityProfile, HostOperationsStatus,
    PatchUserRequest, PostUserRequest, RefreshHostCapabilityResponse,
};
use futures::future::join_all;
use log::warn;
use std::{
    env,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{fs, process::Command};

use crate::app::{
    App, AppError,
    password::StoragePassword,
    storage::{StorageUserAdd, StorageUserModify},
    user::{Admin, AuthenticatedUser, Role, UserId},
};

#[post("/user")]
pub async fn add_user(
    app: Data<App>,
    admin: Admin,
    Json(request): Json<PostUserRequest>,
) -> Result<Json<DetailedUser>, AppError> {
    let mut user = app
        .add_user(
            &admin,
            StorageUserAdd {
                name: request.name.clone(),
                password: Some(StoragePassword::new(&request.password)?),
                role: request.role.into(),
                client_unique_id: request.client_unique_id,
            },
        )
        .await?;

    let detailed_user = user.detailed_user().await?;

    Ok(Json(detailed_user))
}

#[patch("/user")]
pub async fn patch_user(
    app: Data<App>,
    user: AuthenticatedUser,
    Json(request): Json<PatchUserRequest>,
) -> Result<HttpResponse, AppError> {
    let target_user_id = UserId(request.id);

    match Admin::try_from(user).await? {
        Ok(admin) => {
            let mut target_user = app.user_by_id(target_user_id).await?;

            let new_password = if let Some(new_password) = request.password {
                Some(StoragePassword::new(&new_password)?)
            } else {
                None
            };

            target_user
                .modify(
                    &admin,
                    StorageUserModify {
                        password: Some(new_password),
                        role: request.role.map(Role::from),
                        client_unique_id: request.client_unique_id,
                    },
                )
                .await?;
        }
        Err(mut user) => {
            if user.id() != target_user_id {
                return Err(AppError::Forbidden);
            }

            // Only allow changing the password
            let PatchUserRequest {
                id: _,
                password: _,
                role,
                client_unique_id,
            } = &request;
            if role.is_some() || client_unique_id.is_some() {
                return Err(AppError::Forbidden);
            }

            if let Some(new_password) = request.password {
                user.set_password(StoragePassword::new(&new_password)?)
                    .await?;
            }
        }
    }

    Ok(HttpResponse::Ok().finish())
}

#[delete("/user")]
pub async fn delete_user(
    app: Data<App>,
    admin: Admin,
    Json(request): Json<DeleteUserRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = UserId(request.id);

    let user = app.user_by_id(user_id).await?;

    user.delete(&admin).await?;

    Ok(HttpResponse::Ok().finish())
}

#[get("/users")]
pub async fn list_users(app: Data<App>, admin: Admin) -> Result<Json<GetUsersResponse>, AppError> {
    let mut users = app.all_users(admin).await?;

    let user_results = join_all(users.iter_mut().map(|user| user.detailed_user_no_auth())).await;

    let mut out_users = Vec::with_capacity(user_results.len());
    for (result, user) in user_results.into_iter().zip(users) {
        match result {
            Ok(user) => {
                out_users.push(user);
            }
            Err(err) => {
                warn!("Failed to query detailed user of {user:?}: {err}");
            }
        }
    }

    Ok(Json(GetUsersResponse { users: out_users }))
}

fn resolve_host_capability_profile_path() -> Result<PathBuf, AppError> {
    let current_exe = env::current_exe()?;
    let runtime_dir = current_exe.parent().ok_or(AppError::AppDestroyed)?;

    Ok(runtime_dir
        .join("server")
        .join("host_capability_profile.json"))
}

fn resolve_runtime_dir() -> Result<PathBuf, AppError> {
    let current_exe = env::current_exe()?;
    current_exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or(AppError::AppDestroyed)
}

fn resolve_bundle_root() -> Result<PathBuf, AppError> {
    resolve_runtime_dir()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or(AppError::AppDestroyed)
}

async fn read_host_capability_profile() -> Result<Option<HostCapabilityProfile>, AppError> {
    let path = resolve_host_capability_profile_path()?;
    let raw = match fs::read_to_string(&path).await {
        Ok(value) => value,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let profile = match serde_json::from_str::<HostCapabilityProfile>(&raw) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                "Failed to parse host capability profile {}: {err}",
                path.display()
            );
            return Ok(None);
        }
    };

    Ok(Some(profile))
}

#[get("/admin/host-capability")]
pub async fn get_host_capability_profile(_admin: Admin) -> Result<HttpResponse, AppError> {
    let Some(profile) = read_host_capability_profile().await? else {
        return Ok(HttpResponse::NotFound().finish());
    };

    Ok(HttpResponse::Ok().json(profile))
}

async fn read_host_operations_status() -> Result<HostOperationsStatus, AppError> {
    let bundle_root = resolve_bundle_root()?;
    let installer_path = bundle_root.join("host-installer.exe");
    let output = Command::new(&installer_path)
        .arg("--bundle-root")
        .arg(&bundle_root)
        .arg("status")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "Host installer status failed for {}: {}",
            bundle_root.display(),
            stderr.trim()
        );
        return Err(AppError::Io(std::io::Error::other(
            "host installer status returned non-zero status",
        )));
    }

    serde_json::from_slice::<HostOperationsStatus>(&output.stdout).map_err(|err| {
        warn!(
            "Failed to parse host operations status {}: {err}",
            installer_path.display()
        );
        AppError::Io(std::io::Error::other(
            "failed to parse host operations status",
        ))
    })
}

#[get("/admin/host-ops-status")]
pub async fn get_host_operations_status(_admin: Admin) -> Result<HttpResponse, AppError> {
    let status = read_host_operations_status().await?;
    Ok(HttpResponse::Ok().json(status))
}

#[post("/admin/host-capability/refresh")]
pub async fn refresh_host_capability_profile(_admin: Admin) -> Result<HttpResponse, AppError> {
    let runtime_dir = resolve_runtime_dir()?;
    let helper_path = runtime_dir
        .join("server")
        .join("display-prepare-helper.exe");
    let bundle_root = runtime_dir
        .parent()
        .ok_or(AppError::AppDestroyed)?
        .to_path_buf();

    let output = Command::new(&helper_path)
        .arg("preflight")
        .arg("--refresh")
        .arg("--bundle-root")
        .arg(&bundle_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "Host capability preflight failed for {}: {}",
            bundle_root.display(),
            stderr.trim()
        );
        return Ok(HttpResponse::InternalServerError().finish());
    }

    let Some(profile) = read_host_capability_profile().await? else {
        return Ok(HttpResponse::InternalServerError().finish());
    };

    Ok(HttpResponse::Ok().json(RefreshHostCapabilityResponse { profile }))
}
