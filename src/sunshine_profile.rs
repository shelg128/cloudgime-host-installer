use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value};
use std::{fs, path::Path};

const SENTINEL_PROFILE_NAME: &str = "CloudGime Sentinel Login";
const SENTINEL_EXECUTABLE: &str = "Sentinel Launcher.exe";

/// Reconciles only the CloudGime-owned Sentinel profile. Other operator/game
/// profiles and unknown fields are preserved. A corrupt or duplicate profile
/// fails closed rather than rewriting an ambiguous apps.json.
pub fn reconcile_sentinel_profile_file(path: &Path) -> Result<bool> {
    let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut root: Value = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let changed = reconcile_sentinel_profile(&mut root)?;
    if !changed {
        return Ok(false);
    }
    let parent = path.parent().ok_or_else(|| anyhow!("apps.json has no parent"))?;
    let temp = parent.join(format!(".apps.cloudgime-{}.tmp", uuid::Uuid::new_v4()));
    let encoded = serde_json::to_vec_pretty(&root)?;
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("failed to create {}", temp.display()))?;
        use std::io::Write as _;
        file.write_all(&encoded)
            .with_context(|| format!("failed to write {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush {}", temp.display()))?;
        drop(file);
        replace_file(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok(true)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| format!("failed to replace {}", destination.display()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination).with_context(|| format!("failed to replace {}", destination.display()))
}

pub fn reconcile_sentinel_profile(root: &mut Value) -> Result<bool> {
    let object = root.as_object_mut().ok_or_else(|| anyhow!("apps root must be an object"))?;
    let apps = object
        .get_mut("apps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("apps must be an array"))?;
    let matches: Vec<usize> = apps
        .iter()
        .enumerate()
        .filter_map(|(index, app)| {
            (app.get("name").and_then(Value::as_str) == Some(SENTINEL_PROFILE_NAME))
                .then_some(index)
        })
        .collect();
    if matches.len() > 1 {
        bail!("duplicate CloudGime Sentinel Login profiles");
    }
    if matches.is_empty() {
        apps.push(Value::Object(sentinel_profile()));
        return Ok(true);
    }
    let app = apps[matches[0]]
        .as_object_mut()
        .ok_or_else(|| anyhow!("Sentinel profile must be an object"))?;
    let mut changed = false;
    if !app.get("cmd").and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty()) {
        changed |= set(app, "cmd", Value::String(default_sentinel_command()));
    }
    if !app
        .get("working-dir")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        changed |= set(app, "working-dir", Value::String(default_sentinel_working_dir()));
    }
    changed |= set(app, "auto-detach", Value::Bool(true));
    // With auto-detach enabled, Sunshine recognizes the second single-instance
    // process as a launcher and keeps the stream while the existing Sentinel UI
    // remains the captured desktop. wait-all then tracks detached children.
    changed |= set(app, "wait-all", Value::Bool(true));
    if !app.contains_key("image-path") {
        app.insert("image-path".to_string(), Value::String("desktop.png".to_string()));
        changed = true;
    }
    Ok(changed)
}

fn sentinel_profile() -> Map<String, Value> {
    Map::from_iter([
        ("name".to_string(), Value::String(SENTINEL_PROFILE_NAME.to_string())),
        ("cmd".to_string(), Value::String(default_sentinel_command())),
        ("working-dir".to_string(), Value::String(default_sentinel_working_dir())),
        ("auto-detach".to_string(), Value::Bool(true)),
        ("wait-all".to_string(), Value::Bool(true)),
        ("image-path".to_string(), Value::String("desktop.png".to_string())),
    ])
}

fn default_sentinel_working_dir() -> String {
    #[cfg(windows)]
    {
        let program_files = std::env::var("ProgramFiles")
            .unwrap_or_else(|_| r"C:\Program Files".to_string());
        return Path::new(&program_files)
            .join("Sentinel Launcher")
            .to_string_lossy()
            .into_owned();
    }
    #[cfg(not(windows))]
    {
        "Sentinel Launcher".to_string()
    }
}

fn default_sentinel_command() -> String {
    Path::new(&default_sentinel_working_dir())
        .join(SENTINEL_EXECUTABLE)
        .to_string_lossy()
        .into_owned()
}

fn set(object: &mut Map<String, Value>, key: &str, desired: Value) -> bool {
    if object.get(key) == Some(&desired) {
        return false;
    }
    object.insert(key.to_string(), desired);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_existing_profile_and_preserves_other_apps_and_fields() {
        let mut value = serde_json::json!({
            "env": {"KEEP":"1"},
            "apps": [
                {"name":"Desktop","image-path":"desktop.png","custom":"keep"},
                {"name":"CloudGime Sentinel Login","cmd":"old","working-dir":"old","auto-detach":false,"wait-all":true,"image-path":"custom.png","custom":"keep"}
            ],
            "customRoot": true
        });
        assert!(reconcile_sentinel_profile(&mut value).unwrap());
        let sentinel = &value["apps"][1];
        assert_eq!(sentinel["cmd"], "old", "existing operator path is preserved");
        assert_eq!(sentinel["working-dir"], "old", "existing working dir is preserved");
        assert_eq!(sentinel["auto-detach"], true);
        assert_eq!(sentinel["wait-all"], true);
        assert_eq!(sentinel["image-path"], "custom.png");
        assert_eq!(sentinel["custom"], "keep");
        assert_eq!(value["apps"][0]["custom"], "keep");
        assert_eq!(value["customRoot"], true);
        assert!(!reconcile_sentinel_profile(&mut value).unwrap());
    }

    #[test]
    fn adds_missing_profile_and_rejects_ambiguous_or_invalid_documents() {
        let mut missing = serde_json::json!({"env":{},"apps":[{"name":"Desktop"}]});
        assert!(reconcile_sentinel_profile(&mut missing).unwrap());
        assert_eq!(missing["apps"].as_array().unwrap().len(), 2);
        let mut duplicate = serde_json::json!({"apps":[{"name":SENTINEL_PROFILE_NAME},{"name":SENTINEL_PROFILE_NAME}]});
        assert!(reconcile_sentinel_profile(&mut duplicate).is_err());
        assert!(reconcile_sentinel_profile(&mut serde_json::json!([])).is_err());
        assert!(reconcile_sentinel_profile(&mut serde_json::json!({"apps":{}})).is_err());
    }

    #[test]
    fn file_repair_is_atomic_and_idempotent() {
        let root = std::env::temp_dir().join(format!("cg-sunshine-profile-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("apps.json");
        fs::write(&path, br#"{"env":{},"apps":[{"name":"Desktop"}]}"#).unwrap();
        assert!(reconcile_sentinel_profile_file(&path).unwrap());
        assert!(!reconcile_sentinel_profile_file(&path).unwrap());
        let parsed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(parsed["apps"][1]["auto-detach"], true);
        assert_eq!(parsed["apps"][1]["wait-all"], true);
        assert!(
            fs::read_dir(&root)
                .unwrap()
                .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".apps.cloudgime-"))
        );
        let _ = fs::remove_dir_all(root);
    }
}
