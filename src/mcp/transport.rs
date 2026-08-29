use crate::errors::Result;
use crate::mcp::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use std::future::Future;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// Trait for handling incoming JSON-RPC requests.
pub trait RequestHandler: Send + Sync {
    fn handle_request(
        &self,
        req: JsonRpcRequest,
    ) -> impl Future<Output = Option<JsonRpcResponse>> + Send;
}

// Blanket implementation for closures / functions returning a Future
impl<F, Fut> RequestHandler for F
where
    F: Fn(JsonRpcRequest) -> Fut + Send + Sync,
    Fut: Future<Output = Option<JsonRpcResponse>> + Send,
{
    fn handle_request(
        &self,
        req: JsonRpcRequest,
    ) -> impl Future<Output = Option<JsonRpcResponse>> + Send {
        (self)(req)
    }
}

/// JSON-RPC 2.0 stdio framing and transport.
pub struct McpTransport;

impl McpTransport {
    /// Listens for JSON-RPC 2.0 messages on stdin and writes responses to stdout.
    ///
    /// Tracing and logs are sent to stderr to ensure stdout only carries valid JSON-RPC frames.
    pub async fn listen<H: RequestHandler>(handler: H) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        Self::listen_io(stdin, stdout, handler).await
    }

    /// Generic I/O loop reading line-delimited JSON-RPC from `reader` and writing responses to `writer`.
    pub async fn listen_io<R, W, H>(reader: R, mut writer: W, handler: H) -> Result<()>
    where
        R: AsyncRead + Unpin + Send,
        W: AsyncWrite + Unpin + Send,
        H: RequestHandler,
    {
        let mut lines = BufReader::new(reader).lines();

        while let Some(line) = lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            tracing::debug!(target: "mcp", "Received line: {}", trimmed);

            let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(req) => {
                    if req.jsonrpc != "2.0" {
                        if req.id.is_some() {
                            Some(JsonRpcResponse::error(
                                req.id,
                                JsonRpcError::invalid_request(Some(serde_json::json!({
                                    "details": "Only JSON-RPC 2.0 is supported"
                                }))),
                            ))
                        } else {
                            None
                        }
                    } else {
                        handler.handle_request(req).await
                    }
                }
                Err(err) => {
                    tracing::error!(target: "mcp", "JSON-RPC parse error: {}", err);
                    let maybe_id = serde_json::from_str::<serde_json::Value>(trimmed)
                        .ok()
                        .and_then(|v| v.get("id").cloned());
                    Some(JsonRpcResponse::error(
                        maybe_id,
                        JsonRpcError::parse_error(Some(serde_json::json!({
                            "details": err.to_string()
                        }))),
                    ))
                }
            };

            if let Some(resp) = response {
                let json_str = serde_json::to_string(&resp)?;
                tracing::debug!(target: "mcp", "Sending response: {}", json_str);
                writer.write_all(json_str.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
        }

        tracing::info!(target: "mcp", "Transport stream reached EOF, shutting down listener");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    struct EchoHandler;

    impl RequestHandler for EchoHandler {
        async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
            if req.is_notification() {
                None
            } else if req.method == "ping" {
                Some(JsonRpcResponse::success(req.id, json!("pong")))
            } else if req.method == "echo" {
                Some(JsonRpcResponse::success(
                    req.id,
                    req.params.unwrap_or(json!({})),
                ))
            } else {
                Some(JsonRpcResponse::error(
                    req.id,
                    JsonRpcError::method_not_found(None),
                ))
            }
        }
    }

    #[tokio::test]
    async fn test_listen_io_valid_requests_and_notifications() {
        let input_lines = vec![
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            r#""#, // Empty line should be skipped
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, // Notification, no response
            r#"{"jsonrpc":"2.0","id":"two","method":"echo","params":{"hello":"world"}}"#,
        ]
        .join("\n")
            + "\n";

        let reader = Cursor::new(input_lines);
        let mut output = Vec::new();

        McpTransport::listen_io(reader, &mut output, EchoHandler)
            .await
            .expect("transport listen_io should succeed");

        let output_str = String::from_utf8(output).expect("valid utf8 output");
        let output_lines: Vec<&str> = output_str.trim().lines().collect();

        assert_eq!(output_lines.len(), 2);

        let resp1: JsonRpcResponse =
            serde_json::from_str(output_lines[0]).expect("valid json response 1");
        assert_eq!(resp1.id, Some(json!(1)));
        assert_eq!(resp1.result, Some(json!("pong")));

        let resp2: JsonRpcResponse =
            serde_json::from_str(output_lines[1]).expect("valid json response 2");
        assert_eq!(resp2.id, Some(json!("two")));
        assert_eq!(resp2.result, Some(json!({"hello": "world"})));
    }

    #[tokio::test]
    async fn test_listen_io_malformed_json() {
        let input = "not a valid json\n";
        let reader = Cursor::new(input);
        let mut output = Vec::new();

        McpTransport::listen_io(reader, &mut output, EchoHandler)
            .await
            .expect("transport listen_io should handle parse errors gracefully");

        let output_str = String::from_utf8(output).expect("valid utf8 output");
        let resp: JsonRpcResponse =
            serde_json::from_str(output_str.trim()).expect("valid json error response");

        assert_eq!(resp.id, None);
        let err = resp.error.expect("error field should be populated");
        assert_eq!(err.code, -32700);
        assert_eq!(err.message, "Parse error");
    }

    #[tokio::test]
    async fn test_closure_request_handler() {
        let input = "{\"jsonrpc\":\"2.0\",\"id\":100,\"method\":\"test\"}\n";
        let reader = Cursor::new(input);
        let mut output = Vec::new();

        let handler = |req: JsonRpcRequest| async move {
            Some(JsonRpcResponse::success(req.id, json!({"status": "ok"})))
        };

        McpTransport::listen_io(reader, &mut output, handler)
            .await
            .expect("closure handler should succeed");

        let output_str = String::from_utf8(output).expect("valid utf8 output");
        let resp: JsonRpcResponse =
            serde_json::from_str(output_str.trim()).expect("valid json response");
        assert_eq!(resp.id, Some(json!(100)));
        assert_eq!(resp.result, Some(json!({"status": "ok"})));
    }
}
