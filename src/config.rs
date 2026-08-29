use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::errors::{MemexError, Result};

/// Configuration options loaded from `memex.json` at the project root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemexConfig {
    /// Glob patterns for paths to exclude from indexing, applied after `.gitignore`.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Glob patterns for paths to force-include in indexing, overriding `.gitignore`.
    #[serde(default)]
    pub include: Vec<String>,
}

impl MemexConfig {
    /// The canonical configuration file name.
    pub const FILE_NAME: &'static str = "memex.json";

    /// Parse `MemexConfig` from a JSON string.
    pub fn parse(content: &str) -> Result<Self> {
        serde_json::from_str(content).map_err(|e| {
            MemexError::Config(format!("Failed to parse {}: {}", Self::FILE_NAME, e))
        })
    }

    /// Load configuration from `memex.json` in the given root directory.
    ///
    /// If the file does not exist, or if it is malformed, this returns the default configuration
    /// (`MemexConfig::default()`) and emits a warning log when malformed.
    pub fn load_or_default(root: &Path) -> Self {
        let config_path = if root.is_file() && root.file_name().is_some_and(|f| f == Self::FILE_NAME) {
            root.to_path_buf()
        } else {
            root.join(Self::FILE_NAME)
        };

        if !config_path.exists() {
            return Self::default();
        }

        let content = match std::fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(e) => {
                warn!(
                    path = %config_path.display(),
                    error = %e,
                    "Failed to read {}; using default configuration",
                    Self::FILE_NAME
                );
                return Self::default();
            }
        };

        match Self::parse(&content) {
            Ok(config) => config,
            Err(e) => {
                warn!(
                    path = %config_path.display(),
                    error = %e,
                    "Malformed {}; using default configuration",
                    Self::FILE_NAME
                );
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_valid_json_parsing_full() {
        let json_data = r#"{
            "exclude": ["vendor/", "docs/legacy/"],
            "include": ["docs/"]
        }"#;

        let config = MemexConfig::parse(json_data).expect("Should parse valid JSON config");
        assert_eq!(config.exclude, vec!["vendor/", "docs/legacy/"]);
        assert_eq!(config.include, vec!["docs/"]);
    }

    #[test]
    fn test_valid_json_parsing_partial() {
        let only_exclude = r#"{"exclude": ["node_modules/"]}"#;
        let config = MemexConfig::parse(only_exclude).expect("Should parse config with only exclude");
        assert_eq!(config.exclude, vec!["node_modules/"]);
        assert!(config.include.is_empty());

        let only_include = r#"{"include": ["special.md"]}"#;
        let config = MemexConfig::parse(only_include).expect("Should parse config with only include");
        assert!(config.exclude.is_empty());
        assert_eq!(config.include, vec!["special.md"]);

        let empty_json = r#"{}"#;
        let config = MemexConfig::parse(empty_json).expect("Should parse empty JSON config");
        assert!(config.exclude.is_empty());
        assert!(config.include.is_empty());
    }

    #[test]
    fn test_missing_file_returns_defaults() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config = MemexConfig::load_or_default(temp_dir.path());
        assert_eq!(config, MemexConfig::default());
        assert!(config.exclude.is_empty());
        assert!(config.include.is_empty());
    }

    #[test]
    fn test_load_or_default_with_valid_file() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_file = temp_dir.path().join("memex.json");
        let json_data = r#"{
            "exclude": ["target/", "dist/"],
            "include": ["README.md"]
        }"#;
        fs::write(&config_file, json_data).expect("Failed to write test config");

        let config = MemexConfig::load_or_default(temp_dir.path());
        assert_eq!(config.exclude, vec!["target/", "dist/"]);
        assert_eq!(config.include, vec!["README.md"]);
    }

    #[test]
    fn test_load_or_default_with_direct_file_path() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_file = temp_dir.path().join("memex.json");
        let json_data = r#"{
            "exclude": ["temp/"]
        }"#;
        fs::write(&config_file, json_data).expect("Failed to write test config");

        let config = MemexConfig::load_or_default(&config_file);
        assert_eq!(config.exclude, vec!["temp/"]);
        assert!(config.include.is_empty());
    }

    #[test]
    fn test_malformed_json_returns_defaults_and_warns() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_file = temp_dir.path().join("memex.json");
        fs::write(&config_file, "{ invalid json content").expect("Failed to write malformed config");

        let config = MemexConfig::load_or_default(temp_dir.path());
        assert_eq!(config, MemexConfig::default());
    }

    #[test]
    fn test_invalid_type_in_json_returns_defaults() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_file = temp_dir.path().join("memex.json");
        // exclude should be an array of strings, not an integer
        fs::write(&config_file, r#"{"exclude": 12345}"#).expect("Failed to write invalid type config");

        let config = MemexConfig::load_or_default(temp_dir.path());
        assert_eq!(config, MemexConfig::default());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let original = MemexConfig {
            exclude: vec!["foo/".to_string(), "bar/".to_string()],
            include: vec!["baz.md".to_string()],
        };

        let serialized = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: MemexConfig =
            serde_json::from_str(&serialized).expect("Deserialization failed");

        assert_eq!(original, deserialized);
    }
}
