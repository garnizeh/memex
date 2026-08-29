use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

use crate::config::MemexConfig;
use crate::errors::{MemexError, Result};

/// Path filter chain helper that models and checks ignore and include rules
/// for files discovered or checked against `.gitignore` and `memex.json`.
#[derive(Debug, Clone)]
pub struct PathFilter {
    gitignore: Option<Gitignore>,
    exclude: Option<Gitignore>,
    include: Option<Gitignore>,
}

impl PathFilter {
    /// Constructs a `PathFilter` rooted at `root` given a `MemexConfig`.
    pub fn new(root: &Path, config: &MemexConfig) -> Result<Self> {
        // 1. .gitignore in root (if it exists)
        let gitignore = {
            let gitignore_path = root.join(".gitignore");
            if gitignore_path.is_file() {
                let mut builder = GitignoreBuilder::new(root);
                if let Some(err) = builder.add(&gitignore_path) {
                    return Err(MemexError::DiscoveryError {
                        path: gitignore_path.display().to_string(),
                        reason: format!("Failed to parse .gitignore: {}", err),
                    });
                }
                Some(builder.build().map_err(|e| MemexError::DiscoveryError {
                    path: root.display().to_string(),
                    reason: format!("Failed to build gitignore filter: {}", e),
                })?)
            } else {
                None
            }
        };

        // 2. Custom exclude patterns from MemexConfig
        let exclude = if !config.exclude.is_empty() {
            let mut builder = GitignoreBuilder::new(root);
            for pattern in &config.exclude {
                builder
                    .add_line(None, pattern)
                    .map_err(|e| MemexError::DiscoveryError {
                        path: root.display().to_string(),
                        reason: format!("Failed to add exclude pattern '{}': {}", pattern, e),
                    })?;
            }
            Some(builder.build().map_err(|e| MemexError::DiscoveryError {
                path: root.display().to_string(),
                reason: format!("Failed to build exclude matcher: {}", e),
            })?)
        } else {
            None
        };

        // 3. Custom include patterns from MemexConfig (which override excludes)
        let include = if !config.include.is_empty() {
            let mut builder = GitignoreBuilder::new(root);
            for pattern in &config.include {
                builder
                    .add_line(None, pattern)
                    .map_err(|e| MemexError::DiscoveryError {
                        path: root.display().to_string(),
                        reason: format!("Failed to add include pattern '{}': {}", pattern, e),
                    })?;
            }
            Some(builder.build().map_err(|e| MemexError::DiscoveryError {
                path: root.display().to_string(),
                reason: format!("Failed to build include matcher: {}", e),
            })?)
        } else {
            None
        };

        Ok(Self {
            gitignore,
            exclude,
            include,
        })
    }

    /// Evaluates whether a given path is ignored or accepted.
    ///
    /// Evaluation precedence:
    /// 1. If path matches `include` globs, it is explicitly **included** (not ignored).
    /// 2. If path matches `exclude` globs, it is **ignored**.
    /// 3. If path matches `.gitignore` patterns, it is **ignored**.
    /// 4. Otherwise, it is **not ignored**.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        // 1. Check include overrides
        if let Some(ref inc) = self.include
            && inc.matched(path, is_dir).is_ignore()
        {
            return false;
        }

        // 2. Check config exclude
        if let Some(ref exc) = self.exclude
            && exc.matched(path, is_dir).is_ignore()
        {
            return true;
        }

        // 3. Check gitignore
        if let Some(ref gi) = self.gitignore
            && gi.matched(path, is_dir).is_ignore()
        {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_path_filter_without_config_or_gitignore() {
        let temp = TempDir::new().unwrap();
        let config = MemexConfig::default();
        let filter = PathFilter::new(temp.path(), &config).unwrap();

        assert!(!filter.is_ignored(&temp.path().join("readme.md"), false));
    }

    #[test]
    fn test_path_filter_layers_precedence() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let gitignore_file = root.join(".gitignore");
        fs::write(&gitignore_file, "ignored.txt\noverride_me.md\n").unwrap();

        let config = MemexConfig {
            exclude: vec!["custom_excluded.md".to_string(), "shared.md".to_string()],
            include: vec!["override_me.md".to_string(), "shared.md".to_string()],
        };

        let filter = PathFilter::new(root, &config).unwrap();

        // 1. gitignored without include -> ignored
        assert!(filter.is_ignored(&root.join("ignored.txt"), false));

        // 2. gitignored WITH include -> NOT ignored
        assert!(!filter.is_ignored(&root.join("override_me.md"), false));

        // 3. custom excluded without include -> ignored
        assert!(filter.is_ignored(&root.join("custom_excluded.md"), false));

        // 4. custom excluded WITH include -> NOT ignored (include overrides exclude)
        assert!(!filter.is_ignored(&root.join("shared.md"), false));

        // 5. normal file -> NOT ignored
        assert!(!filter.is_ignored(&root.join("normal.md"), false));
    }
}
