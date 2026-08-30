use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::errors::Result;
use crate::installer::config_writer::{atomic_write_json, merge_json_value, read_json_value};
use crate::installer::targets::{AgentTarget, DetectionResult, InstallOptions};

/// Agent target for Zed editor.
#[derive(Debug, Default, Clone, Copy)]
pub struct ZedTarget;

impl ZedTarget {
    /// Resolves the Zed `settings.json` path.
    pub fn resolve_settings_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_zed_settings = workspace.join(".zed").join("settings.json");
            if local_zed_settings.exists() || workspace.join(".zed").exists() {
                return Ok(local_zed_settings);
            }
        }

        let home = options.resolve_home_dir()?;
        let linux_config = home.join(".config").join("zed").join("settings.json");
        let mac_config = home
            .join("Library")
            .join("Application Support")
            .join("Zed")
            .join("settings.json");

        if mac_config.exists() {
            Ok(mac_config)
        } else {
            // Default canonical path: ~/.config/zed/settings.json
            Ok(linux_config)
        }
    }
}

/// Constructs Zed context server configuration snippet.
pub fn make_zed_context_server_config(command: &str, args: &[String]) -> Value {
    serde_json::json!({
        "context_servers": {
            "memex": {
                "command": command,
                "args": args
            }
        }
    })
}

/// Checks if `context_servers.memex` exists in Zed's `settings.json`.
pub fn is_memex_in_zed_config(config_path: &Path) -> bool {
    if let Ok(Some(value)) = read_json_value(config_path)
        && let Some(context_servers) = value.get("context_servers").and_then(|v| v.as_object())
    {
        return context_servers.contains_key("memex");
    }
    false
}

/// Injects or updates `context_servers.memex` into Zed's `settings.json`.
pub fn inject_zed_server_config(config_path: &Path, command: &str, args: &[String]) -> Result<()> {
    let snippet = make_zed_context_server_config(command, args);
    let mut config = read_json_value(config_path)?.unwrap_or_else(|| serde_json::json!({}));
    merge_json_value(&mut config, &snippet);
    atomic_write_json(config_path, &config)?;
    Ok(())
}

impl AgentTarget for ZedTarget {
    fn id(&self) -> &'static str {
        "zed"
    }

    fn name(&self) -> &'static str {
        "Zed"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        let target_config = self.resolve_settings_path(options)?;

        if let Some(ref workspace) = options.workspace_dir {
            let local_zed_dir = workspace.join(".zed");
            let local_zed_settings = local_zed_dir.join("settings.json");
            if local_zed_dir.exists() || local_zed_settings.exists() {
                let is_configured = is_memex_in_zed_config(&local_zed_settings);
                return Ok(DetectionResult::Detected {
                    config_path: local_zed_settings,
                    is_configured,
                    details: Some("Zed workspace environment detected".to_string()),
                });
            }
        }

        let home = options.resolve_home_dir()?;
        let config_zed_dir = home.join(".config").join("zed");
        let mac_zed_dir = home.join("Library").join("Application Support").join("Zed");

        if target_config.exists() || config_zed_dir.exists() || mac_zed_dir.exists() {
            let is_configured = is_memex_in_zed_config(&target_config);
            Ok(DetectionResult::Detected {
                config_path: target_config,
                is_configured,
                details: Some("Zed environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let config_path = self.resolve_settings_path(options)?;
        inject_zed_server_config(&config_path, &options.command, &options.args)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_zed_detection_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let target = ZedTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());
        assert!(!res.is_configured());
    }

    #[test]
    fn test_zed_global_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = ZedTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Create ~/.config/zed directory
        std::fs::create_dir_all(temp_dir.path().join(".config").join("zed")).unwrap();

        let res = target.detect(&opts).unwrap();
        assert!(res.is_detected());
        assert!(!res.is_configured());
        assert_eq!(
            res.config_path(),
            Some(
                temp_dir
                    .path()
                    .join(".config")
                    .join("zed")
                    .join("settings.json")
                    .as_path()
            )
        );

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify config content
        let config_path = temp_dir
            .path()
            .join(".config")
            .join("zed")
            .join("settings.json");
        let parsed = read_json_value(&config_path).unwrap().unwrap();
        assert_eq!(
            parsed["context_servers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );
        assert_eq!(
            parsed["context_servers"]["memex"]["args"][0]
                .as_str()
                .unwrap(),
            "serve"
        );
        assert_eq!(
            parsed["context_servers"]["memex"]["args"][1]
                .as_str()
                .unwrap(),
            "--mcp"
        );
    }

    #[test]
    fn test_zed_preserves_existing_settings() {
        let temp_dir = TempDir::new().unwrap();
        let target = ZedTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let config_path = temp_dir
            .path()
            .join(".config")
            .join("zed")
            .join("settings.json");
        atomic_write_json(
            &config_path,
            &serde_json::json!({
                "theme": "One Dark",
                "buffer_font_size": 14,
                "context_servers": {
                    "other_server": {
                        "command": "other",
                        "args": ["run"]
                    }
                }
            }),
        )
        .unwrap();

        target.install(&opts).unwrap();

        let parsed = read_json_value(&config_path).unwrap().unwrap();
        assert_eq!(parsed["theme"].as_str().unwrap(), "One Dark");
        assert_eq!(parsed["buffer_font_size"].as_i64().unwrap(), 14);
        assert_eq!(
            parsed["context_servers"]["other_server"]["command"]
                .as_str()
                .unwrap(),
            "other"
        );
        assert_eq!(
            parsed["context_servers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );
    }
}
