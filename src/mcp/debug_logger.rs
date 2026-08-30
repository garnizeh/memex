use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::index::find_project_root;

/// Concurrent-safe MCP debug logger that writes incoming MCP requests and client identification
/// to `.memex/debug_mcp.log`.
#[derive(Debug, Clone)]
pub struct McpDebugLogger {
    log_path: PathBuf,
    client_name: Arc<Mutex<String>>,
}

impl McpDebugLogger {
    /// Creates a new `McpDebugLogger` targeting the nearest `.memex/` directory.
    pub fn new(target_path: Option<&Path>) -> Option<Self> {
        let base_dir = if let Some(path) = target_path {
            find_project_root(path).unwrap_or_else(|_| path.to_path_buf())
        } else if let Ok(cwd) = std::env::current_dir() {
            find_project_root(&cwd).unwrap_or(cwd)
        } else {
            return None;
        };

        let memex_dir = base_dir.join(".memex");
        if !memex_dir.exists() {
            let _ = std::fs::create_dir_all(&memex_dir);
        }

        let log_path = memex_dir.join("debug_mcp.log");
        Some(Self {
            log_path,
            client_name: Arc::new(Mutex::new("Unknown".to_string())),
        })
    }

    /// Creates a logger explicitly pointing to a specific log file path (useful for testing).
    pub fn with_log_path(log_path: PathBuf) -> Self {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self {
            log_path,
            client_name: Arc::new(Mutex::new("Unknown".to_string())),
        }
    }

    /// Updates the detected client identity based on `params.clientInfo.name` from an `initialize` request.
    pub fn on_initialize(&self, params: Option<&serde_json::Value>) {
        if let Some(params_val) = params {
            let client_info = params_val.get("clientInfo");
            let raw_name = client_info
                .and_then(|ci| ci.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");

            let version = client_info
                .and_then(|ci| ci.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let normalized = if raw_name.to_lowercase().contains("antigravity")
                || raw_name.to_lowercase().contains("gemini")
            {
                if version.is_empty() {
                    "Antigravity".to_string()
                } else {
                    format!("Antigravity (v{version})")
                }
            } else if raw_name.to_lowercase().contains("claude") {
                if version.is_empty() {
                    "Claude Code".to_string()
                } else {
                    format!("Claude Code (v{version})")
                }
            } else if !raw_name.is_empty() {
                if version.is_empty() {
                    raw_name.to_string()
                } else {
                    format!("{raw_name} (v{version})")
                }
            } else {
                "Unknown".to_string()
            };

            if let Ok(mut lock) = self.client_name.lock() {
                *lock = normalized;
            }
        }

        self.log_event("initialize", params);
    }

    /// Logs an MCP event or request payload atomically to the log file.
    pub fn log_event(&self, method: &str, payload: Option<&serde_json::Value>) {
        let client = self
            .client_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Unknown".to_string());

        let timestamp = current_iso_timestamp();
        let pid = std::process::id();

        let payload_str = match payload {
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            None => "None".to_string(),
        };

        let log_line = format!(
            "[{timestamp}] [PID:{pid}] [CLIENT:{client}] [METHOD:{method}] PAYLOAD: {payload_str}\n"
        );

        self.append_line(&log_line);
    }

    /// Logs a specific tool call execution (`search_documentation` or `traverse_graph`).
    pub fn log_tool_call(&self, tool_name: &str, tool_args: Option<&serde_json::Value>) {
        let client = self
            .client_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Unknown".to_string());

        let timestamp = current_iso_timestamp();
        let pid = std::process::id();

        let args_str = match tool_args {
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };

        let log_line = format!(
            "[{timestamp}] [PID:{pid}] [CLIENT:{client}] [TOOL:{tool_name}] ARGS: {args_str}\n"
        );

        self.append_line(&log_line);
    }

    /// Logs a prompt-hook execution event atomically to the given log file.
    pub fn log_hook_event(
        log_path: &Path,
        client: &str,
        prompt: &str,
        result_summary: Option<&str>,
    ) {
        let timestamp = current_iso_timestamp();
        let pid = std::process::id();

        let prompt_preview = if prompt.len() > 120 {
            format!("{}...", &prompt[..120])
        } else {
            prompt.to_string()
        };

        let summary_str = result_summary.unwrap_or("No context injected");

        let log_line = format!(
            "[{timestamp}] [PID:{pid}] [CLIENT:{client}] [HOOK:prompt-hook] PROMPT: {prompt_preview:?} | INJECTED: {summary_str}\n"
        );

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
    }

    /// Appends a formatted line atomically using `O_APPEND` single buffer write.
    fn append_line(&self, line: &str) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}

/// Helper function to produce an ISO-like UTC timestamp without heavy external dependencies.
fn current_iso_timestamp() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();

    let days_since_epoch = total_secs / 86400;
    let day_secs = total_secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Simple calendar day calculation from Unix epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

/// Computes (year, month, day) from days elapsed since 1970-01-01.
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;
    loop {
        let leap = is_leap_year(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1;
    for &d in &month_days {
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }

    let day = days + 1;
    (year, month, day)
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_mcp_debug_logger_initialization_and_logging() {
        let temp_file = NamedTempFile::new().unwrap();
        let log_path = temp_file.path().to_path_buf();
        let logger = McpDebugLogger::with_log_path(log_path.clone());

        // 1. Initial log before initialize
        logger.log_event("ping", None);

        // 2. Initialize with Antigravity
        let antigravity_params = serde_json::json!({
            "clientInfo": {
                "name": "antigravity",
                "version": "1.0.0"
            }
        });
        logger.on_initialize(Some(&antigravity_params));

        // 3. Tool call
        let tool_args = serde_json::json!({
            "query": "how to configure memex",
            "limit": 5
        });
        logger.log_tool_call("search_documentation", Some(&tool_args));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[CLIENT:Unknown] [METHOD:ping]"));
        assert!(content.contains("[CLIENT:Antigravity (v1.0.0)] [METHOD:initialize]"));
        assert!(content.contains("[CLIENT:Antigravity (v1.0.0)] [TOOL:search_documentation]"));
        assert!(content.contains("how to configure memex"));
    }

    #[test]
    fn test_mcp_debug_logger_claude_identification() {
        let temp_file = NamedTempFile::new().unwrap();
        let log_path = temp_file.path().to_path_buf();
        let logger = McpDebugLogger::with_log_path(log_path.clone());

        let claude_params = serde_json::json!({
            "clientInfo": {
                "name": "claude-code",
                "version": "0.2.29"
            }
        });
        logger.on_initialize(Some(&claude_params));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[CLIENT:Claude Code (v0.2.29)]"));
    }

    #[test]
    fn test_mcp_debug_logger_concurrent_writes() {
        let temp_file = NamedTempFile::new().unwrap();
        let log_path = temp_file.path().to_path_buf();
        let logger = McpDebugLogger::with_log_path(log_path.clone());

        let mut handles = Vec::new();
        for i in 0..10 {
            let logger_clone = logger.clone();
            handles.push(std::thread::spawn(move || {
                for j in 0..50 {
                    logger_clone.log_tool_call(
                        "search_documentation",
                        Some(&serde_json::json!({ "worker": i, "iteration": j })),
                    );
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 500);
    }
}
