//! Non-secret Universal Node binding. Runtime recovery never issues an identity
//! or consumes a setup/license token. Keep this parser free of OS/network writes.
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::HashSet;

pub const CONTROL_PLANE: &str = "https://cloudgime.my.id";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniversalHostBinding {
    pub host_id: String,
    pub pc_id: String,
    pub device_id: String,
    pub keeper_entry_id: String,
    pub machine_identity: String,
    pub install_instance_id: String,
    pub application_activation_id: String,
    pub token_kind: String,
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    let Some(s) = v.get(key).and_then(Value::as_str) else {
        bail!("NODE_BINDING_FIELD_MISSING")
    };
    if s.is_empty() || s.len() > 256 || s.trim() != s || s.chars().any(char::is_control) {
        bail!("NODE_BINDING_FIELD_INVALID");
    }
    Ok(s)
}

pub fn parse_manifest(manifest: &Value, identity: &Value) -> Result<UniversalHostBinding> {
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || manifest.get("status").and_then(Value::as_str) != Some("PROVISIONED_WAITING_HEARTBEATS")
    {
        bail!("NODE_MANIFEST_INVALID");
    }
    // Never send a machine credential to a URL taken solely from mutable state.
    if text(manifest, "controlPlaneUrl")?.trim_end_matches('/') != CONTROL_PLANE {
        bail!("NODE_BACKEND_NOT_TRUSTED");
    }
    let machine = text(manifest, "machineIdentity")?;
    let instance = text(manifest, "installInstanceId")?;
    if machine != text(identity, "machineIdentity")?
        || instance != text(identity, "installInstanceId")?
    {
        bail!("NODE_INSTALLATION_MISMATCH");
    }
    let kind = text(manifest, "tokenKind")?;
    let expected: &[&str] = match kind {
        "always_on_host" => &["host_app", "sentinel"],
        "instance_pair" => &["host_app", "sentinel", "keeper"],
        _ => bail!("NODE_PROFILE_INVALID"),
    };
    let Some(components) = manifest.get("components").and_then(Value::as_array) else {
        bail!("NODE_COMPONENTS_MISSING")
    };
    let mut seen = HashSet::new();
    for c in components {
        let name = text(c, "componentType")?;
        if !expected.contains(&name) || !seen.insert(name) {
            bail!("NODE_COMPONENTS_AMBIGUOUS");
        }
        text(c, "applicationActivationId")?;
        if text(c, "credentialFile")? != format!("{name}.credential.dpapi") {
            bail!("NODE_CREDENTIAL_FILE_INVALID");
        }
    }
    let Some(required) = manifest.get("requiredComponents").and_then(Value::as_array) else {
        bail!("NODE_REQUIREMENTS_MISSING")
    };
    let mut requirements = HashSet::new();
    for item in required {
        let Some(name) = item.as_str() else {
            bail!("NODE_REQUIREMENTS_INVALID")
        };
        if !seen.contains(name) || !requirements.insert(name) {
            bail!("NODE_REQUIREMENTS_INVALID")
        }
    }
    if seen.len() != expected.len() || requirements != seen {
        bail!("NODE_COMPONENTS_INCOMPLETE")
    }
    let host = components
        .iter()
        .find(|c| c["componentType"] == "host_app")
        .unwrap();
    let binding = &manifest["binding"];
    Ok(UniversalHostBinding {
        host_id: text(binding, "hostId")?.into(),
        pc_id: text(binding, "pcId")?.into(),
        device_id: text(binding, "deviceId")?.into(),
        keeper_entry_id: text(binding, "keeperEntryId")?.into(),
        machine_identity: machine.into(),
        install_instance_id: instance.into(),
        application_activation_id: text(host, "applicationActivationId")?.into(),
        token_kind: kind.into(),
    })
}

impl UniversalHostBinding {
    pub fn validate_cached_binding(&self, state: &Value) -> Result<()> {
        for (key, expected) in [
            ("HostId", self.host_id.as_str()),
            ("MachineIdentity", self.machine_identity.as_str()),
            ("InstallInstanceId", self.install_instance_id.as_str()),
            ("SentinelPcId", self.pc_id.as_str()),
            ("SentinelDeviceId", self.device_id.as_str()),
            ("KeeperEntryId", self.keeper_entry_id.as_str()),
            (
                "ApplicationActivationId",
                self.application_activation_id.as_str(),
            ),
            ("ApplicationType", "CLOUDGIME_HOST"),
        ] {
            if let Some(value) = state.get(key) {
                match value {
                    Value::String(s) if s.is_empty() || s == expected => {}
                    Value::Null => {}
                    _ => bail!("NODE_CACHED_BINDING_CONFLICT"),
                }
            }
        }
        if let Some(value) = state.get("ControlPlaneUrl") {
            match value {
                Value::String(s) if s.is_empty() || s.trim_end_matches('/') == CONTROL_PLANE => {}
                Value::Null => {}
                _ => bail!("NODE_CACHED_BACKEND_CONFLICT"),
            }
        }
        Ok(())
    }

    /// Status endpoint authenticates the scoped runtimeToken and installation;
    /// response must still exactly name this host/PC/device/keeper before use.
    pub fn verified_status(&self, value: &Value) -> Result<String> {
        if value.get("ok").and_then(Value::as_bool) != Some(true)
            || text(value, "hostId")? != self.host_id
            || text(value, "sentinelPcId")? != self.pc_id
            || text(value, "sentinelDeviceId")? != self.device_id
            || text(value, "keeperEntryId")? != self.keeper_entry_id
            || text(value, "activationRecordId")? != self.host_id
            || text(value, "controlPlaneUrl")?.trim_end_matches('/') != CONTROL_PLANE
        {
            bail!("NODE_STATUS_BINDING_MISMATCH");
        }
        match text(value, "activationState")? {
            "activated" | "suspended" | "revoked" => {
                Ok(value["activationState"].as_str().unwrap().into())
            }
            _ => bail!("NODE_STATUS_INVALID"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Value, Value) {
        let identity = json!({"machineIdentity":"cgm-fixture","installInstanceId":"cgi-fixture"});
        let manifest = json!({"schemaVersion":2,"status":"PROVISIONED_WAITING_HEARTBEATS",
            "controlPlaneUrl":CONTROL_PLANE,"machineIdentity":"cgm-fixture","installInstanceId":"cgi-fixture",
            "tokenKind":"always_on_host","requiredComponents":["sentinel","host_app"],
            "binding":{"hostId":"host-fixture","pcId":"1","deviceId":"PC-01","keeperEntryId":"pc-01"},
            "components":[{"componentType":"host_app","credentialFile":"host_app.credential.dpapi","applicationActivationId":"act-host"},
                {"componentType":"sentinel","credentialFile":"sentinel.credential.dpapi","applicationActivationId":"act-sentinel"}]});
        (manifest, identity)
    }
    #[test]
    fn manifest_restores_binding_without_setup_token() {
        let (m, i) = fixture();
        let b = parse_manifest(&m, &i).unwrap();
        assert_eq!(b.host_id, "host-fixture");
        assert_eq!(b.pc_id, "1");
        b.validate_cached_binding(&json!({"HostId":"","SchemaVersion":0,"ActivationState":""}))
            .unwrap();
        b.validate_cached_binding(&Value::Null).unwrap();
    }
    #[test]
    fn rejects_identity_backend_component_and_file_changes() {
        let (m, i) = fixture();
        for key in [
            "machineIdentity",
            "installInstanceId",
            "controlPlaneUrl",
            "tokenKind",
            "status",
        ] {
            let mut bad = m.clone();
            bad[key] = json!("other");
            assert!(parse_manifest(&bad, &i).is_err(), "{key}");
        }
        let mut bad = m.clone();
        bad["components"][0]["credentialFile"] = json!("../sentinel.credential.dpapi");
        assert!(parse_manifest(&bad, &i).is_err());
        let mut bad = m.clone();
        bad["components"]
            .as_array_mut()
            .unwrap()
            .push(m["components"][0].clone());
        assert!(parse_manifest(&bad, &i).is_err());
        let mut bad = m.clone();
        bad["requiredComponents"] = json!(["host_app", "host_app"]);
        assert!(parse_manifest(&bad, &i).is_err());
        let mut bad = m;
        bad["binding"]["hostId"] = json!(" ");
        assert!(parse_manifest(&bad, &i).is_err());
    }
    #[test]
    fn never_overwrites_different_cached_owner() {
        let (m, i) = fixture();
        let b = parse_manifest(&m, &i).unwrap();
        for key in [
            "HostId",
            "MachineIdentity",
            "InstallInstanceId",
            "SentinelPcId",
            "SentinelDeviceId",
            "ApplicationActivationId",
            "KeeperEntryId",
            "ApplicationType",
            "ControlPlaneUrl",
        ] {
            let mut s = json!({});
            s[key] = json!("other");
            assert!(b.validate_cached_binding(&s).is_err(), "{key}");
        }
    }
    #[test]
    fn status_requires_exact_binding_and_explicit_state() {
        let (m, i) = fixture();
        let b = parse_manifest(&m, &i).unwrap();
        let v = json!({"ok":true,"hostId":b.host_id,"sentinelPcId":b.pc_id,"sentinelDeviceId":b.device_id,
            "keeperEntryId":b.keeper_entry_id,"activationRecordId":b.host_id,"controlPlaneUrl":CONTROL_PLANE,"activationState":"activated"});
        for phase in ["activated", "suspended", "revoked"] {
            let mut s = v.clone();
            s["activationState"] = json!(phase);
            assert_eq!(b.verified_status(&s).unwrap(), phase)
        }
        for key in [
            "hostId",
            "sentinelPcId",
            "sentinelDeviceId",
            "keeperEntryId",
            "activationRecordId",
            "controlPlaneUrl",
            "activationState",
        ] {
            let mut s = v.clone();
            s[key] = json!("other");
            assert!(b.verified_status(&s).is_err(), "{key}");
        }
        assert!(b.verified_status(&json!({"ok":true})).is_err());
    }
}
