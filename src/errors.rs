use thiserror::Error;

/// The central error type for all Memex operations.
#[derive(Error, Debug)]
pub enum MemexError {
    // === CLI Errors ===
    /// Error when attempting to index or serve a directory that has not been initialized.
    #[error("Memex not initialized in '{path}'. Run 'memex init' first.")]
    NotInitialized { path: String },

    /// Error when attempting to initialize an unsafe root directory (such as `$HOME` or filesystem root).
    #[error(
        "Refusing to initialize in '{path}' — it looks like {reason}. Pass --force to override."
    )]
    UnsafeRoot { path: String, reason: String },

    /// Error when attempting to initialize a directory that already contains a `.memex` database.
    #[error("Already initialized in '{path}'. Use 'memex index' to re-index.")]
    AlreadyInitialized { path: String },

    /// Error for invalid CLI arguments or invocations.
    #[error("Invalid CLI argument or command: {0}")]
    InvalidCommand(String),

    // === Discovery Errors ===
    /// Error when discovering or traversing documentation files.
    #[error("Failed to discover files in '{path}': {reason}")]
    DiscoveryError { path: String, reason: String },

    // === Ingestion & Embedding Errors ===
    /// Error when parsing a Markdown document.
    #[error("Failed to parse markdown in '{file}': {message}")]
    ParseError { file: String, message: String },

    /// Error when generating embedding vectors for a chunk or batch of chunks.
    #[error("Embedding generation failed for batch starting at chunk '{chunk_id}': {message}")]
    EmbeddingError { chunk_id: String, message: String },

    /// Error when downloading or loading ONNX model weights and runtime session.
    #[error("Failed to load embedding model assets: {0}")]
    ModelLoadError(String),

    /// Error during tokenization of text chunks.
    #[error("Tokenizer error: {0}")]
    TokenizerError(String),

    // === Storage Errors ===
    /// SQLite relational database error.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Error loading or initializing the `sqlite-vec` vector search extension.
    #[error("Failed to load sqlite-vec extension: {0}")]
    VecExtension(String),

    /// Error when a requested document, chunk, or edge is not found.
    #[error("{entity} not found with id '{id}'")]
    NotFound { entity: String, id: String },

    /// Error during transactional batch commit or rollback.
    #[error("Database transaction error: {0}")]
    TransactionError(String),

    // === MCP Protocol Errors ===
    /// Error when an unknown MCP tool name is invoked.
    #[error("Unknown MCP tool: '{name}'")]
    UnknownTool { name: String },

    /// Error when invalid or malformed arguments are passed to an MCP tool.
    #[error("Invalid tool arguments: {reason}")]
    InvalidToolArgs { reason: String },

    /// General MCP JSON-RPC protocol error.
    #[error("MCP protocol error: {0}")]
    McpProtocol(String),

    // === Configuration & Installation Errors ===
    /// Error parsing or validating `memex.json` configuration.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Error during agent configuration injection or installation.
    #[error("Installer error: {0}")]
    Installer(String),

    // === I/O & Serialization Errors ===
    /// Standard I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Convenience alias for `Result<T, MemexError>`.
pub type Result<T> = std::result::Result<T, MemexError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_error_formatting() {
        let err = MemexError::NotInitialized {
            path: "/tmp/project".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Memex not initialized in '/tmp/project'. Run 'memex init' first."
        );

        let err = MemexError::UnsafeRoot {
            path: "/home/user".to_string(),
            reason: "a user home directory".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Refusing to initialize in '/home/user' — it looks like a user home directory. Pass --force to override."
        );

        let err = MemexError::AlreadyInitialized {
            path: "./docs".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Already initialized in './docs'. Use 'memex index' to re-index."
        );

        let err = MemexError::InvalidCommand("missing subcommand".to_string());
        assert_eq!(
            err.to_string(),
            "Invalid CLI argument or command: missing subcommand"
        );
    }

    #[test]
    fn test_discovery_error_formatting() {
        let err = MemexError::DiscoveryError {
            path: "nonexistent/".to_string(),
            reason: "directory not found".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to discover files in 'nonexistent/': directory not found"
        );
    }

    #[test]
    fn test_ingestion_error_formatting() {
        let err = MemexError::ParseError {
            file: "guide.md".to_string(),
            message: "unexpected EOF in code block".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to parse markdown in 'guide.md': unexpected EOF in code block"
        );

        let err = MemexError::EmbeddingError {
            chunk_id: "chk_99".to_string(),
            message: "tensor dimension mismatch".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Embedding generation failed for batch starting at chunk 'chk_99': tensor dimension mismatch"
        );

        let err = MemexError::ModelLoadError("model.onnx missing".to_string());
        assert_eq!(
            err.to_string(),
            "Failed to load embedding model assets: model.onnx missing"
        );

        let err = MemexError::TokenizerError("invalid vocab".to_string());
        assert_eq!(err.to_string(), "Tokenizer error: invalid vocab");
    }

    #[test]
    fn test_storage_and_mcp_error_formatting() {
        let err = MemexError::VecExtension("shared library not found".to_string());
        assert_eq!(
            err.to_string(),
            "Failed to load sqlite-vec extension: shared library not found"
        );

        let err = MemexError::NotFound {
            entity: "Document".to_string(),
            id: "doc_123".to_string(),
        };
        assert_eq!(err.to_string(), "Document not found with id 'doc_123'");

        let err = MemexError::TransactionError("lock busy".to_string());
        assert_eq!(err.to_string(), "Database transaction error: lock busy");

        let err = MemexError::UnknownTool {
            name: "delete_all".to_string(),
        };
        assert_eq!(err.to_string(), "Unknown MCP tool: 'delete_all'");

        let err = MemexError::InvalidToolArgs {
            reason: "missing 'query' field".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Invalid tool arguments: missing 'query' field"
        );

        let err = MemexError::McpProtocol("invalid jsonrpc version".to_string());
        assert_eq!(
            err.to_string(),
            "MCP protocol error: invalid jsonrpc version"
        );

        let err = MemexError::Config("syntax error in line 3".to_string());
        assert_eq!(
            err.to_string(),
            "Configuration error: syntax error in line 3"
        );

        let err = MemexError::Installer("failed to backup settings".to_string());
        assert_eq!(
            err.to_string(),
            "Installer error: failed to backup settings"
        );
    }

    #[test]
    fn test_from_conversions() {
        // std::io::Error conversion
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let memex_err: MemexError = io_err.into();
        assert!(matches!(memex_err, MemexError::Io(_)));
        assert!(memex_err.to_string().contains("file not found"));

        // serde_json::Error conversion
        let json_err: serde_json::Error =
            serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let memex_err: MemexError = json_err.into();
        assert!(matches!(memex_err, MemexError::Serialization(_)));
        assert!(memex_err.to_string().contains("Serialization error:"));

        // rusqlite::Error conversion
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let memex_err: MemexError = sqlite_err.into();
        assert!(matches!(memex_err, MemexError::Database(_)));
        assert!(memex_err.to_string().contains("Database error:"));
    }
}
