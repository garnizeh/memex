use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::Result;

pub const MEMEX_START_MARKER: &str = "<!-- MEMEX_START -->";
pub const MEMEX_END_MARKER: &str = "<!-- MEMEX_END -->";

pub const CANONICAL_MEMEX_DIRECTIVE: &str = r#"<!-- MEMEX_START -->
## Documentation Search (Memex)

In repositories indexed by Memex (a `.memex/` directory exists), reach for the Memex MCP tool `search_documentation` BEFORE using `view_file` or `grep` on markdown documentation to minimize token usage and locate relevant sections instantly.
<!-- MEMEX_END -->"#;

/// Injects or updates the standard Memex directive snippet inside a markdown/text content string.
///
/// If `<!-- MEMEX_START -->` and `<!-- MEMEX_END -->` markers exist:
/// - Replaces the content between the markers (inclusive) with [`CANONICAL_MEMEX_DIRECTIVE`].
/// - Retains all surrounding user directives and content intact.
///
/// If markers are missing:
/// - Appends [`CANONICAL_MEMEX_DIRECTIVE`] cleanly to the end of the text.
pub fn inject_memex_directive(content: &str) -> String {
    if let Some(start_idx) = content.find(MEMEX_START_MARKER)
        && let Some(end_idx) = content[start_idx..].find(MEMEX_END_MARKER)
    {
        let actual_end_idx = start_idx + end_idx + MEMEX_END_MARKER.len();
        let before = &content[..start_idx];
        let after = &content[actual_end_idx..];

        let mut result =
            String::with_capacity(before.len() + CANONICAL_MEMEX_DIRECTIVE.len() + after.len());
        result.push_str(before);
        result.push_str(CANONICAL_MEMEX_DIRECTIVE);
        result.push_str(after);
        return result;
    }

    let trimmed = content.trim_end();
    if trimmed.is_empty() {
        format!("{}\n", CANONICAL_MEMEX_DIRECTIVE)
    } else {
        format!("{}\n\n{}\n", trimmed, CANONICAL_MEMEX_DIRECTIVE)
    }
}

/// Injects or updates the Memex directive block in a specific file.
///
/// If the file does not exist, it will be created with parent directories.
/// Returns `Ok(true)` if the file content changed or was created, `Ok(false)` if it was already up-to-date.
pub fn update_rule_file(path: &Path) -> Result<bool> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let original_content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let updated_content = inject_memex_directive(&original_content);

    if original_content == updated_content {
        return Ok(false);
    }

    fs::write(path, updated_content)?;
    Ok(true)
}

/// List of standard project-level agent directive files.
pub const STANDARD_AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

/// Updates or creates standard agent directive files (`AGENTS.md`, `CLAUDE.md`) in the workspace.
///
/// Returns a list of paths that were written or updated.
pub fn update_workspace_agent_rules(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut updated = Vec::new();
    for file_name in STANDARD_AGENT_FILES {
        let file_path = workspace_root.join(file_name);
        if update_rule_file(&file_path)? {
            updated.push(file_path);
        }
    }
    Ok(updated)
}

/// Updates existing standard agent directive files (`AGENTS.md`, `CLAUDE.md`) in the workspace if they already exist.
///
/// Returns a list of paths that were updated.
pub fn update_existing_workspace_agent_rules(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut updated = Vec::new();
    for file_name in STANDARD_AGENT_FILES {
        let file_path = workspace_root.join(file_name);
        if file_path.exists() && update_rule_file(&file_path)? {
            updated.push(file_path);
        }
    }
    Ok(updated)
}

/// Resolves target-specific rule files for a given agent target ID.
pub fn rule_files_for_target(workspace_root: &Path, target_id: &str) -> Vec<PathBuf> {
    match target_id {
        "claude" => vec![
            workspace_root.join("CLAUDE.md"),
            workspace_root.join("AGENTS.md"),
        ],
        "cursor" => vec![
            workspace_root.join(".cursorrules"),
            workspace_root
                .join(".cursor")
                .join("rules")
                .join("memex.mdc"),
            workspace_root.join("AGENTS.md"),
        ],
        "windsurf" => vec![
            workspace_root.join(".windsurfrules"),
            workspace_root.join("AGENTS.md"),
        ],
        "zed" | "antigravity" => vec![workspace_root.join("AGENTS.md")],
        _ => vec![workspace_root.join("AGENTS.md")],
    }
}

/// Injects Memex directives for specific agent targets in the workspace.
pub fn update_target_agent_rules(
    workspace_root: &Path,
    target_ids: &[&str],
) -> Result<Vec<PathBuf>> {
    let mut updated = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for target_id in target_ids {
        for rule_path in rule_files_for_target(workspace_root, target_id) {
            if seen.insert(rule_path.clone()) && update_rule_file(&rule_path)? {
                updated.push(rule_path);
            }
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_inject_directive_empty_content() {
        let injected = inject_memex_directive("");
        assert!(injected.contains(MEMEX_START_MARKER));
        assert!(injected.contains(MEMEX_END_MARKER));
        assert_eq!(injected, format!("{}\n", CANONICAL_MEMEX_DIRECTIVE));
    }

    #[test]
    fn test_inject_directive_existing_content_without_markers() {
        let original = "# My Project\n\nCustom user rule 1.\nCustom user rule 2.";
        let injected = inject_memex_directive(original);

        assert!(injected.starts_with("# My Project\n\nCustom user rule 1.\nCustom user rule 2."));
        assert!(injected.contains(CANONICAL_MEMEX_DIRECTIVE));
    }

    #[test]
    fn test_inject_directive_preserves_surrounding_content_with_markers() {
        let original = r#"# Guidelines

Header stuff

<!-- MEMEX_START -->
Old obsolete directive
<!-- MEMEX_END -->

Footer stuff
Keep this intact!
"#;

        let injected = inject_memex_directive(original);

        assert!(injected.contains("# Guidelines"));
        assert!(injected.contains("Header stuff"));
        assert!(!injected.contains("Old obsolete directive"));
        assert!(injected.contains(CANONICAL_MEMEX_DIRECTIVE));
        assert!(injected.contains("Footer stuff\nKeep this intact!"));
    }

    #[test]
    fn test_inject_directive_idempotent() {
        let original = "# Custom Header\n\nSome guidelines.\n";
        let once = inject_memex_directive(original);
        let twice = inject_memex_directive(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_update_rule_file_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let rule_path = temp_dir.path().join("sub").join("CLAUDE.md");

        // 1. Initial creation
        let changed = update_rule_file(&rule_path).unwrap();
        assert!(changed);
        assert!(rule_path.exists());
        let content = fs::read_to_string(&rule_path).unwrap();
        assert!(content.contains(CANONICAL_MEMEX_DIRECTIVE));

        // 2. Second update should report no change (idempotent)
        let changed_again = update_rule_file(&rule_path).unwrap();
        assert!(!changed_again);

        // 3. Modifying user content outside marker should be preserved
        let modified = format!("# User Rule 1\n\n{content}\n# User Rule 2\n");
        fs::write(&rule_path, &modified).unwrap();

        let changed_after_user_edit = update_rule_file(&rule_path).unwrap();
        assert!(!changed_after_user_edit);

        let final_content = fs::read_to_string(&rule_path).unwrap();
        assert!(final_content.starts_with("# User Rule 1"));
        assert!(final_content.ends_with("# User Rule 2\n"));
        assert!(final_content.contains(CANONICAL_MEMEX_DIRECTIVE));
    }

    #[test]
    fn test_update_workspace_agent_rules() {
        let temp_dir = TempDir::new().unwrap();
        let ws = temp_dir.path();

        let updated = update_workspace_agent_rules(ws).unwrap();
        assert_eq!(updated.len(), 2);
        assert!(ws.join("AGENTS.md").exists());
        assert!(ws.join("CLAUDE.md").exists());

        // Subsequent call updates nothing
        let updated_again = update_workspace_agent_rules(ws).unwrap();
        assert!(updated_again.is_empty());
    }

    #[test]
    fn test_update_target_agent_rules() {
        let temp_dir = TempDir::new().unwrap();
        let ws = temp_dir.path();

        let updated = update_target_agent_rules(ws, &["cursor", "windsurf"]).unwrap();
        assert!(ws.join(".cursorrules").exists());
        assert!(ws.join(".windsurfrules").exists());
        assert!(ws.join("AGENTS.md").exists());
        assert!(!updated.is_empty());
    }
}
