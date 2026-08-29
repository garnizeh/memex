use crate::discovery::hash::compute_bytes_hash;
use crate::ingestion::parser::{AstNode, AstNodeKind, DocumentAst};
use crate::models::{Chunk, ChunkType};

/// Separator used between heading levels in the contextual prefix.
pub const HEADING_SEPARATOR: &str = " > ";

/// Default maximum chunk size in characters (~512 tokens / ~2000 characters).
pub const DEFAULT_MAX_CHUNK_CHARS: usize = 2000;

/// Contextual chunker that traverses Markdown AST and generates contextually-prefixed chunks,
/// enforcing maximum chunk size guardrails and splitting oversized content.
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

    /// Splits a text string into sentence segments based on standard sentence terminators (`.`, `!`, `?`).
    pub fn split_text_into_sentences(text: &str) -> Vec<&str> {
        let mut sentences = Vec::new();
        let mut start = 0;
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let len = chars.len();

        let mut i = 0;
        while i < len {
            let (_byte_idx, ch) = chars[i];
            if ch == '.' || ch == '!' || ch == '?' {
                // Consume consecutive punctuation like '...', '?!', '..'
                let mut end_punc = i;
                while end_punc + 1 < len {
                    let next_ch = chars[end_punc + 1].1;
                    if next_ch == '.' || next_ch == '!' || next_ch == '?' {
                        end_punc += 1;
                    } else {
                        break;
                    }
                }

                let is_at_end = end_punc + 1 == len;
                let followed_by_ws = if !is_at_end {
                    chars[end_punc + 1].1.is_whitespace()
                } else {
                    false
                };

                if is_at_end || followed_by_ws {
                    let end_byte = if is_at_end {
                        text.len()
                    } else {
                        chars[end_punc + 1].0
                    };
                    let sentence = text[start..end_byte].trim();
                    if !sentence.is_empty() {
                        sentences.push(sentence);
                    }

                    // Advance start past trailing whitespace
                    let mut next_start = end_byte;
                    while next_start < text.len() {
                        let next_ch = text[next_start..].chars().next().unwrap();
                        if next_ch.is_whitespace() {
                            next_start += next_ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    start = next_start;
                    i = end_punc + 1;
                    continue;
                }
            }
            i += 1;
        }

        if start < text.len() {
            let remainder = text[start..].trim();
            if !remainder.is_empty() {
                sentences.push(remainder);
            }
        }

        if sentences.is_empty() && !text.trim().is_empty() {
            sentences.push(text.trim());
        }

        sentences
    }

    /// Splits an oversized paragraph at sentence boundaries into sub-chunks of at most `max_chars`.
    ///
    /// If an individual sentence exceeds `max_chars`, it is further split at word boundaries.
    /// If a single word exceeds `max_chars`, it is split on UTF-8 character boundaries.
    pub fn split_paragraph(content: &str, max_chars: usize) -> Vec<String> {
        if content.len() <= max_chars {
            return vec![content.to_string()];
        }

        let raw_sentences = Self::split_text_into_sentences(content);
        if raw_sentences.is_empty() {
            return Self::split_oversized_text(content, max_chars);
        }

        let mut atomic_units = Vec::new();
        for sentence in raw_sentences {
            if sentence.len() > max_chars {
                let sub_sentences = Self::split_oversized_sentence_by_words(sentence, max_chars);
                atomic_units.extend(sub_sentences);
            } else {
                atomic_units.push(sentence.to_string());
            }
        }

        let mut result = Vec::new();
        let mut current_chunk = String::new();

        for unit in atomic_units {
            let needed = if current_chunk.is_empty() {
                unit.len()
            } else {
                current_chunk.len() + 1 + unit.len()
            };

            if needed <= max_chars {
                if !current_chunk.is_empty() {
                    current_chunk.push(' ');
                }
                current_chunk.push_str(&unit);
            } else {
                if !current_chunk.is_empty() {
                    result.push(current_chunk);
                }
                current_chunk = unit;
            }
        }

        if !current_chunk.is_empty() {
            result.push(current_chunk);
        }

        if result.is_empty() {
            vec![content.to_string()]
        } else {
            result
        }
    }

    /// Splits an oversized code block at line boundaries into sub-chunks of at most `max_chars`.
    pub fn split_code_block(content: &str, max_chars: usize) -> Vec<String> {
        if content.len() <= max_chars {
            return vec![content.to_string()];
        }

        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Self::split_oversized_text(content, max_chars);
        }

        let mut atomic_units = Vec::new();
        for line in lines {
            if line.len() > max_chars {
                let sub_lines = Self::split_oversized_text(line, max_chars);
                atomic_units.extend(sub_lines);
            } else {
                atomic_units.push(line.to_string());
            }
        }

        let mut result = Vec::new();
        let mut current_chunk = String::new();

        for unit in atomic_units {
            let needed = if current_chunk.is_empty() {
                unit.len()
            } else {
                current_chunk.len() + 1 + unit.len() // 1 for '\n'
            };

            if needed <= max_chars {
                if !current_chunk.is_empty() {
                    current_chunk.push('\n');
                }
                current_chunk.push_str(&unit);
            } else {
                if !current_chunk.is_empty() {
                    result.push(current_chunk);
                }
                current_chunk = unit;
            }
        }

        if !current_chunk.is_empty() {
            result.push(current_chunk);
        }

        if result.is_empty() {
            vec![content.to_string()]
        } else {
            result
        }
    }

    /// Splits an oversized list into sub-chunks of at most `max_chars`.
    pub fn split_list(content: &str, max_chars: usize) -> Vec<String> {
        Self::split_code_block(content, max_chars)
    }

    /// Splits content according to its [`ChunkType`].
    pub fn split_chunk_content(
        content: &str,
        chunk_type: &ChunkType,
        max_chars: usize,
    ) -> Vec<String> {
        match chunk_type {
            ChunkType::Paragraph => Self::split_paragraph(content, max_chars),
            ChunkType::CodeBlock { .. } => Self::split_code_block(content, max_chars),
            ChunkType::List => Self::split_list(content, max_chars),
            ChunkType::Heading { .. } => {
                if content.len() <= max_chars {
                    vec![content.to_string()]
                } else {
                    Self::split_paragraph(content, max_chars)
                }
            }
        }
    }

    fn split_oversized_text(text: &str, max_chars: usize) -> Vec<String> {
        if text.len() <= max_chars {
            return vec![text.to_string()];
        }
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut current_len = 0;

        for ch in text.chars() {
            let ch_len = ch.len_utf8();
            if current_len + ch_len > max_chars && !current.is_empty() {
                chunks.push(current);
                current = String::new();
                current_len = 0;
            }
            current.push(ch);
            current_len += ch_len;
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }

    fn split_oversized_sentence_by_words(sentence: &str, max_chars: usize) -> Vec<String> {
        if sentence.len() <= max_chars {
            return vec![sentence.to_string()];
        }

        let words: Vec<&str> = sentence.split_whitespace().collect();
        if words.is_empty() {
            return Self::split_oversized_text(sentence, max_chars);
        }

        let mut result = Vec::new();
        let mut current_chunk = String::new();

        for word in words {
            if word.len() > max_chars {
                if !current_chunk.is_empty() {
                    result.push(current_chunk);
                    current_chunk = String::new();
                }
                let word_chunks = Self::split_oversized_text(word, max_chars);
                let num_chunks = word_chunks.len();
                for (idx, wc) in word_chunks.into_iter().enumerate() {
                    if idx + 1 == num_chunks {
                        current_chunk = wc;
                    } else {
                        result.push(wc);
                    }
                }
            } else {
                let needed = if current_chunk.is_empty() {
                    word.len()
                } else {
                    current_chunk.len() + 1 + word.len()
                };

                if needed <= max_chars {
                    if !current_chunk.is_empty() {
                        current_chunk.push(' ');
                    }
                    current_chunk.push_str(word);
                } else {
                    if !current_chunk.is_empty() {
                        result.push(current_chunk);
                    }
                    current_chunk = word.to_string();
                }
            }
        }

        if !current_chunk.is_empty() {
            result.push(current_chunk);
        }

        result
    }

    /// Traverses the parsed [`DocumentAst`] to generate a flat list of [`Chunk`]s with contextual prefixes,
    /// enforcing the default maximum chunk size (~2000 chars).
    pub fn chunk_document(doc_id: &str, ast: &DocumentAst) -> Vec<Chunk> {
        Self::chunk_document_with_max_size(doc_id, ast, DEFAULT_MAX_CHUNK_CHARS)
    }

    /// Traverses the parsed [`DocumentAst`] to generate a flat list of [`Chunk`]s with contextual prefixes,
    /// splitting oversized content according to `max_chars`.
    pub fn chunk_document_with_max_size(
        doc_id: &str,
        ast: &DocumentAst,
        max_chars: usize,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let current_heading_path: Vec<String> = Vec::new();

        for root in &ast.roots {
            Self::traverse_node(
                root,
                doc_id,
                None,
                &current_heading_path,
                max_chars,
                &mut chunks,
            );
        }

        chunks
    }

    fn traverse_node(
        node: &AstNode,
        doc_id: &str,
        parent_chunk_id: Option<&str>,
        current_heading_path: &[String],
        max_chars: usize,
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
                        max_chars,
                        chunks,
                    );
                }
            }
            AstNodeKind::Paragraph => {
                let heading_path = current_heading_path.to_vec();
                let prefix_len = if heading_path.is_empty() {
                    0
                } else {
                    Self::format_prefix(&heading_path).len() + 1
                };
                let content_limit = if max_chars > prefix_len + 50 {
                    max_chars - prefix_len
                } else {
                    max_chars
                };

                let sub_contents = if node.content.len() > content_limit {
                    Self::split_paragraph(&node.content, content_limit)
                } else {
                    vec![node.content.clone()]
                };

                let mut current_line = node.line_start;
                let total_subs = sub_contents.len();
                let mut first_chunk_id: Option<String> = None;

                for (idx, sub_content) in sub_contents.iter().enumerate() {
                    let contextual_content =
                        Self::format_contextual_content(&heading_path, sub_content);
                    let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, sub_content);
                    if first_chunk_id.is_none() {
                        first_chunk_id = Some(chunk_id.clone());
                    }

                    let newline_count = sub_content.matches('\n').count();
                    let sub_line_start = current_line;
                    let sub_line_end = if idx + 1 == total_subs {
                        node.line_end
                    } else {
                        (current_line + newline_count as u32).min(node.line_end)
                    };
                    current_line = (sub_line_end + 1).min(node.line_end);

                    let chunk = Chunk {
                        id: chunk_id,
                        doc_id: doc_id.to_string(),
                        parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                        chunk_type: ChunkType::Paragraph,
                        heading_path: heading_path.clone(),
                        content: sub_content.clone(),
                        contextual_content,
                        line_start: sub_line_start,
                        line_end: sub_line_end,
                    };
                    chunks.push(chunk);
                }

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        first_chunk_id.as_deref().or(parent_chunk_id),
                        current_heading_path,
                        max_chars,
                        chunks,
                    );
                }
            }
            AstNodeKind::CodeBlock { language } => {
                let heading_path = current_heading_path.to_vec();
                let prefix_len = if heading_path.is_empty() {
                    0
                } else {
                    Self::format_prefix(&heading_path).len() + 1
                };
                let content_limit = if max_chars > prefix_len + 50 {
                    max_chars - prefix_len
                } else {
                    max_chars
                };

                let sub_contents = if node.content.len() > content_limit {
                    Self::split_code_block(&node.content, content_limit)
                } else {
                    vec![node.content.clone()]
                };

                let mut current_line = node.line_start;
                let total_subs = sub_contents.len();
                let mut first_chunk_id: Option<String> = None;

                for (idx, sub_content) in sub_contents.iter().enumerate() {
                    let contextual_content =
                        Self::format_contextual_content(&heading_path, sub_content);
                    let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, sub_content);
                    if first_chunk_id.is_none() {
                        first_chunk_id = Some(chunk_id.clone());
                    }

                    let newline_count = sub_content.matches('\n').count();
                    let sub_line_start = current_line;
                    let sub_line_end = if idx + 1 == total_subs {
                        node.line_end
                    } else {
                        (current_line + newline_count as u32).min(node.line_end)
                    };
                    current_line = (sub_line_end + 1).min(node.line_end);

                    let chunk = Chunk {
                        id: chunk_id,
                        doc_id: doc_id.to_string(),
                        parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                        chunk_type: ChunkType::CodeBlock {
                            language: language.clone(),
                        },
                        heading_path: heading_path.clone(),
                        content: sub_content.clone(),
                        contextual_content,
                        line_start: sub_line_start,
                        line_end: sub_line_end,
                    };
                    chunks.push(chunk);
                }

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        first_chunk_id.as_deref().or(parent_chunk_id),
                        current_heading_path,
                        max_chars,
                        chunks,
                    );
                }
            }
            AstNodeKind::List => {
                let heading_path = current_heading_path.to_vec();
                let prefix_len = if heading_path.is_empty() {
                    0
                } else {
                    Self::format_prefix(&heading_path).len() + 1
                };
                let content_limit = if max_chars > prefix_len + 50 {
                    max_chars - prefix_len
                } else {
                    max_chars
                };

                let sub_contents = if node.content.len() > content_limit {
                    Self::split_list(&node.content, content_limit)
                } else {
                    vec![node.content.clone()]
                };

                let mut current_line = node.line_start;
                let total_subs = sub_contents.len();
                let mut first_chunk_id: Option<String> = None;

                for (idx, sub_content) in sub_contents.iter().enumerate() {
                    let contextual_content =
                        Self::format_contextual_content(&heading_path, sub_content);
                    let chunk_id = Self::compute_chunk_id(doc_id, &heading_path, sub_content);
                    if first_chunk_id.is_none() {
                        first_chunk_id = Some(chunk_id.clone());
                    }

                    let newline_count = sub_content.matches('\n').count();
                    let sub_line_start = current_line;
                    let sub_line_end = if idx + 1 == total_subs {
                        node.line_end
                    } else {
                        (current_line + newline_count as u32).min(node.line_end)
                    };
                    current_line = (sub_line_end + 1).min(node.line_end);

                    let chunk = Chunk {
                        id: chunk_id,
                        doc_id: doc_id.to_string(),
                        parent_chunk_id: parent_chunk_id.map(|s| s.to_string()),
                        chunk_type: ChunkType::List,
                        heading_path: heading_path.clone(),
                        content: sub_content.clone(),
                        contextual_content,
                        line_start: sub_line_start,
                        line_end: sub_line_end,
                    };
                    chunks.push(chunk);
                }

                for child in &node.children {
                    Self::traverse_node(
                        child,
                        doc_id,
                        first_chunk_id.as_deref().or(parent_chunk_id),
                        current_heading_path,
                        max_chars,
                        chunks,
                    );
                }
            }
        }
    }
}

/// Convenience helper to generate chunks for a document AST with default max chunk size.
pub fn chunk_document(doc_id: &str, ast: &DocumentAst) -> Vec<Chunk> {
    ContextualChunker::chunk_document(doc_id, ast)
}

/// Convenience helper to generate chunks for a document AST with custom max chunk size.
pub fn chunk_document_with_max_size(
    doc_id: &str,
    ast: &DocumentAst,
    max_chars: usize,
) -> Vec<Chunk> {
    ContextualChunker::chunk_document_with_max_size(doc_id, ast, max_chars)
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

/// Convenience helper to split text into sentences.
pub fn split_text_into_sentences(text: &str) -> Vec<&str> {
    ContextualChunker::split_text_into_sentences(text)
}

/// Convenience helper to split an oversized paragraph.
pub fn split_paragraph(content: &str, max_chars: usize) -> Vec<String> {
    ContextualChunker::split_paragraph(content, max_chars)
}

/// Convenience helper to split an oversized code block.
pub fn split_code_block(content: &str, max_chars: usize) -> Vec<String> {
    ContextualChunker::split_code_block(content, max_chars)
}

/// Convenience helper to split an oversized list.
pub fn split_list(content: &str, max_chars: usize) -> Vec<String> {
    ContextualChunker::split_list(content, max_chars)
}

/// Convenience helper to split content according to chunk type.
pub fn split_chunk_content(content: &str, chunk_type: &ChunkType, max_chars: usize) -> Vec<String> {
    ContextualChunker::split_chunk_content(content, chunk_type, max_chars)
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

    #[test]
    fn test_split_text_into_sentences() {
        let text = "First sentence. Second sentence! Third sentence? Fourth sentence with ellipsis... And next.";
        let sentences = split_text_into_sentences(text);
        assert_eq!(
            sentences,
            vec![
                "First sentence.",
                "Second sentence!",
                "Third sentence?",
                "Fourth sentence with ellipsis...",
                "And next."
            ]
        );
    }

    #[test]
    fn test_split_paragraph_under_limit() {
        let content = "A short paragraph with two sentences. Everything fits nicely.";
        let split = split_paragraph(content, 2000);
        assert_eq!(split.len(), 1);
        assert_eq!(split[0], content);
    }

    #[test]
    fn test_split_oversized_paragraph_4000_chars_preserves_prefix_and_parent() {
        // Construct a paragraph of ~4000 characters consisting of multiple sentences
        let sentence =
            "This is a detailed sentence describing architectural properties of the system. ";
        let repeats = 4000 / sentence.len() + 1;
        let large_paragraph = sentence.repeat(repeats);
        assert!(large_paragraph.len() >= 4000);

        let markdown = format!(
            r#"# System Architecture
## Storage Engine
{large_paragraph}
"#
        );

        let ast = MarkdownParser::parse(&markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_arch_large", &ast);

        // Chunks should be:
        // 0: H1 System Architecture
        // 1: H2 Storage Engine
        // 2..N: Split sub-chunks of the paragraph
        assert!(chunks.len() >= 3);

        let h1 = &chunks[0];
        let h2 = &chunks[1];
        assert_eq!(h1.chunk_type, ChunkType::Heading { level: 1 });
        assert_eq!(h2.chunk_type, ChunkType::Heading { level: 2 });
        assert_eq!(h2.parent_chunk_id, Some(h1.id.clone()));

        let heading_prefix = "[System Architecture > Storage Engine]";
        let heading_path = vec![
            "System Architecture".to_string(),
            "Storage Engine".to_string(),
        ];

        let paragraph_sub_chunks = &chunks[2..];
        for (i, sub_chunk) in paragraph_sub_chunks.iter().enumerate() {
            assert_eq!(sub_chunk.chunk_type, ChunkType::Paragraph);
            assert_eq!(sub_chunk.heading_path, heading_path);
            assert_eq!(sub_chunk.parent_chunk_id, Some(h2.id.clone()));
            assert!(
                sub_chunk.contextual_content.starts_with(heading_prefix),
                "Sub-chunk {} contextual content does not start with expected prefix: {}",
                i,
                sub_chunk.contextual_content
            );
            assert!(
                sub_chunk.contextual_content.len() <= DEFAULT_MAX_CHUNK_CHARS,
                "Sub-chunk {} exceeds maximum chars limit: {} > {}",
                i,
                sub_chunk.contextual_content.len(),
                DEFAULT_MAX_CHUNK_CHARS
            );
            assert!(!sub_chunk.content.is_empty());
        }
    }

    #[test]
    fn test_split_oversized_code_block() {
        let line = "let mut counter = counter + 1; // perform computation step\n";
        let repeats = 3000 / line.len() + 1;
        let large_code = line.repeat(repeats);
        assert!(large_code.len() >= 3000);

        let markdown = format!(
            r#"# Reference
```rust
{large_code}```
"#
        );

        let ast = MarkdownParser::parse(&markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_code_large", &ast);

        assert!(chunks.len() >= 3);
        let h1 = &chunks[0];

        let code_sub_chunks = &chunks[1..];
        for sub_chunk in code_sub_chunks {
            assert_eq!(
                sub_chunk.chunk_type,
                ChunkType::CodeBlock {
                    language: Some("rust".to_string())
                }
            );
            assert_eq!(sub_chunk.heading_path, vec!["Reference".to_string()]);
            assert_eq!(sub_chunk.parent_chunk_id, Some(h1.id.clone()));
            assert!(sub_chunk.contextual_content.starts_with("[Reference]"));
            assert!(sub_chunk.contextual_content.len() <= DEFAULT_MAX_CHUNK_CHARS);
        }
    }

    #[test]
    fn test_split_oversized_list() {
        let item = "- Item detailing an important aspect of configuration and behavior\n";
        let repeats = 2500 / item.len() + 1;
        let large_list = item.repeat(repeats);

        let markdown = format!(
            r#"# Config Options
{large_list}
"#
        );

        let ast = MarkdownParser::parse(&markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_list_large", &ast);

        assert!(chunks.len() >= 3);
        let h1 = &chunks[0];

        let list_sub_chunks = &chunks[1..];
        for sub_chunk in list_sub_chunks {
            assert_eq!(sub_chunk.chunk_type, ChunkType::List);
            assert_eq!(sub_chunk.heading_path, vec!["Config Options".to_string()]);
            assert_eq!(sub_chunk.parent_chunk_id, Some(h1.id.clone()));
            assert!(sub_chunk.contextual_content.starts_with("[Config Options]"));
            assert!(sub_chunk.contextual_content.len() <= DEFAULT_MAX_CHUNK_CHARS);
        }
    }

    #[test]
    fn test_split_no_punctuation_long_text() {
        let words = "unpunctuated text with many words ".repeat(100);
        let split = split_paragraph(&words, 200);
        assert!(split.len() > 1);
        for s in split {
            assert!(s.len() <= 200);
        }
    }
}
