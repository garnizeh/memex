use std::path::PathBuf;

use crate::errors::Result;
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

        if ide_config.exists() {
            // Legacy supported installation
            Ok(ide_config)
        } else if global_config.exists() {
            // Canonical global installation
            Ok(global_config)
        } else if gemini_dir.join("antigravity-ide").exists() {
            Ok(ide_config)
        } else {
            // Default to canonical global path: ~/.gemini/config/mcp_config.json
            Ok(global_config)
        }
    }
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
    }
}
