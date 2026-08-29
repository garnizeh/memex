use crate::errors::Result;
use crate::installer::targets::{
    inject_mcp_server_config, is_memex_in_mcp_config, AgentTarget, DetectionResult, InstallOptions,
};

/// Agent target for Cursor IDE.
#[derive(Debug, Default, Clone, Copy)]
pub struct CursorTarget;

impl AgentTarget for CursorTarget {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn name(&self) -> &'static str {
        "Cursor"
    }

    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult> {
        let home = options.resolve_home_dir()?;
        let cursor_dir = home.join(".cursor");
        let cursor_mcp = cursor_dir.join("mcp.json");

        if cursor_dir.exists() || cursor_mcp.exists() {
            let is_configured = is_memex_in_mcp_config(&cursor_mcp);
            Ok(DetectionResult::Detected {
                config_path: cursor_mcp,
                is_configured,
                details: Some("Cursor environment detected".to_string()),
            })
        } else {
            Ok(DetectionResult::NotDetected)
        }
    }

    fn install(&self, options: &InstallOptions) -> Result<()> {
        let home = options.resolve_home_dir()?;
        let cursor_mcp = home.join(".cursor").join("mcp.json");

        inject_mcp_server_config(&cursor_mcp, &options.command, &options.args)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::config_writer::read_json_value;
    use tempfile::TempDir;

    #[test]
    fn test_cursor_detection_and_installation() {
        let temp_dir = TempDir::new().unwrap();
        let target = CursorTarget;
        let opts = InstallOptions::new().with_home_dir(temp_dir.path());

        let res = target.detect(&opts).unwrap();
        assert!(!res.is_detected());

        // Create ~/.cursor directory
        std::fs::create_dir_all(temp_dir.path().join(".cursor")).unwrap();

        let res_detected = target.detect(&opts).unwrap();
        assert!(res_detected.is_detected());
        assert!(!res_detected.is_configured());

        // Perform install
        target.install(&opts).unwrap();

        let res_after = target.detect(&opts).unwrap();
        assert!(res_after.is_detected());
        assert!(res_after.is_configured());

        // Verify config content
        let cursor_mcp = temp_dir.path().join(".cursor").join("mcp.json");
        let parsed = read_json_value(&cursor_mcp).unwrap().unwrap();
        assert_eq!(
            parsed["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );
    }
}
