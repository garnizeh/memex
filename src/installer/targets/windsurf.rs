use std::path::PathBuf;

use crate::errors::Result;
use crate::installer::targets::{
    AgentTarget, DetectionResult, InstallOptions, inject_mcp_server_config, is_memex_in_mcp_config,
};

/// Agent target for Windsurf editor.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindsurfTarget;

impl WindsurfTarget {
    /// Resolves the target MCP configuration path for Windsurf.
    pub fn resolve_mcp_config_path(&self, options: &InstallOptions) -> Result<PathBuf> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_codeium_dir = workspace.join(".codeium").join("windsurf");
            let local_codeium_file = local_codeium_dir.join("mcp_config.json");
            let local_windsurf_dir = workspace.join(".windsurf");
            let local_windsurf_file = local_windsurf_dir.join("mcp_config.json");

            if local_codeium_file.exists() || local_codeium_dir.exists() {
                return Ok(local_codeium_file);
            }
            if local_windsurf_file.exists() || local_windsurf_dir.exists() {
                return Ok(local_windsurf_file);
            }
        }

        let home = options.resolve_home_dir()?;
        Ok(home
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json"))
    }
}

impl AgentTarget for WindsurfTarget {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn name(&self) -> &'static str {
        "Windsurf"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        if let Some(ref workspace) = options.workspace_dir {
            let local_codeium_dir = workspace.join(".codeium").join("windsurf");
            let local_windsurf_dir = workspace.join(".windsurf");
            let target_config = self.resolve_mcp_config_path(options)?;

            if local_codeium_dir.exists() || local_windsurf_dir.exists() || target_config.exists() {
                let is_configured = is_memex_in_mcp_config(&target_config);
                return Ok(DetectionResult::Detected {
                    config_path: target_config,
                    is_configured,
                    details: Some("Windsurf workspace environment detected".to_string()),
                });
            }
        }

        let home = options.resolve_home_dir()?;
        let target_config = self.resolve_mcp_config_path(options)?;
        let codeium_windsurf_dir = home.join(".codeium").join("windsurf");
        let codeium_dir = home.join(".codeium");

        if target_config.exists() || codeium_windsurf_dir.exists() || codeium_dir.exists() {
            let is_configured = is_memex_in_mcp_config(&target_config);
            Ok(DetectionResult::Detected {
                config_path: target_config,
                is_configured,
                details: Some("Windsurf environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let config_path = self.resolve_mcp_config_path(options)?;
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
    fn test_windsurf_detection_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let target = WindsurfTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());
        assert!(!res.is_configured());
    }

    #[test]
    fn test_windsurf_global_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = WindsurfTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        // Create ~/.codeium/windsurf directory
        std::fs::create_dir_all(temp_dir.path().join(".codeium").join("windsurf")).unwrap();

        let res = target.detect(&opts).unwrap();
        assert!(res.is_detected());
        assert!(!res.is_configured());
        assert_eq!(
            res.config_path(),
            Some(
                temp_dir
                    .path()
                    .join(".codeium")
                    .join("windsurf")
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
        let config_path = temp_dir
            .path()
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json");
        let parsed = read_json_value(&config_path).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );
        assert_eq!(
            parsed["mcpServers"]["memex"]["args"][0].as_str().unwrap(),
            "serve"
        );
        assert_eq!(
            parsed["mcpServers"]["memex"]["args"][1].as_str().unwrap(),
            "--mcp"
        );
    }

    #[test]
    fn test_windsurf_workspace_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir_all(workspace.join(".codeium").join("windsurf")).unwrap();

        let target = WindsurfTarget;
        let opts = InstallOptions::new().with_workspace_dir(&workspace);

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());
        assert_eq!(
            res_detected.config_path(),
            Some(
                workspace
                    .join(".codeium")
                    .join("windsurf")
                    .join("mcp_config.json")
                    .as_path()
            )
        );

        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        let config_file = workspace
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json");
        let parsed = read_json_value(&config_file).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );
    }
}
