//! Receipt observation is the real Host clock, never the command time or a clamped future time.
use chrono::{DateTime, SecondsFormat, Utc};

pub(super) fn observation_timestamp(
    requested_at: &str,
    now: DateTime<Utc>,
) -> Result<String, &'static str> {
    let requested = DateTime::parse_from_rfc3339(requested_at)
        .map_err(|_| "HOST_RECEIPT_COMMAND_TIME_INVALID")?;
    let encoded = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let observed = DateTime::parse_from_rfc3339(&encoded)
        .map_err(|_| "HOST_RECEIPT_OBSERVATION_TIME_INVALID")?;
    if observed < requested {
        return Err("HOST_RECEIPT_CLOCK_BEHIND_COMMAND");
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    #[test]
    fn measured_270ms_clock_skew_cannot_create_a_permanently_rejected_receipt() {
        assert_eq!(
            observation_timestamp("2026-10-01T10:14:37.056Z", at("2026-10-01T10:14:36.786Z")),
            Err("HOST_RECEIPT_CLOCK_BEHIND_COMMAND")
        );
    }
    #[test]
    fn next_poll_uses_real_observation_time_not_clamped_server_time() {
        assert_eq!(
            observation_timestamp("2026-10-01T10:14:37.056Z", at("2026-10-01T10:14:41.786Z"))
                .unwrap(),
            "2026-10-01T10:14:41.786Z"
        );
    }
    #[test]
    fn exact_boundary_is_valid() {
        assert_eq!(
            observation_timestamp("2026-10-01T10:14:37.056Z", at("2026-10-01T10:14:37.056Z"))
                .unwrap(),
            "2026-10-01T10:14:37.056Z"
        );
    }
    #[test]
    fn serialized_millisecond_precision_cannot_move_observation_before_command() {
        assert_eq!(
            observation_timestamp(
                "2026-10-01T10:14:37.056900Z",
                at("2026-10-01T10:14:37.056950Z")
            ),
            Err("HOST_RECEIPT_CLOCK_BEHIND_COMMAND")
        );
    }
    #[test]
    fn malformed_command_time_is_rejected() {
        assert_eq!(
            observation_timestamp("invalid", at("2026-10-01T10:14:37.056Z")),
            Err("HOST_RECEIPT_COMMAND_TIME_INVALID")
        );
    }
    #[test]
    fn backward_clock_step_cannot_forge_later_observation() {
        assert_eq!(
            observation_timestamp("2026-10-01T10:14:37.056Z", at("2026-10-01T10:00:00.000Z")),
            Err("HOST_RECEIPT_CLOCK_BEHIND_COMMAND")
        );
    }
}
