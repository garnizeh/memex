use crate::cli::index::find_project_root;
use crate::errors::{MemexError, Result};
use crate::ingestion::embedder::{EmbeddingEngine, ModelManager};
use crate::mcp::tools::{handle_search_documentation_json, handle_traverse_graph_json};
use crate::mcp::transport::{McpTransport, RequestHandler};
use crate::mcp::types::{
    handle_handshake_or_tools, JsonRpcError, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
};
use crate::storage::db::Database;
use crate::storage::reader::StorageReader;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};

/// Server context holding the database connection and embedding engine for the MCP server.
#[derive(Debug, Clone)]
pub struct McpServer {
    db: Arc<Mutex<Database>>,
    engine: Arc<EmbeddingEngine>,
}

impl McpServer {
    /// Creates a new `McpServer` targeting the given project path.
    ///
    /// Resolves the project root, verifies that `.memex/memex.db` exists,
    /// opens SQLite in read-only mode, and initializes the ONNX embedding engine.
    pub fn new(project_path: impl AsRef<Path>) -> Result<Self> {
        let root = find_project_root(project_path.as_ref())?;
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
        let assets = ModelManager::ensure_model_assets()?;
        let engine = Arc::new(EmbeddingEngine::new(&assets)?);

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            engine,
        })
    }

    /// Creates an `McpServer` with an explicitly provided `Database` and `EmbeddingEngine` (useful for testing).
    pub fn with_components(db: Database, engine: Arc<EmbeddingEngine>) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            engine,
        }
    }

    /// Handles an incoming JSON-RPC request synchronously.
    pub fn handle_request_sync(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        // 1. Check protocol handshake / tools listing first
        if let Some(resp) = handle_handshake_or_tools(&req) {
            return resp;
        }

        // 2. Dispatch tools/call
        if req.method == "tools/call" {
            let id = req.id.clone();
            let params_val = req.params.as_ref();

            let tool_name = params_val
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str());

            let tool_args = params_val.and_then(|p| p.get("arguments")).cloned();

            let db_guard = match self.db.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let reader = StorageReader::new(db_guard.conn());

            let result = match tool_name {
                Some("search_documentation") => {
                    handle_search_documentation_json(&reader, &self.engine, tool_args)
                }
                Some("traverse_graph") => handle_traverse_graph_json(&reader, tool_args),
                Some(unknown) => {
                    return Some(JsonRpcResponse::error(
                        id,
                        JsonRpcError::new(
                            METHOD_NOT_FOUND,
                            format!("Unknown tool: {unknown}"),
                            None,
                        ),
                    ));
                }
                None => {
                    return Some(JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params(Some(serde_json::json!({
                            "details": "Missing 'name' in tools/call params"
                        }))),
                    ));
                }
            };

            return Some(JsonRpcResponse::success(
                id,
                serde_json::to_value(result).unwrap_or_default(),
            ));
        }

        // 3. Unknown method
        if req.is_notification() {
            None
        } else {
            Some(JsonRpcResponse::error(
                req.id,
                JsonRpcError::method_not_found(Some(serde_json::json!({
                    "method": req.method
                }))),
            ))
        }
    }

    /// Runs the MCP transport listening on the provided async I/O streams.
    pub async fn run_io<R, W>(&self, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Unpin + Send,
        W: AsyncWrite + Unpin + Send,
    {
        McpTransport::listen_io(reader, writer, self.clone()).await
    }
}

// Implement RequestHandler for McpServer
impl RequestHandler for McpServer {
    fn handle_request(
        &self,
        req: JsonRpcRequest,
    ) -> impl Future<Output = Option<JsonRpcResponse>> + Send {
        let resp = self.handle_request_sync(req);
        async move { resp }
    }
}

/// Executes the `serve` command in MCP stdio mode.
///
/// Starts the JSON-RPC stdio transport loop reading from stdin and writing to stdout.
/// Diagnostics, progress, and logs are strictly sent to stderr.
pub async fn run_serve(mcp: bool) -> Result<()> {
    if !mcp {
        tracing::warn!(target: "mcp", "Serve called without --mcp flag; defaulting to MCP stdio mode");
    }

    let cwd = std::env::current_dir()?;
    let server = McpServer::new(&cwd)?;

    tracing::info!(target: "mcp", "Memex MCP server running on stdio for root: {}", cwd.display());
    McpTransport::listen(server).await?;
    tracing::info!(target: "mcp", "Memex MCP server shutdown cleanly");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Chunk, ChunkType};
    use crate::storage::schema::initialize_schema;
    use crate::storage::writer::StorageWriter;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn setup_mock_project() -> (TempDir, Database, Arc<EmbeddingEngine>) {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        let memex_dir = project_root.join(".memex");
        fs::create_dir_all(&memex_dir).unwrap();

        let db_path = memex_dir.join("memex.db");
        let mut db = Database::open(&db_path).unwrap();
        initialize_schema(db.conn()).unwrap();

        let mut writer = StorageWriter::new(db.conn_mut());
        let doc = crate::models::Document {
            id: "doc_1".to_string(),
            file_path: "docs/auth.md".to_string(),
            content_hash: "hash123".to_string(),
            title: Some("Authentication".to_string()),
            indexed_at: 1000,
        };
        writer.insert_document(&doc).unwrap();

        let chunk = Chunk {
            id: "chunk_auth_1".to_string(),
            doc_id: "doc_1".to_string(),
            parent_chunk_id: None,
            chunk_type: ChunkType::Paragraph,
            heading_path: vec!["Authentication".to_string(), "OAuth2".to_string()],
            content: "Use bearer token in Authorization header for OAuth2 requests.".to_string(),
            contextual_content: "[Authentication > OAuth2] Use bearer token in Authorization header for OAuth2 requests.".to_string(),
            line_start: 10,
            line_end: 25,
        };
        writer.insert_chunks_batch(&[chunk]).unwrap();

        let mut vec = [0.0f32; 384];
        vec[0] = 1.0;
        writer
            .insert_vectors_batch(&[("chunk_auth_1".to_string(), vec)])
            .unwrap();

        let assets = ModelManager::ensure_model_assets().unwrap();
        let engine = Arc::new(EmbeddingEngine::new(&assets).unwrap());

        (temp_dir, db, engine)
    }

    #[tokio::test]
    async fn test_mcp_server_initialize_and_tools_list() {
        let (_temp_dir, db, engine) = setup_mock_project();
        let server = McpServer::with_components(db, engine);

        // 1. Initialize
        let init_req = JsonRpcRequest::new(1, "initialize", Some(json!({})));
        let init_resp = server.handle_request_sync(init_req).unwrap();
        assert_eq!(init_resp.id, Some(json!(1)));
        assert!(init_resp.error.is_none());
        let res = init_resp.result.unwrap();
        assert_eq!(res["protocolVersion"], "2024-11-05");
        assert_eq!(res["serverInfo"]["name"], "memex");

        // 2. Notification initialized
        let notif_req = JsonRpcRequest::notification("notifications/initialized", None);
        let notif_resp = server.handle_request_sync(notif_req);
        assert!(notif_resp.is_none());

        // 3. Tools list
        let tools_req = JsonRpcRequest::new(2, "tools/list", None);
        let tools_resp = server.handle_request_sync(tools_req).unwrap();
        assert_eq!(tools_resp.id, Some(json!(2)));
        let tools_res = tools_resp.result.unwrap();
        let tools_arr = tools_res["tools"].as_array().unwrap();
        assert_eq!(tools_arr.len(), 2);
    }

    #[tokio::test]
    async fn test_mcp_server_search_documentation_tool_call() {
        let (_temp_dir, db, engine) = setup_mock_project();
        let server = McpServer::with_components(db, engine);

        let call_req = JsonRpcRequest::new(
            10,
            "tools/call",
            Some(json!({
                "name": "search_documentation",
                "arguments": {
                    "query": "oauth2 bearer token",
                    "limit": 5
                }
            })),
        );

        let resp = server.handle_request_sync(call_req).unwrap();
        assert_eq!(resp.id, Some(json!(10)));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let content_arr = result["content"].as_array().unwrap();
        assert!(!content_arr.is_empty());
        let text = content_arr[0]["text"].as_str().unwrap();
        assert!(text.contains("Results for: \"oauth2 bearer token\""));
        assert!(text.contains("docs/auth.md"));
    }

    #[tokio::test]
    async fn test_mcp_server_traverse_graph_tool_call() {
        let (_temp_dir, db, engine) = setup_mock_project();
        let server = McpServer::with_components(db, engine);

        let call_req = JsonRpcRequest::new(
            11,
            "tools/call",
            Some(json!({
                "name": "traverse_graph",
                "arguments": {
                    "chunk_id": "chunk_auth_1",
                    "depth": 2
                }
            })),
        );

        let resp = server.handle_request_sync(call_req).unwrap();
        assert_eq!(resp.id, Some(json!(11)));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let content_arr = result["content"].as_array().unwrap();
        assert!(!content_arr.is_empty());
        let text = content_arr[0]["text"].as_str().unwrap();
        assert!(text.contains("chunk_auth_1"));
        assert!(text.contains("Use bearer token"));
    }

    #[tokio::test]
    async fn test_mcp_server_unknown_tool_and_method() {
        let (_temp_dir, db, engine) = setup_mock_project();
        let server = McpServer::with_components(db, engine);

        // Unknown tool
        let unknown_tool_req = JsonRpcRequest::new(
            20,
            "tools/call",
            Some(json!({
                "name": "non_existent_tool",
                "arguments": {}
            })),
        );
        let resp = server.handle_request_sync(unknown_tool_req).unwrap();
        assert_eq!(resp.id, Some(json!(20)));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);

        // Unknown method
        let unknown_method_req = JsonRpcRequest::new(21, "custom/method", None);
        let resp2 = server.handle_request_sync(unknown_method_req).unwrap();
        assert_eq!(resp2.id, Some(json!(21)));
        assert_eq!(resp2.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_mcp_server_run_io_full_session() {
        let (_temp_dir, db, engine) = setup_mock_project();
        let server = McpServer::with_components(db, engine);

        let input_lines = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_documentation","arguments":{"query":"bearer token"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"traverse_graph","arguments":{"chunk_id":"chunk_auth_1"}}}"#,
        ]
        .join("\n")
            + "\n";

        let reader = std::io::Cursor::new(input_lines);
        let mut output = Vec::new();

        server.run_io(reader, &mut output).await.unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let output_lines: Vec<&str> = output_str.trim().lines().collect();
        assert_eq!(output_lines.len(), 3);

        // 1. Check initialize response
        let resp1: serde_json::Value = serde_json::from_str(output_lines[0]).unwrap();
        assert_eq!(resp1["id"], 1);
        assert_eq!(resp1["result"]["serverInfo"]["name"], "memex");

        // 2. Check search_documentation response
        let resp2: serde_json::Value = serde_json::from_str(output_lines[1]).unwrap();
        assert_eq!(resp2["id"], 2);
        let search_text = resp2["result"]["content"][0]["text"].as_str().unwrap();
        assert!(search_text.contains("Results for: \"bearer token\""));

        // 3. Check traverse_graph response
        let resp3: serde_json::Value = serde_json::from_str(output_lines[2]).unwrap();
        assert_eq!(resp3["id"], 3);
        let traverse_text = resp3["result"]["content"][0]["text"].as_str().unwrap();
        assert!(traverse_text.contains("chunk_auth_1"));
    }

    #[test]
    fn test_mcp_server_new_uninitialized_project() {
        let empty_temp = TempDir::new().unwrap();
        let err = McpServer::new(empty_temp.path()).unwrap_err();
        match err {
            MemexError::NotInitialized { .. } => {}
            other => panic!("Expected NotInitialized error, got: {:?}", other),
        }
    }
}
