use crate::errors::Result;
use crate::installer::targets::{
    AgentTarget, DetectionResult, InstallOptions, inject_mcp_server_config, is_memex_in_mcp_config,
};

/// Agent target for Antigravity IDE.
#[derive(Debug, Default, Clone, Copy)]
pub struct AntigravityTarget;

impl AgentTarget for AntigravityTarget {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn name(&self) -> &'static str {
        "Antigravity IDE"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        let home = options.resolve_home_dir()?;
        let gemini_dir = home.join(".gemini");
        let antigravity_dir = gemini_dir.join("antigravity-ide");
        let config_path = antigravity_dir.join("mcp_config.json");

        if gemini_dir.exists() || antigravity_dir.exists() || config_path.exists() {
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
        let home = options.resolve_home_dir()?;
        let config_path = home
            .join(".gemini")
            .join("antigravity-ide")
            .join("mcp_config.json");

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
    fn test_antigravity_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = AntigravityTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());

        // Create ~/.gemini/antigravity-ide directory
        std::fs::create_dir_all(temp_dir.path().join(".gemini").join("antigravity-ide")).unwrap();

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());

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
}
