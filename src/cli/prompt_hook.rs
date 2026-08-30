use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cli::index::find_project_root;
use crate::errors::Result;
use crate::ingestion::embedder::EmbeddingEngine;
use crate::ingestion::embedder::ModelManager;
use crate::mcp::tools::search_documentation_with_reader;
use crate::storage::db::Database;
use crate::storage::reader::StorageReader;

/// Turn cache TTL in seconds for deduplicating repeated prompt-hook calls in multi-step agent turns.
pub const TURN_CACHE_TTL_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookCache {
    /// Maps conversation/session ID (or "global") to the last processed turn info.
    #[serde(default)]
    pub entries: HashMap<String, TurnCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCacheEntry {
    pub prompt_hash: String,
    pub timestamp_secs: u64,
}

/// Checks whether this prompt invocation is a duplicate within the active turn TTL window.
/// If it is a duplicate, returns `true`.
/// If it is new or expired, updates the cache file with the new prompt hash and timestamp, and returns `false`.
pub fn check_and_update_turn_cache(
    cache_path: &Path,
    conversation_id: Option<&str>,
    prompt: &str,
    ttl_secs: u64,
) -> bool {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let session_key = conversation_id.unwrap_or("global");

    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    let prompt_hash = hex::encode(hasher.finalize());

    let mut cache = if let Ok(file_content) = std::fs::read_to_string(cache_path) {
        serde_json::from_str::<HookCache>(&file_content).unwrap_or_default()
    } else {
        HookCache::default()
    };

    if let Some(entry) = cache.entries.get(session_key)
        && entry.prompt_hash == prompt_hash
        && now_secs.saturating_sub(entry.timestamp_secs) < ttl_secs
    {
        return true;
    }

    cache.entries.insert(
        session_key.to_string(),
        TurnCacheEntry {
            prompt_hash,
            timestamp_secs: now_secs,
        },
    );

    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(serialized) = serde_json::to_string(&cache) {
        let _ = std::fs::write(cache_path, serialized);
    }

    false
}

/// Payload received from Claude Code or Antigravity via stdin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptHookInput {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(rename = "workspacePaths", default)]
    pub workspace_paths: Option<Vec<String>>,
    #[serde(rename = "transcriptPath", default)]
    pub transcript_path: Option<String>,
    #[serde(rename = "conversationId", default)]
    pub conversation_id: Option<String>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
}

impl PromptHookInput {
    /// Returns true if this input matches Antigravity IDE lifecycle hook protocol.
    pub fn is_antigravity(&self) -> bool {
        self.workspace_paths.is_some()
            || self.transcript_path.is_some()
            || self.conversation_id.is_some()
            || self.extra.contains_key("invocationNum")
            || self.extra.contains_key("initialNumSteps")
    }
}

/// Structured response output for Claude Code UserPromptSubmit hook.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptHookOutput {
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookSpecificOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// Structured response output for Antigravity IDE PreInvocation hook.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AntigravityHookOutput {
    #[serde(rename = "injectSteps")]
    pub inject_steps: Vec<AntigravityInjectStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AntigravityInjectStep {
    #[serde(rename = "ephemeralMessage")]
    pub ephemeral_message: String,
}

/// Sanitizes raw user prompt text by stripping XML wrapper tags like `<USER_REQUEST>...</USER_REQUEST>`
/// and metadata blocks injected by IDE harnesses.
pub fn sanitize_prompt_text(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start_idx) = trimmed.find("<USER_REQUEST>") {
        let content_start = start_idx + "<USER_REQUEST>".len();
        if let Some(end_idx) = trimmed[content_start..].find("</USER_REQUEST>") {
            let extracted = &trimmed[content_start..content_start + end_idx];
            let clean = extracted.trim();
            if !clean.is_empty() {
                return clean.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// Attempts to extract the most recent user prompt text from an Antigravity transcript JSONL file.
pub fn extract_last_user_prompt_from_transcript(transcript_path: &Path) -> Option<String> {
    let file = File::open(transcript_path).ok()?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();

    for line in lines.into_iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(&line) {
            let is_user_input = val
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "USER_INPUT")
                .unwrap_or(false)
                || val
                    .get("source")
                    .and_then(|s| s.as_str())
                    .map(|s| s == "USER_EXPLICIT")
                    .unwrap_or(false);

            if is_user_input && let Some(content) = val.get("content").and_then(|c| c.as_str()) {
                let sanitized = sanitize_prompt_text(content);
                if !sanitized.is_empty() {
                    return Some(sanitized);
                }
            }
        }
    }

    None
}

/// Emits an empty response matching the agent target's protocol.
fn emit_empty_response(is_antigravity: bool) -> Result<()> {
    if is_antigravity {
        let empty_output = AntigravityHookOutput::default();
        if let Ok(json_str) = serde_json::to_string(&empty_output) {
            let mut stdout = io::stdout().lock();
            let _ = writeln!(stdout, "{}", json_str);
            let _ = stdout.flush();
        }
    }
    Ok(())
}

/// Executes the `memex prompt-hook` CLI command.
///
/// Reads user prompt or lifecycle hook context from stdin (JSON or plain text), finds the project root,
/// queries top-k documentation chunks using semantic search, and outputs structured XML context:
/// - Verbatim XML text for Claude Code
/// - Structured `injectSteps` JSON for Antigravity IDE
pub fn run_prompt_hook(debug: bool) -> Result<()> {
    let mut stdin_buffer = String::new();
    let _ = io::stdin().read_to_string(&mut stdin_buffer);

    let is_debug = debug
        || std::env::var("MEMEX_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

    let parsed_input = serde_json::from_str::<PromptHookInput>(&stdin_buffer).ok();
    let is_antigravity = parsed_input
        .as_ref()
        .map(|p| p.is_antigravity())
        .unwrap_or(false);

    let raw_prompt = if let Some(ref parsed) = parsed_input {
        parsed
            .prompt
            .as_ref()
            .or(parsed.query.as_ref())
            .map(|s| s.to_string())
            .or_else(|| {
                parsed
                    .transcript_path
                    .as_deref()
                    .and_then(|tp| extract_last_user_prompt_from_transcript(Path::new(tp)))
            })
            .unwrap_or_else(|| stdin_buffer.trim().to_string())
    } else {
        stdin_buffer.trim().to_string()
    };

    let prompt_text = sanitize_prompt_text(&raw_prompt);

    if prompt_text.is_empty() {
        return emit_empty_response(is_antigravity);
    }

    let cwd = parsed_input
        .as_ref()
        .and_then(|p| {
            p.cwd.as_deref().or(p.project_path.as_deref()).or_else(|| {
                p.workspace_paths
                    .as_ref()
                    .and_then(|w| w.first().map(String::as_str))
            })
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let client_tag = if is_antigravity {
        "Antigravity"
    } else {
        "Claude Code"
    };

    let root = match find_project_root(&cwd) {
        Ok(r) => r,
        Err(e) => {
            if is_debug {
                let log_path = cwd.join(".memex").join("debug_mcp.log");
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some(&format!("failed to find project root: {e}")),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    };

    let log_path = root.join(".memex").join("debug_mcp.log");
    let cache_path = root.join(".memex").join(".hook_cache");

    let conversation_id = parsed_input
        .as_ref()
        .and_then(|p| p.conversation_id.as_deref());
    if check_and_update_turn_cache(
        &cache_path,
        conversation_id,
        &prompt_text,
        TURN_CACHE_TTL_SECS,
    ) {
        if is_debug {
            crate::mcp::McpDebugLogger::log_hook_event(
                &log_path,
                client_tag,
                &prompt_text,
                Some("skipped duplicate prompt in active turn window (dedup hit)"),
            );
        }
        return emit_empty_response(is_antigravity);
    }

    let mut db_path = root.join(".memex").join("memex.db");
    if !db_path.exists() {
        let alt = root.join(".memex").join("index.db");
        if alt.exists() {
            db_path = alt;
        } else {
            if is_debug {
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some("database not found (.memex/memex.db)"),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    }

    let db = match Database::open_readonly(&db_path) {
        Ok(d) => d,
        Err(e) => {
            if is_debug {
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some(&format!("failed to open database: {e}")),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    };

    let assets = match ModelManager::ensure_model_assets() {
        Ok(a) => a,
        Err(e) => {
            if is_debug {
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some(&format!("failed to load model assets: {e}")),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    };

    let engine = match EmbeddingEngine::new(&assets) {
        Ok(e) => e,
        Err(e) => {
            if is_debug {
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some(&format!("failed to create embedding engine: {e}")),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    };

    let reader = StorageReader::new(db.conn());
    let results = match search_documentation_with_reader(&reader, &engine, &prompt_text, 3) {
        Ok(r) => r,
        Err(e) => {
            if is_debug {
                crate::mcp::McpDebugLogger::log_hook_event(
                    &log_path,
                    client_tag,
                    &prompt_text,
                    Some(&format!("search query failed: {e}")),
                );
            }
            return emit_empty_response(is_antigravity);
        }
    };

    if results.is_empty() {
        if is_debug {
            crate::mcp::McpDebugLogger::log_hook_event(
                &log_path,
                client_tag,
                &prompt_text,
                Some("0 results matched across 0 documents (no context injected)"),
            );
        }
        return emit_empty_response(is_antigravity);
    }

    // Count unique documents
    let mut unique_docs = std::collections::HashSet::new();
    for item in &results {
        unique_docs.insert(&item.file_path);
    }
    let doc_count = unique_docs.len();
    let result_count = results.len();

    let doc_str = if doc_count == 1 {
        "document"
    } else {
        "documents"
    };
    let res_str = if result_count == 1 {
        "result"
    } else {
        "results"
    };

    if is_debug {
        let summary = format!("{result_count} {res_str} across {doc_count} {doc_str}");
        crate::mcp::McpDebugLogger::log_hook_event(
            &log_path,
            client_tag,
            &prompt_text,
            Some(&summary),
        );
    }

    // Format XML context matching agent harness expectations
    let mut xml_output = format!(
        "<memex_context note=\"Semantic documentation search results from Memex\">\n**Exploration:** {}\n\nFound {} {} across {} {}.\n\n**Documentation References:**\n",
        prompt_text, result_count, res_str, doc_count, doc_str
    );

    for (i, item) in results.iter().enumerate() {
        xml_output.push_str(&format!(
            "\n#### [{}] {} > {} (lines {}-{}, score: {:.2})\n{}\n",
            i + 1,
            item.file_path,
            item.heading_path.join(" > "),
            item.line_start,
            item.line_end,
            item.similarity_score,
            item.content
        ));
    }
    xml_output.push_str("\n</memex_context>\n");

    let mut stdout = io::stdout().lock();
    if is_antigravity {
        let antigravity_response = AntigravityHookOutput {
            inject_steps: vec![AntigravityInjectStep {
                ephemeral_message: xml_output,
            }],
        };
        if let Ok(json_str) = serde_json::to_string(&antigravity_response) {
            let _ = writeln!(stdout, "{}", json_str);
            let _ = stdout.flush();
        }
    } else {
        let _ = writeln!(stdout, "{}", xml_output);
        let _ = stdout.flush();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_claude_code_input_detection() {
        let json_data = r#"{"prompt": "how does search work?", "cwd": "/path/to/project"}"#;
        let parsed: PromptHookInput = serde_json::from_str(json_data).unwrap();
        assert_eq!(parsed.prompt.as_deref(), Some("how does search work?"));
        assert_eq!(parsed.cwd.as_deref(), Some("/path/to/project"));
        assert!(!parsed.is_antigravity());
    }

    #[test]
    fn test_antigravity_input_detection() {
        let json_data = r#"{
            "invocationNum": 1,
            "conversationId": "test-uuid",
            "workspacePaths": ["/path/to/workspace"],
            "transcriptPath": "/path/to/transcript.jsonl"
        }"#;
        let parsed: PromptHookInput = serde_json::from_str(json_data).unwrap();
        assert!(parsed.is_antigravity());
        assert_eq!(
            parsed.workspace_paths.as_ref().unwrap()[0],
            "/path/to/workspace"
        );
    }

    #[test]
    fn test_extract_last_user_prompt_from_transcript() {
        use std::io::Write as IoWrite;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"{{"type":"SYSTEM_INIT","content":"system started"}}"#
        )
        .unwrap();
        writeln!(
            temp_file,
            r#"{{"type":"USER_INPUT","source":"USER_EXPLICIT","content":"how does memex indexing work?"}}"#
        )
        .unwrap();
        writeln!(
            temp_file,
            r#"{{"type":"PLANNER_RESPONSE","content":"I will inspect the codebase."}}"#
        )
        .unwrap();

        let extracted = extract_last_user_prompt_from_transcript(temp_file.path());
        assert_eq!(extracted.as_deref(), Some("how does memex indexing work?"));
    }

    #[test]
    fn test_sanitize_prompt_text() {
        let raw = "<USER_REQUEST>\nwhat does milestone 4 do?\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\ninfo\n</ADDITIONAL_METADATA>";
        assert_eq!(sanitize_prompt_text(raw), "what does milestone 4 do?");

        let clean = "how to configure memex?";
        assert_eq!(sanitize_prompt_text(clean), "how to configure memex?");
    }

    #[test]
    fn test_extract_last_user_prompt_from_transcript_with_metadata() {
        use std::io::Write as IoWrite;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"{{"type":"USER_INPUT","source":"USER_EXPLICIT","content":"<USER_REQUEST>\nwhat does milestone 4 do?\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nsome metadata\n</ADDITIONAL_METADATA>"}}"#
        )
        .unwrap();

        let extracted = extract_last_user_prompt_from_transcript(temp_file.path());
        assert_eq!(extracted.as_deref(), Some("what does milestone 4 do?"));
    }

    #[test]
    fn test_antigravity_output_json_serialization() {
        let output = AntigravityHookOutput {
            inject_steps: vec![AntigravityInjectStep {
                ephemeral_message: "<memex_context>test</memex_context>".to_string(),
            }],
        };
        let serialized = serde_json::to_string(&output).unwrap();
        assert!(serialized.contains("injectSteps"));
        assert!(serialized.contains("ephemeralMessage"));
        assert!(serialized.contains("<memex_context>test</memex_context>"));
    }

    #[test]
    fn test_turn_cache_deduplication() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_path = temp_dir.path().join(".hook_cache");

        let conv_id = Some("conv-123");
        let prompt_a = "how does indexing work?";
        let prompt_b = "explain vector embeddings";

        // 1. Initial call for prompt A -> not duplicate (returns false)
        assert!(!check_and_update_turn_cache(
            &cache_path,
            conv_id,
            prompt_a,
            60
        ));
        assert!(cache_path.exists());

        // 2. Second call for prompt A immediately -> duplicate (returns true)
        assert!(check_and_update_turn_cache(
            &cache_path,
            conv_id,
            prompt_a,
            60
        ));

        // 3. Different prompt B in same conversation -> not duplicate (returns false)
        assert!(!check_and_update_turn_cache(
            &cache_path,
            conv_id,
            prompt_b,
            60
        ));

        // 4. Repeated call for prompt B -> duplicate (returns true)
        assert!(check_and_update_turn_cache(
            &cache_path,
            conv_id,
            prompt_b,
            60
        ));

        // 5. Another conversation with prompt B -> not duplicate because of different session key
        let other_conv = Some("conv-456");
        assert!(!check_and_update_turn_cache(
            &cache_path,
            other_conv,
            prompt_b,
            60
        ));
        assert!(check_and_update_turn_cache(
            &cache_path,
            other_conv,
            prompt_b,
            60
        ));

        // 6. Test TTL expiration (ttl = 0 sec should always be considered expired on subsequent call)
        // With ttl_secs = 0, elapsed >= 0 is always true (unless timestamp is future, which it is not)
        // Let's verify with ttl_secs = 0
        assert!(!check_and_update_turn_cache(
            &cache_path,
            conv_id,
            prompt_b,
            0
        ));
    }
}
