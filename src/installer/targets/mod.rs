use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::errors::{MemexError, Result};
use crate::installer::config_writer::{atomic_write_json, merge_json_value, read_json_value};

pub mod antigravity;
pub mod claude;
pub mod cursor;

pub use antigravity::AntigravityTarget;
pub use claude::ClaudeTarget;
pub use cursor::CursorTarget;

/// Result of probing the system for an agent's presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionResult {
    /// The agent was not found on this system.
    NotDetected,
    /// The agent was detected on this system.
    Detected {
        /// Resolved target configuration file path.
        config_path: PathBuf,
        /// True if Memex is already configured in the agent's settings.
        is_configured: bool,
        /// Optional human-readable details about the detection.
        details: Option<String>,
    },
}

impl DetectionResult {
    /// Returns true if the agent was detected.
    pub fn is_detected(&self) -> bool {
        matches!(self, DetectionResult::Detected { .. })
    }

    /// Returns true if the agent is detected and Memex is already configured.
    pub fn is_configured(&self) -> bool {
        match self {
            DetectionResult::Detected { is_configured, .. } => *is_configured,
            DetectionResult::NotDetected => false,
        }
    }

    /// Returns the target config path if detected.
    pub fn config_path(&self) -> Option<&Path> {
        match self {
            DetectionResult::Detected { config_path, .. } => Some(config_path.as_path()),
            DetectionResult::NotDetected => None,
        }
    }
}

/// Options controlling installation and agent probing.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Override home directory (useful for testing or custom environments).
    pub home_dir: Option<PathBuf>,
    /// Override workspace directory.
    pub workspace_dir: Option<PathBuf>,
    /// Command to launch Memex (defaults to "memex").
    pub command: String,
    /// Command-line arguments for launching Memex in MCP mode.
    pub args: Vec<String>,
    /// Force re-installation even if already configured.
    pub force: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            home_dir: None,
            workspace_dir: None,
            command: "memex".to_string(),
            args: vec!["serve".to_string(), "--mcp".to_string()],
            force: false,
        }
    }
}

impl InstallOptions {
    /// Creates a new `InstallOptions` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the home directory.
    pub fn with_home_dir<P: Into<PathBuf>>(mut self, home: P) -> Self {
        self.home_dir = Some(home.into());
        self
    }

    /// Overrides the workspace directory.
    pub fn with_workspace_dir<P: Into<PathBuf>>(mut self, workspace: P) -> Self {
        self.workspace_dir = Some(workspace.into());
        self
    }

    /// Overrides command and arguments.
    pub fn with_command<S: Into<String>>(mut self, command: S, args: Vec<String>) -> Self {
        self.command = command.into();
        self.args = args;
        self
    }

    /// Overrides the force flag.
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Resolves the effective home directory.
    pub fn resolve_home_dir(&self) -> Result<PathBuf> {
        if let Some(ref home) = self.home_dir {
            return Ok(home.clone());
        }
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return Ok(base_dirs.home_dir().to_path_buf());
        }
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PathBuf::from(home));
        }
        Err(MemexError::Installer(
            "Unable to resolve user home directory".to_string(),
        ))
    }
}

/// Common trait implemented by all agent installer targets.
pub trait AgentTarget: Send + Sync {
    /// Unique identifier for CLI flags and programmatic selection (e.g., "claude", "cursor", "antigravity").
    fn id(&self) -> &'static str;

    /// Human-friendly display name (e.g., "Claude Code", "Cursor", "Antigravity IDE").
    fn name(&self) -> &'static str;

    /// Detect if this agent is present on the system and if Memex is configured.
    fn detect(&self, options: &InstallOptions) -> Result<DetectionResult>;

    /// Inject or update Memex configuration in this agent's config file.
    fn install(&self, options: &InstallOptions) -> Result<()>;
}

/// Registry of known agent targets.
pub struct TargetRegistry {
    targets: Vec<Box<dyn AgentTarget>>,
}

impl Default for TargetRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl TargetRegistry {
    /// Creates an empty registry without any pre-registered targets.
    pub fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    /// Creates a registry initialized with all default agent targets (Claude, Cursor, Antigravity).
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ClaudeTarget));
        registry.register(Box::new(CursorTarget));
        registry.register(Box::new(AntigravityTarget));
        registry
    }

    /// Registers an agent target. If a target with the same ID already exists, it is replaced.
    pub fn register(&mut self, target: Box<dyn AgentTarget>) {
        if let Some(pos) = self.targets.iter().position(|t| t.id() == target.id()) {
            self.targets[pos] = target;
        } else {
            self.targets.push(target);
        }
    }

    /// Retrieves a target by its unique ID (case-insensitive).
    pub fn get(&self, id: &str) -> Option<&dyn AgentTarget> {
        self.targets
            .iter()
            .find(|t| t.id().eq_ignore_ascii_case(id))
            .map(|t| t.as_ref())
    }

    /// Returns a slice of all registered targets.
    pub fn targets(&self) -> &[Box<dyn AgentTarget>] {
        &self.targets
    }

    /// Probes all registered targets and returns pairs of `(target, DetectionResult)`.
    pub fn detect_all(&self, options: &InstallOptions) -> Vec<(&dyn AgentTarget, DetectionResult)> {
        self.targets
            .iter()
            .map(|target| {
                let detection = target
                    .detect(options)
                    .unwrap_or(DetectionResult::NotDetected);
                (target.as_ref(), detection)
            })
            .collect()
    }

    /// Probes a specific target by ID.
    pub fn detect_target(
        &self,
        id: &str,
        options: &InstallOptions,
    ) -> Option<Result<DetectionResult>> {
        self.get(id).map(|target| target.detect(options))
    }

    /// Installs Memex for a specific target by ID.
    pub fn install_target(&self, id: &str, options: &InstallOptions) -> Result<()> {
        let target = self
            .get(id)
            .ok_or_else(|| MemexError::Installer(format!("Unknown agent target: '{id}'")))?;
        target.install(options)
    }

    /// Installs Memex on all targets that are detected on the system.
    pub fn install_all_detected(&self, options: &InstallOptions) -> Result<Vec<&'static str>> {
        let mut installed = Vec::new();
        for target in &self.targets {
            let detection = target.detect(options)?;
            if detection.is_detected() {
                target.install(options)?;
                installed.push(target.id());
            }
        }
        Ok(installed)
    }
}

/// Helper function to construct standard MCP server configuration snippet.
pub fn make_mcp_server_config(command: &str, args: &[String]) -> Value {
    serde_json::json!({
        "mcpServers": {
            "memex": {
                "type": "stdio",
                "command": command,
                "args": args
            }
        }
    })
}

/// Helper function to check if `mcpServers.memex` exists in a JSON configuration file.
pub fn is_memex_in_mcp_config(config_path: &Path) -> bool {
    if let Ok(Some(value)) = read_json_value(config_path)
        && let Some(mcp_servers) = value.get("mcpServers").and_then(|v| v.as_object())
    {
        return mcp_servers.contains_key("memex");
    }
    false
}

/// Helper function to inject or update `mcpServers.memex` into a JSON config file.
pub fn inject_mcp_server_config(config_path: &Path, command: &str, args: &[String]) -> Result<()> {
    let snippet = make_mcp_server_config(command, args);
    let mut config = read_json_value(config_path)?.unwrap_or_else(|| serde_json::json!({}));
    merge_json_value(&mut config, &snippet);
    atomic_write_json(config_path, &config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct DummyTarget {
        target_id: &'static str,
        target_name: &'static str,
        should_detect: bool,
    }

    impl AgentTarget for DummyTarget {
        fn id(&self) -> &'static str {
            self.target_id
        }

        fn name(&self) -> &'static str {
            self.target_name
        }

        fn detect(&self, _options: &InstallOptions) -> Result<DetectionResult> {
            if self.should_detect {
                Ok(DetectionResult::Detected {
                    config_path: PathBuf::from("/dummy/config.json"),
                    is_configured: false,
                    details: Some("Dummy detection".to_string()),
                })
            } else {
                Ok(DetectionResult::NotDetected)
            }
        }

        fn install(&self, _options: &InstallOptions) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_detection_result_helpers() {
        let not_detected = DetectionResult::NotDetected;
        assert!(!not_detected.is_detected());
        assert!(!not_detected.is_configured());
        assert_eq!(not_detected.config_path(), None);

        let detected = DetectionResult::Detected {
            config_path: PathBuf::from("/test/path.json"),
            is_configured: true,
            details: Some("Found".to_string()),
        };
        assert!(detected.is_detected());
        assert!(detected.is_configured());
        assert_eq!(detected.config_path(), Some(Path::new("/test/path.json")));
    }

    #[test]
    fn test_install_options_builder() {
        let opts = InstallOptions::new()
            .with_home_dir("/custom/home")
            .with_workspace_dir("/custom/workspace")
            .with_command("custom-memex", vec!["--arg1".to_string()])
            .with_force(true);

        assert_eq!(opts.home_dir, Some(PathBuf::from("/custom/home")));
        assert_eq!(opts.workspace_dir, Some(PathBuf::from("/custom/workspace")));
        assert_eq!(opts.command, "custom-memex");
        assert_eq!(opts.args, vec!["--arg1"]);
        assert!(opts.force);
        assert_eq!(
            opts.resolve_home_dir().unwrap(),
            PathBuf::from("/custom/home")
        );
    }

    #[test]
    fn test_target_registry_registration_and_lookup() {
        let mut registry = TargetRegistry::new();
        assert_eq!(registry.targets().len(), 0);

        registry.register(Box::new(DummyTarget {
            target_id: "test-agent",
            target_name: "Test Agent",
            should_detect: true,
        }));

        assert_eq!(registry.targets().len(), 1);
        let found = registry.get("test-agent");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "Test Agent");

        // Case insensitivity
        assert!(registry.get("TEST-AGENT").is_some());
        assert!(registry.get("unknown").is_none());

        // Replace existing
        registry.register(Box::new(DummyTarget {
            target_id: "test-agent",
            target_name: "Updated Agent",
            should_detect: false,
        }));
        assert_eq!(registry.targets().len(), 1);
        assert_eq!(registry.get("test-agent").unwrap().name(), "Updated Agent");
    }

    #[test]
    fn test_target_registry_detect_and_install() {
        let mut registry = TargetRegistry::new();
        registry.register(Box::new(DummyTarget {
            target_id: "agent-a",
            target_name: "Agent A",
            should_detect: true,
        }));
        registry.register(Box::new(DummyTarget {
            target_id: "agent-b",
            target_name: "Agent B",
            should_detect: false,
        }));

        let opts = InstallOptions::new();
        let detections = registry.detect_all(&opts);
        assert_eq!(detections.len(), 2);
        assert!(detections[0].1.is_detected());
        assert!(!detections[1].1.is_detected());

        let installed = registry.install_all_detected(&opts).unwrap();
        assert_eq!(installed, vec!["agent-a"]);

        assert!(registry.install_target("agent-a", &opts).is_ok());
        let err = registry.install_target("nonexistent", &opts);
        assert!(err.is_err());
    }

    #[test]
    fn test_target_registry_with_defaults_detection() {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().join("home");
        std::fs::create_dir_all(&temp_home).unwrap();

        let opts = InstallOptions::new().with_home_dir(&temp_home);
        let registry = TargetRegistry::with_defaults();

        // Initially no agents exist in empty temp home
        let detections = registry.detect_all(&opts);
        for (target, detection) in detections {
            assert!(
                !detection.is_detected(),
                "Target {} should not be detected in empty directory",
                target.name()
            );
        }

        // Simulate Claude Code installed: create ~/.claude directory
        std::fs::create_dir_all(temp_home.join(".claude")).unwrap();
        // Simulate Cursor installed: create ~/.cursor directory
        std::fs::create_dir_all(temp_home.join(".cursor")).unwrap();
        // Simulate Antigravity installed: create ~/.gemini/antigravity-ide directory
        std::fs::create_dir_all(temp_home.join(".gemini").join("antigravity-ide")).unwrap();

        let detections_after = registry.detect_all(&opts);
        for (target, detection) in detections_after {
            assert!(
                detection.is_detected(),
                "Target {} should be detected after creating simulated directory",
                target.name()
            );
        }
    }

    #[test]
    fn test_make_mcp_server_config_and_inject() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("test_config.json");

        assert!(!is_memex_in_mcp_config(&config_file));

        inject_mcp_server_config(
            &config_file,
            "memex",
            &["serve".to_string(), "--mcp".to_string()],
        )
        .unwrap();

        assert!(is_memex_in_mcp_config(&config_file));

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&config_file).unwrap()).unwrap();
        assert_eq!(
            content["mcpServers"]["memex"]["command"].as_str().unwrap(),
            "memex"
        );
        assert_eq!(
            content["mcpServers"]["memex"]["args"][0].as_str().unwrap(),
            "serve"
        );
    }
}
