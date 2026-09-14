use anyhow::{Result, bail};
use serde_json::Value;

/// Only reuse canonical identifiers after proving this is the same installation.
/// Explicit recovery mode only. The activation license still proves server-side
/// authorization; this only supplies the canonical local identity to that check.
pub fn explicit_recovery_machine_state(state_json: &Value) -> Result<(String, String)> {
    let text = |name: &str| {
        state_json
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let machine = text("MachineIdentity")
        .or_else(|| text("machineIdentity"))
        .ok_or_else(|| anyhow::anyhow!("existing activation machine identity missing"))?;
    let install = text("InstallInstanceId")
        .or_else(|| text("installInstanceId"))
        .ok_or_else(|| anyhow::anyhow!("existing activation installation id missing"))?;
    Ok((machine.to_string(), install.to_string()))
}

pub fn resolve_retry_identity(
    state_json: &Value,
    machine_identity: &str,
    install_instance_id: &str,
    default_host_id: String,
    default_device_id: String,
) -> Result<(String, String)> {
    let text = |name: &str| {
        state_json
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let state_machine = text("MachineIdentity").or_else(|| text("machineIdentity"));
    let state_install = text("InstallInstanceId").or_else(|| text("installInstanceId"));
    if state_machine.is_none() && state_install.is_none() {
        return Ok((default_host_id, default_device_id));
    }
    if state_machine != Some(machine_identity) || state_install != Some(install_instance_id) {
        bail!("existing activation identity does not match this installation");
    }
    let host = text("HostId")
        .or_else(|| text("hostId"))
        .ok_or_else(|| anyhow::anyhow!("existing activation host id missing"))?;
    let device = text("SentinelDeviceId")
        .or_else(|| text("deviceId"))
        .ok_or_else(|| anyhow::anyhow!("existing activation device id missing"))?;
    Ok((host.to_string(), device.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn matching_installation_reuses_canonical_ids() {
        let s = json!({"MachineIdentity":"m","InstallInstanceId":"i","HostId":"host-old","SentinelDeviceId":"PC-01"});
        assert_eq!(
            resolve_retry_identity(&s, "m", "i", "host-new".into(), "new".into()).unwrap(),
            ("host-old".into(), "PC-01".into())
        );
    }
    #[test]
    fn clone_state_is_rejected() {
        let s = json!({"MachineIdentity":"other","InstallInstanceId":"i","HostId":"h","SentinelDeviceId":"d"});
        assert!(resolve_retry_identity(&s, "m", "i", "host-new".into(), "new".into()).is_err());
    }
    #[test]
    fn explicit_recovery_requires_complete_state() {
        let s = json!({"MachineIdentity":"m","InstallInstanceId":"i"});
        assert_eq!(
            explicit_recovery_machine_state(&s).unwrap(),
            ("m".into(), "i".into())
        );
        assert!(explicit_recovery_machine_state(&json!({"MachineIdentity":"m"})).is_err());
    }
    #[test]
    fn no_state_uses_current_identity() {
        assert_eq!(
            resolve_retry_identity(&json!({}), "m", "i", "h".into(), "d".into()).unwrap(),
            ("h".into(), "d".into())
        );
    }
}
