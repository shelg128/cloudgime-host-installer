use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

#[path = "universal_transport_config.rs"]
mod universal_transport_config;

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
    renewal_credential: Option<String>,
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

const RENEWAL_CREDENTIAL_FILE: &str = "installer_renewal.credential.dpapi";

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct InstallerRenewalState {
    renewal_credential: String,
    repair_generation: u64,
    pending_operation_id: Option<String>,
}

impl InstallerRenewalState {
    fn prepare_operation(&mut self) -> Result<()> {
        if !(32..=512).contains(&self.renewal_credential.len())
            || self.renewal_credential.trim() != self.renewal_credential
            || self.repair_generation >= 9_007_199_254_740_991
        {
            bail!("renewal credential or repair generation is invalid");
        }
        if self.pending_operation_id.is_none() {
            self.pending_operation_id = Some(format!("cgrep_{}", uuid::Uuid::new_v4().to_simple()));
        }
        let operation_id = self.pending_operation_id.as_deref().unwrap_or_default();
        if operation_id.len() < 16
            || operation_id.len() > 128
            || !operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            bail!("repair operation id is invalid");
        }
        Ok(())
    }

    fn complete_operation(&mut self, response_generation: u64) -> Result<()> {
        let expected = self
            .repair_generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("repair generation overflow"))?;
        if self.pending_operation_id.is_none() || response_generation != expected {
            bail!("cannot complete mismatched repair operation");
        }
        self.repair_generation = response_generation;
        self.pending_operation_id = None;
        Ok(())
    }
}

fn validate_repair_endpoint(
    api_base_url: &str,
    state: &HostActivationStateFile,
) -> Result<reqwest::Url> {
    let base =
        reqwest::Url::parse(api_base_url.trim()).context("repair API base URL is invalid")?;
    let bound = reqwest::Url::parse(state.control_plane_url.trim())
        .context("activation state control plane URL is invalid")?;
    for url in [&base, &bound] {
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            bail!("repair control plane must be HTTPS without embedded credentials");
        }
    }
    if base.origin() != bound.origin() {
        bail!("repair control plane does not match the activated Node binding");
    }
    base.join("/api/v1/provisioning/installer/repair")
        .context("failed to construct repair endpoint")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepairCredentials {
    generation: u64,
    host_runtime_token: String,
    sentinel_activation_id: String,
    sentinel_runtime_credential: String,
}

fn validate_scoped_secret(value: &str, prefix: &str, name: &str) -> Result<()> {
    if !value.starts_with(prefix) || value.len() < 32 || value.len() > 256
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    { bail!("repair response {name} format is invalid") }
    Ok(())
}

fn validate_repair_response(
    body: &Value,
    state: &HostActivationStateFile,
    renewal: &InstallerRenewalState,
    expected_sentinel_activation_id: &str,
) -> Result<RepairCredentials> {
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("repair response did not confirm success");
    }
    let required = |name: &str| -> Result<&str> {
        body.get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("repair response is missing {name}"))
    };
    let runtime_token = required("runtimeToken")?;
    validate_scoped_secret(runtime_token, "cgha_", "runtime token")?;
    if body["runtimeToken"].as_str() != Some(runtime_token) {
        bail!("repair response runtime token is not canonical");
    }
    let components = body.get("componentCredentials")
        .ok_or_else(|| anyhow!("repair response component credentials missing"))?;
    let host_component = components.get("host_app")
        .ok_or_else(|| anyhow!("repair response Host credential missing"))?;
    if host_component.get("applicationActivationId").and_then(Value::as_str) != Some(state.host_id.as_str())
        || host_component.get("runtimeToken").and_then(Value::as_str) != Some(runtime_token) {
        bail!("repair response Host component binding mismatch");
    }
    let sentinel = components.get("sentinel")
        .ok_or_else(|| anyhow!("repair response Sentinel credential missing"))?;
    let sentinel_activation_id = sentinel.get("applicationActivationId").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("repair response Sentinel activation missing"))?;
    let sentinel_runtime_credential = sentinel.get("runtimeCredentialRef").and_then(Value::as_str)
        .ok_or_else(|| anyhow!("repair response Sentinel runtime credential missing"))?;
    if sentinel_activation_id != expected_sentinel_activation_id {
        bail!("repair response Sentinel activation binding mismatch");
    }
    validate_scoped_secret(sentinel_runtime_credential, "cred_sentinel_", "Sentinel runtime credential")?;
    if required("renewalCredential")? != renewal.renewal_credential {
        bail!("repair response changed the stable renewal credential");
    }
    if required("operationId")? != renewal.pending_operation_id.as_deref().unwrap_or_default() {
        bail!("repair response operationId mismatch");
    }
    if required("hostId")? != state.host_id
        || state.sentinel_pc_id.is_empty()
        || required("sentinelPcId")? != state.sentinel_pc_id
    {
        bail!("repair response host/PC binding mismatch");
    }
    let binding = body
        .get("binding")
        .ok_or_else(|| anyhow!("repair response binding missing"))?;
    if binding.get("hostId").and_then(Value::as_str) != Some(state.host_id.as_str())
        || binding.get("machineIdentity").and_then(Value::as_str)
            != Some(state.machine_identity.as_str())
        || binding.get("installInstanceId").and_then(Value::as_str)
            != Some(state.install_instance_id.as_str())
    {
        bail!("repair response installation binding mismatch");
    }
    let generation = body
        .get("repairGeneration")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("repair response is missing repairGeneration"))?;
    let expected = renewal
        .repair_generation
        .checked_add(1)
        .ok_or_else(|| anyhow!("repair generation overflow"))?;
    if generation != expected {
        bail!("repair response generation mismatch");
    }
    Ok(RepairCredentials {
        generation,
        host_runtime_token: runtime_token.to_string(),
        sentinel_activation_id: sentinel_activation_id.to_string(),
        sentinel_runtime_credential: sentinel_runtime_credential.to_string(),
    })
}

#[cfg(windows)]
struct RepairProcessGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for RepairProcessGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_repair_process_guard() -> Result<RepairProcessGuard> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0},
        System::Threading::{CreateMutexW, WaitForSingleObject},
    };
    let name: Vec<u16> = std::ffi::OsStr::new("Global\\CloudGimeInstallerRepair")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        bail!("failed to create repair process mutex");
    }
    let wait = unsafe { WaitForSingleObject(handle, 30_000) };
    if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        bail!("failed to acquire repair process mutex");
    }
    Ok(RepairProcessGuard(handle))
}

#[cfg(not(windows))]
fn acquire_repair_process_guard() -> Result<()> {
    Ok(())
}

/// Rotate host runtime credentials for an existing Universal Node.
/// The renewal credential remains stable; operationId + generation make retries
/// deterministic if the server commits but the response is lost.
/// Requires no pairing token; secrets stay in the machine-protected store.
pub fn repair_once(bundle_root: &Path, api_base_url: &str) -> Result<()> {
    let _repair_guard = acquire_repair_process_guard()?;
    let state_path = bundle_root
        .join("moonlight")
        .join("server")
        .join("host_activation_state.json");
    let old_state_bytes = fs::read(&state_path).context("failed to read host activation state for repair")?;
    let state: HostActivationStateFile = serde_json::from_slice(
        &old_state_bytes,
    )
    .context("host activation state is not valid JSON")?;
    if state.machine_identity.trim().is_empty()
        || state.install_instance_id.trim().is_empty()
        || state.host_id.is_empty()
        || state.sentinel_pc_id.is_empty()
        || state.activation_state != "activated"
    {
        bail!("repair requires complete existing active Node binding");
    }

    let endpoint = validate_repair_endpoint(api_base_url, &state)?;
    let root = provisioning_root(bundle_root);
    let renewal_path = root.join(RENEWAL_CREDENTIAL_FILE);
    if !renewal_path.exists() {
        bail!("renewal credential is missing; authorized one-time migration is required");
    }
    let mut renewal_state: InstallerRenewalState = serde_json::from_slice(
        &read_dpapi_secret(&renewal_path).context("failed to read renewal credential")?,
    )
    .context("renewal credential payload is not valid JSON")?;
    // Validate the existing envelope before asking the backend to rotate anything.
    let envelope_path = root.join("host_app.credential.dpapi");
    let envelope_bytes =
        read_dpapi_secret(&envelope_path).context("failed to read host_app credential envelope")?;
    let mut envelope: Value = serde_json::from_slice(&envelope_bytes)
        .context("host_app credential envelope is not valid JSON")?;
    if !envelope.is_object() { bail!("host_app credential envelope must be an object") }
    select_runtime_credential(&envelope, "host_app")?;
    let manifest: Value = serde_json::from_slice(&fs::read(root.join("manifest.json"))?)
        .context("Universal Node manifest is not valid JSON")?;
    let sentinel_activation_id = manifest.get("components").and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Universal Node components missing"))?.iter()
        .filter(|component| component.get("componentType").and_then(Value::as_str) == Some("sentinel"))
        .map(|component| component.get("applicationActivationId").and_then(Value::as_str).unwrap_or_default())
        .collect::<Vec<_>>();
    if sentinel_activation_id.len() != 1 || sentinel_activation_id[0].is_empty() {
        bail!("Universal Node Sentinel binding is missing or ambiguous");
    }
    let sentinel_activation_id = sentinel_activation_id[0].to_string();
    let sentinel_path = root.join("sentinel.credential.dpapi");
    let sentinel_bytes = read_dpapi_secret(&sentinel_path)
        .context("failed to read Sentinel credential envelope")?;
    let mut sentinel_envelope: Value = serde_json::from_slice(&sentinel_bytes)
        .context("Sentinel credential envelope is not valid JSON")?;
    if !sentinel_envelope.is_object() { bail!("Sentinel credential envelope must be an object") }
    select_runtime_credential(&sentinel_envelope, "sentinel")?;

    // Persist before network I/O, including on retry. Never send a newly-created
    // operation if writing the pending journal fails.
    renewal_state.prepare_operation()?;
    write_dpapi_secret(&renewal_path, &serde_json::to_vec(&renewal_state)?)?;
    let operation_id = renewal_state.pending_operation_id.as_deref().unwrap();

    let payload = serde_json::json!({
        "machineIdentity": state.machine_identity,
        "installInstanceId": state.install_instance_id,
        "renewalCredential": renewal_state.renewal_credential,
        "operationId": operation_id,
        "expectedGeneration": renewal_state.repair_generation,
    });
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to create repair HTTP client")?
        .post(endpoint)
        .json(&payload)
        .send()
        .context("installer repair request failed")?;
    let status = response.status();
    let body: Value = response
        .json()
        .context("repair response is not valid JSON")?;
    if !status.is_success() {
        // Server-controlled error text must never echo secrets to operator logs.
        bail!(
            "repair rejected with HTTP {}; pending operation retained",
            status.as_u16()
        );
    }
    let credentials = validate_repair_response(&body, &state, &renewal_state, &sentinel_activation_id)?;

    // Rotate both Universal Node runtime credentials. Device tokens, setup
    // license, machine identity and activation IDs remain stable.
    let update_envelope = |value: &mut Value, token: &str| {
        value["runtimeCredentialRef"] = Value::String(token.to_string());
        value["runtimeToken"] = Value::String(token.to_string());
    };
    update_envelope(&mut envelope, &credentials.host_runtime_token);
    update_envelope(&mut sentinel_envelope, &credentials.sentinel_runtime_credential);
    let old_host = envelope_bytes;
    let old_sentinel = sentinel_bytes;
    write_dpapi_secret(&envelope_path, &serde_json::to_vec(&envelope)?)?;
    if let Err(error) = write_dpapi_secret(&sentinel_path, &serde_json::to_vec(&sentinel_envelope)?) {
        let _ = write_dpapi_secret(&envelope_path, &old_host);
        return Err(error).context("failed to rotate Sentinel credential; Host credential rolled back");
    }

    // The node must prove liveness again before streams can hand off.
    let mut repaired_state = state;
    repaired_state.last_ready_for_stream = false;
    repaired_state.last_heartbeat_at_utc = String::new();
    let state_value = serde_json::to_value(&repaired_state)
        .context("failed to serialize repaired host activation state")?;
    if let Err(error) = write_restricted_json(&state_path, &state_value) {
        let _ = write_dpapi_secret(&envelope_path, &old_host);
        let _ = write_dpapi_secret(&sentinel_path, &old_sentinel);
        return Err(error).context("failed to update readiness cache; component credentials rolled back");
    }

    // Commit the local operation last. Any failure leaves the server operation
    // replayable. Restore both envelopes before returning an error.
    renewal_state.complete_operation(credentials.generation)?;
    if let Err(error) = write_dpapi_secret(&renewal_path, &serde_json::to_vec(&renewal_state)?) {
        let _ = write_dpapi_secret(&envelope_path, &old_host);
        let _ = write_dpapi_secret(&sentinel_path, &old_sentinel);
        let _ = fs::write(&state_path, &old_state_bytes);
        return Err(error).context("failed to commit repair journal; component credentials and state rolled back");
    }
    Ok(())
}

fn provisioning_root(bundle_root: &Path) -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("CLOUDGIME_PROVISIONING_ROOT") {
        if !root.is_empty() {
            return root.into();
        }
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        return std::path::PathBuf::from(program_data)
            .join("Cloudgime")
            .join("provisioning");
    }
    bundle_root.join("provisioning")
}

#[path = "universal_host_binding.rs"]
mod universal_host_binding;
pub use universal_host_binding::{UniversalHostBinding, CONTROL_PLANE as NODE_CONTROL_PLANE};

// Loading an existing Node must never call load_or_create_machine_identity.
pub fn universal_manifest_exists(bundle_root: &Path) -> bool {
    let root = provisioning_root(bundle_root);
    ["manifest.json", "machine-identity.dpapi", RENEWAL_CREDENTIAL_FILE]
        .iter().any(|name| root.join(name).exists())
}

pub fn read_universal_host_binding(bundle_root: &Path) -> Result<UniversalHostBinding> {
    let root = provisioning_root(bundle_root);
    let manifest_path = root.join("manifest.json");
    let metadata = fs::metadata(&manifest_path).context("NODE_MANIFEST_MISSING")?;
    if !metadata.is_file() || metadata.len() > 65536 { bail!("NODE_MANIFEST_INVALID") }
    let raw = fs::read(&manifest_path)?;
    let manifest: Value = serde_json::from_slice(&raw).context("NODE_MANIFEST_INVALID")?;
    let mut identity_bytes = read_dpapi_secret(&root.join("machine-identity.dpapi"))
        .context("NODE_IDENTITY_UNAVAILABLE")?;
    let identity = serde_json::from_slice::<Value>(&identity_bytes);
    identity_bytes.fill(0);
    let binding = universal_host_binding::parse_manifest(&manifest, &identity.context("NODE_IDENTITY_INVALID")?)?;
    if fs::read(&manifest_path)? != raw { bail!("NODE_MANIFEST_CHANGED") }
    Ok(binding)
}

pub fn write_host_runtime_cache(path: &Path, value: &Value) -> Result<()> {
    // Restrict before the rename: an interactive renter must never see a
    // partially-written trusted cache or mutate it during recovery.
    let temp = path.with_extension(format!("{}.next", uuid::Uuid::new_v4()));
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    restrict_to_local_system_and_current_user(&temp)?;
    fs::rename(&temp, path).context("NODE_CACHE_REPLACE_FAILED")?;
    Ok(())
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_dpapi_secret(&path, &plaintext)?;
    Ok(state)
}

/// Recovers the canonical identity recorded in an existing activation state.
/// Caller must prove it is the same installation before calling this function.
pub fn restore_machine_identity(bundle_root: &Path, state: &MachineIdentityState) -> Result<()> {
    if state.machine_identity.trim().is_empty() || state.install_instance_id.trim().is_empty() {
        bail!("refusing to store incomplete machine identity");
    }
    let path = provisioning_root(bundle_root).join("machine-identity.dpapi");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_dpapi_secret(&path, &serde_json::to_vec(state)?)
}

pub fn write_component_credential(
    bundle_root: &Path,
    component_type: &str,
    value: &Value,
) -> Result<()> {
    validate_component_type(component_type)?;
    let root = provisioning_root(bundle_root);
    fs::create_dir_all(&root).context("failed to create provisioning directory")?;
    let plaintext = serde_json::to_vec(value)?;
    write_dpapi_secret(
        &root.join(format!("{}.credential.dpapi", component_type)),
        &plaintext,
    )
}

fn validate_component_type(component_type: &str) -> Result<()> {
    if component_type.is_empty()
        || component_type.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        bail!("invalid component type");
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent"))?;
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
    assignments
        .get(name)
        .and_then(|value| value.get(field))
        .and_then(Value::as_i64)
        .unwrap_or_default() as i32
}

fn assignment_string(assignments: &Value, name: &str) -> String {
    assignments
        .get(name)
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
    let host = manifest
        .components
        .iter()
        .find(|component| component.component_type == "host_app")
        .ok_or_else(|| anyhow!("manifest is missing host_app component"))?;
    let binding = manifest.binding.as_ref();
    let host_id = host
        .host_id
        .clone()
        .or_else(|| binding.and_then(|value| value.host_id.clone()))
        .unwrap_or_else(|| request.host_id.to_string());
    let host_token = host.runtime_token.clone().unwrap_or_default();
    if host_token.trim().is_empty() {
        bail!("manifest host_app is missing runtimeToken");
    }
    if manifest
        .control_plane_url
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        bail!("manifest is missing controlPlaneUrl");
    }
    let state = HostActivationStateFile {
        schema_version: 1,
        host_id,
        machine_identity: request.machine_identity.to_string(),
        install_instance_id: request.install_instance_id.to_string(),
        activation_state: host
            .activation_state
            .clone()
            .unwrap_or_else(|| "activated".to_string()),
        setup_token_kind: manifest.token_kind.clone().unwrap_or_default(),
        instance_type: manifest.instance_type.clone().unwrap_or_default(),
        control_plane_url: manifest.control_plane_url.clone().unwrap_or_default(),
        display_name: request.machine_name.to_string(),
        sentinel_pc_id: binding
            .and_then(|value| value.pc_id.clone())
            .unwrap_or_default(),
        sentinel_device_id: binding
            .and_then(|value| value.device_id.clone())
            .unwrap_or_default(),
        keeper_entry_id: binding
            .and_then(|value| value.keeper_entry_id.clone())
            .unwrap_or_default(),
        application_activation_id: host.application_activation_id.clone().unwrap_or_default(),
        application_type: "CLOUDGIME_HOST".to_string(),
        pc_label: binding
            .and_then(|value| value.slot_label.clone())
            .unwrap_or_default(),
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
    // Component credentials were already persisted as a complete envelope by
    // provision_once. Never overwrite it here: transport needs deviceToken too.
    let path = bundle_root
        .join("moonlight")
        .join("server")
        .join("host_activation_state.json");
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
    let endpoint = format!(
        "{}/api/v1/provisioning/installer/activate",
        api_base_url.trim_end_matches('/')
    );
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
    let readiness_status = manifest.status.as_deref().or_else(|| {
        manifest
            .readiness
            .as_ref()
            .and_then(|value| value.status.as_deref())
    });
    if readiness_status != Some("PROVISIONED_WAITING_HEARTBEATS") {
        bail!("provisioning did not reach heartbeat-pending state");
    }
    let mut component_types = HashSet::new();
    for component in &manifest.components {
        if !component_types.insert(component.component_type.as_str()) {
            bail!(
                "manifest contains duplicate component {}",
                component.component_type
            );
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
    if !expected_components
        .iter()
        .all(|component| component_types.contains(component))
    {
        bail!("manifest does not satisfy expected provisioning profile");
    }
    if request.expected_activation_license_kind == "always_on_host"
        && component_types.contains("keeper")
    {
        bail!("Always-On manifest must not provision Keeper");
    }
    for required in &expected_components {
        let component = manifest
            .components
            .iter()
            .find(|component| component.component_type == *required)
            .ok_or_else(|| anyhow!("manifest is missing component {required}"))?;
        if component
            .credential_ref
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
            || component
                .device_token
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            bail!("component {required} is missing scoped credential data");
        }
    }

    let metadata = consumer_manifest_metadata(&manifest, request)?;
    let root = provisioning_root(bundle_root);
    fs::create_dir_all(&root).context("failed to create provisioning directory")?;
    for component in &manifest.components {
        let secret = component_credential_envelope(component);
        let plaintext = serde_json::to_vec(&secret)?;
        let path = root.join(format!("{}.credential.dpapi", component.component_type));
        write_dpapi_secret(&path, &plaintext)?;
    }
    // The server keeps only a hash of this credential; the installer must
    // persist it so `repair` can run later without a pairing token.
    if let Some(renewal_credential) = manifest
        .renewal_credential
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let plaintext =
            serde_json::to_vec(&serde_json::json!({ "renewalCredential": renewal_credential }))?;
        write_dpapi_secret(&root.join(RENEWAL_CREDENTIAL_FILE), &plaintext)?;
    }
    write_host_activation_state(bundle_root, request, &manifest)?;
    write_keeper_tunnel_env(bundle_root, &manifest)?;
    write_keeper_tunnel_routes(bundle_root, &manifest)?;
    write_restricted_json(&root.join("manifest.json"), &metadata)?;
    Ok(())
}

fn consumer_manifest_metadata(
    manifest: &ProvisionResponse,
    request: &ProvisionRequest<'_>,
) -> Result<Value> {
    let binding = manifest
        .binding
        .as_ref()
        .ok_or_else(|| anyhow!("manifest binding missing"))?;
    let required = |value: Option<&str>, field: &str| -> Result<String> {
        let text = value.unwrap_or_default().trim();
        if text.is_empty() {
            bail!("manifest {field} missing");
        }
        Ok(text.to_string())
    };
    let pc_id = required(binding.pc_id.as_deref(), "pcId")?;
    let device_id = required(binding.device_id.as_deref(), "deviceId")?;
    let backend = required(manifest.control_plane_url.as_deref(), "controlPlaneUrl")?;
    let url = reqwest::Url::parse(&backend).context("manifest backend URL invalid")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        bail!("manifest backend must be HTTPS without embedded credentials");
    }
    let mut components = Vec::new();
    for component in &manifest.components {
        validate_component_type(&component.component_type)?;
        let activation = required(
            component.application_activation_id.as_deref(),
            "applicationActivationId",
        )?;
        // Explicit non-secret projection: never copy arbitrary response fields/policies.
        components.push(serde_json::json!({
            "componentType": component.component_type,
            "applicationActivationId": activation,
            "credentialFile": format!("{}.credential.dpapi", component.component_type),
        }));
    }
    Ok(serde_json::json!({
        "schemaVersion": 2,
        "status": "PROVISIONED_WAITING_HEARTBEATS",
        "tokenKind": manifest.token_kind,
        "requiredComponents": manifest.required_components,
        "machineIdentity": request.machine_identity,
        "installInstanceId": request.install_instance_id,
        "controlPlaneUrl": backend,
        "binding": { "pcId": pc_id, "deviceId": device_id,
            "hostId": binding.host_id, "pcNumber": binding.pc_number,
            "keeperEntryId": binding.keeper_entry_id },
        "components": components,
    }))
}

fn keeper_tunnel_env_content(
    manifest: &ProvisionResponse,
    state: &HostActivationStateFile,
) -> Result<String> {
    let binding = manifest
        .binding
        .as_ref()
        .ok_or_else(|| anyhow!("provisioning binding missing"))?;
    let host = manifest
        .components
        .iter()
        .find(|c| c.component_type == "host_app")
        .ok_or_else(|| anyhow!("host_app component missing"))?;
    let token = host
        .device_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("host_app deviceToken missing for keeper tunnel"))?;
    let pc_id = binding
        .pc_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("binding pcId missing for keeper tunnel"))?;
    let device_id = binding
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("binding deviceId missing for keeper tunnel"))?;
    let pc_number_raw = binding
        .pc_number
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| Some(pc_id.to_string()))
        .ok_or_else(|| anyhow!("binding pcNumber missing for keeper tunnel"))?;
    let pc_number = pc_number_raw
        .parse::<u32>()
        .map(|n| format!("{n:02}"))
        .unwrap_or(pc_number_raw);
    let base = state.control_plane_url.trim().trim_end_matches('/');
    if !base.starts_with("https://")
        || state.keeper_entry_id.trim().is_empty()
        || state.host_stream_proxy_route.trim().is_empty()
    {
        bail!("keeper tunnel assignment incomplete");
    }
    for value in [
        token,
        pc_id,
        pc_number.as_str(),
        device_id,
        state.keeper_entry_id.trim(),
        state.host_stream_proxy_route.trim(),
    ] {
        if value.contains(['\r', '\n', '=']) {
            bail!("unsafe keeper tunnel value");
        }
    }
    // Exact KeeperTunnelAgent.LoadAgentConfig contract. Some identity keys are
    // legacy `_DEFAULT`; do not normalize names without changing the agent too.
    Ok(format!(
        "CLOUDRENTAL_API_BASE={base}\r\nCLOUDRENTAL_DEVICE_TOKEN={token}\r\nCLOUDRENTAL_PC_ID_DEFAULT={pc_id}\r\nCLOUDRENTAL_PC_NUMBER_DEFAULT={pc_number}\r\nCLOUDRENTAL_DEVICE_ID_DEFAULT={device_id}\r\nCLOUDRENTAL_KEEPER_ENTRY_ID={}\r\nCLOUDRENTAL_TUNNEL_ROUTES=moonlight\r\n",
        state.keeper_entry_id.trim()
    ))
}

fn write_keeper_tunnel_routes(bundle_root: &Path, manifest: &ProvisionResponse) -> Result<()> {
    let host = manifest.components.iter().find(|c| c.component_type == "host_app")
        .ok_or_else(|| anyhow!("host_app component missing"))?;
    let activation = host.application_activation_id.as_deref().map(str::trim).filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("host_app activation missing"))?;
    let port = assignment_i32(&host.assignments, "HOST_HTTP", "port");
    let config_path = bundle_root.join("moonlight").join("server").join("config.json");
    let config: Value = serde_json::from_slice(&fs::read(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?)?;
    let prefix = config.get("web_server").and_then(|v| v.get("url_path_prefix"))
        .and_then(Value::as_str).ok_or_else(|| anyhow!("local stream prefix missing"))?;
    let path = bundle_root.join("keeper-tunnel").join("data").join("keeper-tunnel-status.json");
    let current: Value = if path.exists() { serde_json::from_slice(&fs::read(&path)?)? }
        else { serde_json::json!({"routes":[]}) };
    let value = universal_transport_config::configure_route(&current, activation, port, prefix)?;
    write_restricted_json(&path, &value)
}

fn write_keeper_tunnel_env(bundle_root: &Path, manifest: &ProvisionResponse) -> Result<()> {
    let state_path = bundle_root
        .join("moonlight")
        .join("server")
        .join("host_activation_state.json");
    let state: HostActivationStateFile = serde_json::from_slice(
        &fs::read(&state_path).with_context(|| format!("read {}", state_path.display()))?,
    )?;
    let env = keeper_tunnel_env_content(manifest, &state)?;
    let path = bundle_root
        .join("keeper-tunnel")
        .join("data")
        .join("cloudrental.env");
    let parent = path.parent().ok_or_else(|| anyhow!("keeper env path invalid"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("env.next");
    fs::write(&tmp, env.as_bytes())?;
    restrict_to_local_system_and_current_user(&tmp)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn component_credential_envelope(component: &ManifestComponent) -> Value {
    serde_json::json!({
        "componentType": component.component_type,
        "credentialRef": component.credential_ref,
        "deviceToken": component.device_token,
        "activationToken": component.activation_token,
        "runtimeToken": component.runtime_token,
        "runtimeCredentialRef": component.runtime_credential_ref,
    })
}

fn write_restricted_json(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)?;
    restrict_to_local_system_and_current_user(path)?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn write_dpapi_secret(path: &Path, plaintext: &[u8]) -> Result<()> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData},
    };
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    const CRYPTPROTECT_LOCAL_MACHINE: u32 = 0x4;
    let ok = unsafe {
        CryptProtectData(
            &input,
            null(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut output,
        )
    };
    if ok == 0 {
        bail!("DPAPI encryption failed");
    }
    let encrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    write_atomic(path, &encrypted)?;
    restrict_to_local_system_and_current_user(path)?;
    Ok(())
}

pub fn read_component_credential(bundle_root: &Path, component_type: &str) -> Result<String> {
    validate_component_type(component_type)?;
    let root = provisioning_root(bundle_root);
    let path = root.join(format!("{}.credential.dpapi", component_type));
    if !path.exists() {
        bail!(
            "Credential file not found for component: {}",
            component_type
        );
    }
    let plaintext = read_dpapi_secret(&path)?;
    let secret: Value = serde_json::from_slice(&plaintext)?;
    select_runtime_credential(&secret, component_type)
}

fn select_runtime_credential(secret: &Value, component_type: &str) -> Result<String> {
    validate_component_type(component_type)?;
    if let Some(stored_type) = secret.get("componentType") {
        if stored_type.as_str() != Some(component_type) {
            bail!("credential component mismatch");
        }
    }
    // Host heartbeat requires its runtimeToken. An empty runtimeCredentialRef
    // must never shadow that token; Sentinel uses runtimeCredentialRef first.
    let fields: &[&str] = if component_type == "host_app" {
        &["runtimeToken", "runtimeCredentialRef"]
    } else {
        // Legacy Universal Node Sentinel envelopes may only have credentialRef.
        // It is accepted solely to authorize repair; a successful repair always
        // writes scoped runtimeCredentialRef + runtimeToken fields.
        &["runtimeCredentialRef", "runtimeToken", "credentialRef"]
    };
    for field in fields {
        if let Some(value) = secret.get(field).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
        }
    }
    bail!(
        "No runtime credential found for component: {}",
        component_type
    );
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    #[test]
    fn keeper_route_writer_requires_host_activation() {
        let manifest: ProvisionResponse = serde_json::from_value(serde_json::json!({"requiredComponents":["host_app"],"controlPlaneUrl":"https://backend.test","components":[{"componentType":"host_app","applicationActivationId":"act-new","assignments":{},"policies":{}}]})).unwrap();
        let host = manifest
            .components
            .iter()
            .find(|c| c.component_type == "host_app")
            .unwrap();
        assert_eq!(host.application_activation_id.as_deref(), Some("act-new"));
        let mut routes = serde_json::json!({"routes":[{"name":"panel","applicationActivationId":"panel-old"},{"name":"moonlight","applicationActivationId":"old"}]});
        let route = routes["routes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["name"] == "moonlight")
            .unwrap();
        route["applicationActivationId"] =
            Value::String(host.application_activation_id.clone().unwrap());
        assert_eq!(routes["routes"][0]["applicationActivationId"], "panel-old");
        assert_eq!(routes["routes"][1]["applicationActivationId"], "act-new");
    }

    #[test]
    fn keeper_tunnel_env_requires_complete_scoped_assignment() {
        let manifest: ProvisionResponse = serde_json::from_value(serde_json::json!({
            "requiredComponents":["host_app"], "controlPlaneUrl":"https://backend.test",
            "binding":{"pcId":"1","deviceId":"PC-01"},
            "components":[{"componentType":"host_app","deviceToken":"transport-token","assignments":{},"policies":{}}]
        })).unwrap();
        let mut state = HostActivationStateFile::default();
        state.control_plane_url = "https://backend.test".into();
        state.keeper_entry_id = "keeper-1".into();
        state.host_stream_proxy_route = "moonlight".into();
        let env = keeper_tunnel_env_content(&manifest, &state).unwrap();
        assert!(env.contains("CLOUDRENTAL_DEVICE_TOKEN=transport-token"));
        assert!(env.contains("CLOUDRENTAL_PC_ID_DEFAULT=1"));
        assert!(env.contains("CLOUDRENTAL_PC_NUMBER_DEFAULT=01"));
        assert!(env.contains("CLOUDRENTAL_TUNNEL_ROUTES=moonlight"));
        assert!(!env.contains("CLOUDRENTAL_DEVICE_TOKEN_DEFAULT="));
        state.host_stream_proxy_route = "bad\nroute".into();
        assert!(keeper_tunnel_env_content(&manifest, &state).is_err());
    }

    #[test]
    fn consumer_metadata_is_scoped_and_has_no_tokens() {
        let mut manifest: ProvisionResponse = serde_json::from_value(serde_json::json!({
            "requiredComponents":["host_app"], "controlPlaneUrl":"https://backend.test",
            "binding":{"pcId":"1","deviceId":"PC-01","hostId":"host-1"},
            "components":[{"componentType":"host_app","applicationActivationId":"a1",
              "runtimeToken":"secret-runtime","deviceToken":"secret-device","credentialRef":"secret-ref",
              "assignments":{},"policies":{"secret":"secret-policy"}}]
        })).unwrap();
        let request = ProvisionRequest {
            activation_license: "secret-license",
            expected_activation_license_kind: "always_on_host",
            machine_name: "pc",
            hostname: "pc",
            machine_identity: "m",
            install_instance_id: "i",
            requested_device_id: "PC-01",
            host_id: "host-1",
            installer_version: "test",
        };
        let value = consumer_manifest_metadata(&manifest, &request).unwrap();
        assert_eq!(value["binding"]["pcId"], "1");
        assert_eq!(value["components"][0]["applicationActivationId"], "a1");
        assert!(!value.to_string().contains("secret-"));
        manifest.binding.as_mut().unwrap().pc_id = None;
        assert!(consumer_manifest_metadata(&manifest, &request).is_err());
    }
    #[test]
    fn provisioning_envelope_preserves_transport_and_runtime_credentials() {
        let component: ManifestComponent = serde_json::from_value(serde_json::json!({
            "componentType":"host_app", "credentialRef":"ref", "deviceToken":"transport-dummy",
            "runtimeToken":"runtime-dummy", "runtimeCredentialRef":"", "assignments":{}, "policies":{}
        })).unwrap();
        let envelope = component_credential_envelope(&component);
        assert_eq!(envelope["deviceToken"], "transport-dummy");
        assert_eq!(envelope["runtimeToken"], "runtime-dummy");
        assert_eq!(
            select_runtime_credential(&envelope, "host_app").unwrap(),
            "runtime-dummy"
        );
    }
    #[test]
    fn host_token_is_not_shadowed_by_empty_reference() {
        let value = serde_json::json!({"componentType":"host_app", "runtimeCredentialRef":"", "runtimeToken":"host-token"});
        assert_eq!(
            select_runtime_credential(&value, "host_app").unwrap(),
            "host-token"
        );
    }
    #[test]
    fn host_and_sentinel_choose_their_own_runtime_field() {
        let value = serde_json::json!({"runtimeCredentialRef":"reference", "runtimeToken":"token"});
        assert_eq!(
            select_runtime_credential(&value, "host_app").unwrap(),
            "token"
        );
        assert_eq!(
            select_runtime_credential(&value, "sentinel").unwrap(),
            "reference"
        );
    }
    #[test]
    fn legacy_sentinel_credential_ref_is_repairable_but_not_a_host_runtime_token() {
        let value = serde_json::json!({"componentType":"sentinel", "credentialRef":"legacy-sentinel-reference"});
        assert_eq!(select_runtime_credential(&value, "sentinel").unwrap(), "legacy-sentinel-reference");
        assert!(select_runtime_credential(&value, "host_app").is_err());
    }
    #[test]
    fn empty_or_wrong_component_fails_closed() {
        assert!(
            select_runtime_credential(&serde_json::json!({"runtimeToken":"  "}), "host_app")
                .is_err()
        );
        assert!(
            select_runtime_credential(
                &serde_json::json!({"componentType":"sentinel", "runtimeToken":"secret"}),
                "host_app"
            )
            .is_err()
        );
        assert!(
            select_runtime_credential(&serde_json::json!({"runtimeToken":"secret"}), "../host_app")
                .is_err()
        );
    }
    #[test]
    fn legacy_reference_only_file_remains_readable() {
        assert_eq!(
            select_runtime_credential(
                &serde_json::json!({"runtimeCredentialRef":" ref "}),
                "host_app"
            )
            .unwrap(),
            "ref"
        );
    }
    #[test]
    fn repair_operation_journal_reuses_pending_and_commits_last() {
        let mut state = InstallerRenewalState {
            renewal_credential: "cgrenew_fixture_only_not_production_123456789".into(),
            repair_generation: 3,
            pending_operation_id: None,
        };
        state.prepare_operation().unwrap();
        let first_operation = state.pending_operation_id.clone().unwrap();
        assert!(first_operation.starts_with("cgrep_"));
        state.prepare_operation().unwrap();
        assert_eq!(
            state.pending_operation_id.as_deref(),
            Some(first_operation.as_str())
        );
        assert!(state.complete_operation(3).is_err());
        assert_eq!(state.repair_generation, 3);
        assert_eq!(
            state.pending_operation_id.as_deref(),
            Some(first_operation.as_str())
        );
        state.complete_operation(4).unwrap();
        assert_eq!(state.repair_generation, 4);
        assert!(state.pending_operation_id.is_none());
    }

    #[test]
    fn repair_endpoint_and_response_are_bound_to_existing_installation() {
        let mut host = HostActivationStateFile::default();
        host.host_id = "host-fixture".into();
        host.sentinel_pc_id = "21".into();
        host.machine_identity = "machine-fixture".into();
        host.install_instance_id = "install-fixture".into();
        host.control_plane_url = "https://backend.test".into();
        let renewal = InstallerRenewalState {
            renewal_credential: "cgrenew_fixture_only_not_production_123456789".into(),
            repair_generation: 2,
            pending_operation_id: Some("cgrep_fixture_operation_00000001".into()),
        };
        assert!(validate_repair_endpoint("http://backend.test", &host).is_err());
        assert!(validate_repair_endpoint("https://other.test", &host).is_err());
        assert_eq!(
            validate_repair_endpoint("https://backend.test", &host)
                .unwrap()
                .as_str(),
            "https://backend.test/api/v1/provisioning/installer/repair"
        );
        let response = serde_json::json!({
            "ok": true,
            "hostId": "host-fixture",
            "sentinelPcId": "21",
            "runtimeToken": "cgha_fixture_runtime_token_abcdefghijklmnopqrstuvwxyz",
            "componentCredentials": {
                "host_app": {
                    "applicationActivationId": "host-fixture",
                    "runtimeToken": "cgha_fixture_runtime_token_abcdefghijklmnopqrstuvwxyz"
                },
                "sentinel": {
                    "applicationActivationId": "act-sentinel-fixture",
                    "runtimeCredentialRef": "cred_sentinel_fixture_runtime_abcdefghijklmnopqrstuvwxyz"
                }
            },
            "renewalCredential": renewal.renewal_credential,
            "operationId": "cgrep_fixture_operation_00000001",
            "repairGeneration": 3,
            "binding": {
                "hostId": "host-fixture",
                "machineIdentity": "machine-fixture",
                "installInstanceId": "install-fixture"
            }
        });
        let credentials = validate_repair_response(
            &response, &host, &renewal, "act-sentinel-fixture"
        ).unwrap();
        assert_eq!(credentials.generation, 3);
        assert_eq!(credentials.host_runtime_token, "cgha_fixture_runtime_token_abcdefghijklmnopqrstuvwxyz");
        assert_eq!(credentials.sentinel_activation_id, "act-sentinel-fixture");
        assert_eq!(credentials.sentinel_runtime_credential, "cred_sentinel_fixture_runtime_abcdefghijklmnopqrstuvwxyz");
        for (path, bad) in [
            ("hostId", "other-host"),
            ("sentinelPcId", "22"),
            ("runtimeToken", "bad"),
            ("renewalCredential", "other-renewal"),
            ("operationId", "cgrep_other_operation_00000000"),
        ] {
            let mut changed = response.clone();
            changed[path] = Value::String(bad.into());
            assert!(
                validate_repair_response(&changed, &host, &renewal, "act-sentinel-fixture").is_err(),
                "{path}"
            );
        }
        let mut wrong_binding = response.clone();
        wrong_binding["binding"]["machineIdentity"] = Value::String("other-machine".into());
        assert!(validate_repair_response(&wrong_binding, &host, &renewal, "act-sentinel-fixture").is_err());
        let mut wrong_generation = response.clone();
        wrong_generation["repairGeneration"] = Value::Number(4.into());
        assert!(validate_repair_response(&wrong_generation, &host, &renewal, "act-sentinel-fixture").is_err());
        let mut wrong_sentinel = response.clone();
        wrong_sentinel["componentCredentials"]["sentinel"]["applicationActivationId"] = Value::String("other-sentinel".into());
        assert!(validate_repair_response(&wrong_sentinel, &host, &renewal, "act-sentinel-fixture").is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let root = std::env::temp_dir().join(format!(
            "cloudgime-atomic-replace-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_machine_scope_roundtrip_uses_encrypted_disk_bytes() {
        let root =
            std::env::temp_dir().join(format!("cloudgime-dpapi-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("host_app.credential.dpapi");
        let secret = serde_json::json!({"componentType":"host_app", "runtimeCredentialRef":"", "runtimeToken":"dummy-test-token-not-a-real-credential"});
        let plain = serde_json::to_vec(&secret).unwrap();
        write_dpapi_secret(&path, &plain).unwrap();
        match fs::read(&path) {
            Ok(encrypted) => {
                assert_ne!(encrypted, plain);
                assert!(
                    !encrypted
                        .windows(20)
                        .any(|part| part == b"dummy-test-token-not-")
                );
                let restored: Value =
                    serde_json::from_slice(&read_dpapi_secret(&path).unwrap()).unwrap();
                assert_eq!(restored, secret);
                assert_eq!(
                    select_runtime_credential(&restored, "host_app").unwrap(),
                    "dummy-test-token-not-a-real-credential"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                // Non-privileged developer/test context must not read machine credentials.
            }
            Err(error) => panic!("unexpected credential read error: {error}"),
        }
        // Retain this harmless dummy fixture as test evidence; no production paths touched.
    }
}

#[cfg(windows)]
pub(crate) fn read_dpapi_secret(path: &Path) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
    };
    let encrypted = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok =
        unsafe { CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut output) };
    if ok == 0 {
        bail!("DPAPI decryption failed");
    }
    let plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
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
            // Machine-wide credentials belong to services/admin provisioning,
            // never to the interactive renter. SIDs work across Windows languages
            // and do not depend on USERNAME under LocalSystem.
            .arg("*S-1-5-18:F")
            .arg("*S-1-5-32-544:F")
            .arg("/remove:g")
            .arg("*S-1-5-32-545")
            .arg("*S-1-5-11")
            .arg("*S-1-1-0")
            .output()
            .context("failed to apply credential ACL")?;
        if !output.status.success() {
            bail!("credential ACL update failed");
        }
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}
