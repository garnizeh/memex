use serde::{Deserialize, Serialize};
use serde_json::Value;

// Standard JSON-RPC 2.0 error codes
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

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
}
