use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionRequest<'a> {
    pub activation_license: &'a str,
    pub expected_activation_license_kind: &'a str,
    pub machine_name: &'a str,
    pub hostname: &'a str,
    pub machine_identity: &'a str,
    pub install_instance_id: &'a str,
    pub requested_device_id: &'a str,
    pub host_id: &'a str,
    pub installer_version: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestComponent {
    component_type: String,
    credential_ref: Option<String>,
    device_token: Option<String>,
    activation_token: Option<String>,
    runtime_token: Option<String>,
    runtime_credential_ref: Option<String>,
    application_activation_id: Option<String>,
    host_id: Option<String>,
    activation_record_id: Option<String>,
    activation_state: Option<String>,
    assignments: Value,
    policies: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionResponse {
    #[serde(default)]
    status: Option<String>,
    token_kind: Option<String>,
    required_components: Vec<String>,
    components: Vec<ManifestComponent>,
    readiness: Option<ProvisionReadiness>,
    binding: Option<ProvisionBinding>,
    instance_type: Option<String>,
    control_plane_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct HostActivationStateFile {
    schema_version: i32,
    host_id: String,
    machine_identity: String,
    install_instance_id: String,
    activation_state: String,
    setup_token_kind: String,
    instance_type: String,
    control_plane_url: String,
    display_name: String,
    sentinel_pc_id: String,
    sentinel_device_id: String,
    keeper_entry_id: String,
    application_activation_id: String,
    application_type: String,
    pc_label: String,
    credential_ref: String,
    host_http_port: i32,
    host_stream_udp_start: i32,
    host_stream_udp_end: i32,
    host_stream_proxy_route: String,
    license_assignments: Value,
    license_policies: Value,
    #[serde(skip_serializing)]
    runtime_token: String,
    activation_record_id: String,
    activated_at_utc: String,
    redeemed_at_utc: String,
    last_heartbeat_at_utc: String,
    last_ready_for_stream: bool,
    updated_at_utc: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionBinding {
    host_id: Option<String>,
    pc_id: Option<String>,
    pc_number: Option<String>,
    device_id: Option<String>,
    slot_label: Option<String>,
    keeper_entry_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionReadiness {
    status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineIdentityState {
    pub machine_identity: String,
    pub install_instance_id: String,
}

fn provisioning_root(bundle_root: &Path) -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("CLOUDGIME_PROVISIONING_ROOT") {
        if !root.is_empty() {
            return root.into();
        }
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        return std::path::PathBuf::from(program_data).join("Cloudgime").join("provisioning");
    }
    bundle_root.join("provisioning")
}

pub fn load_or_create_machine_identity(bundle_root: &Path) -> Result<MachineIdentityState> {
    let path = provisioning_root(bundle_root).join("machine-identity.dpapi");
    if path.exists() {
        let plaintext = read_dpapi_secret(&path)?;
        return serde_json::from_slice(&plaintext).context("invalid stored machine identity");
    }
    let state = MachineIdentityState {
        machine_identity: format!("cgm-{}", uuid::Uuid::new_v4()),
        install_instance_id: format!("cgi-{}", uuid::Uuid::new_v4()),
    };
    let plaintext = serde_json::to_vec(&state)?;
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    write_dpapi_secret(&path, &plaintext)?;
    Ok(state)
}

pub fn read_component_credential(bundle_root: &Path, component_type: &str) -> Result<Value> {
    if component_type.is_empty() || component_type.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
        bail!("invalid component type");
    }
    let path = provisioning_root(bundle_root).join(format!("{}.credential.dpapi", component_type));
    let plaintext = read_dpapi_secret(&path)?;
    serde_json::from_slice(&plaintext).context("invalid component credential")
}

pub fn write_component_credential(bundle_root: &Path, component_type: &str, value: &Value) -> Result<()> {
    validate_component_type(component_type)?;
    let root = provisioning_root(bundle_root);
    fs::create_dir_all(&root).context("failed to create provisioning directory")?;
    let plaintext = serde_json::to_vec(value)?;
    write_dpapi_secret(&root.join(format!("{}.credential.dpapi", component_type)), &plaintext)
}

fn validate_component_type(component_type: &str) -> Result<()> {
    if component_type.is_empty() || component_type.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
        bail!("invalid component type");
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("target path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error).with_context(|| format!("failed to replace {}", path.display()));
    }
    Ok(())
}

fn assignment_i32(assignments: &Value, name: &str, field: &str) -> i32 {
    assignments.get(name)
        .and_then(|value| value.get(field))
        .and_then(Value::as_i64)
        .unwrap_or_default() as i32
}

fn assignment_string(assignments: &Value, name: &str) -> String {
    assignments.get(name)
        .and_then(|value| value.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn write_host_activation_state(
    bundle_root: &Path,
    request: &ProvisionRequest<'_>,
    manifest: &ProvisionResponse,
) -> Result<()> {
    let host = manifest.components.iter()
        .find(|component| component.component_type == "host_app")
        .ok_or_else(|| anyhow!("manifest is missing host_app component"))?;
    let binding = manifest.binding.as_ref();
    let host_id = host.host_id.clone()
        .or_else(|| binding.and_then(|value| value.host_id.clone()))
        .unwrap_or_else(|| request.host_id.to_string());
    let host_token = host.runtime_token.clone().unwrap_or_default();
    if host_token.trim().is_empty() {
        bail!("manifest host_app is missing runtimeToken");
    }
    if manifest.control_plane_url.as_deref().unwrap_or_default().trim().is_empty() {
        bail!("manifest is missing controlPlaneUrl");
    }
    let state = HostActivationStateFile {
        schema_version: 1,
        host_id,
        machine_identity: request.machine_identity.to_string(),
        install_instance_id: request.install_instance_id.to_string(),
        activation_state: host.activation_state.clone().unwrap_or_else(|| "activated".to_string()),
        setup_token_kind: manifest.token_kind.clone().unwrap_or_default(),
        instance_type: manifest.instance_type.clone().unwrap_or_default(),
        control_plane_url: manifest.control_plane_url.clone().unwrap_or_default(),
        display_name: request.machine_name.to_string(),
        sentinel_pc_id: binding.and_then(|value| value.pc_id.clone()).unwrap_or_default(),
        sentinel_device_id: binding.and_then(|value| value.device_id.clone()).unwrap_or_default(),
        keeper_entry_id: binding.and_then(|value| value.keeper_entry_id.clone()).unwrap_or_default(),
        application_activation_id: host.application_activation_id.clone().unwrap_or_default(),
        application_type: "CLOUDGIME_HOST".to_string(),
        pc_label: binding.and_then(|value| value.slot_label.clone()).unwrap_or_default(),
        credential_ref: host.credential_ref.clone().unwrap_or_default(),
        host_http_port: assignment_i32(&host.assignments, "HOST_HTTP", "port"),
        host_stream_udp_start: assignment_i32(&host.assignments, "HOST_STREAM_UDP", "portStart"),
        host_stream_udp_end: assignment_i32(&host.assignments, "HOST_STREAM_UDP", "portEnd"),
        host_stream_proxy_route: assignment_string(&host.assignments, "HOST_STREAM_PROXY_ROUTE"),
        license_assignments: host.assignments.clone(),
        license_policies: host.policies.clone(),
        runtime_token: String::new(),
        activation_record_id: host.activation_record_id.clone().unwrap_or_default(),
        activated_at_utc: String::new(),
        redeemed_at_utc: String::new(),
        last_heartbeat_at_utc: String::new(),
        last_ready_for_stream: false,
        updated_at_utc: String::new(),
    };
    let root = provisioning_root(bundle_root);
    fs::create_dir_all(&root)?;
    write_dpapi_secret(&root.join("host_app.credential.dpapi"), &serde_json::to_vec(&serde_json::json!({
        "componentType": "host_app",
        "runtimeToken": host_token,
        "credentialRef": host.credential_ref,
    }))?)?;
    let path = bundle_root.join("moonlight").join("server").join("host_activation_state.json");
    write_atomic(&path, &serde_json::to_vec_pretty(&state)?)?;
    restrict_to_local_system_and_current_user(&path)?;
    Ok(())
}

pub fn provision_once(
    bundle_root: &Path,
    api_base_url: &str,
    request: &ProvisionRequest<'_>,
) -> Result<()> {
    if request.activation_license.trim().is_empty() {
        bail!("activation license is empty");
    }
    let endpoint = format!("{}/api/v1/provisioning/installer/activate", api_base_url.trim_end_matches('/'));
    let response = reqwest::blocking::Client::new()
        .post(endpoint)
        .json(request)
        .send()
        .context("installer provisioning request failed")?;
    let status = response.status();
    let body: Value = response.json().context("invalid provisioning response")?;
    if !status.is_success() {
        bail!("provisioning rejected with HTTP {}", status.as_u16());
    }
    let manifest: ProvisionResponse = serde_json::from_value(body.clone())
        .context("provisioning response is not a valid manifest")?;
    let readiness_status = manifest
        .status
        .as_deref()
        .or_else(|| manifest.readiness.as_ref().and_then(|value| value.status.as_deref()));
    if readiness_status != Some("PROVISIONED_WAITING_HEARTBEATS") {
        bail!("provisioning did not reach heartbeat-pending state");
    }
    let mut component_types = HashSet::new();
    for component in &manifest.components {
        if !component_types.insert(component.component_type.as_str()) {
            bail!("manifest contains duplicate component {}", component.component_type);
        }
    }
    for required in &manifest.required_components {
        if !component_types.contains(required.as_str()) {
            bail!("manifest is missing required component {}", required);
        }
    }
    let expected_components: HashSet<&str> = match request.expected_activation_license_kind {
        "always_on_host" => ["sentinel", "host_app"].into_iter().collect(),
        "instance_pair" => ["sentinel", "keeper", "host_app"].into_iter().collect(),
        other => bail!("unsupported provisioning profile: {other}"),
    };
    if !expected_components.iter().all(|component| component_types.contains(component)) {
        bail!("manifest does not satisfy expected provisioning profile");
    }
    if request.expected_activation_license_kind == "always_on_host" && component_types.contains("keeper") {
        bail!("Always-On manifest must not provision Keeper");
    }
    for required in &expected_components {
        let component = manifest.components.iter()
            .find(|component| component.component_type == *required)
            .ok_or_else(|| anyhow!("manifest is missing component {required}"))?;
        if component.credential_ref.as_deref().unwrap_or_default().trim().is_empty()
            || component.device_token.as_deref().unwrap_or_default().trim().is_empty()
        {
            bail!("component {required} is missing scoped credential data");
        }
    }

    let root = provisioning_root(bundle_root);
    fs::create_dir_all(&root).context("failed to create provisioning directory")?;
    let metadata = serde_json::json!({
        "schemaVersion": 1,
        "status": readiness_status,
        "tokenKind": manifest.token_kind,
        "requiredComponents": manifest.required_components,
        "machineIdentity": request.machine_identity,
        "installInstanceId": request.install_instance_id,
        "hostId": request.host_id,
    });
    write_restricted_json(&root.join("manifest.json"), &metadata)?;

    for component in &manifest.components {
        let secret = serde_json::json!({
            "componentType": component.component_type,
            "credentialRef": component.credential_ref,
            "deviceToken": component.device_token,
            "activationToken": component.activation_token,
            "runtimeToken": component.runtime_token,
            "runtimeCredentialRef": component.runtime_credential_ref,
        });
        let plaintext = serde_json::to_vec(&secret)?;
        let path = root.join(format!("{}.credential.dpapi", component.component_type));
        write_dpapi_secret(&path, &plaintext)?;
    }
    write_host_activation_state(bundle_root, request, &manifest)?;
    Ok(())
}

fn write_restricted_json(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)?;
    restrict_to_local_system_and_current_user(path)?;
    Ok(())
}

#[cfg(windows)]
fn write_dpapi_secret(path: &Path, plaintext: &[u8]) -> Result<()> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB}};
    let mut input = CRYPT_INTEGER_BLOB { cbData: plaintext.len() as u32, pbData: plaintext.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    const CRYPTPROTECT_LOCAL_MACHINE: u32 = 0x4;
    let ok = unsafe { CryptProtectData(&input, null(), null(), null(), null(), CRYPTPROTECT_LOCAL_MACHINE, &mut output) };
    if ok == 0 { bail!("DPAPI encryption failed"); }
    let encrypted = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData.cast()); }
    write_atomic(path, &encrypted)?;
    restrict_to_local_system_and_current_user(path)?;
    Ok(())
}

#[cfg(windows)]
fn read_dpapi_secret(path: &Path) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB}};
    let encrypted = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let input = CRYPT_INTEGER_BLOB { cbData: encrypted.len() as u32, pbData: encrypted.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    let ok = unsafe { CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut output) };
    if ok == 0 { bail!("DPAPI decryption failed"); }
    let plaintext = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData.cast()); }
    Ok(plaintext)
}

#[cfg(not(windows))]
fn read_dpapi_secret(_path: &Path) -> Result<Vec<u8>> {
    Err(anyhow!("DPAPI provisioning is supported only on Windows"))
}

#[cfg(not(windows))]
fn write_dpapi_secret(_path: &Path, _plaintext: &[u8]) -> Result<()> {
    Err(anyhow!("DPAPI provisioning is supported only on Windows"))
}

fn restrict_to_local_system_and_current_user(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg("SYSTEM:F")
            .arg(format!("{}:F", std::env::var("USERNAME").unwrap_or_else(|_| "Administrators".to_string())))
            .output()
            .context("failed to apply credential ACL")?;
        if !output.status.success() { bail!("credential ACL update failed"); }
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}
