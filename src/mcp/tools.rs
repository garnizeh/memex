//! MCP tool implementations for documentation search and graph traversal.

use crate::errors::{MemexError, Result};
use crate::ingestion::embedder::{EmbeddingEngine, ModelManager};
use crate::mcp::types::{CallToolResult, SearchDocumentationParams};
use crate::storage::db::Database;
use crate::storage::reader::{SearchResult, StorageReader, Subgraph};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Default limit for search results if not specified or set to 0.
pub const DEFAULT_SEARCH_LIMIT: usize = 5;

/// Maximum allowed limit for search results.
pub const MAX_SEARCH_LIMIT: usize = 20;

/// Default depth for graph traversal if not specified or set to 0.
pub const DEFAULT_TRAVERSE_DEPTH: u32 = 2;

/// Maximum allowed depth for graph traversal.
pub const MAX_TRAVERSE_DEPTH: u32 = 5;

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

/// Normalizes and clamps the graph traversal depth to valid bounds `[1, MAX_TRAVERSE_DEPTH]`.
pub fn normalize_traverse_depth(depth: usize) -> u32 {
    if depth == 0 {
        DEFAULT_TRAVERSE_DEPTH
    } else {
        (depth as u32).min(MAX_TRAVERSE_DEPTH)
    }
}

/// Formats a traversed [`Subgraph`] into structured Markdown for an LLM / MCP client.
pub fn format_subgraph_markdown(subgraph: &Subgraph) -> String {
    let root = match &subgraph.root {
        Some(r) => r,
        None => return "Chunk not found in knowledge graph.\n".to_string(),
    };

    let mut out = String::new();

    let root_heading_crumb = if root.heading_path.is_empty() {
        "Document Root".to_string()
    } else {
        root.heading_path.join(" > ")
    };

    out.push_str(&format!(
        "## Traversal Context for Chunk: `{}`\n**Section:** {}\n**Lines:** {}-{}\n\n",
        root.id, root_heading_crumb, root.line_start, root.line_end
    ));

    out.push_str("### Focal Chunk Content\n");
    out.push_str(root.content.trim());
    out.push_str("\n\n");

    // Other nodes in the subgraph
    let other_nodes: Vec<_> = subgraph.nodes.iter().filter(|n| n.id != root.id).collect();

    if !other_nodes.is_empty() {
        out.push_str("### Surrounding Context & Connected Nodes\n");
        for node in other_nodes {
            let heading_crumb = if node.heading_path.is_empty() {
                "Root".to_string()
            } else {
                node.heading_path.join(" > ")
            };

            out.push_str(&format!(
                "\n#### [{}] (lines {}-{})\n{}\n",
                heading_crumb,
                node.line_start,
                node.line_end,
                node.content.trim()
            ));
        }
    }

    if !subgraph.edges.is_empty() {
        out.push_str("\n### Graph Relationships\n");
        for edge in &subgraph.edges {
            match edge.edge_type {
                crate::models::EdgeType::Hierarchy => {
                    out.push_str(&format!(
                        "- Hierarchy: `{}` → `{}`\n",
                        edge.source_chunk_id, edge.target_chunk_id
                    ));
                }
                crate::models::EdgeType::ExplicitLink => {
                    let link_desc = edge
                        .link_text
                        .as_deref()
                        .map(|t| format!(" (text: \"{t}\")"))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "- Link{}: `{}` → `{}`\n",
                        link_desc, edge.source_chunk_id, edge.target_chunk_id
                    ));
                }
            }
        }
    }

    out
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
    let results: Vec<DocSearchResult> =
        raw_results.into_iter().map(DocSearchResult::from).collect();

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
            Err(e) => {
                return CallToolResult::error(format!(
                    "Invalid search_documentation arguments: {e}"
                ));
            }
        },
        None => {
            return CallToolResult::error(
                "Missing required 'query' argument for search_documentation",
            );
        }
    };

    match handle_search_documentation(reader, engine, params) {
        Ok(result) => result,
        Err(e) => CallToolResult::error(format!("Search failed: {e}")),
    }
}

/// Executes graph traversal around a chunk ID using a provided [`StorageReader`].
pub fn traverse_graph_with_reader(
    reader: &StorageReader,
    chunk_id: &str,
    depth: usize,
) -> Result<crate::storage::reader::Subgraph> {
    let trimmed = chunk_id.trim();
    if trimmed.is_empty() {
        return Ok(crate::storage::reader::Subgraph::default());
    }

    let effective_depth = normalize_traverse_depth(depth);
    reader.traverse_subgraph(trimmed, effective_depth)
}

/// Handles the MCP `traverse_graph` tool invocation given strongly-typed parameters.
pub fn handle_traverse_graph(
    reader: &StorageReader,
    params: crate::mcp::types::TraverseGraphParams,
) -> Result<CallToolResult> {
    let trimmed_id = params.chunk_id.trim();
    if trimmed_id.is_empty() {
        return Err(MemexError::InvalidToolArgs {
            reason: "chunk_id parameter cannot be empty".to_string(),
        });
    }

    let subgraph = traverse_graph_with_reader(reader, trimmed_id, params.depth)?;
    let markdown = format_subgraph_markdown(&subgraph);
    Ok(CallToolResult::text(markdown))
}

/// Handles the MCP `traverse_graph` tool invocation from raw JSON parameters.
pub fn handle_traverse_graph_json(
    reader: &StorageReader,
    arguments: Option<Value>,
) -> CallToolResult {
    let params: crate::mcp::types::TraverseGraphParams = match arguments {
        Some(val) => match serde_json::from_value(val) {
            Ok(p) => p,
            Err(e) => {
                return CallToolResult::error(format!("Invalid traverse_graph arguments: {e}"));
            }
        },
        None => {
            return CallToolResult::error(
                "Missing required 'chunk_id' argument for traverse_graph",
            );
        }
    };

    match handle_traverse_graph(reader, params) {
        Ok(result) => result,
        Err(e) => CallToolResult::error(format!("Traversal failed: {e}")),
    }
}

/// Programmatic helper to execute `search_documentation` directly against a project path.
///
/// Discovers the `.memex/memex.db` file, loads the embedding engine, and executes the search.
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

/// Programmatic helper to execute `traverse_graph` directly against a project path.
///
/// Discovers the `.memex/memex.db` file and executes the traversal.
pub fn traverse_graph(
    project_path: impl AsRef<Path>,
    chunk_id: &str,
    depth: usize,
) -> Result<crate::storage::reader::Subgraph> {
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

    traverse_graph_with_reader(&reader, chunk_id, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::init::init_project_with_embedder;
    use crate::models::{Chunk, ChunkType, Edge, EdgeType};
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
        assert!(md.contains(
            "### 1. docs/api/auth.md > Authentication > OAuth2 (lines 45-67, score: 0.89)"
        ));
        assert!(md.contains("The client must send a Bearer token in the Authorization header."));
        assert!(md.contains(
            "### 2. docs/api/auth.md > Authentication > Token Refresh (lines 70-85, score: 0.76)"
        ));
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
        let res_invalid =
            handle_search_documentation_json(&reader, &engine, Some(json!({"limit": 5})));
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

        let res_spaces =
            search_documentation_with_reader(&reader, &engine, "   \t\n  ", 5).unwrap();
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

        // 4. Test traverse_graph programmatic helper
        let first_auth_chunk_id = &results[0].chunk_id;
        let subgraph = traverse_graph(project_dir, first_auth_chunk_id, 2)
            .expect("traverse_graph should succeed");

        assert!(subgraph.root.is_some());
        assert_eq!(subgraph.root.as_ref().unwrap().id, *first_auth_chunk_id);
        assert!(!subgraph.nodes.is_empty());

        // 5. Test handle_traverse_graph_json tool handler
        let traverse_tool_res = handle_traverse_graph_json(
            &reader,
            Some(json!({
                "chunk_id": first_auth_chunk_id,
                "depth": 2
            })),
        );
        assert_eq!(traverse_tool_res.is_error, None);
        match &traverse_tool_res.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("## Traversal Context for Chunk:"));
                assert!(text.contains(first_auth_chunk_id));
                assert!(text.contains("### Focal Chunk Content"));
            }
        }
    }

    #[test]
    fn test_normalize_traverse_depth() {
        assert_eq!(normalize_traverse_depth(0), 2);
        assert_eq!(normalize_traverse_depth(1), 1);
        assert_eq!(normalize_traverse_depth(2), 2);
        assert_eq!(normalize_traverse_depth(5), 5);
        assert_eq!(normalize_traverse_depth(10), 5);
    }

    #[test]
    fn test_format_subgraph_markdown_empty_and_populated() {
        let empty_subgraph = Subgraph::default();
        let md_empty = format_subgraph_markdown(&empty_subgraph);
        assert_eq!(md_empty, "Chunk not found in knowledge graph.\n");

        let root_chunk = Chunk {
            id: "chk_root".to_string(),
            doc_id: "doc_1".to_string(),
            parent_chunk_id: None,
            chunk_type: ChunkType::Heading { level: 1 },
            heading_path: vec!["Root Heading".to_string()],
            content: "# Root Heading".to_string(),
            contextual_content: "# Root Heading".to_string(),
            line_start: 1,
            line_end: 1,
        };

        let child_chunk = Chunk {
            id: "chk_child".to_string(),
            doc_id: "doc_1".to_string(),
            parent_chunk_id: Some("chk_root".to_string()),
            chunk_type: ChunkType::Paragraph,
            heading_path: vec!["Root Heading".to_string(), "Section 1".to_string()],
            content: "Child paragraph content.".to_string(),
            contextual_content: "[Root Heading > Section 1] Child paragraph content.".to_string(),
            line_start: 3,
            line_end: 6,
        };

        let edge_hier = Edge {
            source_chunk_id: "chk_root".to_string(),
            target_chunk_id: "chk_child".to_string(),
            edge_type: EdgeType::Hierarchy,
            link_text: None,
        };

        let edge_link = Edge {
            source_chunk_id: "chk_child".to_string(),
            target_chunk_id: "chk_root".to_string(),
            edge_type: EdgeType::ExplicitLink,
            link_text: Some("Back to Top".to_string()),
        };

        let populated = Subgraph {
            root: Some(root_chunk.clone()),
            nodes: vec![root_chunk, child_chunk],
            edges: vec![edge_hier, edge_link],
        };

        let md = format_subgraph_markdown(&populated);
        assert!(md.contains("## Traversal Context for Chunk: `chk_root`"));
        assert!(md.contains("**Section:** Root Heading"));
        assert!(md.contains("### Focal Chunk Content"));
        assert!(md.contains("# Root Heading"));
        assert!(md.contains("### Surrounding Context & Connected Nodes"));
        assert!(md.contains("#### [Root Heading > Section 1] (lines 3-6)"));
        assert!(md.contains("Child paragraph content."));
        assert!(md.contains("### Graph Relationships"));
        assert!(md.contains("- Hierarchy: `chk_root` → `chk_child`"));
        assert!(md.contains("- Link (text: \"Back to Top\"): `chk_child` → `chk_root`"));
    }

    #[test]
    fn test_handle_traverse_graph_json_argument_validation() {
        let db = Database::open_in_memory().unwrap();
        crate::storage::schema::initialize_schema(db.conn()).unwrap();
        let reader = StorageReader::new(db.conn());

        // Missing arguments
        let res_none = handle_traverse_graph_json(&reader, None);
        assert_eq!(res_none.is_error, Some(true));
        match &res_none.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("Missing required 'chunk_id' argument"));
            }
        }

        // Invalid arguments (missing chunk_id field)
        let res_invalid = handle_traverse_graph_json(&reader, Some(json!({"depth": 3})));
        assert_eq!(res_invalid.is_error, Some(true));
        match &res_invalid.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("Invalid traverse_graph arguments"));
            }
        }

        // Empty chunk_id field
        let res_empty_id = handle_traverse_graph_json(&reader, Some(json!({"chunk_id": "   "})));
        assert_eq!(res_empty_id.is_error, Some(true));
        match &res_empty_id.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("chunk_id parameter cannot be empty"));
            }
        }

        // Valid arguments on non-existent chunk in DB
        let res_valid = handle_traverse_graph_json(
            &reader,
            Some(json!({"chunk_id": "chk_nonexistent", "depth": 2})),
        );
        assert_eq!(res_valid.is_error, None);
        match &res_valid.content[0] {
            crate::mcp::types::ToolContent::Text { text } => {
                assert!(text.contains("Chunk not found in knowledge graph."));
            }
        }
    }
}
