use crate::discovery::hash::compute_bytes_hash;
use crate::ingestion::parser::{AstNode, AstNodeKind, DocumentAst};
use crate::models::{Chunk, ChunkType};

/// Separator used between heading levels in the contextual prefix.
pub const HEADING_SEPARATOR: &str = " > ";

/// Contextual chunker that traverses Markdown AST and generates contextually-prefixed chunks.
pub struct ContextualChunker;

impl ContextualChunker {
    /// Formats a list of ancestor heading titles into a contextual prefix string (e.g. `"[Title > Section]"`).
    ///
    /// Returns an empty string if `heading_path` is empty.
    pub fn format_prefix(heading_path: &[String]) -> String {
        if heading_path.is_empty() {
            String::new()
        } else {
            format!("[{}]", heading_path.join(HEADING_SEPARATOR))
        }
    }

    /// Injects heading breadcrumbs into chunk text to generate `contextual_content`.
    ///
    /// - For empty `heading_path`: returns `content.to_string()`.
    /// - For non-empty `heading_path`: returns `"[H1 > H2 > ...] {content}"`.
    pub fn format_contextual_content(heading_path: &[String], content: &str) -> String {
        if heading_path.is_empty() {
            content.to_string()
        } else {
            let prefix = Self::format_prefix(heading_path);
            format!("{prefix} {content}")
        }
    }

    /// Computes a deterministic SHA-256 chunk ID from `doc_id`, `heading_path`, and `content`.
    pub fn compute_chunk_id(doc_id: &str, heading_path: &[String], content: &str) -> String {
        let joined_path = heading_path.join(HEADING_SEPARATOR);
        let key = format!("{doc_id}:{joined_path}:{content}");
        compute_bytes_hash(key.as_bytes())
    }

    /// Traverses the parsed [`DocumentAst`] to generate a flat list of [`Chunk`]s with contextual prefixes.
    pub fn chunk_document(doc_id: &str, ast: &DocumentAst) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let current_heading_path: Vec<String> = Vec::new();

        for root in &ast.roots {
            Self::traverse_node(root, doc_id, None, &current_heading_path, &mut chunks);
        }

        chunks
    }

    fn traverse_node(
        node: &AstNode,
        doc_id: &str,
        parent_chunk_id: Option<&str>,
        current_heading_path: &[String],
        chunks: &mut Vec<Chunk>,
    ) {
        match &node.kind {
            AstNodeKind::Heading { level, title } => {
                let heading_path = current_heading_path.to_vec();
                let contextual_content =
                    Self::format_contextual_content(&heading_path, &node.content);
                let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, &node.content);

                let chunk = Chunk {
                    id: chunk_id.clone(),
                    doc_id: doc_id.to_string(),
                    parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                    chunk_type: ChunkType::Heading { level: *level },
                    heading_path,
                    content: node.content.clone(),
                    contextual_content,
                    line_start: node.line_start,
                    line_end: node.line_end,
                };
                chunks.push(chunk);

                // For child nodes, append this heading's title to the heading path
                let mut child_heading_path = current_heading_path.to_vec();
                child_heading_path.push(title.clone());
                let child_parent_id = Some(chunk_id.as_str());

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        child_parent_id,
                        &child_heading_path,
                        chunks,
                    );
                }
            }
            AstNodeKind::Paragraph => {
                let heading_path = current_heading_path.to_vec();
                let contextual_content =
                    Self::format_contextual_content(&heading_path, &node.content);
                let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, &node.content);

                let chunk = Chunk {
                    id: chunk_id.clone(),
                    doc_id: doc_id.to_string(),
                    parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                    chunk_type: ChunkType::Paragraph,
                    heading_path,
                    content: node.content.clone(),
                    contextual_content,
                    line_start: node.line_start,
                    line_end: node.line_end,
                };
                chunks.push(chunk);

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        Some(chunk_id.as_str()),
                        current_heading_path,
                        chunks,
                    );
                }
            }
            AstNodeKind::CodeBlock { language } => {
                let heading_path = current_heading_path.to_vec();
                let contextual_content =
                    Self::format_contextual_content(&heading_path, &node.content);
                let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, &node.content);

                let chunk = Chunk {
                    id: chunk_id.clone(),
                    doc_id: doc_id.to_string(),
                    parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                    chunk_type: ChunkType::CodeBlock {
                        language: language.clone(),
                    },
                    heading_path,
                    content: node.content.clone(),
                    contextual_content,
                    line_start: node.line_start,
                    line_end: node.line_end,
                };
                chunks.push(chunk);

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        Some(chunk_id.as_str()),
                        current_heading_path,
                        chunks,
                    );
                }
            }
            AstNodeKind::List => {
                let heading_path = current_heading_path.to_vec();
                let contextual_content =
                    Self::format_contextual_content(&heading_path, &node.content);
                let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, &node.content);

                let chunk = Chunk {
                    id: chunk_id.clone(),
                    doc_id: doc_id.to_string(),
                    parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                    chunk_type: ChunkType::List,
                    heading_path,
                    content: node.content.clone(),
                    contextual_content,
                    line_start: node.line_start,
                    line_end: node.line_end,
                };
                chunks.push(chunk);

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        Some(chunk_id.as_str()),
                        current_heading_path,
                        chunks,
                    );
                }
            }
        }
    }
}

/// Convenience helper to generate chunks for a document AST.
pub fn chunk_document(doc_id: &str, ast: &DocumentAst) -> Vec<Chunk> {
    ContextualChunker::chunk_document(doc_id, ast)
}

/// Convenience helper to format contextual content given a heading path and content.
pub fn format_contextual_content(heading_path: &[String], content: &str) -> String {
    ContextualChunker::format_contextual_content(heading_path, content)
}

/// Convenience helper to format heading prefix given a heading path.
pub fn format_prefix(heading_path: &[String]) -> String {
    ContextualChunker::format_prefix(heading_path)
}

/// Convenience helper to compute deterministic chunk ID.
pub fn compute_chunk_id(doc_id: &str, heading_path: &[String], content: &str) -> String {
    ContextualChunker::compute_chunk_id(doc_id, heading_path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::parser::MarkdownParser;

    #[test]
    fn test_format_prefix_empty_and_non_empty() {
        assert_eq!(format_prefix(&[]), "");
        assert_eq!(format_prefix(&["Guide".to_string()]), "[Guide]");
        assert_eq!(
            format_prefix(&[
                "API Reference".to_string(),
                "Authentication".to_string(),
                "OAuth2".to_string()
            ]),
            "[API Reference > Authentication > OAuth2]"
        );
    }

    #[test]
    fn test_format_contextual_content() {
        let content = "The client must send a Bearer token in the Authorization header.";
        let path = vec![
            "API Reference".to_string(),
            "Authentication".to_string(),
            "OAuth2".to_string(),
        ];

        let contextual = format_contextual_content(&path, content);
        assert_eq!(
            contextual,
            "[API Reference > Authentication > OAuth2] The client must send a Bearer token in the Authorization header."
        );

        let empty_path: Vec<String> = Vec::new();
        let unadorned = format_contextual_content(&empty_path, content);
        assert_eq!(unadorned, content);
    }

    #[test]
    fn test_compute_chunk_id_deterministic() {
        let doc_id = "doc_test_123";
        let path = vec!["Section".to_string()];
        let content = "Hello world";

        let id1 = compute_chunk_id(doc_id, &path, content);
        let id2 = compute_chunk_id(doc_id, &path, content);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64); // SHA-256 hex string length

        let id_diff_doc = compute_chunk_id("doc_other", &path, content);
        assert_ne!(id1, id_diff_doc);

        let id_diff_path = compute_chunk_id(doc_id, &["OtherSection".to_string()], content);
        assert_ne!(id1, id_diff_path);

        let id_diff_content = compute_chunk_id(doc_id, &path, "Different content");
        assert_ne!(id1, id_diff_content);
    }

    #[test]
    fn test_contextual_prefix_h1_h2_paragraph() {
        let markdown = r#"# Architecture Guide

## Storage Engine

The storage engine uses SQLite with the sqlite-vec extension.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_arch", &ast);

        // Chunks:
        // 0: H1 Architecture Guide
        // 1: H2 Storage Engine
        // 2: Paragraph The storage engine...
        assert_eq!(chunks.len(), 3);

        let h1 = &chunks[0];
        assert_eq!(h1.chunk_type, ChunkType::Heading { level: 1 });
        assert_eq!(h1.heading_path, Vec::<String>::new());
        assert_eq!(h1.content, "# Architecture Guide");
        assert_eq!(h1.contextual_content, "# Architecture Guide");
        assert_eq!(h1.parent_chunk_id, None);

        let h2 = &chunks[1];
        assert_eq!(h2.chunk_type, ChunkType::Heading { level: 2 });
        assert_eq!(h2.heading_path, vec!["Architecture Guide".to_string()]);
        assert_eq!(
            h2.contextual_content,
            "[Architecture Guide] ## Storage Engine"
        );
        assert_eq!(h2.parent_chunk_id, Some(h1.id.clone()));

        let p = &chunks[2];
        assert_eq!(p.chunk_type, ChunkType::Paragraph);
        assert_eq!(
            p.heading_path,
            vec![
                "Architecture Guide".to_string(),
                "Storage Engine".to_string()
            ]
        );
        assert_eq!(
            p.contextual_content,
            "[Architecture Guide > Storage Engine] The storage engine uses SQLite with the sqlite-vec extension."
        );
        assert_eq!(p.parent_chunk_id, Some(h2.id.clone()));
    }

    #[test]
    fn test_contextual_prefix_deeply_nested() {
        let markdown = r#"# H1 Title
## H2 Section
### H3 Subsection
#### H4 Topic
Deeply nested paragraph text.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_deep", &ast);

        assert_eq!(chunks.len(), 5);

        let p = &chunks[4];
        assert_eq!(p.chunk_type, ChunkType::Paragraph);
        assert_eq!(
            p.heading_path,
            vec![
                "H1 Title".to_string(),
                "H2 Section".to_string(),
                "H3 Subsection".to_string(),
                "H4 Topic".to_string(),
            ]
        );
        assert_eq!(
            p.contextual_content,
            "[H1 Title > H2 Section > H3 Subsection > H4 Topic] Deeply nested paragraph text."
        );
    }

    #[test]
    fn test_heading_path_construction() {
        let markdown = r#"# API Title
## Authentication
```rust
fn authenticate() -> bool { true }
```
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_code", &ast);

        assert_eq!(chunks.len(), 3);

        let code_chunk = &chunks[2];
        assert_eq!(
            code_chunk.chunk_type,
            ChunkType::CodeBlock {
                language: Some("rust".to_string())
            }
        );
        assert_eq!(
            code_chunk.heading_path,
            vec!["API Title".to_string(), "Authentication".to_string()]
        );
        assert_eq!(
            code_chunk.contextual_content,
            "[API Title > Authentication] fn authenticate() -> bool { true }"
        );
    }

    #[test]
    fn test_parent_chunk_id_links_to_heading() {
        let markdown = r#"## Getting Started
First paragraph under H2.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_start", &ast);

        assert_eq!(chunks.len(), 2);
        let h2_chunk = &chunks[0];
        let p_chunk = &chunks[1];

        assert_eq!(h2_chunk.parent_chunk_id, None);
        assert_eq!(p_chunk.parent_chunk_id, Some(h2_chunk.id.clone()));
    }

    #[test]
    fn test_chunk_empty_ast() {
        let ast = DocumentAst {
            roots: Vec::new(),
            title: None,
        };
        let chunks = chunk_document("doc_empty", &ast);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_orphan_paragraphs_no_headings() {
        let markdown = r#"Orphan paragraph 1.

Orphan paragraph 2.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_orphan", &ast);

        assert_eq!(chunks.len(), 2);
        for chunk in &chunks {
            assert_eq!(chunk.chunk_type, ChunkType::Paragraph);
            assert!(chunk.heading_path.is_empty());
            assert_eq!(chunk.parent_chunk_id, None);
            assert_eq!(chunk.content, chunk.contextual_content);
        }
    }

    #[test]
    fn test_chunk_multiple_sibling_sections() {
        let markdown = r#"# Main Guide

## Section One
Content one.

## Section Two
Content two.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_siblings", &ast);

        assert_eq!(chunks.len(), 5);

        let h1 = &chunks[0];
        let h2_one = &chunks[1];
        let p_one = &chunks[2];
        let h2_two = &chunks[3];
        let p_two = &chunks[4];

        assert_eq!(h2_one.parent_chunk_id, Some(h1.id.clone()));
        assert_eq!(p_one.parent_chunk_id, Some(h2_one.id.clone()));
        assert_eq!(
            p_one.heading_path,
            vec!["Main Guide".to_string(), "Section One".to_string()]
        );

        assert_eq!(h2_two.parent_chunk_id, Some(h1.id.clone()));
        assert_eq!(p_two.parent_chunk_id, Some(h2_two.id.clone()));
        assert_eq!(
            p_two.heading_path,
            vec!["Main Guide".to_string(), "Section Two".to_string()]
        );
    }

    #[test]
    fn test_chunk_list_items_and_line_numbers() {
        let markdown = r#"# Overview
- Item 1
- Item 2
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_list", &ast);

        assert_eq!(chunks.len(), 2);
        let list_chunk = &chunks[1];
        assert_eq!(list_chunk.chunk_type, ChunkType::List);
        assert_eq!(list_chunk.heading_path, vec!["Overview".to_string()]);
        assert_eq!(list_chunk.line_start, 2);
        assert_eq!(
            list_chunk.contextual_content,
            "[Overview] - Item 1\n- Item 2"
        );
    }
}
