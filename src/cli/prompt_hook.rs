use std::io::{self, Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::index::find_project_root;
use crate::errors::Result;
use crate::ingestion::embedder::EmbeddingEngine;
use crate::ingestion::embedder::ModelManager;
use crate::mcp::tools::search_documentation_with_reader;
use crate::storage::db::Database;
use crate::storage::reader::StorageReader;

/// Payload received from Claude Code via stdin for UserPromptSubmit hook.
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
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
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

/// Executes the `memex prompt-hook` CLI command.
///
/// Reads user prompt from stdin (JSON or plain text), finds the project root,
/// queries top-k documentation chunks using semantic search, and outputs
/// structured XML context to stdout for Claude Code.
pub fn run_prompt_hook() -> Result<()> {
    let mut stdin_buffer = String::new();
    let _ = io::stdin().read_to_string(&mut stdin_buffer);

    let parsed_input = serde_json::from_str::<PromptHookInput>(&stdin_buffer).ok();

    let prompt_text = if let Some(ref parsed) = parsed_input {
        parsed
            .prompt
            .as_ref()
            .or(parsed.query.as_ref())
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdin_buffer.trim().to_string())
    } else {
        stdin_buffer.trim().to_string()
    };

    if prompt_text.is_empty() {
        return Ok(());
    }

    let cwd = parsed_input
        .as_ref()
        .and_then(|p| p.cwd.as_deref().or(p.project_path.as_deref()))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let root = match find_project_root(&cwd) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let mut db_path = root.join(".memex").join("memex.db");
    if !db_path.exists() {
        let alt = root.join(".memex").join("index.db");
        if alt.exists() {
            db_path = alt;
        } else {
            return Ok(());
        }
    }

    let db = match Database::open_readonly(&db_path) {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };

    let assets = match ModelManager::ensure_model_assets() {
        Ok(a) => a,
        Err(_) => return Ok(()),
    };

    let engine = match EmbeddingEngine::new(&assets) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let reader = StorageReader::new(db.conn());
    let results = match search_documentation_with_reader(&reader, &engine, &prompt_text, 3) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    if results.is_empty() {
        return Ok(());
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

    // Format XML context matching Claude Code agent harness expectations
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
    let _ = writeln!(stdout, "{}", xml_output);
    let _ = stdout.flush();

    // NOTE: Preserved legacy JSON output mode for future protocol changes or alternative harnesses.
    // To re-enable JSON mode, uncomment the following block and disable direct XML write above:
    /*
    let output = PromptHookOutput {
        hook_specific_output: Some(HookSpecificOutput {
            additional_context: Some(xml_output),
        }),
    };
    if let Ok(json_str) = serde_json::to_string(&output) {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{}", json_str);
        let _ = stdout.flush();
    }
    */

    Ok(())
}
