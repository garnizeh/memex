use std::io::{self, Read, Write};

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
/// structured additional context to stdout for Claude Code.
pub fn run_prompt_hook() -> Result<()> {
    let mut stdin_buffer = String::new();
    let _ = io::stdin().read_to_string(&mut stdin_buffer);

    let prompt_text = if let Ok(parsed) = serde_json::from_str::<PromptHookInput>(&stdin_buffer) {
        parsed
            .prompt
            .or(parsed.query)
            .unwrap_or(stdin_buffer.trim().to_string())
    } else {
        stdin_buffer.trim().to_string()
    };

    if prompt_text.is_empty() {
        return Ok(());
    }

    let cwd = std::env::current_dir().unwrap_or_default();
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

    let mut context_md = String::from(
        "<!-- MEMEX_DOCS_START -->\n### Relevant Project Documentation (via Memex):\n",
    );
    for (i, item) in results.iter().enumerate() {
        context_md.push_str(&format!(
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
    context_md.push_str("\n<!-- MEMEX_DOCS_END -->\n");

    let output = PromptHookOutput {
        hook_specific_output: Some(HookSpecificOutput {
            additional_context: Some(context_md),
        }),
    };

    if let Ok(json_str) = serde_json::to_string(&output) {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{}", json_str);
        let _ = stdout.flush();
    }

    Ok(())
}
