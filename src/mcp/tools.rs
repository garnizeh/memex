//! MCP tool implementations for documentation search and graph traversal.

use crate::errors::{MemexError, Result};
use crate::ingestion::embedder::{EmbeddingEngine, ModelManager};
use crate::mcp::types::{CallToolResult, SearchDocumentationParams};
use crate::storage::db::Database;
use crate::storage::reader::{SearchResult, StorageReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Default limit for search results if not specified or set to 0.
pub const DEFAULT_SEARCH_LIMIT: usize = 5;

/// Maximum allowed limit for search results.
pub const MAX_SEARCH_LIMIT: usize = 20;

/// Structured representation of a documentation chunk returned by semantic search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocSearchResult {
    /// Unique SHA256 ID of the matched chunk.
    pub chunk_id: String,
    /// Relative file path of the source document.
    pub file_path: String,
    /// Optional document title.
    pub document_title: Option<String>,
    /// Hierarchical heading path breadcrumb (e.g. `["Authentication", "OAuth2"]`).
    pub heading_path: Vec<String>,
    /// Raw un-prefixed content of the chunk.
    pub content: String,
    /// 1-indexed starting line number in the source file.
    pub line_start: u32,
    /// 1-indexed ending line number in the source file.
    pub line_end: u32,
    /// Normalized cosine similarity score in the range [0.0, 1.0].
    pub similarity_score: f32,
    /// Raw L2 vector distance computed by `sqlite-vec`.
    pub distance: f32,
}

impl From<SearchResult> for DocSearchResult {
    fn from(res: SearchResult) -> Self {
        Self {
            chunk_id: res.chunk.id,
            file_path: res.file_path,
            document_title: res.document_title,
            heading_path: res.chunk.heading_path,
            content: res.chunk.content,
            line_start: res.chunk.line_start,
            line_end: res.chunk.line_end,
            similarity_score: res.score,
            distance: res.distance,
        }
    }
}

/// Formats a list of search results into Markdown suitable for returning to an LLM / MCP client.
pub fn format_search_markdown(query: &str, results: &[DocSearchResult]) -> String {
    let mut out = String::new();
    out.push_str(&format!("## Results for: \"{}\"\n\n", query.trim()));

    if results.is_empty() {
        out.push_str("No matching documentation found.\n");
        return out;
    }

    for (i, res) in results.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }

        let heading_suffix = if res.heading_path.is_empty() {
            String::new()
        } else {
            format!(" > {}", res.heading_path.join(" > "))
        };

        out.push_str(&format!(
            "### {}. {}{} (lines {}-{}, score: {:.2})\n{}",
            i + 1,
            res.file_path,
            heading_suffix,
            res.line_start,
            res.line_end,
            res.similarity_score,
            res.content.trim()
        ));
    }

    out
}

/// Normalizes and clamps the search result limit to valid bounds `[1, MAX_SEARCH_LIMIT]`.
pub fn normalize_search_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_SEARCH_LIMIT
    } else {
        limit.min(MAX_SEARCH_LIMIT)
    }
}

/// Executes documentation semantic search using a provided [`StorageReader`] and [`EmbeddingEngine`].
pub fn search_documentation_with_reader(
    reader: &StorageReader,
    engine: &EmbeddingEngine,
    query: &str,
    limit: usize,
) -> Result<Vec<DocSearchResult>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let effective_limit = normalize_search_limit(limit);

    // 1. Embed query
    let query_vec = engine.embed(trimmed)?;

    // 2. Perform KNN vector similarity search
    let raw_results = reader.search_knn(&query_vec, effective_limit)?;

    // 3. Map to structured DocSearchResult
    let results: Vec<DocSearchResult> = raw_results.into_iter().map(DocSearchResult::from).collect();

    Ok(results)
}

/// Handles the MCP `search_documentation` tool invocation given strongly-typed parameters.
pub fn handle_search_documentation(
    reader: &StorageReader,
    engine: &EmbeddingEngine,
    params: SearchDocumentationParams,
) -> Result<CallToolResult> {
    let results = search_documentation_with_reader(reader, engine, &params.query, params.limit)?;
    let markdown = format_search_markdown(&params.query, &results);
    Ok(CallToolResult::text(markdown))
}

/// Handles the MCP `search_documentation` tool invocation from raw JSON parameters.
pub fn handle_search_documentation_json(
    reader: &StorageReader,
    engine: &EmbeddingEngine,
    arguments: Option<Value>,
) -> CallToolResult {
    let params: SearchDocumentationParams = match arguments {
        Some(val) => match serde_json::from_value(val) {
            Ok(p) => p,
            Err(e) => return CallToolResult::error(format!("Invalid search_documentation arguments: {e}")),
        },
        None => return CallToolResult::error("Missing required 'query' argument for search_documentation"),
    };

    match handle_search_documentation(reader, engine, params) {
        Ok(result) => result,
        Err(e) => CallToolResult::error(format!("Search failed: {e}")),
    }
}

/// Programmatic helper to execute `search_documentation` directly against a project path.
///
/// Discovers the `.memex/index.db` file, loads the embedding engine, and executes the search.
pub fn search_documentation(
    project_path: impl AsRef<Path>,
    query: &str,
    limit: usize,
) -> Result<Vec<DocSearchResult>> {
    let root = crate::cli::index::find_project_root(project_path.as_ref())?;
    let mut db_path = root.join(".memex").join("memex.db");
    if !db_path.exists() {
        let alt_path = root.join(".memex").join("index.db");
        if alt_path.exists() {
            db_path = alt_path;
        } else {
            return Err(MemexError::NotInitialized {
                path: project_path.as_ref().display().to_string(),
            });
        }
    }

    let db = Database::open_readonly(&db_path)?;
    let reader = StorageReader::new(db.conn());

    let assets = ModelManager::ensure_model_assets()?;
    let engine = EmbeddingEngine::new(&assets)?;

    search_documentation_with_reader(&reader, &engine, query, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::init::init_project_with_embedder;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_normalize_search_limit() {
        assert_eq!(normalize_search_limit(0), 5);
        assert_eq!(normalize_search_limit(1), 1);
        assert_eq!(normalize_search_limit(5), 5);
        assert_eq!(normalize_search_limit(20), 20);
        assert_eq!(normalize_search_limit(50), 20);
        assert_eq!(normalize_search_limit(100), 20);
    }

    #[test]
    fn test_format_search_markdown_empty() {
        let md = format_search_markdown("nonexistent topic", &[]);
        assert!(md.contains("## Results for: \"nonexistent topic\""));
        assert!(md.contains("No matching documentation found."));
    }

    #[test]
    fn test_format_search_markdown_with_results() {
        let items = vec![
            DocSearchResult {
                chunk_id: "chk_1".to_string(),
                file_path: "docs/api/auth.md".to_string(),
                document_title: Some("Auth".to_string()),
                heading_path: vec!["Authentication".to_string(), "OAuth2".to_string()],
                content: "The client must send a Bearer token in the Authorization header."
                    .to_string(),
                line_start: 45,
                line_end: 67,
                similarity_score: 0.8912,
                distance: 0.46,
            },
            DocSearchResult {
                chunk_id: "chk_2".to_string(),
                file_path: "docs/api/auth.md".to_string(),
                document_title: Some("Auth".to_string()),
                heading_path: vec!["Authentication".to_string(), "Token Refresh".to_string()],
                content: "Access tokens expire after 3600 seconds.".to_string(),
                line_start: 70,
                line_end: 85,
                similarity_score: 0.7634,
                distance: 0.68,
            },
            DocSearchResult {
                chunk_id: "chk_3".to_string(),
                file_path: "README.md".to_string(),
                document_title: None,
                heading_path: vec![],
                content: "Project overview documentation.".to_string(),
                line_start: 1,
                line_end: 10,
                similarity_score: 0.65,
                distance: 0.83,
            },
        ];

        let md = format_search_markdown("how does OAuth2 authentication work", &items);

        assert!(md.contains("## Results for: \"how does OAuth2 authentication work\""));
        assert!(md.contains("### 1. docs/api/auth.md > Authentication > OAuth2 (lines 45-67, score: 0.89)"));
        assert!(md.contains("The client must send a Bearer token in the Authorization header."));
        assert!(md.contains("### 2. docs/api/auth.md > Authentication > Token Refresh (lines 70-85, score: 0.76)"));
        assert!(md.contains("Access tokens expire after 3600 seconds."));
        assert!(md.contains("### 3. README.md (lines 1-10, score: 0.65)"));
        assert!(md.contains("Project overview documentation."));
    }

    #[test]
    fn test_handle_search_documentation_json_argument_validation() {
        let db = Database::open_in_memory().unwrap();
        crate::storage::schema::initialize_schema(db.conn()).unwrap();
        let reader = StorageReader::new(db.conn());

        let assets = match ModelManager::ensure_model_assets() {
            Ok(a) => a,
            Err(_) => return,
        };
        let engine = match EmbeddingEngine::new(&assets) {
            Ok(e) => e,
            Err(_) => return,
        };

        // Missing arguments entirely
        let res_none = handle_search_documentation_json(&reader, &engine, None);
        assert_eq!(res_none.is_error, Some(true));
        match &res_none.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("Missing required 'query' argument"));
            }
        }

        // Invalid JSON type (missing query field)
        let res_invalid = handle_search_documentation_json(&reader, &engine, Some(json!({"limit": 5})));
        assert_eq!(res_invalid.is_error, Some(true));
        match &res_invalid.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("Invalid search_documentation arguments"));
            }
        }

        // Valid arguments with empty DB
        let res_valid = handle_search_documentation_json(
            &reader,
            &engine,
            Some(json!({"query": "OAuth2 flow", "limit": 3})),
        );
        assert_eq!(res_valid.is_error, None);
        match &res_valid.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("## Results for: \"OAuth2 flow\""));
                assert!(text.contains("No matching documentation found."));
            }
        }
    }

    #[test]
    fn test_search_empty_query_returns_graceful_response() {
        let db = Database::open_in_memory().unwrap();
        crate::storage::schema::initialize_schema(db.conn()).unwrap();
        let reader = StorageReader::new(db.conn());

        let assets = match ModelManager::ensure_model_assets() {
            Ok(a) => a,
            Err(_) => return,
        };
        let engine = match EmbeddingEngine::new(&assets) {
            Ok(e) => e,
            Err(_) => return,
        };

        let res_empty = search_documentation_with_reader(&reader, &engine, "", 5).unwrap();
        assert!(res_empty.is_empty());

        let res_spaces = search_documentation_with_reader(&reader, &engine, "   \t\n  ", 5).unwrap();
        assert!(res_spaces.is_empty());
    }

    #[test]
    fn test_search_documentation_end_to_end_indexed_project() {
        let assets = match ModelManager::ensure_model_assets() {
            Ok(a) => a,
            Err(_) => return,
        };
        let engine = match EmbeddingEngine::new(&assets) {
            Ok(e) => e,
            Err(_) => return,
        };

        let temp = TempDir::new().unwrap();
        let project_dir = temp.path();

        // Create sample documentation files
        let docs_dir = project_dir.join("docs");
        fs::create_dir_all(&docs_dir).unwrap();

        fs::write(
            docs_dir.join("auth.md"),
            "# Authentication\n\n## OAuth2\n\nThe client must send a Bearer token in the Authorization header. Tokens are obtained via the /oauth/token endpoint using client credentials.\n\n## Token Refresh\n\nAccess tokens expire after 3600 seconds. Use the refresh token to obtain a new access token.\n",
        )
        .unwrap();

        fs::write(
            docs_dir.join("database.md"),
            "# Database Setup\n\n## SQLite Configuration\n\nMemex uses SQLite with WAL mode enabled for concurrent read transactions.\n",
        )
        .unwrap();

        // Initialize and index the project
        init_project_with_embedder(project_dir, false, false, &engine)
            .expect("init_project_with_embedder should succeed");

        // 1. Search for OAuth2 authentication
        let results = search_documentation(project_dir, "OAuth2 authentication flow", 5)
            .expect("search_documentation should succeed");

        assert!(!results.is_empty(), "should return results");
        assert!(
            results[0].file_path.contains("auth.md"),
            "top result should be from auth.md, got: {}",
            results[0].file_path
        );
        assert!(
            results[0]
                .heading_path
                .iter()
                .any(|h| h.contains("OAuth") || h.contains("Auth")),
            "top result should be in the authentication section, got: {:?}",
            results[0].heading_path
        );
        assert!(
            results[0].similarity_score > 0.4,
            "top result should have substantial similarity score, got: {}",
            results[0].similarity_score
        );
        assert!(
            !results[0].chunk_id.is_empty(),
            "chunk_id must be populated"
        );

        // 2. Test MCP JSON tool handler on indexed db
        let db_path = project_dir.join(".memex").join("memex.db");
        let db = Database::open_readonly(&db_path).unwrap();
        let reader = StorageReader::new(db.conn());

        let tool_res = handle_search_documentation_json(
            &reader,
            &engine,
            Some(json!({
                "query": "OAuth2 authentication flow",
                "limit": 2
            })),
        );

        assert_eq!(tool_res.is_error, None);
        match &tool_res.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("## Results for: \"OAuth2 authentication flow\""));
                assert!(text.contains("auth.md"));
                assert!(text.contains("score:"));
            }
        }

        // 3. Search for database topic
        let db_results = search_documentation(project_dir, "SQLite WAL mode concurrent", 5)
            .expect("search_documentation should succeed for db query");
        assert!(!db_results.is_empty());
        assert!(
            db_results[0].file_path.contains("database.md"),
            "top result should be from database.md, got: {}",
            db_results[0].file_path
        );
    }
}
