use serde::{Deserialize, Serialize};
use serde_json::Value;

// Standard JSON-RPC 2.0 error codes
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// MCP protocol version supported by this server.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// A JSON-RPC 2.0 Request or Notification object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc_version")]
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

fn default_jsonrpc_version() -> String {
    "2.0".to_string()
}

impl JsonRpcRequest {
    /// Creates a new JSON-RPC request with an ID.
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id.into()),
            method: method.into(),
            params,
        }
    }

    /// Creates a new JSON-RPC notification (no ID).
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params,
        }
    }

    /// Returns true if this message is a notification (lacks an `id`).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A JSON-RPC 2.0 Response object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Creates a successful response with a result.
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Creates an error response.
    pub fn error(id: Option<Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A JSON-RPC 2.0 Error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    pub fn parse_error(data: Option<Value>) -> Self {
        Self::new(PARSE_ERROR, "Parse error", data)
    }

    pub fn invalid_request(data: Option<Value>) -> Self {
        Self::new(INVALID_REQUEST, "Invalid Request", data)
    }

    pub fn method_not_found(data: Option<Value>) -> Self {
        Self::new(METHOD_NOT_FOUND, "Method not found", data)
    }

    pub fn invalid_params(data: Option<Value>) -> Self {
        Self::new(INVALID_PARAMS, "Invalid params", data)
    }

    pub fn internal_error(data: Option<Value>) -> Self {
        Self::new(INTERNAL_ERROR, "Internal error", data)
    }
}

// ==========================================================================
// MCP Protocol Types
// ==========================================================================

/// MCP Server implementation info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "memex".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// MCP Client implementation info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// MCP Tools capability descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// MCP Server capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
}

/// MCP initialize request parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
}

/// MCP initialize response result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// MCP tool definition descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

impl Tool {
    /// Schema descriptor for `search_documentation` matching Section 7 of architecture.md.
    pub fn search_documentation() -> Self {
        Self {
            name: "search_documentation".to_string(),
            description: Some(
                "Search the project's Markdown documentation using semantic similarity. Returns the most relevant documentation chunks with their source file, heading context, and line numbers."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query describing what documentation you need."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5, max: 20).",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        }
    }

    /// Schema descriptor for `traverse_graph` matching Section 7 of architecture.md.
    pub fn traverse_graph() -> Self {
        Self {
            name: "traverse_graph".to_string(),
            description: Some(
                "Retrieve surrounding documentation context for a specific chunk. Traverses the document graph upward (to parent headings) and downward/sideways (to child sections and linked content)."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "chunk_id": {
                        "type": "string",
                        "description": "The ID of the chunk to expand context around (obtained from search_documentation results)."
                    },
                    "depth": {
                        "type": "integer",
                        "description": "How many levels of the graph to traverse (default: 2, max: 5).",
                        "default": 2
                    }
                },
                "required": ["chunk_id"]
            }),
        }
    }
}

/// Returns the complete list of tools exposed by the Memex MCP server.
pub fn all_tools() -> Vec<Tool> {
    vec![Tool::search_documentation(), Tool::traverse_graph()]
}

/// MCP tools/list response result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Content item within a tool call result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolContent {
    Text { text: String },
}

/// Result returned from `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl CallToolResult {
    /// Creates a successful tool result with single text content.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Creates an error tool result with single text message.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: message.into(),
            }],
            is_error: Some(true),
        }
    }
}

/// Parsed arguments for `search_documentation`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchDocumentationParams {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    5
}

/// Parsed arguments for `traverse_graph`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraverseGraphParams {
    pub chunk_id: String,
    #[serde(default = "default_traverse_depth")]
    pub depth: usize,
}

fn default_traverse_depth() -> usize {
    2
}

// ==========================================================================
// Handshake & Tool Request Handlers
// ==========================================================================

/// Handles the MCP `initialize` request.
pub fn handle_initialize(id: Option<Value>, _params: Option<Value>) -> JsonRpcResponse {
    let result = InitializeResult {
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(false),
            }),
            ..Default::default()
        },
        server_info: ServerInfo::default(),
        instructions: Some(
            "Memex provides semantic documentation search and knowledge graph traversal tools."
                .to_string(),
        ),
    };

    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or(Value::Null))
}

/// Handles the MCP `tools/list` request.
pub fn handle_tools_list(id: Option<Value>, _params: Option<Value>) -> JsonRpcResponse {
    let result = ListToolsResult {
        tools: all_tools(),
        next_cursor: None,
    };

    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or(Value::Null))
}

/// Dispatches standard MCP protocol handshake, ping, and tool listing requests.
///
/// Returns `Some(response)` for handled requests, `None` for notifications (e.g. `notifications/initialized`),
/// or `None` if the request is not a handshake/tools/list method.
pub fn handle_handshake_or_tools(req: &JsonRpcRequest) -> Option<Option<JsonRpcResponse>> {
    match req.method.as_str() {
        "initialize" => Some(Some(handle_initialize(req.id.clone(), req.params.clone()))),
        "notifications/initialized" | "initialized" => {
            // Notification: no response required
            Some(None)
        }
        "ping" => Some(Some(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::json!({}),
        ))),
        "tools/list" => Some(Some(handle_tools_list(req.id.clone(), req.params.clone()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_serialization_and_deserialization() {
        let req = JsonRpcRequest::new(1, "tools/list", Some(json!({"cursor": "abc"})));
        let serialized = serde_json::to_string(&req).expect("serialize request");
        let deserialized: JsonRpcRequest =
            serde_json::from_str(&serialized).expect("deserialize request");

        assert_eq!(req, deserialized);
        assert!(!deserialized.is_notification());
        assert_eq!(deserialized.id, Some(json!(1)));
        assert_eq!(deserialized.method, "tools/list");
    }

    #[test]
    fn test_notification_handling() {
        let notif = JsonRpcRequest::notification("notifications/initialized", None);
        assert!(notif.is_notification());
        let serialized = serde_json::to_string(&notif).expect("serialize notification");
        assert!(!serialized.contains("\"id\""));

        let deserialized: JsonRpcRequest =
            serde_json::from_str(&serialized).expect("deserialize notification");
        assert!(deserialized.is_notification());
        assert_eq!(deserialized.method, "notifications/initialized");
    }

    #[test]
    fn test_success_response_serialization() {
        let resp = JsonRpcResponse::success(Some(json!("req-123")), json!({"tools": []}));
        let serialized = serde_json::to_string(&resp).expect("serialize success response");

        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"id\":\"req-123\""));
        assert!(serialized.contains("\"result\":{\"tools\":[]}"));
        assert!(!serialized.contains("\"error\""));

        let deserialized: JsonRpcResponse =
            serde_json::from_str(&serialized).expect("deserialize response");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_error_response_serialization() {
        let err = JsonRpcError::method_not_found(Some(json!({"tool": "unknown_tool"})));
        let resp = JsonRpcResponse::error(Some(json!(42)), err);
        let serialized = serde_json::to_string(&resp).expect("serialize error response");

        assert!(serialized.contains("\"code\":-32601"));
        assert!(serialized.contains("\"message\":\"Method not found\""));
        assert!(!serialized.contains("\"result\""));

        let deserialized: JsonRpcResponse =
            serde_json::from_str(&serialized).expect("deserialize response");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_initialize_handler() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "claude", "version": "1.0.0" }
            })),
        );

        let resp = handle_handshake_or_tools(&req)
            .expect("should match initialize")
            .expect("should return response");

        assert_eq!(resp.id, Some(json!(1)));
        assert!(resp.error.is_none());

        let result_val = resp.result.expect("result should be present");
        let init_result: InitializeResult =
            serde_json::from_value(result_val).expect("valid InitializeResult");

        assert_eq!(init_result.protocol_version, "2024-11-05");
        assert_eq!(init_result.server_info.name, "memex");
        assert_eq!(init_result.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(init_result.capabilities.tools.is_some());
    }

    #[test]
    fn test_notifications_initialized() {
        let notif1 = JsonRpcRequest::notification("notifications/initialized", None);
        let handled1 = handle_handshake_or_tools(&notif1);
        assert_eq!(handled1, Some(None));

        let notif2 = JsonRpcRequest::notification("initialized", None);
        let handled2 = handle_handshake_or_tools(&notif2);
        assert_eq!(handled2, Some(None));
    }

    #[test]
    fn test_tools_list_handler_and_schemas() {
        let req = JsonRpcRequest::new(2, "tools/list", None);
        let resp = handle_handshake_or_tools(&req)
            .expect("should match tools/list")
            .expect("should return response");

        assert_eq!(resp.id, Some(json!(2)));
        let result_val = resp.result.expect("result should be present");
        let list_result: ListToolsResult =
            serde_json::from_value(result_val).expect("valid ListToolsResult");

        assert_eq!(list_result.tools.len(), 2);

        // Verify Tool 1: search_documentation schema matches architecture.md Section 7
        let search_tool = list_result
            .tools
            .iter()
            .find(|t| t.name == "search_documentation")
            .expect("search_documentation tool must exist");
        assert!(
            search_tool
                .description
                .as_ref()
                .unwrap()
                .contains("semantic similarity")
        );

        let schema = &search_tool.input_schema;
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["query"]));
        assert_eq!(schema["properties"]["query"]["type"], "string");
        assert_eq!(schema["properties"]["limit"]["type"], "integer");
        assert_eq!(schema["properties"]["limit"]["default"], 5);

        // Verify Tool 2: traverse_graph schema matches architecture.md Section 7
        let traverse_tool = list_result
            .tools
            .iter()
            .find(|t| t.name == "traverse_graph")
            .expect("traverse_graph tool must exist");
        assert!(
            traverse_tool
                .description
                .as_ref()
                .unwrap()
                .contains("surrounding documentation context")
        );

        let schema2 = &traverse_tool.input_schema;
        assert_eq!(schema2["type"], "object");
        assert_eq!(schema2["required"], json!(["chunk_id"]));
        assert_eq!(schema2["properties"]["chunk_id"]["type"], "string");
        assert_eq!(schema2["properties"]["depth"]["type"], "integer");
        assert_eq!(schema2["properties"]["depth"]["default"], 2);
    }

    #[test]
    fn test_ping_handler() {
        let req = JsonRpcRequest::new(99, "ping", None);
        let resp = handle_handshake_or_tools(&req)
            .expect("should match ping")
            .expect("should return response");

        assert_eq!(resp.id, Some(json!(99)));
        assert_eq!(resp.result, Some(json!({})));
    }

    #[test]
    fn test_unmatched_method_returns_none() {
        let req = JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({"name": "search_documentation"})),
        );
        assert_eq!(handle_handshake_or_tools(&req), None);
    }

    #[test]
    fn test_params_deserialization() {
        let search_json = json!({
            "query": "how to configure oauth",
            "limit": 10
        });
        let search_params: SearchDocumentationParams =
            serde_json::from_value(search_json).expect("deserialize search params");
        assert_eq!(search_params.query, "how to configure oauth");
        assert_eq!(search_params.limit, 10);

        let search_default_json = json!({
            "query": "authentication"
        });
        let search_default_params: SearchDocumentationParams =
            serde_json::from_value(search_default_json)
                .expect("deserialize search params with default");
        assert_eq!(search_default_params.query, "authentication");
        assert_eq!(search_default_params.limit, 5);

        let traverse_json = json!({
            "chunk_id": "chunk_abc123",
            "depth": 4
        });
        let traverse_params: TraverseGraphParams =
            serde_json::from_value(traverse_json).expect("deserialize traverse params");
        assert_eq!(traverse_params.chunk_id, "chunk_abc123");
        assert_eq!(traverse_params.depth, 4);

        let traverse_default_json = json!({
            "chunk_id": "chunk_xyz789"
        });
        let traverse_default_params: TraverseGraphParams =
            serde_json::from_value(traverse_default_json)
                .expect("deserialize traverse params with default");
        assert_eq!(traverse_default_params.chunk_id, "chunk_xyz789");
        assert_eq!(traverse_default_params.depth, 2);
    }

    #[test]
    fn test_call_tool_result_serialization() {
        let success = CallToolResult::text("Search results formatted markdown");
        let serialized_success =
            serde_json::to_string(&success).expect("serialize success CallToolResult");
        assert!(serialized_success.contains("\"type\":\"text\""));
        assert!(serialized_success.contains("\"text\":\"Search results formatted markdown\""));
        assert!(!serialized_success.contains("\"isError\""));

        let error = CallToolResult::error("Chunk not found");
        let serialized_error =
            serde_json::to_string(&error).expect("serialize error CallToolResult");
        assert!(serialized_error.contains("\"isError\":true"));
        assert!(serialized_error.contains("\"text\":\"Chunk not found\""));
    }
}
