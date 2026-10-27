use serde::{Deserialize, Serialize};

/// Bound by the Host to an authenticated launch/ticket, never from RouteTelemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedStreamRoute {
    Direct,
    Relay,
}
impl SelectedStreamRoute {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            Some("direct") => Some(Self::Direct),
            Some("relay") => Some(Self::Relay),
            _ => None,
        }
    }
}

/// Host-to-owned-child IPC only. Missing policy defaults to disabled for legacy clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectRecoveryGrant {
    pub authority_expires_at_unix_ms: u64,
    pub max_extra_kbps: u32,
    pub max_extra_packets_per_second: u32,
}
impl DirectRecoveryGrant {
    pub fn for_ticket(route: Option<SelectedStreamRoute>, expires: u64, now: u64) -> Option<Self> {
        (route == Some(SelectedStreamRoute::Direct) && expires > now).then_some(Self {
            authority_expires_at_unix_ms: expires,
            max_extra_kbps: 2000,
            max_extra_packets_per_second: 1200,
        })
    }
    pub fn valid_at(&self, now: u64) -> bool {
        self.authority_expires_at_unix_ms > now
            && (1..=8000).contains(&self.max_extra_kbps)
            && (1..=2000).contains(&self.max_extra_packets_per_second)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_relay_unknown_and_expired_cannot_authorize() {
        for r in [
            None,
            Some("relay"),
            Some("DIRECT"),
            Some("direct "),
            Some("anything"),
        ] {
            assert!(
                DirectRecoveryGrant::for_ticket(SelectedStreamRoute::parse(r), 200, 100).is_none()
            );
        }
        assert!(
            DirectRecoveryGrant::for_ticket(Some(SelectedStreamRoute::Direct), 100, 100).is_none()
        );
    }
    #[test]
    fn old_ipc_defaults_disabled_and_unknown_grant_keys_refused() {
        let json = serde_json::json!({"webrtc":{},"log_level":"Info"});
        let config: crate::ipc::StreamerConfig = serde_json::from_value(json).unwrap();
        assert!(config.direct_recovery.is_none());
        assert!(serde_json::from_value::<DirectRecoveryGrant>(serde_json::json!({"authority_expires_at_unix_ms":200,"max_extra_kbps":100,"max_extra_packets_per_second":100,"route":"direct"})).is_err());
    }
    #[test]
    fn direct_gets_bounded_policy() {
        let p =
            DirectRecoveryGrant::for_ticket(Some(SelectedStreamRoute::Direct), 200, 100).unwrap();
        assert!(p.valid_at(100));
        assert!(!p.valid_at(200));
        let mut p = p;
        p.max_extra_kbps = 8001;
        assert!(!p.valid_at(100));
    }
}
