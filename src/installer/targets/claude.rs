use std::path::{Path, PathBuf};

use crate::errors::Result;
use crate::installer::config_writer::{atomic_write_json, read_json_value};
use crate::installer::targets::{
    AgentTarget, DetectionResult, InstallOptions, inject_mcp_server_config, is_memex_in_mcp_config,
};

/// Agent target for Claude Code.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeTarget;

impl ClaudeTarget {
    /// Resolves the target MCP configuration path (workspace `.mcp.json` or global `~/.claude.json`).
    pub fn resolve_mcp_config_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        if let Some(ref workspace) = options.workspace_dir {
            Ok(workspace.join(".mcp.json"))
        } else {
            let home = options.resolve_home_dir()?;
            Ok(home.join(".claude.json"))
        }
    }

    /// Resolves the Claude settings path (`~/.claude/settings.json`).
    pub fn resolve_settings_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        let home = options.resolve_home_dir()?;
        Ok(home.join(".claude").join("settings.json"))
    }
}

impl AgentTarget for ClaudeTarget {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        let target_config_path = self.resolve_mcp_config_path(options)?;

        // If a workspace is specified, check workspace-local detection first
        if let Some(ref workspace) = options.workspace_dir {
            let local_mcp = workspace.join(".mcp.json");
            let local_claude = workspace.join(".claude");
            if local_mcp.exists() || local_claude.exists() {
                let is_configured = is_memex_in_mcp_config(&local_mcp);
                return Ok(DetectionResult::Detected {
                    config_path: local_mcp,
                    is_configured,
                    details: Some("Claude Code workspace environment detected".to_string()),
                });
            }
        }

        // Global detection in user home directory
        let home = options.resolve_home_dir()?;
        let global_claude_dir = home.join(".claude");
        let global_claude_json = home.join(".claude.json");

        if global_claude_dir.exists() || global_claude_json.exists() {
            let is_configured = is_memex_in_mcp_config(&target_config_path);
            Ok(DetectionResult::Detected {
                config_path: target_config_path,
                is_configured,
                details: Some("Claude Code environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let mcp_config_path = self.resolve_mcp_config_path(options)?;
        let settings_path = self.resolve_settings_path(options)?;

        // 1. Inject MCP server definition into target mcp config file
        inject_mcp_server_config(&mcp_config_path, &options.command, &options.args)?;

        // 2. Inject Claude permissions into ~/.claude/settings.json
        inject_claude_permissions(&settings_path)?;

        // 3. Inject Claude hooks into ~/.claude/settings.json
        inject_claude_hooks(&settings_path)?;

        Ok(())
    }
}

/// Helper function to safely inject `"mcp__memex__*"` into `permissions.allow` in `settings.json`.
///
/// Preserves any existing settings and permissions without duplication.
pub fn inject_claude_permissions(settings_path: &Path) -> Result<()> {
    let mut settings = read_json_value(settings_path)?.unwrap_or_else(|| serde_json::json!({}));

    if !settings.is_object() {
        settings = serde_json::json!({});
    }

    let settings_obj = settings
        .as_object_mut()
        .expect("settings must be a JSON object");

    let permissions = settings_obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));

    if !permissions.is_object() {
        *permissions = serde_json::json!({});
    }

    let perm_obj = permissions
        .as_object_mut()
        .expect("permissions must be a JSON object");

    let allow = perm_obj
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]));

    if let Some(allow_arr) = allow.as_array_mut() {
        let memex_perm = serde_json::Value::String("mcp__memex__*".to_string());
        if !allow_arr.contains(&memex_perm) {
            allow_arr.push(memex_perm);
        }
    } else {
        *allow = serde_json::json!(["mcp__memex__*"]);
    }

    atomic_write_json(settings_path, &settings)?;
    Ok(())
}

/// Helper function to safely inject Memex prompt-hook into `settings.json`.
///
/// Configures UserPromptSubmit hook matching Claude Code's command schema:
/// `{ "matcher": "", "hooks": [ { "type": "command", "command": "memex prompt-hook" } ] }`
pub fn inject_claude_hooks(settings_path: &Path) -> Result<()> {
    let mut settings = read_json_value(settings_path)?.unwrap_or_else(|| serde_json::json!({}));

    if !settings.is_object() {
        settings = serde_json::json!({});
    }

    let settings_obj = settings
        .as_object_mut()
        .expect("settings must be a JSON object");

    let hooks = settings_obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }

    let hooks_obj = hooks.as_object_mut().expect("hooks must be a JSON object");

    let user_prompt = hooks_obj
        .entry("UserPromptSubmit")
        .or_insert_with(|| serde_json::json!([]));

    if !user_prompt.is_array() {
        *user_prompt = serde_json::json!([]);
    }

    let user_prompt_arr = user_prompt
        .as_array_mut()
        .expect("UserPromptSubmit must be an array");

    // 1. Remove any old prompt-only or invalidly formatted entries for memex
    user_prompt_arr.retain(|entry| {
        if let Some(inner_hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
            let is_memex_prompt = inner_hooks.iter().any(|h| {
                h.get("type").and_then(|t| t.as_str()) == Some("prompt")
                    && h.get("prompt")
                        .and_then(|p| p.as_str())
                        .map(|s| s.contains("Memex") || s.contains(".memex/"))
                        .unwrap_or(false)
            });
            return !is_memex_prompt;
        }
        true
    });

    // 2. Check if a valid command hook for memex is present
    let already_present = user_prompt_arr.iter().any(|entry| {
        if let Some(inner_hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
            inner_hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.contains("memex prompt-hook"))
                    .unwrap_or(false)
            })
        } else {
            false
        }
    });

    let memex_command_entry = serde_json::json!({
        "hooks": [
            {
                "type": "command",
                "command": "memex prompt-hook"
            }
        ]
    });

    if !already_present {
        user_prompt_arr.push(memex_command_entry);
    }

    atomic_write_json(settings_path, &settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_claude_detection_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let target = ClaudeTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());
        assert!(!res.is_configured());
    }

    #[test]
    fn test_claude_global_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = ClaudeTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Create ~/.claude directory
        std::fs::create_dir_all(temp_dir.path().join(".claude")).unwrap();

        let res = target.detect(&opts).unwrap();
        assert!(res.is_detected());
        assert!(!res.is_configured());
        assert_eq!(
            res.config_path(),
            Some(temp_dir.path().join(".claude.json").as_path())
        );

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify ~/.claude.json
        let claude_json = temp_dir.path().join(".claude.json");
        let parsed_json = read_json_value(&claude_json).unwrap().unwrap();
        assert_eq!(
            parsed_json["mcpServers"]["memex"]["type"].as_str().unwrap(),
            "stdio"
        );
        assert_eq!(
            parsed_json["mcpServers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );
        assert_eq!(
            parsed_json["mcpServers"]["memex"]["args"][0]
                .as_str()
                .unwrap(),
            "serve"
        );
        assert_eq!(
            parsed_json["mcpServers"]["memex"]["args"][1]
                .as_str()
                .unwrap(),
            "--mcp"
        );

        // Verify ~/.claude/settings.json
        let settings_json = temp_dir.path().join(".claude").join("settings.json");
        let parsed_settings = read_json_value(&settings_json).unwrap().unwrap();
        let allows = parsed_settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allows, &vec![serde_json::json!("mcp__memex__*")]);
    }

    #[test]
    fn test_claude_workspace_local_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        let workspace_dir = temp_dir.path().join("workspace");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&workspace_dir).unwrap();

        // Create workspace-level .claude directory
        std::fs::create_dir_all(workspace_dir.join(".claude")).unwrap();

        let target = ClaudeTarget;
        let opts = InstallOptions::new()
            .with_home_dir(&home_dir)
            .with_workspace_dir(&workspace_dir);

        let res = target.detect(&opts).unwrap();
        assert!(res.is_detected());
        assert!(!res.is_configured());
        assert_eq!(
            res.config_path(),
            Some(workspace_dir.join(".mcp.json").as_path())
        );

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify workspace .mcp.json
        let local_mcp = workspace_dir.join(".mcp.json");
        let parsed_mcp = read_json_value(&local_mcp).unwrap().unwrap();
        assert_eq!(
            parsed_mcp["mcpServers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );

        // Verify global settings.json received permissions
        let settings_json = home_dir.join(".claude").join("settings.json");
        let parsed_settings = read_json_value(&settings_json).unwrap().unwrap();
        let allows = parsed_settings["permissions"]["allow"].as_array().unwrap();
        assert!(allows.iter().any(|v| v.as_str() == Some("mcp__memex__*")));
    }

    #[test]
    fn test_claude_preserves_existing_mcp_servers_and_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let target = ClaudeTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Pre-create .claude.json with another MCP server
        let claude_json = temp_dir.path().join(".claude.json");
        atomic_write_json(
            &claude_json,
            &serde_json::json!({
                "mcpServers": {
                    "other_tool": {
                        "command": "other",
                        "args": ["run"]
                    }
                }
            }),
        )
        .unwrap();

        // Pre-create ~/.claude/settings.json with other permissions & settings
        let settings_json = temp_dir.path().join(".claude").join("settings.json");
        atomic_write_json(
            &settings_json,
            &serde_json::json!({
                "theme": "dark",
                "permissions": {
                    "allow": ["read_file", "mcp__other__*"]
                }
            }),
        )
        .unwrap();

        // Install Memex
        target.install(&opts).unwrap();

        // Check .claude.json preserves other_tool
        let parsed_mcp = read_json_value(&claude_json).unwrap().unwrap();
        assert_eq!(
            parsed_mcp["mcpServers"]["other_tool"]["command"]
                .as_str()
                .unwrap(),
            "other"
        );
        assert_eq!(
            parsed_mcp["mcpServers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );

        // Check settings.json preserves theme and existing permissions without duplicates
        let parsed_settings = read_json_value(&settings_json).unwrap().unwrap();
        assert_eq!(parsed_settings["theme"].as_str().unwrap(), "dark");
        let allows = parsed_settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(
            allows,
            &vec![
                serde_json::json!("read_file"),
                serde_json::json!("mcp__other__*"),
                serde_json::json!("mcp__memex__*"),
            ]
        );

        // Re-run install to check idempotence (no duplicate permissions)
        target.install(&opts).unwrap();
        let parsed_settings_re = read_json_value(&settings_json).unwrap().unwrap();
        let allows_re = parsed_settings_re["permissions"]["allow"]
            .as_array()
            .unwrap();
        assert_eq!(
            allows_re,
            &vec![
                serde_json::json!("read_file"),
                serde_json::json!("mcp__other__*"),
                serde_json::json!("mcp__memex__*"),
            ]
        );
    }
}
