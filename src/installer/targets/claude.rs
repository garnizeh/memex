use crate::errors::Result;
use crate::installer::config_writer::{atomic_write_json, merge_json_value, read_json_value};
use crate::installer::targets::{
    inject_mcp_server_config, is_memex_in_mcp_config, AgentTarget, DetectionResult, InstallOptions,
};

/// Agent target for Claude Code.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeTarget;

impl AgentTarget for ClaudeTarget {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        let home = options.resolve_home_dir()?;
        let claude_dir = home.join(".claude");
        let claude_json = home.join(".claude.json");

        if claude_dir.exists() || claude_json.exists() {
            let is_configured = is_memex_in_mcp_config(&claude_json);
            Ok(DetectionResult::Detected {
                config_path: claude_json,
                is_configured,
                details: Some("Claude Code environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let home = options.resolve_home_dir()?;
        let claude_json = home.join(".claude.json");
        let claude_settings = home.join(".claude").join("settings.json");

        // 1. Inject MCP server definition
        inject_mcp_server_config(&claude_json, &options.command, &options.args)?;

        // 2. Inject Claude permissions into ~/.claude/settings.json
        let mut settings =
            read_json_value(&claude_settings)?.unwrap_or_else(|| serde_json::json!({}));
        let permissions_snippet = serde_json::json!({
            "permissions": {
                "allow": ["mcp__memex__*"]
            }
        });
        merge_json_value(&mut settings, &permissions_snippet);
        atomic_write_json(&claude_settings, &settings)?;

        Ok(())
    }
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
    }

    #[test]
    fn test_claude_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = ClaudeTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Create ~/.claude directory
        std::fs::create_dir_all(temp_dir.path().join(".claude")).unwrap();

        let res = target.detect(&opts).unwrap();
        assert!(res.is_detected());
        assert!(!res.is_configured());

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify .claude.json
        let claude_json = temp_dir.path().join(".claude.json");
        let parsed_json = read_json_value(&claude_json).unwrap().unwrap();
        assert_eq!(
            parsed_json["mcpServers"]["memex"]["command"]
                .as_str()
                .unwrap(),
            "memex"
        );

        // Verify .claude/settings.json
        let settings_json = temp_dir.path().join(".claude").join("settings.json");
        let parsed_settings = read_json_value(&settings_json).unwrap().unwrap();
        let allows = parsed_settings["permissions"]["allow"].as_array().unwrap();
        assert!(allows.iter().any(|v| v.as_str() == Some("mcp__memex__*")));
    }
}
