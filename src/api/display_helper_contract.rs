use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct DynamicDisplayAppliedMode {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) frequency: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct DynamicDisplayResult {
    pub(super) ok: bool,
    pub(super) changed: bool,
    pub(super) restored: bool,
    pub(super) skipped: bool,
    pub(super) reason: String,
    pub(super) applied: Option<DynamicDisplayAppliedMode>,
    pub(super) sunshine_capture_changed: bool,
    pub(super) sunshine_capture_target_changed: bool,
    pub(super) sunshine_capture_display: Option<String>,
    pub(super) sunshine_capture_config_path: Option<String>,
}

/// Successful JSON cannot override a failed process. Runtime resize/project
/// acknowledgements must contain observed dimensions, never request defaults.
/// Stale cleanup may be a safe no-op; a stale mutation request is not success.
pub(super) fn parse_display_helper_result(
    action: &str,
    exit_code: i32,
    stdout: &str,
) -> Result<DynamicDisplayResult, String> {
    if exit_code != 0 {
        // Keep arbitrary helper output out of client-facing errors (it may contain secrets).
        return Err(format!(
            "display helper {action} failed with exit code {exit_code}"
        ));
    }
    let result: DynamicDisplayResult = serde_json::from_str(stdout)
        .map_err(|_| format!("invalid display helper {action} response"))?;
    if !result.ok {
        return Err(format!("display helper {action} reported failure"));
    }
    let mutation = matches!(action, "prepare" | "resize" | "project-display");
    if mutation
        && result
            .reason
            .trim()
            .eq_ignore_ascii_case("superseded_by_newer_session")
    {
        return Err("display request does not own the current display session".to_string());
    }
    if matches!(action, "resize" | "project-display") {
        match &result.applied {
            Some(mode) if mode.width > 0 && mode.height > 0 && mode.frequency > 0 => {}
            _ => return Err("display helper did not confirm an applied display mode".to_string()),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonzero_exit_is_never_success_even_with_ok_json() {
        for status in [1, -1073741502, 2, 255] {
            assert!(
                parse_display_helper_result(
                    "resize",
                    status,
                    r#"{"ok":true,"applied":{"width":1920,"height":1080,"frequency":60}}"#
                )
                .is_err()
            );
        }
    }
    #[test]
    fn stale_mutation_is_rejected_but_stale_restore_is_safe_noop() {
        let stale = r#"{"ok":true,"skipped":true,"reason":"superseded_by_newer_session"}"#;
        for action in ["prepare", "resize", "project-display"] {
            assert!(parse_display_helper_result(action, 0, stale).is_err());
        }
        assert!(parse_display_helper_result("restore", 0, stale).is_ok());
    }
    #[test]
    fn resize_requires_verified_mode_not_requested_defaults() {
        for s in [
            r#"{"ok":true}"#,
            r#"{"ok":true,"skipped":true}"#,
            r#"{"ok":true,"applied":{"width":1920,"height":1080}}"#,
            r#"{"ok":true,"applied":{"width":0,"height":1080,"frequency":60}}"#,
        ] {
            assert!(parse_display_helper_result("resize", 0, s).is_err());
        }
    }
    #[test]
    fn unchanged_verified_mode_remains_valid() {
        let r=parse_display_helper_result("resize",0,
            r#"{"ok":true,"changed":false,"skipped":true,"applied":{"width":1280,"height":720,"frequency":60}}"#).unwrap();
        assert!(r.skipped);
        assert_eq!(r.applied.unwrap().width, 1280);
    }
    #[test]
    fn invalid_or_failed_output_does_not_leak_details() {
        for s in [
            "",
            "secret-from-stderr",
            r#"{"ok":false,"reason":"token=PRIVATE"}"#,
            "{}",
        ] {
            let err = parse_display_helper_result("resize", 0, s).unwrap_err();
            assert!(!err.contains("PRIVATE"));
            assert!(!err.contains("secret-from-stderr"));
        }
    }
}
