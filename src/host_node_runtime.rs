//! Universal Node runtime path. Setup tokens and legacy Keeper fallback are not
//! consulted here. Authorization is memory-only and expires; metadata != READY.
use super::*;
use std::sync::Mutex;
use sha2::{Digest, Sha256};
#[path = "host_status_transport.rs"]
mod status_transport;

struct Verified {
    binding: provisioning::UniversalHostBinding,
    credential_hash: [u8; 32],
    phase: String,
    at: Instant,
}
static VERIFIED: Mutex<Option<Verified>> = Mutex::new(None);
const STATUS_LEASE: Duration = Duration::from_secs(90);

fn credential_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
fn same_binding(state: &HostActivationStateRecord, binding: &provisioning::UniversalHostBinding) -> bool {
    state.host_id == binding.host_id && state.machine_identity == binding.machine_identity
        && state.install_instance_id == binding.install_instance_id
        && state.sentinel_pc_id == binding.pc_id && state.sentinel_device_id == binding.device_id
        && state.keeper_entry_id == binding.keeper_entry_id
        && state.application_activation_id == binding.application_activation_id
        && state.control_plane_url == provisioning::NODE_CONTROL_PLANE
}
fn verified_allows(v: &Verified, state: &HostActivationStateRecord, age: Duration) -> bool {
    state.activation_state == "activated" && !state.runtime_token.is_empty()
        && v.phase == "activated" && age <= STATUS_LEASE
        && same_binding(state, &v.binding) && v.credential_hash == credential_hash(&state.runtime_token)
}
fn invalidate() { if let Ok(mut cache) = VERIFIED.lock() { *cache = None; } }

pub(super) fn hydrate(paths: &BundlePaths, state: &mut HostActivationStateRecord) -> Result<()> {
    let binding = provisioning::read_universal_host_binding(&paths.bundle_root)?;
    binding.validate_cached_binding(&serde_json::to_value(&*state)?)?;
    // No legacy plaintext token migration and no new identity creation here.
    let token = provisioning::read_component_credential(&paths.bundle_root, "host_app")?;
    if token.is_empty() { bail!("NODE_RUNTIME_CREDENTIAL_UNAVAILABLE") }
    state.schema_version = 1;
    state.host_id = binding.host_id.clone();
    state.machine_identity = binding.machine_identity.clone();
    state.install_instance_id = binding.install_instance_id.clone();
    state.control_plane_url = provisioning::NODE_CONTROL_PLANE.into();
    state.sentinel_pc_id = binding.pc_id.clone();
    state.sentinel_device_id = binding.device_id.clone();
    state.keeper_entry_id = binding.keeper_entry_id.clone();
    state.application_activation_id = binding.application_activation_id.clone();
    state.activation_record_id = binding.host_id.clone();
    state.application_type = "CLOUDGIME_HOST".into();
    state.setup_token_kind = binding.token_kind.clone();
    state.instance_type = if binding.token_kind == "always_on_host" { "always-on" } else { "power-managed" }.into();
    state.runtime_token = token;
    state.credential_ref.clear();
    let cache = VERIFIED.lock().map_err(|_| anyhow!("NODE_AUTH_CACHE_UNAVAILABLE"))?;
    if let Some(valid) = cache.as_ref().filter(|v| v.binding == binding
        && v.credential_hash == credential_hash(&state.runtime_token) && v.at.elapsed() <= STATUS_LEASE) {
        state.activation_state = valid.phase.clone();
        if valid.phase != "activated" { state.last_ready_for_stream = false; }
    } else {
        state.last_ready_for_stream = false;
        if !matches!(state.activation_state.as_str(), "revoked" | "suspended") {
            state.activation_state = "node_revalidating".into();
        }
    }
    Ok(())
}

pub(super) fn allows(state: &HostActivationStateRecord) -> bool {
    if state.activation_state != "activated" || state.runtime_token.is_empty() { return false; }
    VERIFIED.lock().ok().is_some_and(|cache| cache.as_ref().is_some_and(|v|
        verified_allows(v, state, v.at.elapsed())))
}

pub(super) fn invalidate_erased_cache() { invalidate(); }

pub(super) fn failed_state() -> HostActivationStateRecord {
    invalidate();
    HostActivationStateRecord { universal_node: true, activation_state: "node_binding_unavailable".into(), ..Default::default() }
}

#[cfg(windows)]
pub(super) fn sync(paths: &BundlePaths, state: &mut HostActivationStateRecord) -> Result<()> {
    let binding = provisioning::read_universal_host_binding(&paths.bundle_root)?;
    if !same_binding(state, &binding) || state.runtime_token.is_empty() { invalidate(); bail!("NODE_STATUS_CONTEXT_INVALID") }
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(10))
        .build().map_err(|_| anyhow!("NODE_STATUS_CLIENT_FAILED"))?;
    // Status verifies the existing credential. It neither reclaims the license
    // nor sends observations of readiness/heartbeat on behalf of the runtime.
    let request=serde_json::json!({"hostId": binding.host_id, "machineIdentity":binding.machine_identity,
        "installInstanceId":binding.install_instance_id,"runtimeToken":state.runtime_token});
    let response = client.post(format!("{}/api/v1/host-activation/status", provisioning::NODE_CONTROL_PLANE))
        .json(&request).send();
    let (code,payload)=match response {
        Ok(response)=>{
            let code=response.status().as_u16();
            let payload=if response.status().is_success(){response.json::<Value>().map_err(|_|{invalidate();anyhow!("NODE_STATUS_RESPONSE_INVALID")})?}else{Value::Null};
            (code,payload)
        },
        Err(_)=>match status_transport::status(&request){
            Ok(response)=>response,
            Err(_)=>{if !allows(state){state.activation_state="node_backend_unavailable".into();}bail!("NODE_STATUS_NETWORK_UNAVAILABLE");}
        }
    };
    if !(200..300).contains(&code) {
        invalidate(); state.activation_state = match code {
            401 => "node_credential_repair_required",
            403 => "suspended",
            409 => "node_binding_conflict",
            _ => "node_backend_unavailable",
        }.into();
        state.last_ready_for_stream = false;
        // Persist negative cache; never convert a status error into activated.
        save_activation_state(paths, state)?;
        bail!("NODE_STATUS_HTTP_{code}");
    }
    let phase = binding.verified_status(&payload).map_err(|_| { invalidate(); anyhow!("NODE_STATUS_BINDING_MISMATCH") })?;
    // Recheck files after network I/O; repair/rotation during request invalidates it.
    let latest = provisioning::read_universal_host_binding(&paths.bundle_root)?;
    let token = provisioning::read_component_credential(&paths.bundle_root, "host_app")?;
    if latest != binding || token != state.runtime_token { invalidate(); bail!("NODE_PROVISIONING_CHANGED") }
    if state.activation_state != phase || phase != "activated" { state.last_ready_for_stream = false; }
    state.activation_state = phase.clone();
    state.updated_at_utc = now_rfc3339();
    if let Some(name) = payload.get("displayName").and_then(Value::as_str) { state.display_name = name.into(); }
    // Durable cache contains no runtime token; source of truth stays the Node
    // manifest/identity. Nothing writes or rotates component DPAPI envelopes.
    save_activation_state(paths, state)?;
    *VERIFIED.lock().map_err(|_| anyhow!("NODE_AUTH_CACHE_UNAVAILABLE"))? = Some(Verified {
        binding, credential_hash: credential_hash(&state.runtime_token), phase, at: Instant::now(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_activated_disk_cache_is_not_startup_authorization() {
        let s = HostActivationStateRecord { universal_node:true, activation_state:"activated".into(),
            runtime_token:"fixture-only".into(), host_id:"h".into(), ..Default::default() };
        assert!(!allows(&s));
        assert!(!allows(&HostActivationStateRecord { activation_state:"suspended".into(), ..s.clone() }));
        assert!(!allows(&HostActivationStateRecord { activation_state:"revoked".into(), ..s }));
    }
    #[test]
    fn lease_binds_all_identity_fields_and_credential_and_expires() {
        let binding = provisioning::UniversalHostBinding { host_id:"h".into(),pc_id:"1".into(),device_id:"PC-01".into(),
            keeper_entry_id:"k".into(),machine_identity:"m".into(),install_instance_id:"i".into(),
            application_activation_id:"a".into(),token_kind:"always_on_host".into() };
        let s = HostActivationStateRecord { universal_node:true,activation_state:"activated".into(),host_id:"h".into(),
            sentinel_pc_id:"1".into(),sentinel_device_id:"PC-01".into(),keeper_entry_id:"k".into(),
            machine_identity:"m".into(),install_instance_id:"i".into(),application_activation_id:"a".into(),
            control_plane_url:provisioning::NODE_CONTROL_PLANE.into(),runtime_token:"fixture-token".into(),..Default::default() };
        let v = Verified { binding, credential_hash:credential_hash(&s.runtime_token),phase:"activated".into(),at:Instant::now() };
        assert!(verified_allows(&v,&s,Duration::from_secs(89)));
        assert!(!verified_allows(&v,&s,Duration::from_secs(91)));
        for field in 0..11 {
            let mut bad=s.clone();
            match field {
                0=>bad.host_id="other".into(),1=>bad.sentinel_pc_id="other".into(),2=>bad.sentinel_device_id="other".into(),
                3=>bad.keeper_entry_id="other".into(),4=>bad.machine_identity="other".into(),5=>bad.install_instance_id="other".into(),
                6=>bad.application_activation_id="other".into(),7=>bad.control_plane_url="https://other.test".into(),
                8=>bad.runtime_token="other".into(),9=>bad.activation_state="suspended".into(),_=>bad.activation_state="revoked".into(),
            }
            assert!(!verified_allows(&v,&bad,Duration::ZERO),"field {field}");
        }
    }
    #[test]
    fn node_failure_does_not_fall_back_to_pairing() {
        let state = failed_state();
        assert!(state.universal_node);
        assert_eq!(state.activation_state,"node_binding_unavailable");
        assert!(state.runtime_token.is_empty());
        assert!(!state.last_ready_for_stream);
    }
}
