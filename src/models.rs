use serde::{Deserialize, Serialize};

/// Represents a discovered and indexed Markdown document in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// SHA256 hash of the relative file path from project root.
    pub id: String,
    /// Relative path from the project root (e.g., "docs/architecture.md").
    pub file_path: String,
    /// Extracted document title (e.g., from the first H1 heading), if present.
    pub title: Option<String>,
    /// SHA256 hash of raw file content, used for incremental change detection.
    pub content_hash: String,
    /// Unix timestamp in seconds when the document was indexed.
    pub indexed_at: i64,
}

/// The semantic classification of a documentation chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkType {
    /// Markdown heading (H1 through H6).
    Heading {
        /// Heading level from 1 to 6.
        level: u8,
    },
    /// Standard body paragraph text.
    Paragraph,
    /// Code block snippet with optional syntax language specifier.
    CodeBlock {
        /// Programming or markup language identifier (e.g., "rust", "json").
        language: Option<String>,
    },
    /// List item or list block content.
    List,
}

/// A contextually-enriched unit of documentation stored and retrieved by the search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Deterministic SHA256 hash derived from doc_id, heading_path, and raw content.
    pub id: String,
    /// Foreign key referencing the parent [`Document::id`].
    pub doc_id: String,
    /// Foreign key referencing the parent heading [`Chunk::id`], if nested.
    pub parent_chunk_id: Option<String>,
    /// Semantic unit type for this chunk.
    pub chunk_type: ChunkType,
    /// Breadcrumb trail of ancestor heading titles (e.g., `["Guide", "Setup"]`).
    pub heading_path: Vec<String>,
    /// Raw un-prefixed content of this chunk.
    pub content: String,
    /// Contextually prefixed content used for vector embedding and LLM prompt context.
    pub contextual_content: String,
    /// 1-indexed starting line number in the source file.
    pub line_start: u32,
    /// 1-indexed ending line number (inclusive) in the source file.
    pub line_end: u32,
}

/// Represents the nature of the directed edge connecting two chunks in the documentation graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    /// Structural hierarchy link (e.g., parent heading to child section or paragraph).
    Hierarchy,
    /// Explicit Markdown cross-reference link (e.g., `[text](target)`).
    ExplicitLink,
}

/// A directed edge connecting two chunks in the semantic knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Originating chunk ID.
    pub source_chunk_id: String,
    /// Destination chunk ID.
    pub target_chunk_id: String,
    /// Classification of the relationship.
    pub edge_type: EdgeType,
    /// Optional anchor text for explicit links.
    pub link_text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_serialization_roundtrip() {
        let doc = Document {
            id: "doc_123abc".to_string(),
            file_path: "docs/guide.md".to_string(),
            title: Some("User Guide".to_string()),
            content_hash: "hash_456def".to_string(),
            indexed_at: 1700000000,
        };

        let json = serde_json::to_string(&doc).expect("Serialization failed");
        let deserialized: Document = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(doc, deserialized);
        assert_eq!(doc.clone(), deserialized);
    }

    #[test]
    fn test_document_without_title_roundtrip() {
        let doc = Document {
            id: "doc_no_title".to_string(),
            file_path: "README.md".to_string(),
            title: None,
            content_hash: "hash_789".to_string(),
            indexed_at: 1700000050,
        };

        let json = serde_json::to_string(&doc).expect("Serialization failed");
        let deserialized: Document = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(doc, deserialized);
    }

    #[test]
    fn test_chunk_types_roundtrip() {
        let variants = vec![
            ChunkType::Heading { level: 1 },
            ChunkType::Heading { level: 6 },
            ChunkType::Paragraph,
            ChunkType::CodeBlock {
                language: Some("rust".to_string()),
            },
            ChunkType::CodeBlock { language: None },
            ChunkType::List,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant).expect("Serialization failed");
            let deserialized: ChunkType =
                serde_json::from_str(&json).expect("Deserialization failed");
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn test_chunk_serialization_roundtrip() {
        let chunk = Chunk {
            id: "chunk_001".to_string(),
            doc_id: "doc_123abc".to_string(),
            parent_chunk_id: Some("chunk_heading_0".to_string()),
            chunk_type: ChunkType::CodeBlock {
                language: Some("rust".to_string()),
            },
            heading_path: vec!["API Reference".to_string(), "Authentication".to_string()],
            content: "fn login() {}".to_string(),
            contextual_content: "[API Reference > Authentication] fn login() {}".to_string(),
            line_start: 42,
            line_end: 50,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization failed");
        let deserialized: Chunk = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(chunk, deserialized);
    }

    #[test]
    fn test_chunk_without_parent_roundtrip() {
        let chunk = Chunk {
            id: "chunk_root".to_string(),
            doc_id: "doc_123abc".to_string(),
            parent_chunk_id: None,
            chunk_type: ChunkType::Heading { level: 1 },
            heading_path: vec!["Overview".to_string()],
            content: "# Overview".to_string(),
            contextual_content: "# Overview".to_string(),
            line_start: 1,
            line_end: 1,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization failed");
        let deserialized: Chunk = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(chunk, deserialized);
    }

    #[test]
    fn test_edge_serialization_roundtrip() {
        let hierarchy_edge = Edge {
            source_chunk_id: "chunk_h1".to_string(),
            target_chunk_id: "chunk_p1".to_string(),
            edge_type: EdgeType::Hierarchy,
            link_text: None,
        };

        let json_h = serde_json::to_string(&hierarchy_edge).expect("Serialization failed");
        let deserialized_h: Edge = serde_json::from_str(&json_h).expect("Deserialization failed");
        assert_eq!(hierarchy_edge, deserialized_h);

        let explicit_edge = Edge {
            source_chunk_id: "chunk_p1".to_string(),
            target_chunk_id: "chunk_target".to_string(),
            edge_type: EdgeType::ExplicitLink,
            link_text: Some("See Authentication Guide".to_string()),
        };

        let json_e = serde_json::to_string(&explicit_edge).expect("Serialization failed");
        let deserialized_e: Edge = serde_json::from_str(&json_e).expect("Deserialization failed");
        assert_eq!(explicit_edge, deserialized_e);
    }
}
