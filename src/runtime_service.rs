//! Bounded, auditable SCM transitions. Never terminate a service PID by image name.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Start,
    Stop,
}

pub(crate) fn service_name(bundle_root: &Path) -> Result<String, String> {
    let suffix = bundle_root
        .file_name()
        .and_then(|x| x.to_str())
        .filter(|x| {
            !x.is_empty()
                && x.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
        .ok_or("invalid bundle directory for runtime service name")?;
    Ok(format!("CloudgimeRuntime-{suffix}"))
}

#[cfg(windows)]
fn missing_service(error: &windows_service::Error) -> bool {
    matches!(error, windows_service::Error::Winapi(e) if e.raw_os_error() == Some(1060))
}

/// False means the service is not installed, not that access was denied.
/// An operator-requested transition is recorded before the control is sent.
#[cfg(windows)]
pub(crate) fn transition(
    bundle_root: &Path,
    action: Action,
    limit: std::time::Duration,
    mut log: impl FnMut(&str),
) -> Result<bool, String> {
    use std::{thread, time::Instant};
    use windows_service::{
        service::{ServiceAccess, ServiceState},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };
    let name = service_name(bundle_root)?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("SCM connect: {e}"))?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::QUERY_CONFIG
        | match action {
            Action::Start => ServiceAccess::START,
            Action::Stop => ServiceAccess::STOP,
        };
    let service = match manager.open_service(&name, access) {
        Ok(s) => s,
        Err(e) if missing_service(&e) => {
            log(&format!("RUNTIME_SERVICE_ABSENT service={name}"));
            return Ok(false);
        }
        Err(e) => return Err(format!("SCM open {name}: {e}")),
    };
    let config = service
        .query_config()
        .map_err(|e| format!("SCM config {name}: {e}"))?;
    // Canonical paths ensure a service name collision cannot control another install.
    let root = std::fs::canonicalize(bundle_root).map_err(|e| format!("bundle path: {e}"))?;
    let image = std::fs::canonicalize(&config.executable_path)
        .map_err(|e| format!("service image: {e}"))?;
    if !image.starts_with(&root)
        || !image
            .file_name()
            .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("sunshinesvc.exe"))
    {
        return Err(format!("SCM binding mismatch for {name}"));
    }
    let target = match action {
        Action::Start => ServiceState::Running,
        Action::Stop => ServiceState::Stopped,
    };
    let started = Instant::now();
    let mut sent = false;
    loop {
        let status = service
            .query_status()
            .map_err(|e| format!("SCM status {name}: {e}"))?;
        if !sent {
            log(&format!(
                "RUNTIME_SERVICE_OBSERVED service={name} requested={action:?} state={:?} pid={:?} scm_exit={:?}",
                status.current_state, status.process_id, status.exit_code
            ));
        }
        if status.current_state == target {
            log(&format!(
                "RUNTIME_SERVICE_COMPLETE service={name} action={action:?} planned={sent} state={:?} pid={:?} scm_exit={:?} elapsed_ms={}",
                status.current_state,
                status.process_id,
                status.exit_code,
                started.elapsed().as_millis()
            ));
            return Ok(true);
        }
        if started.elapsed() >= limit {
            log(&format!(
                "RUNTIME_SERVICE_TIMEOUT service={name} action={action:?} state={:?} pid={:?} scm_exit={:?}",
                status.current_state, status.process_id, status.exit_code
            ));
            return Err(format!(
                "SCM timeout {name}: target={target:?} current={:?} exit={:?}",
                status.current_state, status.exit_code
            ));
        }
        let pending = matches!(
            status.current_state,
            ServiceState::StartPending | ServiceState::StopPending
        );
        if !sent && !pending {
            log(&format!(
                "RUNTIME_SERVICE_REQUEST service={name} action={action:?} planned=true pid={:?}",
                status.process_id
            ));
            let result = match action {
                Action::Start => service.start::<&str>(&[]),
                Action::Stop => service.stop().map(|_| ()),
            };
            if let Err(e) = result {
                // Concurrent control can already have reached the requested state.
                if !service
                    .query_status()
                    .is_ok_and(|s| s.current_state == target)
                {
                    log(&format!(
                        "RUNTIME_SERVICE_CONTROL_FAILED service={name} action={action:?} error={e}"
                    ));
                    return Err(format!("SCM control {name}: {e}"));
                }
            }
            sent = true;
        }
        thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derives_only_current_bundle_service() {
        assert_eq!(
            service_name(Path::new("C:/ProgramData/CloudGime/Host")).unwrap(),
            "CloudgimeRuntime-Host"
        );
        assert_eq!(
            service_name(Path::new("C:/ProgramData/CloudGime/Node-02")).unwrap(),
            "CloudgimeRuntime-Node-02"
        );
        assert!(service_name(Path::new("/")).is_err());
        assert!(service_name(Path::new("invalid;service")).is_err());
        assert!(service_name(Path::new("bad name")).is_err());
    }
}
