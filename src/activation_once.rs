#[cfg(test)]
include!("activation_once_tests.rs");

// Durable Windows activation intent. Contains secrets: deliberately no Debug,
// display or public serialization outside DPAPI. Read before prompting license.
use super::*;
use std::io::Read;
const INTENT: &str = "installer-activation-intent.dpapi";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Intent {
    version: u32,
    operation_id: String,
    renewal: String,
    phase: String,
    machine: String,
    install: String,
    host: String,
    device: String,
    profile: String,
    machine_name: String,
    hostname: String,
    installer_version: String,
    license: String,
    response: Option<Value>,
}
fn load(root: &Path) -> Result<Option<Intent>> {
    let file = root.join(INTENT);
    if !file.exists() {
        return Ok(None);
    }
    let size = fs::metadata(&file)?.len();
    if size == 0 || size > 524288 {
        bail!("ACTIVATION_INTENT_SIZE_INVALID")
    }
    let cipher = activation_store::read_private(&file)?;
    let mut bytes = decrypt_dpapi(&cipher)?;
    let parsed = serde_json::from_slice::<Intent>(&bytes);
    bytes.fill(0);
    let s = parsed.context("ACTIVATION_INTENT_INVALID")?;
    if s.version != 1
        || uuid::Uuid::parse_str(&s.operation_id).is_err()
        || s.renewal.len() != 64
        || !s.renewal.bytes().all(|b| b.is_ascii_hexdigit())
        || !["REQUESTED", "RESPONSE_SAVED", "COMPLETE"].contains(&s.phase.as_str())
        || s.machine.is_empty()
        || s.install.is_empty()
        || s.host.is_empty()
        || s.license.len() > 4096
        || s.phase == "REQUESTED" && s.license.is_empty()
        || s.phase != "REQUESTED" && s.response.is_none()
    {
        bail!("ACTIVATION_INTENT_INVALID")
    }
    Ok(Some(s))
}
fn save(root: &Path, s: &Intent) -> Result<()> {
    let mut bytes = serde_json::to_vec(s)?;
    let result = write_dpapi_secret(&root.join(INTENT), &bytes);
    bytes.fill(0);
    result?;
    let _ = activation_store::read_private(&root.join(INTENT))?;
    Ok(())
}
pub(super) fn can_resume(bundle: &Path) -> Result<bool> {
    Ok(load(&provisioning_root(bundle))?.is_some())
}
fn matches(s: &Intent, r: &ProvisionRequest<'_>) -> Result<()> {
    if s.machine != r.machine_identity
        || s.install != r.install_instance_id
        || s.host != r.host_id
        || s.device != r.requested_device_id
        || s.profile != r.expected_activation_license_kind
        || s.machine_name != r.machine_name
        || s.hostname != r.hostname
        || !r.activation_license.is_empty()
            && !s.license.is_empty()
            && s.license != r.activation_license
    {
        bail!("ACTIVATION_INTENT_BINDING_CHANGED")
    }
    Ok(())
}
fn validate(body: &Value, s: &Intent, origin: &str) -> Result<()> {
    if body.get("ok").and_then(Value::as_bool) != Some(true)
        || body.get("renewalCredential").and_then(Value::as_str) != Some(&s.renewal)
        || body.get("tokenKind").and_then(Value::as_str) != Some(&s.profile)
        || body
            .pointer("/binding/machineIdentity")
            .and_then(Value::as_str)
            != Some(&s.machine)
        || body
            .pointer("/binding/installInstanceId")
            .and_then(Value::as_str)
            != Some(&s.install)
        || body.pointer("/binding/hostId").and_then(Value::as_str) != Some(&s.host)
        || body
            .get("controlPlaneUrl")
            .and_then(Value::as_str)
            .map(|v| v.trim_end_matches('/'))
            != Some(origin)
    {
        bail!("ACTIVATION_RESPONSE_BINDING_INVALID")
    }
    let components = body
        .get("components")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("ACTIVATION_COMPONENTS_INVALID"))?;
    let expected = if s.profile == "always_on_host" {
        vec!["sentinel", "host_app"]
    } else if s.profile == "instance_pair" {
        vec!["sentinel", "host_app", "keeper"]
    } else {
        bail!("ACTIVATION_PROFILE_INVALID")
    };
    let mut names = HashSet::new();
    for c in components {
        let name = c
            .get("componentType")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !expected.contains(&name) || !names.insert(name) {
            bail!("ACTIVATION_COMPONENTS_INVALID")
        }
        for field in ["applicationActivationId", "deviceToken", "credentialRef"] {
            let v = c.get(field).and_then(Value::as_str).unwrap_or_default();
            if v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control) {
                bail!("ACTIVATION_CREDENTIAL_INVALID")
            }
        }
        if name == "host_app"
            && c.get("runtimeToken")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .is_empty()
            || name == "sentinel"
                && c.get("runtimeCredentialRef")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .is_empty()
        {
            bail!("ACTIVATION_RUNTIME_CREDENTIAL_INVALID")
        }
    }
    if names.len() != expected.len() {
        bail!("ACTIVATION_COMPONENTS_INVALID")
    }
    let required = body
        .get("requiredComponents")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("ACTIVATION_COMPONENTS_INVALID"))?;
    if required.len() != expected.len()
        || required
            .iter()
            .filter_map(Value::as_str)
            .collect::<HashSet<_>>()
            != names
    {
        bail!("ACTIVATION_COMPONENTS_INVALID")
    }
    Ok(())
}
pub(super) fn run(bundle: &Path, api: &str, r: &ProvisionRequest<'_>) -> Result<()> {
    let _guard = acquire_repair_process_guard()?;
    let origin = api.trim_end_matches('/');
    if origin != NODE_CONTROL_PLANE {
        bail!("ACTIVATION_ORIGIN_DENIED")
    }
    run_inner(bundle, origin, r, |endpoint, payload| {
        let response = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(45))
            .build()?
            .post(endpoint)
            .json(payload)
            .send()
            .context("ACTIVATION_NETWORK_RETRY")?;
        if response.status() != reqwest::StatusCode::OK {
            bail!("ACTIVATION_HTTP_RETRY_{}", response.status().as_u16())
        }
        let mut bytes = Vec::new();
        response
            .take(262145)
            .read_to_end(&mut bytes)
            .context("ACTIVATION_RESPONSE_RETRY")?;
        if bytes.len() > 262144 {
            bail!("ACTIVATION_RESPONSE_TOO_LARGE")
        }
        serde_json::from_slice(&bytes).context("ACTIVATION_RESPONSE_INVALID")
    })
}
pub(super) fn run_inner(
    bundle: &Path,
    origin: &str,
    r: &ProvisionRequest<'_>,
    mut send: impl FnMut(&str, &Value) -> Result<Value>,
) -> Result<()> {
    let root = provisioning_root(bundle);
    fs::create_dir_all(&root)?;
    let mut s = match load(&root)? {
        Some(s) => {
            matches(&s, r)?;
            s
        }
        None => {
            if r.activation_license.is_empty()
                || r.activation_license.len() > 4096
                || r.activation_license.chars().any(char::is_control)
            {
                bail!("ACTIVATION_LICENSE_REQUIRED")
            }
            if [
                "manifest.json",
                "host_app.credential.dpapi",
                "sentinel.credential.dpapi",
                RENEWAL_CREDENTIAL_FILE,
            ]
            .iter()
            .any(|p| root.join(p).exists())
            {
                bail!("ACTIVATION_EXISTING_NODE_USE_REPAIR")
            }
            let s = Intent {
                version: 1,
                operation_id: uuid::Uuid::new_v4().to_hyphenated().to_string(),
                renewal: format!(
                    "{}{}",
                    uuid::Uuid::new_v4().to_simple(),
                    uuid::Uuid::new_v4().to_simple()
                ),
                phase: "REQUESTED".into(),
                machine: r.machine_identity.into(),
                install: r.install_instance_id.into(),
                host: r.host_id.into(),
                device: r.requested_device_id.into(),
                profile: r.expected_activation_license_kind.into(),
                machine_name: r.machine_name.into(),
                hostname: r.hostname.into(),
                installer_version: r.installer_version.into(),
                license: r.activation_license.into(),
                response: None,
            };
            save(&root, &s)?;
            s
        }
    };
    if s.phase == "COMPLETE" {
        let binding = read_universal_host_binding(bundle)?;
        if binding.machine_identity != s.machine
            || binding.install_instance_id != s.install
            || binding.host_id != s.host
            || binding.token_kind != s.profile
        {
            bail!("ACTIVATION_COMPLETED_BINDING_CHANGED")
        }
        for name in ["host_app", "sentinel"] {
            let _ = read_component_credential(bundle, name)?;
        }
        let saved: InstallerRenewalState =
            serde_json::from_slice(&read_dpapi_secret(&root.join(RENEWAL_CREDENTIAL_FILE))?)?;
        if saved.renewal_credential != s.renewal {
            bail!("ACTIVATION_RENEWAL_CHANGED")
        }
        return Ok(());
    }
    if s.phase == "REQUESTED" {
        let payload = serde_json::json!({"activationLicense":s.license,"expectedActivationLicenseKind":s.profile,
            "machineName":s.machine_name,"hostname":s.hostname,"machineIdentity":s.machine,"installInstanceId":s.install,
            "requestedDeviceId":s.device,"hostId":s.host,"installerVersion":s.installer_version,
            "activationOperationId":s.operation_id,"activationRenewalCredential":s.renewal});
        let body = send(
            &format!("{origin}/api/v1/provisioning/installer/activate"),
            &payload,
        )?;
        validate(&body, &s, origin)?;
        s.response = Some(body);
        s.phase = "RESPONSE_SAVED".into();
        s.license.clear();
        save(&root, &s)?;
        #[cfg(test)]
        if std::env::var("CG_ACTIVATION_FIXTURE_PHASE").as_deref() == Ok("crash-after-response") {
            std::process::exit(73);
        }
    }
    let body = s
        .response
        .as_ref()
        .ok_or_else(|| anyhow!("ACTIVATION_RESPONSE_MISSING"))?;
    validate(body, &s, origin)?;
    let request = ProvisionRequest {
        activation_license: "",
        expected_activation_license_kind: &s.profile,
        machine_name: &s.machine_name,
        hostname: &s.hostname,
        machine_identity: &s.machine,
        install_instance_id: &s.install,
        requested_device_id: &s.device,
        host_id: &s.host,
        installer_version: &s.installer_version,
    };
    apply_provision_response(bundle, &request, body.clone())?;
    s.phase = "COMPLETE".into();
    save(&root, &s)?;
    Ok(())
}
