//! Backend-authenticated revocation only. NOT an OS-cleanup receipt producer.
//! Polling never ACKs commands or releases a PC. The worker is disabled by default.
use crate::{
    app::App,
    provisioning::{self, UniversalHostBinding},
};
use actix_web::web::Data;
use chrono::{DateTime, Utc};
use openssl::{hash::MessageDigest, pkey::PKey, sign::Signer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_BODY: usize = 64 * 1024;
#[path = "host_receipt_clock.rs"]
mod receipt_clock;
const POLL_INTERVAL: Duration = Duration::from_secs(5);

// Intentionally no Debug: the challenge is sensitive authorization material.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RevokeCommand {
    pub schema_version: u32,
    pub source: String,
    pub operation_id: String,
    pub session_id: String,
    pub prepare_operation_id: String,
    pub handoff_binding_id: String,
    pub native_session_id: String,
    pub claim_id: String,
    pub pc_id: String,
    pub user_id: String,
    pub application_activation_id: String,
    pub command_id: String,
    pub challenge: String,
    pub requested_at: String,
}
pub(crate) struct VerifiedRevokeCommand(RevokeCommand);
impl VerifiedRevokeCommand {
    pub(crate) fn command(&self) -> &RevokeCommand {
        &self.0
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PollResponse {
    ok: bool,
    commands: Vec<RevokeCommand>,
}

fn text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn nonzero_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| !id.is_nil() && id.to_hyphenated().to_string() == value)
}
impl RevokeCommand {
    pub(crate) fn validate(
        &self,
        pc_id: &str,
        activation: &str,
        credential: &str,
        now: DateTime<Utc>,
    ) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.source != "HOST"
            || self.pc_id != pc_id
            || self.application_activation_id != activation
            || ![
                &self.session_id,
                &self.native_session_id,
                &self.claim_id,
                &self.pc_id,
                &self.user_id,
                &self.application_activation_id,
            ]
            .iter()
            .all(|s| text(s))
            || ![
                &self.operation_id,
                &self.handoff_binding_id,
                &self.command_id,
            ]
            .iter()
            .all(|s| nonzero_uuid(s))
            || !(16..=128).contains(&self.prepare_operation_id.len())
            || !self
                .prepare_operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || self.challenge.len() != 64
            || !self
                .challenge
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !text(credential)
        {
            return Err("HOST_REVOKE_COMMAND_INVALID");
        }
        let requested = DateTime::parse_from_rfc3339(&self.requested_at)
            .map_err(|_| "HOST_REVOKE_TIME_INVALID")?;
        if !self.requested_at.ends_with('Z')
            || requested.timestamp_millis() > now.timestamp_millis().saturating_add(30_000)
        {
            return Err("HOST_REVOKE_TIME_INVALID");
        }
        let expected = self.expected_challenge(credential)?;
        if !openssl::memcmp::eq(expected.as_bytes(), self.challenge.as_bytes()) {
            return Err("HOST_REVOKE_CHALLENGE_INVALID");
        }
        Ok(())
    }
    pub(crate) fn expected_challenge(&self, credential: &str) -> Result<String, &'static str> {
        // Exactly matches backend deriveCleanupChallenge() v2 framing/order.
        let bytes = serde_json::to_vec(&[
            "cloudgime-cleanup-challenge-v2",
            "HOST",
            &self.operation_id,
            &self.session_id,
            &self.prepare_operation_id,
            &self.handoff_binding_id,
            &self.native_session_id,
            &self.claim_id,
            &self.pc_id,
            &self.user_id,
            &self.application_activation_id,
            &self.command_id,
        ])
        .map_err(|_| "HOST_REVOKE_ENCODING_FAILED")?;
        let key = PKey::hmac(credential.as_bytes()).map_err(|_| "HOST_REVOKE_CRYPTO_FAILED")?;
        let mut signer =
            Signer::new(MessageDigest::sha256(), &key).map_err(|_| "HOST_REVOKE_CRYPTO_FAILED")?;
        signer
            .update(&bytes)
            .map_err(|_| "HOST_REVOKE_CRYPTO_FAILED")?;
        Ok(hex::encode(
            signer
                .sign_to_vec()
                .map_err(|_| "HOST_REVOKE_CRYPTO_FAILED")?,
        ))
    }
    pub(crate) fn digest(&self) -> Result<[u8; 32], &'static str> {
        Ok(
            Sha256::digest(serde_json::to_vec(self).map_err(|_| "HOST_REVOKE_ENCODING_FAILED")?)
                .into(),
        )
    }
}
fn parse_commands(
    body: &[u8],
    pc: &str,
    activation: &str,
    credential: &str,
) -> Result<Vec<VerifiedRevokeCommand>, &'static str> {
    if body.is_empty() || body.len() > MAX_BODY {
        return Err("HOST_REVOKE_RESPONSE_SIZE");
    }
    let response: PollResponse =
        serde_json::from_slice(body).map_err(|_| "HOST_REVOKE_RESPONSE_INVALID")?;
    if !response.ok || response.commands.len() > 50 {
        return Err("HOST_REVOKE_RESPONSE_INVALID");
    }
    let mut sessions = std::collections::HashSet::new();
    // Validate the whole batch before any OS action.
    for command in &response.commands {
        command.validate(pc, activation, credential, Utc::now())?;
        if !sessions.insert(command.native_session_id.clone()) {
            return Err("HOST_REVOKE_DUPLICATE_SESSION");
        }
    }
    Ok(response
        .commands
        .into_iter()
        .map(VerifiedRevokeCommand)
        .collect())
}
// HostActivation.applicationActivationId is binding.hostId on Universal Node.
// components[].applicationActivationId is a separate provisioning component ID.
// Keep outbound identity and response validation on the same database authority.
fn cleanup_activation_id(binding: &UniversalHostBinding) -> &str {
    &binding.host_id
}
fn poll_identity(binding: &UniversalHostBinding) -> serde_json::Value {
    serde_json::json!({"pcId":binding.pc_id,"applicationActivationId":cleanup_activation_id(binding)})
}
struct Context {
    binding: UniversalHostBinding,
    credential: String,
}
fn context(root: &Path) -> Result<Context, &'static str> {
    let binding = provisioning::read_universal_host_binding(root)
        .map_err(|_| "HOST_REVOKE_BINDING_UNAVAILABLE")?;
    let credential = provisioning::read_component_credential(root, "host_app")
        .map_err(|_| "HOST_REVOKE_CREDENTIAL_UNAVAILABLE")?;
    if !text(&credential) {
        return Err("HOST_REVOKE_CREDENTIAL_INVALID");
    }
    Ok(Context {
        binding,
        credential,
    })
}
async fn poll(
    client: &reqwest::Client,
    root: &Path,
    pinned: &Context,
) -> Result<Vec<VerifiedRevokeCommand>, &'static str> {
    let before = context(root)?;
    if before.binding != pinned.binding || before.credential != pinned.credential {
        return Err("HOST_REVOKE_BINDING_CHANGED");
    }
    let endpoint = format!(
        "{}/api/v1/host/rental-cleanup-commands",
        provisioning::NODE_CONTROL_PLANE
    );
    let mut response = client
        .post(&endpoint)
        .bearer_auth(&pinned.credential)
        .json(&poll_identity(&pinned.binding))
        .send()
        .await
        .map_err(|_| "HOST_REVOKE_NETWORK_UNAVAILABLE")?;
    if response.status() != reqwest::StatusCode::OK || response.url().as_str() != endpoint {
        return Err("HOST_REVOKE_POLL_REJECTED");
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY as u64)
    {
        return Err("HOST_REVOKE_RESPONSE_SIZE");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "HOST_REVOKE_RESPONSE_READ_FAILED")?
    {
        if body.len() + chunk.len() > MAX_BODY {
            return Err("HOST_REVOKE_RESPONSE_SIZE");
        }
        body.extend_from_slice(&chunk);
    }
    let after = context(root)?;
    if after.binding != pinned.binding || after.credential != pinned.credential {
        return Err("HOST_REVOKE_BINDING_CHANGED");
    }
    let parsed = parse_commands(
        &body,
        &pinned.binding.pc_id,
        cleanup_activation_id(&pinned.binding),
        &pinned.credential,
    );
    body.fill(0);
    parsed
}

async fn send_receipt(client:&reqwest::Client,root:&Path,pinned:&Context,verified:&VerifiedRevokeCommand)->Result<(),&'static str>{
    let c=verified.command();
    // Called only after stream revocation/inspection. If the local clock is
    // still behind the command, persist nothing; the next poll must inspect
    // again. Never clamp observedAt or mutate an already durable receipt.
    let observed_at=receipt_clock::observation_timestamp(&c.requested_at,Utc::now())?;
    let mut id=c.digest()?;id[6]=(id[6]&0x0f)|0x40;id[8]=(id[8]&0x3f)|0x80;
    let receipt_id=uuid::Uuid::from_bytes(id[..16].try_into().map_err(|_|"HOST_RECEIPT_ID_FAILED")?).to_hyphenated().to_string();
    let fresh=serde_json::json!({"schemaVersion":1,"source":"HOST","receiptId":receipt_id,
      "operationId":c.operation_id,"sessionId":c.session_id,"prepareOperationId":c.prepare_operation_id,
      "handoffBindingId":c.handoff_binding_id,"nativeSessionId":c.native_session_id,"claimId":c.claim_id,
      "pcId":c.pc_id,"userId":c.user_id,"applicationActivationId":c.application_activation_id,
      "commandId":c.command_id,"challenge":c.challenge,"observedAt":observed_at,
      "proof":{"streamTerminated":true,"activeRentalStreams":0,"inspectionDigest":hex::encode(c.digest()?)}});
    let receipt_path=root.join("moonlight").join("server").join("cleanup-receipts").join(format!("{}.dpapi",c.operation_id));
    let receipt:serde_json::Value=if receipt_path.exists(){
      serde_json::from_slice(&provisioning::read_dpapi_secret(&receipt_path).map_err(|_|"HOST_RECEIPT_STORAGE_INVALID")?)
        .map_err(|_|"HOST_RECEIPT_STORAGE_INVALID")?
    }else{
      fs::create_dir_all(receipt_path.parent().ok_or("HOST_RECEIPT_PATH_INVALID")?).map_err(|_|"HOST_RECEIPT_STORAGE_FAILED")?;
      provisioning::write_dpapi_secret(&receipt_path,&serde_json::to_vec(&fresh).map_err(|_|"HOST_RECEIPT_ENCODING_FAILED")?)
        .map_err(|_|"HOST_RECEIPT_STORAGE_FAILED")?; fresh.clone()
    };
    for key in ["operationId","sessionId","prepareOperationId","handoffBindingId","nativeSessionId","claimId","pcId","userId","applicationActivationId","commandId","challenge","proof"]{
      if receipt.get(key)!=fresh.get(key){return Err("HOST_RECEIPT_REPLAY_CONFLICT")}
    }
    let current=context(root)?;if current.binding!=pinned.binding||current.credential!=pinned.credential{return Err("HOST_RECEIPT_BINDING_CHANGED")}
    let endpoint=format!("{}/api/v1/host/rental-cleanup-receipt",provisioning::NODE_CONTROL_PLANE);
    let response=client.post(&endpoint).bearer_auth(&pinned.credential).json(&receipt).send().await.map_err(|_|"HOST_RECEIPT_NETWORK_UNAVAILABLE")?;
    if response.status()!=reqwest::StatusCode::OK||response.url().as_str()!=endpoint{return Err("HOST_RECEIPT_REJECTED")}
    let bytes=response.bytes().await.map_err(|_|"HOST_RECEIPT_RESPONSE_INVALID")?;if bytes.len()>MAX_BODY{return Err("HOST_RECEIPT_RESPONSE_SIZE")}
    let v:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|"HOST_RECEIPT_RESPONSE_INVALID")?;
    if v.get("ok")!=Some(&serde_json::Value::Bool(true))||v.get("operationId").and_then(|x|x.as_str())!=Some(c.operation_id.as_str())
      ||v.get("cleanupComplete").and_then(|x|x.as_bool()).is_none()||v.get("claimReleased").and_then(|x|x.as_bool()).is_none(){return Err("HOST_RECEIPT_RESPONSE_INVALID")}
    let current=context(root)?;if current.binding!=pinned.binding||current.credential!=pinned.credential{return Err("HOST_RECEIPT_BINDING_CHANGED")} Ok(())
}

pub(crate) struct RevokeWorker(Option<tokio::task::JoinHandle<()>>);
impl Drop for RevokeWorker {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}
pub(crate) fn spawn_if_enabled(app: Data<App>, root: PathBuf, enabled: bool) -> RevokeWorker {
    if !enabled {
        return RevokeWorker(None);
    }
    RevokeWorker(Some(tokio::spawn(async move {
        let result = run(app, root).await;
        if let Err(code) = result {
            log::warn!("{code}");
        }
    })))
}
async fn run(app: Data<App>, root: PathBuf) -> Result<(), &'static str> {
    let pinned = context(&root)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "HOST_REVOKE_CLIENT_FAILED")?;
    let mut last_poll_state: Option<&'static str> = None;
    loop {
        match poll(&client, &root, &pinned).await {
            Ok(commands) => {
                if last_poll_state != Some("HOST_REVOKE_POLL_AUTHENTICATED") {
                    log::info!("HOST_REVOKE_POLL_AUTHENTICATED");
                    last_poll_state = Some("HOST_REVOKE_POLL_AUTHENTICATED");
                }
                for command in commands {
                    match app.revoke_backend_rental_stream(&command).await {
                        Ok(_) => if let Err(code)=send_receipt(&client,&root,&pinned,&command).await{log::warn!("{code}");},
                        Err(code) => log::warn!("{code}"),
                    }
                }
            }
            Err(code) => {
                if last_poll_state != Some(code) {
                    log::warn!("{code}");
                }
                last_poll_state = Some(code);
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
#[path = "host_rental_revoke_tests.rs"]
mod tests;
