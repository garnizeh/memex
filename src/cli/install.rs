use std::io::{BufRead, Write};

use crate::errors::{MemexError, Result};
use crate::installer::targets::{ClaudeTarget, DetectionResult, InstallOptions, TargetRegistry};

/// Parses target arguments into a validated list of static target IDs.
///
/// Supports comma-delimited strings (e.g. `"claude,cursor"`), `"all"` for all registered targets,
/// and case-insensitive matching.
pub fn parse_target_ids(target: &str, registry: &TargetRegistry) -> Result<Vec<&'static str>> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.eq_ignore_ascii_case("all") {
        return Ok(registry.targets().iter().map(|t| t.id()).collect());
    }

    let mut target_ids = Vec::new();
    let available: Vec<&'static str> = registry.targets().iter().map(|t| t.id()).collect();

    for item in trimmed.split(',') {
        let name = item.trim();
        if name.is_empty() {
            continue;
        }

        if let Some(target) = registry.get(name) {
            if !target_ids.contains(&target.id()) {
                target_ids.push(target.id());
            }
        } else {
            return Err(MemexError::Installer(format!(
                "Unknown agent target '{}'. Available targets: {}",
                name,
                available.join(", ")
            )));
        }
    }

    Ok(target_ids)
}

/// Executes the installer with custom I/O streams and options.
///
/// Returns the list of target IDs that were successfully configured.
pub fn install_with_options<R: BufRead, W: Write>(
    target: Option<&str>,
    yes: bool,
    options: &InstallOptions,
    registry: &TargetRegistry,
    reader: &mut R,
    writer: &mut W,
) -> Result<Vec<&'static str>> {
    let detections = registry.detect_all(options);

    let targets_to_install: Vec<&'static str> = if let Some(target_str) = target {
        parse_target_ids(target_str, registry)?
    } else if yes {
        // Non-interactive auto-detection
        detections
            .iter()
            .filter(|(_, d)| d.is_detected())
            .map(|(t, _)| t.id())
            .collect()
    } else {
        // Interactive mode: display status and prompt user
        writeln!(writer, "Memex AI Coding Agent Auto-Installer")?;
        writeln!(writer, "====================================")?;
        writeln!(writer, "Probing system for supported AI coding agents...\n")?;

        let mut detected_indices = Vec::new();
        for (i, (target, detection)) in detections.iter().enumerate() {
            let status = match detection {
                DetectionResult::Detected {
                    config_path,
                    is_configured: true,
                    ..
                } => format!("detected ({}) [already configured]", config_path.display()),
                DetectionResult::Detected {
                    config_path,
                    is_configured: false,
                    ..
                } => format!("detected ({}) [not configured]", config_path.display()),
                DetectionResult::NotDetected => "not detected".to_string(),
            };

            if detection.is_detected() {
                detected_indices.push(i);
            }

            writeln!(writer, "  [{}] {} — {}", i + 1, target.name(), status)?;
        }
        writeln!(writer)?;

        if !detected_indices.is_empty() {
            write!(
                writer,
                "Install Memex MCP server for detected agents? [Y/n/all/1-{}/q]: ",
                detections.len()
            )?;
            writer.flush()?;

            let mut input = String::new();
            reader.read_line(&mut input)?;
            let trimmed = input.trim().to_lowercase();

            if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
                detected_indices
                    .iter()
                    .map(|&idx| detections[idx].0.id())
                    .collect()
            } else if trimmed == "all" {
                registry.targets().iter().map(|t| t.id()).collect()
            } else if trimmed == "n" || trimmed == "no" || trimmed == "q" || trimmed == "quit" {
                writeln!(writer, "Installation cancelled by user.")?;
                return Ok(Vec::new());
            } else {
                // Check if user entered numbers or target names
                let mut selected = Vec::new();
                for token in trimmed.split(&[',', ' '][..]) {
                    let token = token.trim();
                    if token.is_empty() {
                        continue;
                    }
                    if let Ok(num) = token.parse::<usize>() {
                        if num >= 1 && num <= detections.len() {
                            let tid = detections[num - 1].0.id();
                            if !selected.contains(&tid) {
                                selected.push(tid);
                            }
                        }
                    } else if let Some(target) = registry.get(token) {
                        let tid = target.id();
                        if !selected.contains(&tid) {
                            selected.push(tid);
                        }
                    }
                }
                selected
            }
        } else {
            write!(
                writer,
                "No supported agents detected. Install Memex MCP server for all supported agents? [y/N]: "
            )?;
            writer.flush()?;

            let mut input = String::new();
            reader.read_line(&mut input)?;
            let trimmed = input.trim().to_lowercase();

            if trimmed == "y" || trimmed == "yes" {
                registry.targets().iter().map(|t| t.id()).collect()
            } else {
                writeln!(
                    writer,
                    "No changes made. You can specify --target <name> to install for a specific agent."
                )?;
                return Ok(Vec::new());
            }
        }
    };

    if targets_to_install.is_empty() {
        if target.is_none() && yes {
            writeln!(
                writer,
                "ℹ No supported AI coding agents detected on this system."
            )?;
            writeln!(
                writer,
                "  Use 'memex install --target <claude|cursor|antigravity>' to force configuration."
            )?;
        }
        return Ok(Vec::new());
    }

    let mut installed = Vec::new();
    for target_id in &targets_to_install {
        let agent = registry
            .get(target_id)
            .ok_or_else(|| MemexError::Installer(format!("Unknown agent target: '{target_id}'")))?;

        agent.install(options)?;
        installed.push(*target_id);

        let detection = agent.detect(options)?;
        let config_str = detection
            .config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "default config".to_string());

        writeln!(
            writer,
            "✓ Configured Memex MCP server for {} ({})",
            agent.name(),
            config_str
        )?;

        if *target_id == "claude" {
            let claude = ClaudeTarget;
            if let Ok(settings_path) = claude.resolve_settings_path(options) {
                writeln!(
                    writer,
                    "  Granted permissions in {} (allow: mcp__memex__*)",
                    settings_path.display()
                )?;
            }
        }
    }

    writeln!(writer)?;
    writeln!(
        writer,
        "✓ Successfully configured Memex MCP server for {} agent(s).",
        installed.len()
    )?;
    writeln!(
        writer,
        "Restart your AI coding agent(s) to activate documentation context tools:"
    )?;
    writeln!(writer, "  • search_documentation")?;
    writeln!(writer, "  • traverse_graph")?;

    Ok(installed)
}

/// Executes the `install` command.
pub fn run_install(target: Option<&str>, yes: bool) -> Result<()> {
    let options = InstallOptions::default();
    let registry = TargetRegistry::with_defaults();
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();

    install_with_options(target, yes, &options, &registry, &mut stdin, &mut stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::config_writer::read_json_value;
    use std::io::Cursor;
    use tempfile::TempDir;

    #[test]
    fn test_parse_target_ids_valid() {
        let registry = TargetRegistry::with_defaults();

        let parsed = parse_target_ids("claude", &registry).unwrap();
        assert_eq!(parsed, vec!["claude"]);

        let parsed_multi = parse_target_ids("claude,cursor", &registry).unwrap();
        assert_eq!(parsed_multi, vec!["claude", "cursor"]);

        let parsed_all = parse_target_ids("all", &registry).unwrap();
        assert_eq!(parsed_all.len(), 3);
        assert!(parsed_all.contains(&"claude"));
        assert!(parsed_all.contains(&"cursor"));
        assert!(parsed_all.contains(&"antigravity"));
    }

    #[test]
    fn test_parse_target_ids_unknown_fails() {
        let registry = TargetRegistry::with_defaults();
        let err = parse_target_ids("invalid_agent", &registry);
        assert!(err.is_err());
        match err {
            Err(MemexError::Installer(msg)) => {
                assert!(msg.contains("Unknown agent target 'invalid_agent'"));
                assert!(msg.contains("claude"));
            }
            _ => panic!("Expected Installer error"),
        }
    }

    #[test]
    fn test_install_non_interactive_with_target() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(&home_dir).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"");
        let mut output = Vec::new();

        let installed = install_with_options(
            Some("claude"),
            true,
            &options,
            &registry,
            &mut input,
            &mut output,
        )
        .expect("install should succeed");

        assert_eq!(installed, vec!["claude"]);

        let out_str = String::from_utf8(output).unwrap();
        assert!(out_str.contains("Configured Memex MCP server for Claude Code"));
        assert!(out_str.contains("Granted permissions in"));
        assert!(out_str.contains("search_documentation"));

        // Verify config was written
        let claude_json = home_dir.join(".claude.json");
        assert!(claude_json.exists());
        let val = read_json_value(&claude_json).unwrap().unwrap();
        assert_eq!(val["mcpServers"]["memex"]["command"], "memex");
    }

    #[test]
    fn test_install_non_interactive_auto_detect() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        // Simulate Cursor installed
        std::fs::create_dir_all(home_dir.join(".cursor")).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, true, &options, &registry, &mut input, &mut output)
                .expect("install should succeed");

        assert_eq!(installed, vec!["cursor"]);

        let cursor_mcp = home_dir.join(".cursor").join("mcp.json");
        assert!(cursor_mcp.exists());
    }

    #[test]
    fn test_install_non_interactive_no_agents_detected() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("empty_home");
        std::fs::create_dir_all(&home_dir).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, true, &options, &registry, &mut input, &mut output).unwrap();

        assert!(installed.is_empty());
        let out_str = String::from_utf8(output).unwrap();
        assert!(out_str.contains("No supported AI coding agents detected"));
    }

    #[test]
    fn test_install_interactive_confirm_yes() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(home_dir.join(".claude")).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, false, &options, &registry, &mut input, &mut output)
                .unwrap();

        assert_eq!(installed, vec!["claude"]);
        let out_str = String::from_utf8(output).unwrap();
        assert!(out_str.contains("Memex AI Coding Agent Auto-Installer"));
        assert!(out_str.contains("Claude Code"));
    }

    #[test]
    fn test_install_interactive_cancel() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(home_dir.join(".claude")).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"n\n");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, false, &options, &registry, &mut input, &mut output)
                .unwrap();

        assert!(installed.is_empty());
        let out_str = String::from_utf8(output).unwrap();
        assert!(out_str.contains("Installation cancelled by user"));
    }

    #[test]
    fn test_install_interactive_numeric_selection() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(home_dir.join(".claude")).unwrap();
        std::fs::create_dir_all(home_dir.join(".cursor")).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        // Select item 1 (Claude Code) only
        let mut input = Cursor::new(b"1\n");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, false, &options, &registry, &mut input, &mut output)
                .unwrap();

        assert_eq!(installed, vec!["claude"]);
        assert!(home_dir.join(".claude.json").exists());
        assert!(!home_dir.join(".cursor").join("mcp.json").exists());
    }

    #[test]
    fn test_install_interactive_no_agents_confirm_yes() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("empty_home");
        std::fs::create_dir_all(&home_dir).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();

        let installed =
            install_with_options(None, false, &options, &registry, &mut input, &mut output)
                .unwrap();

        assert_eq!(installed.len(), 3);
        assert!(home_dir.join(".claude.json").exists());
        assert!(home_dir.join(".cursor").join("mcp.json").exists());
        assert!(
            home_dir
                .join(".gemini")
                .join("antigravity-ide")
                .join("mcp_config.json")
                .exists()
        );
    }

    #[test]
    fn test_install_all_targets_non_interactive() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(&home_dir).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"");
        let mut output = Vec::new();

        let installed = install_with_options(
            Some("all"),
            true,
            &options,
            &registry,
            &mut input,
            &mut output,
        )
        .expect("install all should succeed");

        assert_eq!(installed.len(), 3);
        assert!(home_dir.join(".claude.json").exists());
        assert!(home_dir.join(".cursor").join("mcp.json").exists());
        assert!(
            home_dir
                .join(".gemini")
                .join("antigravity-ide")
                .join("mcp_config.json")
                .exists()
        );
    }

    #[test]
    fn test_install_idempotency() {
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        std::fs::create_dir_all(&home_dir).unwrap();

        let options = InstallOptions::new().with_home_dir(&home_dir);
        let registry = TargetRegistry::with_defaults();

        let mut input = Cursor::new(b"");
        let mut output1 = Vec::new();
        install_with_options(
            Some("claude"),
            true,
            &options,
            &registry,
            &mut input,
            &mut output1,
        )
        .unwrap();

        let mut output2 = Vec::new();
        install_with_options(
            Some("claude"),
            true,
            &options,
            &registry,
            &mut input,
            &mut output2,
        )
        .unwrap();

        let claude_json = home_dir.join(".claude.json");
        let val = read_json_value(&claude_json).unwrap().unwrap();
        assert_eq!(val["mcpServers"]["memex"]["command"], "memex");
    }
}
