use serde::{Deserialize, Serialize};

/// Document representation stored in the SQLite database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub file_path: String,
    pub title: Option<String>,
    pub content_hash: String,
    pub indexed_at: i64,
}

/// The type of semantic chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkType {
    Heading { level: u8 },
    Paragraph,
    CodeBlock { language: Option<String> },
    List,
}

/// A chunk of documentation enriched with hierarchical context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub doc_id: String,
    pub parent_chunk_id: Option<String>,
    pub chunk_type: ChunkType,
    pub heading_path: Vec<String>,
    pub content: String,
    pub contextual_content: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// An edge connecting chunks in the documentation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source_chunk_id: String,
    pub target_chunk_id: String,
    pub edge_type: EdgeType,
    pub link_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    Hierarchy,
    ExplicitLink,
}
