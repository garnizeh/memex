use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemexError {
    #[error("Memex not initialized in '{path}'. Run 'memex init' first.")]
    NotInitialized { path: String },

    #[error(
        "Refusing to initialize in '{path}' — it looks like {reason}. Pass --force to override."
    )]
    UnsafeRoot { path: String, reason: String },

    #[error("Already initialized in '{path}'. Use 'memex index' to re-index.")]
    AlreadyInitialized { path: String },

    #[error("Failed to parse markdown in '{file}': {message}")]
    ParseError { file: String, message: String },

    #[error("Embedding generation failed for batch starting at chunk '{chunk_id}': {message}")]
    EmbeddingError { chunk_id: String, message: String },

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Failed to load sqlite-vec extension: {0}")]
    VecExtension(String),

    #[error("Unknown MCP tool: '{name}'")]
    UnknownTool { name: String },

    #[error("Invalid tool arguments: {reason}")]
    InvalidToolArgs { reason: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}
