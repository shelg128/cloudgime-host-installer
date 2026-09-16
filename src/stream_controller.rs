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
