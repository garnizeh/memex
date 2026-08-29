use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::errors::{MemexError, Result};

/// Atomically writes JSON data to the target file path.
///
/// Workflow:
/// 1. If the target file already exists, verify its JSON validity.
///    If invalid/corrupted, back it up to `<path>.backup`.
/// 2. Ensure all parent directories exist.
/// 3. Serialize `data` to formatted JSON.
/// 4. Write to a temporary file named `<path>.tmp.<pid>`.
/// 5. Flush and sync temporary file contents to disk.
/// 6. Atomically rename the temporary file to `path`.
pub fn atomic_write_json(path: &Path, data: &Value) -> Result<()> {
    if path.exists() {
        let is_valid_json = match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str::<Value>(&content).is_ok(),
            Err(_) => false,
        };

        if !is_valid_json {
            let backup_path = backup_path_for(path);
            tracing::warn!(
                target_path = %path.display(),
                backup_path = %backup_path.display(),
                "Existing configuration file contains invalid JSON. Creating backup."
            );
            fs::copy(path, &backup_path)?;
        }
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let json_bytes = serde_json::to_vec_pretty(data)?;
    let tmp_path = temp_path_for(path);

    let write_result = (|| -> Result<()> {
        let mut tmp_file = fs::File::create(&tmp_path)?;
        tmp_file.write_all(&json_bytes)?;
        tmp_file.write_all(b"\n")?;
        tmp_file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(MemexError::Io(err));
    }

    Ok(())
}

/// Reads and deserializes a JSON configuration file if it exists.
///
/// Returns:
/// - `Ok(None)` if the file does not exist.
/// - `Ok(Some(value))` if the file exists and is valid JSON.
/// - `Err(MemexError)` if I/O fails or JSON parsing fails.
pub fn read_json_value(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let parsed: Value = serde_json::from_str(&content)?;
    Ok(Some(parsed))
}

/// Deep merges `source` into `target`.
///
/// If both keys map to JSON objects, they are recursively merged.
/// Otherwise, `source` replaces the value in `target`.
pub fn merge_json_value(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, val) in source_map {
                merge_json_value(target_map.entry(key.clone()).or_insert(Value::Null), val);
            }
        }
        (target_slot, source_val) => {
            *target_slot = source_val.clone();
        }
    }
}

/// Constructs the `.backup` filepath for a given path.
fn backup_path_for(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".backup");
    PathBuf::from(backup)
}

/// Constructs the `.tmp.<pid>` filepath for a given path.
fn temp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(format!(".tmp.{}", std::process::id()));
    PathBuf::from(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_new_file() {
        let temp_dir = tempdir().unwrap();
        let config_file = temp_dir.path().join("nested").join("config.json");

        let data = json!({
            "name": "memex",
            "version": "0.1.0",
            "enabled": true
        });

        atomic_write_json(&config_file, &data).unwrap();

        assert!(config_file.exists());
        let read_val = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(read_val, data);
    }

    #[test]
    fn test_atomic_write_overwrite_valid_json() {
        let temp_dir = tempdir().unwrap();
        let config_file = temp_dir.path().join("config.json");

        let initial_data = json!({
            "mcpServers": {
                "old": { "command": "old_cmd" }
            }
        });
        atomic_write_json(&config_file, &initial_data).unwrap();

        let updated_data = json!({
            "mcpServers": {
                "memex": { "command": "memex", "args": ["serve", "--mcp"] }
            }
        });
        atomic_write_json(&config_file, &updated_data).unwrap();

        let read_val = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(read_val, updated_data);

        let backup_file = config_file.with_file_name("config.json.backup");
        assert!(!backup_file.exists());
    }

    #[test]
    fn test_atomic_write_corrupted_json_creates_backup() {
        let temp_dir = tempdir().unwrap();
        let config_file = temp_dir.path().join("config.json");
        let backup_file = temp_dir.path().join("config.json.backup");

        let corrupted_content = "{ this is invalid json content !!!";
        fs::write(&config_file, corrupted_content).unwrap();

        let new_data = json!({
            "mcpServers": {
                "memex": { "command": "memex" }
            }
        });

        atomic_write_json(&config_file, &new_data).unwrap();

        // Target file should now have new valid JSON
        let read_val = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(read_val, new_data);

        // Backup file must exist and contain the corrupted content
        assert!(backup_file.exists());
        let backup_content = fs::read_to_string(&backup_file).unwrap();
        assert_eq!(backup_content, corrupted_content);
    }

    #[test]
    fn test_atomic_write_empty_existing_file_creates_backup() {
        let temp_dir = tempdir().unwrap();
        let config_file = temp_dir.path().join("config.json");
        let backup_file = temp_dir.path().join("config.json.backup");

        fs::write(&config_file, "").unwrap();

        let new_data = json!({ "status": "ok" });
        atomic_write_json(&config_file, &new_data).unwrap();

        assert_eq!(read_json_value(&config_file).unwrap().unwrap(), new_data);
        assert!(backup_file.exists());
        assert_eq!(fs::read_to_string(&backup_file).unwrap(), "");
    }

    #[test]
    fn test_read_json_value_nonexistent_and_invalid() {
        let temp_dir = tempdir().unwrap();
        let nonexistent = temp_dir.path().join("missing.json");
        assert_eq!(read_json_value(&nonexistent).unwrap(), None);

        let invalid = temp_dir.path().join("invalid.json");
        fs::write(&invalid, "not json").unwrap();
        assert!(read_json_value(&invalid).is_err());
    }

    #[test]
    fn test_merge_json_value() {
        let mut base = json!({
            "mcpServers": {
                "existing": {
                    "command": "tool1"
                }
            },
            "permissions": {
                "allow": ["read"]
            }
        });

        let update = json!({
            "mcpServers": {
                "memex": {
                    "command": "memex",
                    "args": ["serve", "--mcp"]
                }
            },
            "permissions": {
                "allow": ["read", "mcp__memex__*"]
            }
        });

        merge_json_value(&mut base, &update);

        assert_eq!(
            base,
            json!({
                "mcpServers": {
                    "existing": {
                        "command": "tool1"
                    },
                    "memex": {
                        "command": "memex",
                        "args": ["serve", "--mcp"]
                    }
                },
                "permissions": {
                    "allow": ["read", "mcp__memex__*"]
                }
            })
        );
    }
}
