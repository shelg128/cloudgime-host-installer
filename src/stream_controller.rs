use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

pub const SENTINEL_PRELOGIN_APP_ID: u32 = 1379015545;

/// Resolve only the backend's explicitly requested tunnel profile. Never
/// interpret an unknown profile as the legacy/default Desktop selection.
pub fn resolve_tunnel_launch_app(profile: Option<&str>, app_id: Option<u32>) -> Result<u32> {
    match profile {
        Some("sentinel_prelogin") => match app_id {
            None | Some(SENTINEL_PRELOGIN_APP_ID) => Ok(SENTINEL_PRELOGIN_APP_ID),
            _ => Err(anyhow!("prelogin app/profile conflict")),
        },
        Some(_) => Err(anyhow!("unsupported tunnel stream profile")),
        None => Ok(app_id.unwrap_or_default()), // legacy caller compatibility only
    }
}

/// Ticket-bound launches and Sentinel prelogin must never be retargeted to
/// Desktop (including when the advertised application list has only one item).
/// Legacy non-ticket launches retain the historical Desktop preference.
pub(crate) fn select_stream_app_index(
    requested: u32,
    ticket_bound: bool,
    apps: &[(u32, &str)],
) -> Option<usize> {
    if ticket_bound || requested == SENTINEL_PRELOGIN_APP_ID {
        let mut exact = apps
            .iter()
            .enumerate()
            .filter(|(_, app)| app.0 == requested);
        let (index, _) = exact.next()?;
        return exact.next().is_none().then_some(index);
    }
    apps.iter()
        .position(|(_, title)| title.eq_ignore_ascii_case("desktop"))
        .or_else(|| apps.iter().position(|(id, _)| *id == requested))
        .or_else(|| (apps.len() == 1).then_some(0))
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StreamProfile {
    SentinelPrelogin,
    DesktopRental,
    SteamRental,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StreamApplication {
    pub host_app_id: u64,
    pub sunshine_profile: String,
    pub command: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StreamControllerRegistry {
    pub schema_version: u32,
    pub profiles: BTreeMap<StreamProfile, StreamApplication>,
}

impl StreamControllerRegistry {
    pub fn app_id(&self, profile: StreamProfile) -> Result<u64> {
        let app = self
            .profiles
            .get(&profile)
            .ok_or_else(|| anyhow!("stream profile missing"))?;
        if app.host_app_id == 0 && profile != StreamProfile::DesktopRental {
            return Err(anyhow!("non-desktop profile may not use desktop app id"));
        }
        if app.sunshine_profile.trim().is_empty() || app.command.trim().is_empty() {
            return Err(anyhow!("stream profile incomplete"));
        }
        Ok(app.host_app_id)
    }
}

pub fn read_registry(path: &Path) -> Result<StreamControllerRegistry> {
    let value: StreamControllerRegistry = serde_json::from_slice(&fs::read(path)?)?;
    if value.schema_version != 1 {
        return Err(anyhow!("unsupported stream controller schema"));
    }
    for profile in [
        StreamProfile::SentinelPrelogin,
        StreamProfile::DesktopRental,
        StreamProfile::SteamRental,
    ] {
        value.app_id(profile)?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tunnel_prelogin_request_never_defaults_to_desktop() {
        assert_eq!(
            resolve_tunnel_launch_app(Some("sentinel_prelogin"), None).unwrap(),
            SENTINEL_PRELOGIN_APP_ID
        );
        assert!(resolve_tunnel_launch_app(Some("sentinel_prelogin"), Some(0)).is_err());
        assert!(resolve_tunnel_launch_app(Some("sentinel_prelogin"), Some(881448767)).is_err());
        assert!(resolve_tunnel_launch_app(Some("unknown"), None).is_err());
        assert!(resolve_tunnel_launch_app(Some(""), Some(0)).is_err());
        assert_eq!(resolve_tunnel_launch_app(None, Some(12)).unwrap(), 12);
        assert_eq!(resolve_tunnel_launch_app(None, None).unwrap(), 0);
    }

    #[test]
    fn runtime_prelogin_selects_exact_app_even_with_desktop_first() {
        let apps = [
            (881448767, "Desktop"),
            (SENTINEL_PRELOGIN_APP_ID, "Sentinel"),
        ];
        assert_eq!(
            select_stream_app_index(SENTINEL_PRELOGIN_APP_ID, false, &apps),
            Some(1)
        );
        assert_eq!(
            select_stream_app_index(SENTINEL_PRELOGIN_APP_ID, true, &apps),
            Some(1)
        );
        assert_eq!(select_stream_app_index(881448767, true, &apps), Some(0));
    }

    #[test]
    fn ticket_and_prelogin_never_fall_back_or_accept_duplicate_ids() {
        for ticket_bound in [false, true] {
            assert_eq!(
                select_stream_app_index(SENTINEL_PRELOGIN_APP_ID, ticket_bound, &[]),
                None
            );
            assert_eq!(
                select_stream_app_index(
                    SENTINEL_PRELOGIN_APP_ID,
                    ticket_bound,
                    &[(881448767, "Desktop")]
                ),
                None
            );
            assert_eq!(
                select_stream_app_index(
                    SENTINEL_PRELOGIN_APP_ID,
                    ticket_bound,
                    &[
                        (SENTINEL_PRELOGIN_APP_ID, "Sentinel"),
                        (SENTINEL_PRELOGIN_APP_ID, "other")
                    ]
                ),
                None
            );
        }
        assert_eq!(
            select_stream_app_index(12, true, &[(881448767, "Desktop")]),
            None
        );
        assert_eq!(
            select_stream_app_index(12, true, &[(12, "A"), (12, "B")]),
            None
        );
        assert_eq!(
            select_stream_app_index(12, true, &[(881448767, "Desktop"), (12, "Game")]),
            Some(1)
        );
    }

    #[test]
    fn legacy_non_ticket_desktop_selection_is_unchanged() {
        assert_eq!(
            select_stream_app_index(12, false, &[(881448767, "Desktop"), (12, "Game")]),
            Some(0)
        );
        assert_eq!(
            select_stream_app_index(12, false, &[(12, "Game"), (13, "other")]),
            Some(0)
        );
        assert_eq!(select_stream_app_index(12, false, &[(13, "only")]), Some(0));
        assert_eq!(
            select_stream_app_index(12, false, &[(13, "A"), (14, "B")]),
            None
        );
    }

    #[test]
    fn stream_runtime_uses_the_strict_selector_before_starting_streamer() {
        let source = include_str!("api/stream.rs");
        let selection = source
            .find("let selected_app_index = crate::stream_controller::select_stream_app_index(")
            .unwrap();
        let block = &source
            [selection..source[selection..].find("let effective_app_id").unwrap() + selection];
        assert!(block.contains("android_native_stream_ticket_record.is_some()"));
        assert!(block.contains("let _ = session.close(None).await;"));
        assert!(!block.contains(".or(requested_app_index)"));
        assert!(
            selection
                < source
                    .find("// -- Starting stage: launch streamer")
                    .unwrap()
        );
    }

    #[test]
    fn prelogin_never_resolves_desktop() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            StreamProfile::SentinelPrelogin,
            StreamApplication {
                host_app_id: 1379015545,
                sunshine_profile: "CloudGime Sentinel Login".into(),
                command: "sentinel".into(),
            },
        );
        profiles.insert(
            StreamProfile::DesktopRental,
            StreamApplication {
                host_app_id: 881448767,
                sunshine_profile: "Desktop".into(),
                command: "desktop".into(),
            },
        );
        profiles.insert(
            StreamProfile::SteamRental,
            StreamApplication {
                host_app_id: 1093255277,
                sunshine_profile: "Steam Big Picture".into(),
                command: "steam".into(),
            },
        );
        let registry = StreamControllerRegistry {
            schema_version: 1,
            profiles,
        };
        assert_eq!(
            registry.app_id(StreamProfile::SentinelPrelogin).unwrap(),
            1379015545
        );
        assert_ne!(
            registry.app_id(StreamProfile::SentinelPrelogin).unwrap(),
            registry.app_id(StreamProfile::DesktopRental).unwrap()
        );
    }
}
