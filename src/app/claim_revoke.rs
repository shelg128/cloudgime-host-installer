use super::*;

// Authenticated machine-only cancellation of an unstarted exact claim root.
impl App {
    pub(crate) async fn revoke_unstarted_claim_root(&self, root_id: &str, claim_id: &str, handoff_binding_id: &str) -> Result<(), AppError> {
        {
            let mut states = self.inner.android_native_session_lifecycle.write().await;
            if let Some(state) = states.get_mut(root_id) {
                let binding = state.rental_binding.as_ref().ok_or(AppError::Forbidden)?;
                if state.session_id != root_id || binding.claim_id != claim_id || binding.handoff_binding_id != handoff_binding_id
                    || binding.rental_session_id.is_some() || binding.prepare_operation_id.is_some() {
                    return Err(AppError::Forbidden);
                }
                state.hidden_state_status = "revoking".into();
                state.last_action = "backend_claim_cancelled".into();
                state.last_updated_unix_ms = unix_time_ms_now();
            }
        }
        self.invalidate_android_native_session_artifacts(root_id).await;
        if crate::native_stream_scope::NATIVE_STREAM_SCOPES.stop(root_id).await.is_err() {
            crate::native_stream_scope::NATIVE_STREAM_SCOPES.tombstone_if_unregistered(root_id).await.map_err(|_| AppError::Forbidden)?;
        }
        Ok(())
    }
}
