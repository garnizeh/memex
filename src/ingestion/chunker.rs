use crate::discovery::hash::compute_bytes_hash;
use crate::ingestion::parser::{AstNode, AstNodeKind, DocumentAst};
use crate::models::{Chunk, ChunkType, Edge, EdgeType};

/// Separator used between heading levels in the contextual prefix.
pub const HEADING_SEPARATOR: &str = " > ";

/// Default maximum chunk size in characters (~512 tokens / ~2000 characters).
pub const DEFAULT_MAX_CHUNK_CHARS: usize = 2000;

/// Contextual chunker that traverses Markdown AST and generates contextually-prefixed chunks,
/// builds hierarchy edges, enforces maximum chunk size guardrails, and splits oversized content.
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

    /// Generates hierarchy edges (`EdgeType::Hierarchy`) from a slice of [`Chunk`]s.
    ///
    /// An edge is constructed for every chunk that has a `parent_chunk_id`, linking the parent heading
    /// chunk (`source_chunk_id`) to the child chunk (`target_chunk_id`).
    pub fn build_hierarchy_edges(chunks: &[Chunk]) -> Vec<Edge> {
        let mut edges = Vec::new();
        for chunk in chunks {
            if let Some(parent_id) = &chunk.parent_chunk_id {
                edges.push(Edge {
                    source_chunk_id: parent_id.clone(),
                    target_chunk_id: chunk.id.clone(),
                    edge_type: EdgeType::Hierarchy,
                    link_text: None,
                });
            }
        }
        edges
    }

    /// Converts text (e.g. a heading title or anchor) into a normalized URL/anchor slug.
    ///
    /// Converts to lowercase, strips leading/trailing markdown characters (like `#` or `` ` ``),
    /// converts whitespace and underscores/hyphens to single hyphens, and removes punctuation.
    pub fn slugify(text: &str) -> String {
        let mut clean = text.trim().trim_start_matches('#').trim().to_string();

        // If text contains markdown link [text](url), extract the inner text
        while let Some(start_bracket) = clean.find('[') {
            if let Some(end_bracket) = clean[start_bracket..].find(']') {
                let end_bracket_idx = start_bracket + end_bracket;
                if let Some(start_paren) = clean[end_bracket_idx..].find('(') {
                    if start_paren == 1 {
                        let start_paren_idx = end_bracket_idx + start_paren;
                        if let Some(end_paren) = clean[start_paren_idx..].find(')') {
                            let end_paren_idx = start_paren_idx + end_paren;
                            let link_text = clean[start_bracket + 1..end_bracket_idx].to_string();
                            clean = format!(
                                "{}{}{}",
                                &clean[..start_bracket],
                                link_text,
                                &clean[end_paren_idx + 1..]
                            );
                            continue;
                        }
                    }
                }
            }
            break;
        }

        let mut slug = String::with_capacity(clean.len());
        let mut prev_is_dash = false;

        for ch in clean.chars() {
            if ch.is_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
                prev_is_dash = false;
            } else if ch.is_whitespace() || ch == '-' || ch == '_' {
                if !prev_is_dash && !slug.is_empty() {
                    slug.push('-');
                    prev_is_dash = true;
                }
            }
        }

        if slug.ends_with('-') {
            slug.pop();
        }

        slug
    }

    /// Extracts inline Markdown links `[text](target)` from raw content.
    pub fn extract_markdown_links(content: &str) -> Vec<(String, String)> {
        let mut links = Vec::new();
        let parser = pulldown_cmark::Parser::new(content);
        let mut in_link = false;
        let mut current_dest = String::new();
        let mut current_text = String::new();

        for event in parser {
            match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                    in_link = true;
                    current_dest = dest_url.to_string();
                    current_text.clear();
                }
                pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Link) => {
                    if in_link {
                        let text = current_text.trim().to_string();
                        let dest = current_dest.trim().to_string();
                        if !dest.is_empty() {
                            links.push((text, dest));
                        }
                        in_link = false;
                        current_dest.clear();
                        current_text.clear();
                    }
                }
                pulldown_cmark::Event::Text(t)
                | pulldown_cmark::Event::Code(t)
                | pulldown_cmark::Event::InlineMath(t)
                | pulldown_cmark::Event::DisplayMath(t)
                    if in_link =>
                {
                    current_text.push_str(&t);
                }
                _ => {}
            }
        }
        links
    }

    /// Returns `true` if the URL is an external link (e.g. `http://`, `https://`, `mailto:`),
    /// which should not generate knowledge graph edges.
    pub fn is_external_link(url: &str) -> bool {
        let trimmed = url.trim().to_ascii_lowercase();
        trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("mailto:")
            || trimmed.starts_with("ftp://")
            || trimmed.starts_with("ftps://")
            || trimmed.starts_with("//")
            || (trimmed.contains("://") && !trimmed.starts_with("file://"))
    }

    /// Parses a link target into an optional document file path and an optional anchor slug.
    ///
    /// Examples:
    /// - `"#oauth2"` -> `(None, Some("oauth2"))`
    /// - `"auth.md#oauth2"` -> `(Some("auth.md"), Some("oauth2"))`
    /// - `"auth.md"` -> `(Some("auth.md"), None)`
    /// - `"../api/auth.md#oauth2"` -> `(Some("../api/auth.md"), Some("oauth2"))`
    pub fn parse_link_target(target: &str) -> (Option<&str>, Option<&str>) {
        let trimmed = target.trim();
        if trimmed.is_empty() || trimmed == "#" {
            return (None, None);
        }

        if let Some(anchor) = trimmed.strip_prefix('#') {
            let anchor_clean = anchor.trim();
            if anchor_clean.is_empty() {
                (None, None)
            } else {
                (None, Some(anchor_clean))
            }
        } else if let Some((file_part, anchor_part)) = trimmed.split_once('#') {
            let file_clean = file_part.trim();
            let anchor_clean = anchor_part.trim();
            let file_opt = if file_clean.is_empty() {
                None
            } else {
                Some(file_clean)
            };
            let anchor_opt = if anchor_clean.is_empty() {
                None
            } else {
                Some(anchor_clean)
            };
            (file_opt, anchor_opt)
        } else {
            (Some(trimmed), None)
        }
    }

    /// Normalizes a relative target path against a base directory string.
    pub fn normalize_relative_path(base_dir: &str, relative_target: &str) -> String {
        let mut parts = Vec::new();
        if !base_dir.is_empty() && base_dir != "." {
            for part in base_dir.split('/') {
                if !part.is_empty() && part != "." {
                    parts.push(part);
                }
            }
        }
        for part in relative_target.split('/') {
            if part.is_empty() || part == "." {
                continue;
            } else if part == ".." {
                parts.pop();
            } else {
                parts.push(part);
            }
        }
        parts.join("/")
    }

    /// Resolves explicit Markdown links (`[text](target)`) within or across documents into [`EdgeType::ExplicitLink`] edges.
    ///
    /// - Resolves internal anchors (`#slug`) to matching heading chunks.
    /// - Resolves cross-document links (`other.md#slug` or `other.md`) to matching chunks in target documents.
    /// - Skips external HTTP/HTTPS/mailto URLs.
    /// - Skips unresolvable/broken links without error.
    pub fn resolve_explicit_links(
        chunks: &[Chunk],
        doc_paths: Option<&[(String, String)]>,
    ) -> Vec<Edge> {
        let mut edges = Vec::new();
        if chunks.is_empty() {
            return edges;
        }

        // Build heading slug index: (doc_id, slug) -> chunk_id
        // Also build doc roots index: doc_id -> first chunk_id
        // Also build doc first heading index: doc_id -> first heading chunk_id
        let mut heading_slug_map: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        let mut doc_first_chunk: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut doc_first_heading: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for chunk in chunks {
            doc_first_chunk
                .entry(chunk.doc_id.clone())
                .or_insert_with(|| chunk.id.clone());

            if let ChunkType::Heading { .. } = &chunk.chunk_type {
                doc_first_heading
                    .entry(chunk.doc_id.clone())
                    .or_insert_with(|| chunk.id.clone());

                let heading_title = chunk.content.trim_start_matches('#').trim();
                let slug = Self::slugify(heading_title);
                if !slug.is_empty() {
                    heading_slug_map
                        .entry((chunk.doc_id.clone(), slug))
                        .or_insert_with(|| chunk.id.clone());
                }
            }
        }

        // Map doc_id -> file_path and file_path -> doc_id
        let mut doc_id_to_path: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut path_to_doc_id: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if let Some(paths) = doc_paths {
            for (doc_id, path) in paths {
                let normalized = path.replace('\\', "/");
                doc_id_to_path.insert(doc_id.clone(), normalized.clone());
                path_to_doc_id.insert(normalized, doc_id.clone());
            }
        }

        for chunk in chunks {
            let links = Self::extract_markdown_links(&chunk.content);
            for (link_text, dest_url) in links {
                if Self::is_external_link(&dest_url) {
                    continue;
                }

                let (file_opt, anchor_opt) = Self::parse_link_target(&dest_url);

                // Determine target doc_id
                let target_doc_id = match file_opt {
                    None => Some(chunk.doc_id.clone()),
                    Some(file_target) => {
                        let clean_target = file_target.replace('\\', "/");
                        // 1. Direct doc_id match
                        if doc_first_chunk.contains_key(&clean_target) {
                            Some(clean_target)
                        } else if let Some(target_id) = path_to_doc_id.get(&clean_target) {
                            Some(target_id.clone())
                        } else {
                            // Try resolving relative path from current document's directory
                            let source_path = doc_id_to_path.get(&chunk.doc_id);
                            let source_dir = source_path
                                .and_then(|p| std::path::Path::new(p).parent())
                                .and_then(|p| p.to_str())
                                .unwrap_or("");
                            let normalized =
                                Self::normalize_relative_path(source_dir, &clean_target);

                            if let Some(target_id) = path_to_doc_id.get(&normalized) {
                                Some(target_id.clone())
                            } else {
                                // Try matching by filename / basename
                                let target_file_name = std::path::Path::new(&clean_target)
                                    .file_name()
                                    .and_then(|f| f.to_str());

                                let mut found_id = None;
                                if let Some(t_fname) = target_file_name {
                                    for (p, d_id) in &path_to_doc_id {
                                        if let Some(p_fname) = std::path::Path::new(p)
                                            .file_name()
                                            .and_then(|f| f.to_str())
                                        {
                                            if p_fname == t_fname {
                                                found_id = Some(d_id.clone());
                                                break;
                                            }
                                        }
                                    }
                                }
                                found_id
                            }
                        }
                    }
                };

                let target_doc_id = match target_doc_id {
                    Some(id) => id,
                    None => continue, // Unresolvable document (broken link)
                };

                // Determine target chunk_id
                let target_chunk_id = match anchor_opt {
                    Some(anchor) => {
                        let slug = Self::slugify(anchor);
                        heading_slug_map
                            .get(&(target_doc_id.clone(), slug))
                            .cloned()
                    }
                    None => {
                        // Point to first heading if available, or first chunk of target doc
                        doc_first_heading
                            .get(&target_doc_id)
                            .or_else(|| doc_first_chunk.get(&target_doc_id))
                            .cloned()
                    }
                };

                if let Some(target_id) = target_chunk_id {
                    edges.push(Edge {
                        source_chunk_id: chunk.id.clone(),
                        target_chunk_id: target_id,
                        edge_type: EdgeType::ExplicitLink,
                        link_text: if link_text.is_empty() {
                            None
                        } else {
                            Some(link_text)
                        },
                    });
                }
            }
        }

        edges
    }

    /// Convenience method to build explicit link edges for chunks within a single document or pre-indexed chunks.
    pub fn build_explicit_link_edges(chunks: &[Chunk]) -> Vec<Edge> {
        Self::resolve_explicit_links(chunks, None)
    }
}

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

/// Convenience helper to build hierarchy edges from chunks.
pub fn build_hierarchy_edges(chunks: &[Chunk]) -> Vec<Edge> {
    ContextualChunker::build_hierarchy_edges(chunks)
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

/// Convenience helper to convert text into a URL/anchor slug.
pub fn slugify(text: &str) -> String {
    ContextualChunker::slugify(text)
}

/// Convenience helper to extract inline Markdown links `(link_text, destination)` from content.
pub fn extract_markdown_links(content: &str) -> Vec<(String, String)> {
    ContextualChunker::extract_markdown_links(content)
}

/// Convenience helper to check if a URL is an external link.
pub fn is_external_link(url: &str) -> bool {
    ContextualChunker::is_external_link(url)
}

/// Convenience helper to parse a link target into `(Option<file_path>, Option<anchor_slug>)`.
pub fn parse_link_target(target: &str) -> (Option<&str>, Option<&str>) {
    ContextualChunker::parse_link_target(target)
}

/// Convenience helper to normalize a relative path against a base directory.
pub fn normalize_relative_path(base_dir: &str, relative_target: &str) -> String {
    ContextualChunker::normalize_relative_path(base_dir, relative_target)
}

/// Convenience helper to resolve explicit markdown links across chunks and documents.
pub fn resolve_explicit_links(
    chunks: &[Chunk],
    doc_paths: Option<&[(String, String)]>,
) -> Vec<Edge> {
    ContextualChunker::resolve_explicit_links(chunks, doc_paths)
}

/// Convenience helper to build explicit link edges for chunks within a document.
pub fn build_explicit_link_edges(chunks: &[Chunk]) -> Vec<Edge> {
    ContextualChunker::build_explicit_link_edges(chunks)
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

    #[test]
    fn test_build_hierarchy_edges_nested_sections() {
        let markdown = r#"# Root H1
Introduction under root.

## Child H2 A
Paragraph under H2 A.

### Subchild H3
Paragraph under H3.

```rust
fn code_under_h3() {}
```

## Child H2 B
Paragraph under H2 B.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_hierarchy_test", &ast);

        assert_eq!(chunks.len(), 9);

        let edges = build_hierarchy_edges(&chunks);
        assert_eq!(edges.len(), 8);

        for edge in &edges {
            assert_eq!(edge.edge_type, EdgeType::Hierarchy);
            assert_eq!(edge.link_text, None);
        }

        let h1_id = &chunks[0].id;
        let p_intro_id = &chunks[1].id;
        let h2_a_id = &chunks[2].id;
        let p_h2_a_id = &chunks[3].id;
        let h3_id = &chunks[4].id;
        let p_h3_id = &chunks[5].id;
        let code_h3_id = &chunks[6].id;
        let h2_b_id = &chunks[7].id;
        let p_h2_b_id = &chunks[8].id;

        assert_eq!(edges[0].source_chunk_id, *h1_id);
        assert_eq!(edges[0].target_chunk_id, *p_intro_id);

        assert_eq!(edges[1].source_chunk_id, *h1_id);
        assert_eq!(edges[1].target_chunk_id, *h2_a_id);

        assert_eq!(edges[2].source_chunk_id, *h2_a_id);
        assert_eq!(edges[2].target_chunk_id, *p_h2_a_id);

        assert_eq!(edges[3].source_chunk_id, *h2_a_id);
        assert_eq!(edges[3].target_chunk_id, *h3_id);

        assert_eq!(edges[4].source_chunk_id, *h3_id);
        assert_eq!(edges[4].target_chunk_id, *p_h3_id);

        assert_eq!(edges[5].source_chunk_id, *h3_id);
        assert_eq!(edges[5].target_chunk_id, *code_h3_id);

        assert_eq!(edges[6].source_chunk_id, *h1_id);
        assert_eq!(edges[6].target_chunk_id, *h2_b_id);

        assert_eq!(edges[7].source_chunk_id, *h2_b_id);
        assert_eq!(edges[7].target_chunk_id, *p_h2_b_id);
    }

    #[test]
    fn test_build_hierarchy_edges_empty_or_no_parents() {
        assert_eq!(build_hierarchy_edges(&[]).len(), 0);

        let markdown = "Intro paragraph without headings.\n\nSecond orphan paragraph.";
        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_no_headings", &ast);
        let edges = build_hierarchy_edges(&chunks);
        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn test_slugify_various_inputs() {
        assert_eq!(slugify("Storage Engine"), "storage-engine");
        assert_eq!(slugify("OAuth2 Authentication!"), "oauth2-authentication");
        assert_eq!(
            slugify("### Vector Search (384-dim)"),
            "vector-search-384-dim"
        );
        assert_eq!(slugify("  Heading  With   Spaces  "), "heading-with-spaces");
        assert_eq!(slugify("`MemexError` Type"), "memexerror-type");
        assert_eq!(slugify("What is Memex?"), "what-is-memex");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_extract_markdown_links() {
        let content = "Check [OAuth2 Guide](auth.md#oauth2) and [Section](#section) or [Google](https://google.com).";
        let links = extract_markdown_links(content);
        assert_eq!(
            links,
            vec![
                ("OAuth2 Guide".to_string(), "auth.md#oauth2".to_string()),
                ("Section".to_string(), "#section".to_string()),
                ("Google".to_string(), "https://google.com".to_string()),
            ]
        );
    }

    #[test]
    fn test_is_external_link() {
        assert!(is_external_link("http://example.com"));
        assert!(is_external_link("https://github.com/garnizeh/memex"));
        assert!(is_external_link("mailto:dev@example.com"));
        assert!(is_external_link("ftp://ftp.example.com"));
        assert!(is_external_link("//cdn.example.com/asset.js"));

        assert!(!is_external_link("#internal-anchor"));
        assert!(!is_external_link("other.md"));
        assert!(!is_external_link("../api/auth.md#oauth2"));
        assert!(!is_external_link("docs/architecture.md"));
    }

    #[test]
    fn test_parse_link_target() {
        assert_eq!(parse_link_target("#oauth2"), (None, Some("oauth2")));
        assert_eq!(
            parse_link_target("auth.md#oauth2"),
            (Some("auth.md"), Some("oauth2"))
        );
        assert_eq!(parse_link_target("auth.md"), (Some("auth.md"), None));
        assert_eq!(
            parse_link_target("../api/auth.md#oauth2"),
            (Some("../api/auth.md"), Some("oauth2"))
        );
        assert_eq!(parse_link_target("#"), (None, None));
        assert_eq!(parse_link_target(""), (None, None));
    }

    #[test]
    fn test_normalize_relative_path() {
        assert_eq!(
            normalize_relative_path("docs/guides", "../api/auth.md"),
            "docs/api/auth.md"
        );
        assert_eq!(
            normalize_relative_path("docs", "architecture.md"),
            "docs/architecture.md"
        );
        assert_eq!(normalize_relative_path("", "auth.md"), "auth.md");
        assert_eq!(
            normalize_relative_path("docs", "./setup.md"),
            "docs/setup.md"
        );
    }

    #[test]
    fn test_explicit_link_edge_resolved_internal_anchor() {
        let markdown = r#"# User Guide

## Overview
For details on login, see [Authentication Section](#authentication).

## Authentication
Here are authentication details.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");
        let chunks = chunk_document("doc_guide", &ast);

        // Chunks:
        // 0: H1 User Guide
        // 1: H2 Overview
        // 2: Paragraph with link [Authentication Section](#authentication)
        // 3: H2 Authentication
        // 4: Paragraph under Authentication
        assert_eq!(chunks.len(), 5);

        let edges = build_explicit_link_edges(&chunks);
        assert_eq!(edges.len(), 1);

        let link_edge = &edges[0];
        let p_overview = &chunks[2];
        let h2_auth = &chunks[3];

        assert_eq!(link_edge.edge_type, EdgeType::ExplicitLink);
        assert_eq!(link_edge.source_chunk_id, p_overview.id);
        assert_eq!(link_edge.target_chunk_id, h2_auth.id);
        assert_eq!(
            link_edge.link_text,
            Some("Authentication Section".to_string())
        );
    }

    #[test]
    fn test_explicit_link_edge_resolved_cross_document() {
        let doc1_md = r#"# Guide
Read [see auth](auth.md#oauth2) for credentials.
"#;
        let doc2_md = r#"# Security
## OAuth2
OAuth2 token configuration.
"#;

        let ast1 = MarkdownParser::parse(doc1_md).unwrap();
        let ast2 = MarkdownParser::parse(doc2_md).unwrap();

        let doc1_chunks = chunk_document("doc_guide_id", &ast1);
        let doc2_chunks = chunk_document("doc_auth_id", &ast2);

        let mut all_chunks = doc1_chunks;
        all_chunks.extend(doc2_chunks);

        let doc_paths = vec![
            ("doc_guide_id".to_string(), "docs/guide.md".to_string()),
            ("doc_auth_id".to_string(), "docs/auth.md".to_string()),
        ];

        let edges = resolve_explicit_links(&all_chunks, Some(&doc_paths));
        assert_eq!(edges.len(), 1);

        let edge = &edges[0];
        assert_eq!(edge.edge_type, EdgeType::ExplicitLink);
        assert_eq!(edge.link_text, Some("see auth".to_string()));

        // Source chunk is Paragraph in doc1
        assert_eq!(edge.source_chunk_id, all_chunks[1].id);
        // Target chunk is H2 OAuth2 in doc2 (index 3 in all_chunks: 0=H1, 1=P, 2=H1 Security, 3=H2 OAuth2)
        assert_eq!(edge.target_chunk_id, all_chunks[3].id);
    }

    #[test]
    fn test_broken_link_no_edge() {
        let markdown = r#"# Broken Links
Here is a broken link: [see nothing](missing.md#nonexistent).
And internal broken: [missing section](#not-here).
"#;

        let ast = MarkdownParser::parse(markdown).unwrap();
        let chunks = chunk_document("doc_broken", &ast);

        let edges = build_explicit_link_edges(&chunks);
        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn test_external_urls_do_not_generate_edges() {
        let markdown = r#"# External Links
Visit [Official Website](https://example.com/docs) or [Email](mailto:info@example.com).
"#;

        let ast = MarkdownParser::parse(markdown).unwrap();
        let chunks = chunk_document("doc_ext", &ast);

        let edges = build_explicit_link_edges(&chunks);
        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn test_multiple_explicit_links_in_single_chunk() {
        let markdown = r#"# Reference

## Section A
Alpha text.

## Section B
Beta text.

## Summary
See [Section A](#section-a) and also [Section B](#section-b).
"#;

        let ast = MarkdownParser::parse(markdown).unwrap();
        let chunks = chunk_document("doc_multi", &ast);

        let edges = build_explicit_link_edges(&chunks);
        assert_eq!(edges.len(), 2);

        let h2_a = &chunks[1];
        let h2_b = &chunks[3];
        let p_summary = &chunks[6];

        assert_eq!(edges[0].source_chunk_id, p_summary.id);
        assert_eq!(edges[0].target_chunk_id, h2_a.id);
        assert_eq!(edges[0].link_text, Some("Section A".to_string()));

        assert_eq!(edges[1].source_chunk_id, p_summary.id);
        assert_eq!(edges[1].target_chunk_id, h2_b.id);
        assert_eq!(edges[1].link_text, Some("Section B".to_string()));
    }
}
