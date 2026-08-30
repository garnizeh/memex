use std::path::{Path, PathBuf};

use crate::errors::{MemexError, Result};
use crate::installer::config_writer::{atomic_write_json, read_jsonc_value};
use crate::installer::targets::{
    AgentTarget, DetectionResult, InstallOptions, inject_mcp_server_config, is_memex_in_mcp_config,
};

/// Agent target for Antigravity IDE.
#[derive(Debug, Default, Clone, Copy)]
pub struct AntigravityTarget;

impl AntigravityTarget {
    /// Resolves the MCP configuration path for Antigravity IDE.
    pub fn resolve_config_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_agents_dir = workspace.join(".agents");
            let local_config = local_agents_dir.join("mcp_config.json");
            return Ok(local_config);
        }

        let home = options.resolve_home_dir()?;
        let gemini_dir = home.join(".gemini");
        let ide_config = gemini_dir.join("antigravity-ide").join("mcp_config.json");
        let global_config = gemini_dir.join("config").join("mcp_config.json");

        if global_config.exists() {
            // Canonical global installation
            Ok(global_config)
        } else if ide_config.exists() {
            // Legacy supported installation
            Ok(ide_config)
        } else if gemini_dir.join("config").exists() {
            Ok(global_config)
        } else if gemini_dir.join("antigravity-ide").exists() {
            Ok(ide_config)
        } else {
            // Default to canonical global path: ~/.gemini/config/mcp_config.json
            Ok(global_config)
        }
    }

    /// Resolves the lifecycle hooks configuration path for Antigravity IDE.
    pub fn resolve_hooks_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_agents_dir = workspace.join(".agents");
            let local_hooks = local_agents_dir.join("hooks.json");
            return Ok(local_hooks);
        }

        let home = options.resolve_home_dir()?;
        let gemini_dir = home.join(".gemini");
        let ide_hooks = gemini_dir.join("antigravity-ide").join("hooks.json");
        let global_hooks = gemini_dir.join("config").join("hooks.json");

        if global_hooks.exists() {
            Ok(global_hooks)
        } else if ide_hooks.exists() {
            Ok(ide_hooks)
        } else if gemini_dir.join("config").exists() {
            Ok(global_hooks)
        } else if gemini_dir.join("antigravity-ide").exists() {
            Ok(ide_hooks)
        } else {
            Ok(global_hooks)
        }
    }
}

/// Helper function to safely inject the Memex PreInvocation hook into Antigravity's `hooks.json`.
pub fn inject_antigravity_hooks(hooks_path: &Path) -> Result<()> {
    let mut root = match read_jsonc_value(hooks_path) {
        Ok(Some(val)) => val,
        Ok(None) => serde_json::json!({}),
        Err(MemexError::Serialization(_)) => serde_json::json!({}),
        Err(err) => return Err(err),
    };

    if !root.is_object() {
        root = serde_json::json!({});
    }

    let root_obj = root.as_object_mut().expect("hooks root must be an object");
    let memex_entry = root_obj
        .entry("memex")
        .or_insert_with(|| serde_json::json!({}));

    if !memex_entry.is_object() {
        *memex_entry = serde_json::json!({});
    }

    let memex_obj = memex_entry
        .as_object_mut()
        .expect("memex hook must be an object");
    let pre_invocation = memex_obj
        .entry("PreInvocation")
        .or_insert_with(|| serde_json::json!([]));

    if !pre_invocation.is_array() {
        *pre_invocation = serde_json::json!([]);
    }

    let pre_inv_arr = pre_invocation
        .as_array_mut()
        .expect("PreInvocation must be an array");
    let hook_item = serde_json::json!({
        "type": "command",
        "command": "memex prompt-hook"
    });

    let already_present = pre_inv_arr
        .iter()
        .any(|item| item.get("command").and_then(|c| c.as_str()) == Some("memex prompt-hook"));

    if !already_present {
        pre_inv_arr.push(hook_item);
    }

    atomic_write_json(hooks_path, &root)?;
    Ok(())
}

impl AgentTarget for AntigravityTarget {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn name(&self) -> &'static str {
        "Antigravity IDE"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_agents_dir = workspace.join(".agents");
            let local_config = local_agents_dir.join("mcp_config.json");
            if local_agents_dir.exists() || local_config.exists() {
                let is_configured = is_memex_in_mcp_config(&local_config);
                return Ok(DetectionResult::Detected {
                    config_path: local_config,
                    is_configured,
                    details: Some("Antigravity IDE workspace environment detected".to_string()),
                });
            }
        }

        let home = options.resolve_home_dir()?;
        let gemini_dir = home.join(".gemini");
        let config_path = self.resolve_config_path(options)?;

        let antigravity_ide_dir = gemini_dir.join("antigravity-ide");
        let gemini_config_dir = gemini_dir.join("config");

        if config_path.exists()
            || antigravity_ide_dir.exists()
            || gemini_config_dir.exists()
            || gemini_dir.exists()
        {
            let is_configured = is_memex_in_mcp_config(&config_path);
            Ok(DetectionResult::Detected {
                config_path,
                is_configured,
                details: Some("Antigravity IDE environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let config_path = self.resolve_config_path(options)?;
        inject_mcp_server_config(&config_path, &options.command, &options.args)?;

        let hooks_path = self.resolve_hooks_path(options)?;
        inject_antigravity_hooks(&hooks_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::config_writer::read_json_value;
    use tempfile::TempDir;

    #[test]
    fn test_antigravity_detection_and_installation_ide_dir() {
        let temp_dir = TempDir::new().unwrap();
        let target = AntigravityTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());

        // Create ~/.gemini/antigravity-ide directory (legacy path)
        std::fs::create_dir_all(temp_dir.path().join(".gemini").join("antigravity-ide")).unwrap();

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());
        assert_eq!(
            res_detected.config_path(),
            Some(
                temp_dir
                    .path()
                    .join(".gemini")
                    .join("antigravity-ide")
                    .join("mcp_config.json")
                    .as_path()
            )
        );

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify config content
        let config_file = temp_dir
            .path()
            .join(".gemini")
            .join("antigravity-ide")
            .join("mcp_config.json");
        let parsed = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );

        // Verify hooks content
        let hooks_file = temp_dir
            .path()
            .join(".gemini")
            .join("antigravity-ide")
            .join("hooks.json");
        assert!(hooks_file.exists());
        let parsed_hooks = read_json_value(&hooks_file).unwrap().unwrap();
        let pre_inv = parsed_hooks["memex"]["PreInvocation"].as_array().unwrap();
        assert_eq!(pre_inv[0]["command"].as_str().unwrap(), "memex prompt-hook");
    }

    #[test]
    fn test_antigravity_detection_and_installation_global_config_dir() {
        let temp_dir = TempDir::new().unwrap();
        let target = AntigravityTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Create ~/.gemini/config directory (canonical path)
        std::fs::create_dir_all(temp_dir.path().join(".gemini").join("config")).unwrap();

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());
        assert_eq!(
            res_detected.config_path(),
            Some(
                temp_dir
                    .path()
                    .join(".gemini")
                    .join("config")
                    .join("mcp_config.json")
                    .as_path()
            )
        );

        target.install(&opts).unwrap();

        let config_file = temp_dir
            .path()
            .join(".gemini")
            .join("config")
            .join("mcp_config.json");
        let parsed = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );

        let hooks_file = temp_dir
            .path()
            .join(".gemini")
            .join("config")
            .join("hooks.json");
        assert!(hooks_file.exists());
        let parsed_hooks = read_json_value(&hooks_file).unwrap().unwrap();
        assert_eq!(
            parsed_hooks["memex"]["PreInvocation"][0]["command"]
                .as_str()
                .unwrap(),
            "memex prompt-hook"
        );
    }

    #[test]
    fn test_antigravity_workspace_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir_all(workspace.join(".agents")).unwrap();

        let target = AntigravityTarget;
        let opts = InstallOptions::new().with_workspace_dir(&workspace);

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());
        assert_eq!(
            res_detected.config_path(),
            Some(workspace.join(".agents").join("mcp_config.json").as_path())
        );

        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        let config_file = workspace.join(".agents").join("mcp_config.json");
        let parsed = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );

        let hooks_file = workspace.join(".agents").join("hooks.json");
        assert!(hooks_file.exists());
        let parsed_hooks = read_json_value(&hooks_file).unwrap().unwrap();
        assert_eq!(
            parsed_hooks["memex"]["PreInvocation"][0]["command"]
                .as_str()
                .unwrap(),
            "memex prompt-hook"
        );
    }

    #[test]
    fn test_antigravity_hooks_idempotency_and_preservation() {
        let temp_dir = TempDir::new().unwrap();
        let hooks_file = temp_dir.path().join("hooks.json");

        // Pre-create hooks.json with other existing hook
        atomic_write_json(
            &hooks_file,
            &serde_json::json!({
                "other_tool": {
                    "PostToolUse": [
                        { "type": "command", "command": "other-check" }
                    ]
                }
            }),
        )
        .unwrap();

        // Inject twice
        inject_antigravity_hooks(&hooks_file).unwrap();
        inject_antigravity_hooks(&hooks_file).unwrap();

        let parsed = read_json_value(&hooks_file).unwrap().unwrap();
        assert!(parsed["other_tool"]["PostToolUse"].is_array());
        let pre_inv = parsed["memex"]["PreInvocation"].as_array().unwrap();
        assert_eq!(pre_inv.len(), 1);
        assert_eq!(pre_inv[0]["command"].as_str().unwrap(), "memex prompt-hook");
    }

    #[test]
    fn test_antigravity_hooks_with_jsonc_and_corrupted_json() {
        let temp_dir = TempDir::new().unwrap();
        let hooks_file = temp_dir.path().join("hooks.json");

        // 1. JSON with comments (JSONC)
        let jsonc_content = r#"{
            // existing hook configuration
            "pre_existing": {
                "PreInvocation": [
                    { "type": "command", "command": "echo test" }
                ]
            }
        }"#;
        std::fs::write(&hooks_file, jsonc_content).unwrap();

        inject_antigravity_hooks(&hooks_file).unwrap();
        let parsed = read_jsonc_value(&hooks_file).unwrap().unwrap();
        assert!(parsed["pre_existing"]["PreInvocation"].is_array());
        assert_eq!(
            parsed["memex"]["PreInvocation"][0]["command"]
                .as_str()
                .unwrap(),
            "memex prompt-hook"
        );

        // 2. Corrupted JSON file falls back to empty object and creates backup
        let corrupted_file = temp_dir.path().join("corrupted_hooks.json");
        std::fs::write(&corrupted_file, "{ invalid json content").unwrap();

        inject_antigravity_hooks(&corrupted_file).unwrap();
        assert!(temp_dir.path().join("corrupted_hooks.json.backup").exists());
        let parsed_corrupted = read_json_value(&corrupted_file).unwrap().unwrap();
        assert_eq!(
            parsed_corrupted["memex"]["PreInvocation"][0]["command"]
                .as_str()
                .unwrap(),
            "memex prompt-hook"
        );
    }
}
