use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::config::MemexConfig;
use crate::errors::{MemexError, Result};

/// Default directories that should be skipped during file discovery.
pub const BUILTIN_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".memex",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
];

/// Helper to determine if a given file path is a Markdown file (`.md` or `.markdown`, case-insensitive).
pub fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            lower == "md" || lower == "markdown"
        })
        .unwrap_or(false)
}

/// Discovers Markdown files in a project root while respecting `.gitignore`,
/// built-in ignore lists, and `memex.json` configuration overrides.
pub struct FileDiscovery;

impl FileDiscovery {
    /// Recursively scans `root` for Markdown documents (`.md`, `.markdown`).
    ///
    /// Applies the following filtering rules in order:
    /// 1. If `root` is a single file, verifies if it is a markdown file and returns it.
    /// 2. Built-in skips for `.git/`, `.memex/`, `node_modules/`, `target/`, `dist/`, `build/`, `vendor/`.
    /// 3. Hidden directories and files (starting with `.`) are ignored by default.
    /// 4. `.gitignore` rules in the root and parent/child directories.
    /// 5. `memex.json` `exclude` globs.
    /// 6. `memex.json` `include` globs (which override ignores).
    ///
    /// Returns a sorted list of discovered file paths.
    pub fn scan(root: &Path, config: &MemexConfig) -> Result<Vec<PathBuf>> {
        if !root.exists() {
            return Err(MemexError::DiscoveryError {
                path: root.display().to_string(),
                reason: "Path does not exist".to_string(),
            });
        }

        if root.is_file() {
            if is_markdown_file(root) {
                return Ok(vec![root.to_path_buf()]);
            }
            return Ok(Vec::new());
        }

        let filter = crate::discovery::PathFilter::new(root, config)?;

        // If include patterns are specified, we don't let git_ignore completely prune files upfront
        // so that include overrides can recover gitignored files, but instead filter through PathFilter.
        let has_custom_includes = !config.include.is_empty();

        let mut walker = WalkBuilder::new(root);
        walker
            .hidden(true)
            .parents(true)
            .git_ignore(!has_custom_includes)
            .git_global(!has_custom_includes)
            .git_exclude(!has_custom_includes)
            .require_git(false)
            .filter_entry({
                let filter = filter.clone();
                move |entry| {
                    if let Some(name) = entry.file_name().to_str() {
                        if name == ".git" || name == ".memex" {
                            return false;
                        }
                        if BUILTIN_IGNORED_DIRS.contains(&name) {
                            return false;
                        }
                    }
                    let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
                    if is_dir && filter.is_ignored(entry.path(), true) {
                        return false;
                    }
                    true
                }
            });

        let mut discovered = Vec::new();

        for result in walker.build() {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                    if !is_dir && is_markdown_file(path) {
                        if !filter.is_ignored(path, false) {
                            discovered.push(path.to_path_buf());
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        root = %root.display(),
                        error = %err,
                        "Error encountered during directory traversal; skipping entry"
                    );
                }
            }
        }

        discovered.sort();
        discovered.dedup();
        Ok(discovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_markdown_file() {
        assert!(is_markdown_file(Path::new("README.md")));
        assert!(is_markdown_file(Path::new("docs/intro.MD")));
        assert!(is_markdown_file(Path::new("guide.markdown")));
        assert!(is_markdown_file(Path::new("PAGE.MARKDOWN")));
        assert!(!is_markdown_file(Path::new("script.rs")));
        assert!(!is_markdown_file(Path::new("data.json")));
        assert!(!is_markdown_file(Path::new("notes.txt")));
        assert!(!is_markdown_file(Path::new("md")));
    }

    #[test]
    fn test_scan_single_file_direct() {
        let temp = TempDir::new().unwrap();
        let md_file = temp.path().join("single.md");
        fs::write(&md_file, "# Hello").unwrap();

        let txt_file = temp.path().join("other.txt");
        fs::write(&txt_file, "plain").unwrap();

        let config = MemexConfig::default();
        let results = FileDiscovery::scan(&md_file, &config).unwrap();
        assert_eq!(results, vec![md_file]);

        let txt_results = FileDiscovery::scan(&txt_file, &config).unwrap();
        assert!(txt_results.is_empty());
    }

    #[test]
    fn test_scan_nonexistent_path() {
        let temp = TempDir::new().unwrap();
        let non_existent = temp.path().join("does_not_exist");
        let config = MemexConfig::default();
        let err = FileDiscovery::scan(&non_existent, &config).unwrap_err();
        assert!(matches!(err, MemexError::DiscoveryError { .. }));
    }

    #[test]
    fn test_scan_discovers_all_md_files_in_tree() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let f1 = root.join("README.md");
        let f2 = root.join("docs").join("guide.MD");
        let f3 = root.join("docs").join("api").join("spec.markdown");
        let f4 = root.join("src").join("main.rs");
        let f5 = root.join("package.json");

        fs::create_dir_all(f2.parent().unwrap()).unwrap();
        fs::create_dir_all(f3.parent().unwrap()).unwrap();
        fs::create_dir_all(f4.parent().unwrap()).unwrap();

        fs::write(&f1, "# Readme").unwrap();
        fs::write(&f2, "# Guide").unwrap();
        fs::write(&f3, "# Spec").unwrap();
        fs::write(&f4, "fn main() {}").unwrap();
        fs::write(&f5, "{}").unwrap();

        let config = MemexConfig::default();
        let results = FileDiscovery::scan(root, &config).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results, vec![f1, f3, f2]);
    }

    #[test]
    fn test_scan_skips_builtin_directories() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let valid_md = root.join("README.md");
        fs::write(&valid_md, "# Readme").unwrap();

        // Built-in ignored directories
        let ignored_dirs = [
            ".git",
            ".memex",
            "node_modules",
            "target",
            "dist",
            "build",
            "vendor",
        ];

        for dir_name in &ignored_dirs {
            let dir = root.join(dir_name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("should_be_skipped.md"), "# Ignored").unwrap();
        }

        let config = MemexConfig::default();
        let results = FileDiscovery::scan(root, &config).unwrap();

        assert_eq!(results, vec![valid_md]);
    }

    #[test]
    fn test_scan_skips_hidden_directories() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let valid_md = root.join("doc.md");
        fs::write(&valid_md, "# Doc").unwrap();

        let hidden_dir = root.join(".hidden");
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("secret.md"), "# Secret").unwrap();

        let config = MemexConfig::default();
        let results = FileDiscovery::scan(root, &config).unwrap();

        assert_eq!(results, vec![valid_md]);
    }

    #[test]
    fn test_scan_empty_directory() {
        let temp = TempDir::new().unwrap();
        let config = MemexConfig::default();
        let results = FileDiscovery::scan(temp.path(), &config).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_respects_gitignore() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let allowed = root.join("allowed.md");
        let ignored = root.join("ignored.md");
        let sub_ignored = root.join("drafts").join("wip.md");
        fs::create_dir_all(sub_ignored.parent().unwrap()).unwrap();

        fs::write(&allowed, "# Allowed").unwrap();
        fs::write(&ignored, "# Ignored").unwrap();
        fs::write(&sub_ignored, "# Draft").unwrap();

        // Write .gitignore
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "ignored.md\ndrafts/\n").unwrap();

        let config = MemexConfig::default();
        let results = FileDiscovery::scan(root, &config).unwrap();

        assert_eq!(results, vec![allowed]);
    }

    #[test]
    fn test_scan_respects_custom_exclude() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let doc1 = root.join("doc1.md");
        let doc2 = root.join("temp_notes.md");
        let doc3 = root.join("archive").join("old.md");
        fs::create_dir_all(doc3.parent().unwrap()).unwrap();

        fs::write(&doc1, "# Doc 1").unwrap();
        fs::write(&doc2, "# Temp Notes").unwrap();
        fs::write(&doc3, "# Old").unwrap();

        let config = MemexConfig {
            exclude: vec!["temp_*.md".to_string(), "archive/**".to_string()],
            include: vec![],
        };

        let results = FileDiscovery::scan(root, &config).unwrap();
        assert_eq!(results, vec![doc1]);
    }

    #[test]
    fn test_scan_custom_include_overrides_gitignore_and_exclude() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let normal = root.join("normal.md");
        let gitignored_file = root.join("gitignored.md");
        let excluded_file = root.join("excluded.md");
        let force_included_gitignored = root.join("keep_this_ignored.md");
        let force_included_excluded = root.join("keep_this_excluded.md");

        fs::write(&normal, "# Normal").unwrap();
        fs::write(&gitignored_file, "# Gitignored").unwrap();
        fs::write(&excluded_file, "# Excluded").unwrap();
        fs::write(&force_included_gitignored, "# Keep Gitignored").unwrap();
        fs::write(&force_included_excluded, "# Keep Excluded").unwrap();

        // .gitignore ignores gitignored.md and keep_this_ignored.md
        let gitignore = root.join(".gitignore");
        fs::write(&gitignore, "gitignored.md\nkeep_this_ignored.md\n").unwrap();

        // config excludes excluded.md and keep_this_excluded.md, but includes keep_this_*.md
        let config = MemexConfig {
            exclude: vec![
                "excluded.md".to_string(),
                "keep_this_excluded.md".to_string(),
            ],
            include: vec![
                "keep_this_ignored.md".to_string(),
                "keep_this_excluded.md".to_string(),
            ],
        };

        let results = FileDiscovery::scan(root, &config).unwrap();

        let mut expected = vec![normal, force_included_excluded, force_included_gitignored];
        expected.sort();

        assert_eq!(results, expected);
    }
}
