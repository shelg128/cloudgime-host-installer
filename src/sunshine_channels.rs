use anyhow::{Context, Result, anyhow, bail};
use std::{fs, io::Write as _, path::Path};

const REQUIRED_CHANNELS: u32 = 3;

/// Ensures Sunshine can serve the supported same-claim device count. Unknown
/// settings and comments are preserved. Ambiguous or malformed `channels`
/// settings fail closed instead of rewriting the operator configuration.
pub fn reconcile_sunshine_channels_file(path: &Path) -> Result<bool> {
    let input =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let (next, changed) = reconcile_sunshine_channels(&input)?;
    if !changed {
        return Ok(false);
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Sunshine config has no parent"))?;
    let temp = parent.join(format!(".sunshine.cloudgime-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("failed to create {}", temp.display()))?;
        file.write_all(next.as_bytes())
            .with_context(|| format!("failed to write {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush {}", temp.display()))?;
        drop(file);
        replace_file(&temp, path)
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
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to replace {}", destination.display()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)
        .with_context(|| format!("failed to replace {}", destination.display()))
}

pub fn reconcile_sunshine_channels(input: &str) -> Result<(String, bool)> {
    if input.contains('\0') {
        bail!("Sunshine config contains NUL");
    }

    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = input.replace("\r\n", "\n");
    let mut seen = false;
    let mut changed = false;
    let mut lines = Vec::new();

    for line in normalized.split('\n') {
        let (bom, content) = match line.strip_prefix('\u{feff}') {
            Some(content) => ("\u{feff}", content),
            None => ("", line),
        };
        let trimmed = content.trim();
        if trimmed.starts_with('#') || trimmed.starts_with(';') || !trimmed.contains('=') {
            lines.push(line.to_string());
            continue;
        }
        let (key, raw_value) = trimmed
            .split_once('=')
            .expect("contains('=') was checked above");
        if !key.trim().eq_ignore_ascii_case("channels") {
            lines.push(line.to_string());
            continue;
        }
        if seen {
            bail!("duplicate Sunshine channels setting");
        }
        seen = true;
        let value = raw_value
            .trim()
            .parse::<u32>()
            .context("invalid Sunshine channels value")?;
        if value != REQUIRED_CHANNELS || trimmed != "channels = 3" {
            changed = true;
        }
        let indent = &content[..content.len() - content.trim_start().len()];
        lines.push(format!("{bom}{indent}channels = {REQUIRED_CHANNELS}"));
    }

    if !seen {
        let insertion = if input.starts_with('\u{feff}') {
            "\u{feff}channels = 3"
        } else {
            "channels = 3"
        };
        if input.starts_with('\u{feff}') {
            if let Some(first) = lines.first_mut() {
                *first = first.trim_start_matches('\u{feff}').to_string();
            }
        }
        lines.insert(0, insertion.to_string());
        changed = true;
    }
    Ok((lines.join(newline), changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_updates_preserves_content_and_is_idempotent() {
        let (inserted, changed) = reconcile_sunshine_channels("port = 49000\n# keep\n").unwrap();
        assert!(changed);
        assert_eq!(inserted, "channels = 3\nport = 49000\n# keep\n");
        let (same, changed) = reconcile_sunshine_channels(&inserted).unwrap();
        assert!(!changed);
        assert_eq!(same, inserted);

        let (updated, changed) =
            reconcile_sunshine_channels("\u{feff}channels=1\r\nport = 1\r\n").unwrap();
        assert!(changed);
        assert_eq!(updated, "\u{feff}channels = 3\r\nport = 1\r\n");
    }

    #[test]
    fn duplicate_invalid_and_nul_values_fail_closed() {
        assert!(reconcile_sunshine_channels("channels=1\nchannels=3\n").is_err());
        assert!(reconcile_sunshine_channels("channels=oops\n").is_err());
        assert!(reconcile_sunshine_channels("port=1\0channels=1\n").is_err());
    }

    #[test]
    fn file_repair_is_atomic_and_does_not_rewrite_invalid_config() {
        let root =
            std::env::temp_dir().join(format!("cg-sunshine-channels-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("sunshine.conf");
        fs::write(&path, b"port = 49000\ncustom = keep\n").unwrap();
        assert!(reconcile_sunshine_channels_file(&path).unwrap());
        assert!(!reconcile_sunshine_channels_file(&path).unwrap());
        let repaired = fs::read_to_string(&path).unwrap();
        assert!(repaired.starts_with("channels = 3\n"));
        assert!(repaired.contains("custom = keep\n"));
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sunshine.cloudgime-")
        }));

        fs::write(&path, b"channels=1\nchannels=2\n").unwrap();
        let before = fs::read(&path).unwrap();
        assert!(reconcile_sunshine_channels_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let _ = fs::remove_dir_all(root);
    }
}
