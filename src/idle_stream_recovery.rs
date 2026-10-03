//! Recovery eligibility only. Never publishes READY, changes billing, or releases
//! a claim. The existing supervisor recovery revalidates the real runtime.
#[derive(Clone, Copy)]
pub struct IdleStreamRecovery<'a> {
    pub phase: &'a str,
    pub reason: Option<&'a str>,
    pub updated_ms: Option<u64>,
    pub now_ms: u64,
    pub processes_ready: bool,
    pub http_ready: bool,
    pub command_pending: bool,
    pub command_busy: bool,
    pub active_stream: bool,
}
pub fn should_recover(s: IdleStreamRecovery<'_>) -> bool {
    let known_abandoned_transition = matches!((s.phase, s.reason),
        ("recovering", Some("route_failure_reconnect")) |
        ("starting", Some("stream_start_requested")));
    known_abandoned_transition && s.processes_ready && s.http_ready &&
        !s.command_pending && !s.command_busy && !s.active_stream &&
        s.updated_ms.is_some_and(|t| t > 0 && s.now_ms.checked_sub(t).is_some_and(|age| age > 60_000))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn abandoned() -> IdleStreamRecovery<'static> { IdleStreamRecovery {
        phase:"starting",reason:Some("stream_start_requested"),updated_ms:Some(100_000),now_ms:160_001,
        processes_ready:true,http_ready:true,command_pending:false,command_busy:false,active_stream:false,
    }}
    #[test] fn reproduces_start_requested_then_native_scope_returns_without_ready() {
        let s=abandoned(); assert!(should_recover(s)); assert_eq!(s.phase,"starting");
    }
    #[test] fn does_not_interrupt_new_or_live_stream_or_commands() {
        assert!(!should_recover(IdleStreamRecovery{active_stream:true,..abandoned()}));
        assert!(!should_recover(IdleStreamRecovery{command_pending:true,..abandoned()}));
        assert!(!should_recover(IdleStreamRecovery{command_busy:true,..abandoned()}));
        assert!(!should_recover(IdleStreamRecovery{now_ms:160_000,..abandoned()}));
    }
    #[test] fn missing_future_or_rolled_back_clock_cannot_trigger() {
        for t in [None,Some(0),Some(200_000)] {assert!(!should_recover(IdleStreamRecovery{updated_ms:t,..abandoned()}));}
    }
    #[test] fn unknown_lifecycle_and_unhealthy_dependencies_fail_closed() {
        for phase in ["ready","failed","stopping","idle"] {assert!(!should_recover(IdleStreamRecovery{phase,..abandoned()}));}
        for reason in [None,Some("bundle_start"),Some("capture_failed")] {assert!(!should_recover(IdleStreamRecovery{reason,..abandoned()}));}
        assert!(!should_recover(IdleStreamRecovery{processes_ready:false,..abandoned()}));
        assert!(!should_recover(IdleStreamRecovery{http_ready:false,..abandoned()}));
    }
    #[test] fn existing_route_recovery_is_preserved() {
        assert!(should_recover(IdleStreamRecovery{phase:"recovering",reason:Some("route_failure_reconnect"),..abandoned()}));
        assert!(!should_recover(IdleStreamRecovery{phase:"starting",reason:Some("route_failure_reconnect"),..abandoned()}));
    }
}
