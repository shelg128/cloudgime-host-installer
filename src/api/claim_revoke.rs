use actix_web::{post, web::{Data,Json}, HttpRequest, HttpResponse};
use serde::Deserialize;
use crate::app::{App,AppError};

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct RevokeUnstartedClaimRequest { root_native_session_id:String, claim_id:String, handoff_binding_id:String }
fn bound(v:&str,max:usize)->bool{!v.is_empty()&&v.len()<=max&&v==v.trim()&&!v.chars().any(char::is_control)}
#[post("/android-native/revoke-unstarted-claim")]
pub(crate) async fn post_revoke_unstarted_claim(app:Data<App>,req:HttpRequest,Json(x):Json<RevokeUnstartedClaimRequest>)->Result<HttpResponse,AppError>{
    super::android_native::authorize_host_tunnel(&req,super::android_native::installed_host_runtime_credential)?;
    if !bound(&x.root_native_session_id,256)||!bound(&x.claim_id,256)||uuid::Uuid::parse_str(&x.handoff_binding_id).is_err(){return Err(AppError::BadRequest)}
    app.revoke_unstarted_claim_root(&x.root_native_session_id,&x.claim_id,&x.handoff_binding_id).await?;
    Ok(HttpResponse::Ok().insert_header(("Cache-Control","no-store")).json(serde_json::json!({"ok":true,"revoked":true})))
}
