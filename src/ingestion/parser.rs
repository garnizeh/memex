use crate::errors::Result;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

/// Represents the type and metadata of a content node in the Markdown AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AstNodeKind {
    /// Heading node with level (1 to 6) and heading title text.
    Heading { level: u8, title: String },
    /// Paragraph text block.
    Paragraph,
    /// Code block with optional language specifier (e.g., "rust", "python").
    CodeBlock { language: Option<String> },
    /// List block containing item text or nested list content.
    List,
}

/// Represents an individual structured node extracted from a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstNode {
    /// The node type and specific metadata.
    pub kind: AstNodeKind,
    /// The unformatted/raw text content of this node.
    pub content: String,
    /// 1-indexed starting line number in the source markdown.
    pub line_start: u32,
    /// 1-indexed ending line number (inclusive) in the source markdown.
    pub line_end: u32,
    /// Byte offset where this node starts in the source string.
    pub byte_start: usize,
    /// Byte offset where this node ends in the source string (inclusive / end offset).
    pub byte_end: usize,
    /// Child nodes (e.g., sections, paragraphs, code blocks nested under a heading).
    pub children: Vec<AstNode>,
}

/// Represents the structured Abstract Syntax Tree of a parsed Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentAst {
    /// Top-level AST nodes (e.g., root headings or orphan introductory paragraphs before any heading).
    pub roots: Vec<AstNode>,
    /// Extracted document title if found (typically from the first H1 heading, or first heading).
    pub title: Option<String>,
}

/// Computes a line lookup index from raw markdown content to map byte offsets to 1-indexed line numbers.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Creates a new `LineIndex` from the document text.
    pub fn new(content: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in content.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { line_starts }
    }

    /// Converts a byte offset to a 1-indexed line number using binary search.
    pub fn line_number(&self, byte_offset: usize) -> u32 {
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => (idx + 1) as u32,
            Err(idx) => idx as u32,
        }
    }
}

enum BlockState {
    None,
    Heading {
        level: u8,
        title: String,
        byte_start: usize,
    },
    Paragraph {
        text: String,
        byte_start: usize,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
        byte_start: usize,
    },
    List {
        text: String,
        byte_start: usize,
    },
}

/// Markdown event-to-AST parser built on `pulldown-cmark`.
pub struct MarkdownParser;

impl MarkdownParser {
    /// Parses Markdown content into a structured [`DocumentAst`].
    pub fn parse(content: &str) -> Result<DocumentAst> {
        let line_index = LineIndex::new(content);

        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

        let parser = Parser::new_ext(content, options).into_offset_iter();

        let mut flat_nodes = Vec::new();
        let mut current_state = BlockState::None;
        let mut list_depth: usize = 0;

        for (event, range) in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    // Flush any current state if needed
                    Self::flush_state(
                        &mut current_state,
                        &line_index,
                        &mut flat_nodes,
                        range.start,
                    );
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    current_state = BlockState::Heading {
                        level: lvl,
                        title: String::new(),
                        byte_start: range.start,
                    };
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let BlockState::Heading {
                        level,
                        title,
                        byte_start,
                    } = current_state
                    {
                        let byte_end = range.end;
                        let line_start = line_index.line_number(byte_start);
                        let line_end =
                            line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                        let raw_content = content
                            .get(byte_start..byte_end)
                            .unwrap_or(&title)
                            .trim()
                            .to_string();

                        flat_nodes.push((
                            AstNode {
                                kind: AstNodeKind::Heading {
                                    level,
                                    title: title.trim().to_string(),
                                },
                                content: if raw_content.is_empty() {
                                    title.trim().to_string()
                                } else {
                                    raw_content
                                },
                                line_start,
                                line_end,
                                byte_start,
                                byte_end,
                                children: Vec::new(),
                            },
                            Some(level),
                        ));
                        current_state = BlockState::None;
                    }
                }
                Event::Start(Tag::Paragraph) => {
                    if list_depth == 0 {
                        Self::flush_state(
                            &mut current_state,
                            &line_index,
                            &mut flat_nodes,
                            range.start,
                        );
                        current_state = BlockState::Paragraph {
                            text: String::new(),
                            byte_start: range.start,
                        };
                    }
                }
                Event::End(TagEnd::Paragraph) => {
                    if list_depth == 0 {
                        if let BlockState::Paragraph { text, byte_start } = current_state {
                            let byte_end = range.end;
                            let line_start = line_index.line_number(byte_start);
                            let line_end =
                                line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                flat_nodes.push((
                                    AstNode {
                                        kind: AstNodeKind::Paragraph,
                                        content: trimmed.to_string(),
                                        line_start,
                                        line_end,
                                        byte_start,
                                        byte_end,
                                        children: Vec::new(),
                                    },
                                    None,
                                ));
                            }
                            current_state = BlockState::None;
                        }
                    }
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    Self::flush_state(
                        &mut current_state,
                        &line_index,
                        &mut flat_nodes,
                        range.start,
                    );
                    let language = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let lang_str = lang.trim().to_string();
                            if lang_str.is_empty() {
                                None
                            } else {
                                // Extract first word of language tag (e.g. "rust,ignore" -> "rust" or "rust")
                                Some(
                                    lang_str
                                        .split_whitespace()
                                        .next()
                                        .unwrap_or(&lang_str)
                                        .to_string(),
                                )
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    current_state = BlockState::CodeBlock {
                        language,
                        code: String::new(),
                        byte_start: range.start,
                    };
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let BlockState::CodeBlock {
                        language,
                        code,
                        byte_start,
                    } = current_state
                    {
                        let byte_end = range.end;
                        let line_start = line_index.line_number(byte_start);
                        let line_end =
                            line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                        let trimmed = code.trim_end_matches('\n');
                        flat_nodes.push((
                            AstNode {
                                kind: AstNodeKind::CodeBlock { language },
                                content: trimmed.to_string(),
                                line_start,
                                line_end,
                                byte_start,
                                byte_end,
                                children: Vec::new(),
                            },
                            None,
                        ));
                        current_state = BlockState::None;
                    }
                }
                Event::Start(Tag::List(_)) => {
                    if list_depth == 0 {
                        Self::flush_state(
                            &mut current_state,
                            &line_index,
                            &mut flat_nodes,
                            range.start,
                        );
                        current_state = BlockState::List {
                            text: String::new(),
                            byte_start: range.start,
                        };
                    }
                    list_depth += 1;
                }
                Event::End(TagEnd::List(_)) => {
                    list_depth = list_depth.saturating_sub(1);
                    if list_depth == 0 {
                        if let BlockState::List { text, byte_start } = current_state {
                            let byte_end = range.end;
                            let line_start = line_index.line_number(byte_start);
                            let line_end =
                                line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                flat_nodes.push((
                                    AstNode {
                                        kind: AstNodeKind::List,
                                        content: trimmed.to_string(),
                                        line_start,
                                        line_end,
                                        byte_start,
                                        byte_end,
                                        children: Vec::new(),
                                    },
                                    None,
                                ));
                            }
                            current_state = BlockState::None;
                        }
                    }
                }
                Event::Start(Tag::Item) => {
                    if let BlockState::List { ref mut text, .. } = current_state {
                        if !text.is_empty() && !text.ends_with('\n') {
                            text.push('\n');
                        }
                        text.push_str("- ");
                    }
                }
                Event::End(TagEnd::Item) => {
                    if let BlockState::List { ref mut text, .. } = current_state {
                        if !text.ends_with('\n') {
                            text.push('\n');
                        }
                    }
                }
                Event::Text(t) => match current_state {
                    BlockState::Heading { ref mut title, .. } => {
                        title.push_str(&t);
                    }
                    BlockState::Paragraph { ref mut text, .. } => {
                        text.push_str(&t);
                    }
                    BlockState::CodeBlock { ref mut code, .. } => {
                        code.push_str(&t);
                    }
                    BlockState::List { ref mut text, .. } => {
                        text.push_str(&t);
                    }
                    BlockState::None => {}
                },
                Event::Code(c) => match current_state {
                    BlockState::Heading { ref mut title, .. } => {
                        title.push('`');
                        title.push_str(&c);
                        title.push('`');
                    }
                    BlockState::Paragraph { ref mut text, .. } => {
                        text.push('`');
                        text.push_str(&c);
                        text.push('`');
                    }
                    BlockState::CodeBlock { ref mut code, .. } => {
                        code.push_str(&c);
                    }
                    BlockState::List { ref mut text, .. } => {
                        text.push('`');
                        text.push_str(&c);
                        text.push('`');
                    }
                    BlockState::None => {}
                },
                Event::SoftBreak | Event::HardBreak => match current_state {
                    BlockState::Paragraph { ref mut text, .. } => {
                        text.push(' ');
                    }
                    BlockState::Heading { ref mut title, .. } => {
                        title.push(' ');
                    }
                    BlockState::List { ref mut text, .. } => {
                        text.push(' ');
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        // Flush any remaining state at EOF
        Self::flush_state(
            &mut current_state,
            &line_index,
            &mut flat_nodes,
            content.len(),
        );

        // Find document title: look for the first H1, or if none, the first heading
        let mut first_h1 = None;
        let mut first_any_heading = None;

        for (node, level_opt) in &flat_nodes {
            if let Some(level) = level_opt {
                if let AstNodeKind::Heading { title, .. } = &node.kind {
                    if first_any_heading.is_none() {
                        first_any_heading = Some(title.clone());
                    }
                    if *level == 1 && first_h1.is_none() {
                        first_h1 = Some(title.clone());
                    }
                }
            }
        }

        let doc_title = first_h1.or(first_any_heading);

        // Build hierarchical tree using heading stack
        // Stack holds: (AstNode, HeadingLevel)
        let mut root_nodes: Vec<AstNode> = Vec::new();
        let mut stack: Vec<(AstNode, u8)> = Vec::new();

        for (node, level_opt) in flat_nodes {
            match level_opt {
                Some(level) => {
                    // Pop nodes from stack with level >= current heading level
                    while let Some((_, top_level)) = stack.last() {
                        if *top_level >= level {
                            let (popped_node, _) = stack.pop().unwrap();
                            if let Some((parent_node, _)) = stack.last_mut() {
                                parent_node.children.push(popped_node);
                            } else {
                                root_nodes.push(popped_node);
                            }
                        } else {
                            break;
                        }
                    }
                    // Push current heading onto stack
                    stack.push((node, level));
                }
                None => {
                    // Non-heading content (Paragraph, CodeBlock, List)
                    if let Some((parent_node, _)) = stack.last_mut() {
                        parent_node.children.push(node);
                    } else {
                        // Orphan content before any heading
                        root_nodes.push(node);
                    }
                }
            }
        }

        // Unwind any remaining nodes on stack
        while let Some((popped_node, _)) = stack.pop() {
            if let Some((parent_node, _)) = stack.last_mut() {
                parent_node.children.push(popped_node);
            } else {
                root_nodes.push(popped_node);
            }
        }

        Ok(DocumentAst {
            roots: root_nodes,
            title: doc_title,
        })
    }

    fn flush_state(
        state: &mut BlockState,
        line_index: &LineIndex,
        flat_nodes: &mut Vec<(AstNode, Option<u8>)>,
        end_offset: usize,
    ) {
        match std::mem::replace(state, BlockState::None) {
            BlockState::None => {}
            BlockState::Heading {
                level,
                title,
                byte_start,
            } => {
                let byte_end = end_offset.max(byte_start);
                let line_start = line_index.line_number(byte_start);
                let line_end = line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                let trimmed = title.trim().to_string();
                flat_nodes.push((
                    AstNode {
                        kind: AstNodeKind::Heading {
                            level,
                            title: trimmed.clone(),
                        },
                        content: trimmed,
                        line_start,
                        line_end,
                        byte_start,
                        byte_end,
                        children: Vec::new(),
                    },
                    Some(level),
                ));
            }
            BlockState::Paragraph { text, byte_start } => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let byte_end = end_offset.max(byte_start);
                    let line_start = line_index.line_number(byte_start);
                    let line_end =
                        line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                    flat_nodes.push((
                        AstNode {
                            kind: AstNodeKind::Paragraph,
                            content: trimmed.to_string(),
                            line_start,
                            line_end,
                            byte_start,
                            byte_end,
                            children: Vec::new(),
                        },
                        None,
                    ));
                }
            }
            BlockState::CodeBlock {
                language,
                code,
                byte_start,
            } => {
                let byte_end = end_offset.max(byte_start);
                let line_start = line_index.line_number(byte_start);
                let line_end = line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                flat_nodes.push((
                    AstNode {
                        kind: AstNodeKind::CodeBlock { language },
                        content: code.trim_end_matches('\n').to_string(),
                        line_start,
                        line_end,
                        byte_start,
                        byte_end,
                        children: Vec::new(),
                    },
                    None,
                ));
            }
            BlockState::List { text, byte_start } => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let byte_end = end_offset.max(byte_start);
                    let line_start = line_index.line_number(byte_start);
                    let line_end =
                        line_index.line_number(byte_end.saturating_sub(1).max(byte_start));
                    flat_nodes.push((
                        AstNode {
                            kind: AstNodeKind::List,
                            content: trimmed.to_string(),
                            line_start,
                            line_end,
                            byte_start,
                            byte_end,
                            children: Vec::new(),
                        },
                        None,
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_lookup() {
        let content = "line 1\nline 2\nline 3";
        let index = LineIndex::new(content);

        assert_eq!(index.line_number(0), 1);
        assert_eq!(index.line_number(3), 1);
        assert_eq!(index.line_number(7), 2);
        assert_eq!(index.line_number(14), 3);
    }

    #[test]
    fn test_parse_empty_and_whitespace() {
        let ast = MarkdownParser::parse("").unwrap();
        assert!(ast.roots.is_empty());
        assert_eq!(ast.title, None);

        let ast = MarkdownParser::parse("   \n\n  \t  \n").unwrap();
        assert!(ast.roots.is_empty());
        assert_eq!(ast.title, None);
    }

    #[test]
    fn test_parse_simple_paragraph_no_heading() {
        let content = "This is a lone paragraph.\nWith multiple lines.";
        let ast = MarkdownParser::parse(content).unwrap();

        assert_eq!(ast.roots.len(), 1);
        assert_eq!(ast.title, None);
        assert_eq!(ast.roots[0].kind, AstNodeKind::Paragraph);
        assert_eq!(
            ast.roots[0].content,
            "This is a lone paragraph. With multiple lines."
        );
        assert_eq!(ast.roots[0].line_start, 1);
        assert_eq!(ast.roots[0].line_end, 2);
    }

    #[test]
    fn test_parse_h1_h2_h3_hierarchy() {
        let markdown = r#"# Architecture Guide

This is an introductory overview.

## Storage Layer

The storage layer uses SQLite and sqlite-vec.

```rust
fn init_db() -> Result<()> {
    Ok(())
}
```

### Vector Search

Vector search uses 384-dimensional cosine similarity.

- Fast KNN queries
- Zero external dependencies

## MCP Server

Serves documentation to LLMs.
"#;

        let ast = MarkdownParser::parse(markdown).expect("Failed to parse markdown");

        assert_eq!(ast.title, Some("Architecture Guide".to_string()));
        assert_eq!(ast.roots.len(), 1);

        // Root H1
        let h1 = &ast.roots[0];
        assert_eq!(
            h1.kind,
            AstNodeKind::Heading {
                level: 1,
                title: "Architecture Guide".to_string()
            }
        );
        assert_eq!(h1.line_start, 1);

        // H1 children: Intro Paragraph, H2 Storage Layer, H2 MCP Server
        assert_eq!(h1.children.len(), 3);

        // Child 0: Intro Paragraph
        assert_eq!(h1.children[0].kind, AstNodeKind::Paragraph);
        assert_eq!(h1.children[0].content, "This is an introductory overview.");

        // Child 1: H2 Storage Layer
        let h2_storage = &h1.children[1];
        assert_eq!(
            h2_storage.kind,
            AstNodeKind::Heading {
                level: 2,
                title: "Storage Layer".to_string()
            }
        );
        // H2 Storage Layer children: Paragraph, CodeBlock, H3 Vector Search
        assert_eq!(h2_storage.children.len(), 3);
        assert_eq!(h2_storage.children[0].kind, AstNodeKind::Paragraph);
        assert_eq!(
            h2_storage.children[1].kind,
            AstNodeKind::CodeBlock {
                language: Some("rust".to_string())
            }
        );
        assert!(h2_storage.children[1]
            .content
            .contains("fn init_db() -> Result<()>"));

        // H3 Vector Search
        let h3_vec = &h2_storage.children[2];
        assert_eq!(
            h3_vec.kind,
            AstNodeKind::Heading {
                level: 3,
                title: "Vector Search".to_string()
            }
        );
        // H3 children: Paragraph, List
        assert_eq!(h3_vec.children.len(), 2);
        assert_eq!(h3_vec.children[0].kind, AstNodeKind::Paragraph);
        assert_eq!(h3_vec.children[1].kind, AstNodeKind::List);
        assert!(h3_vec.children[1].content.contains("- Fast KNN queries"));

        // Child 2 of H1: H2 MCP Server
        let h2_mcp = &h1.children[2];
        assert_eq!(
            h2_mcp.kind,
            AstNodeKind::Heading {
                level: 2,
                title: "MCP Server".to_string()
            }
        );
        assert_eq!(h2_mcp.children.len(), 1);
        assert_eq!(h2_mcp.children[0].kind, AstNodeKind::Paragraph);
        assert_eq!(h2_mcp.children[0].content, "Serves documentation to LLMs.");
    }

    #[test]
    fn test_multiple_top_level_headings() {
        let markdown = r#"# First Section
Content in first section.

# Second Section
Content in second section.
"#;

        let ast = MarkdownParser::parse(markdown).unwrap();
        assert_eq!(ast.title, Some("First Section".to_string()));
        assert_eq!(ast.roots.len(), 2);

        assert_eq!(
            ast.roots[0].kind,
            AstNodeKind::Heading {
                level: 1,
                title: "First Section".to_string()
            }
        );
        assert_eq!(ast.roots[0].children.len(), 1);

        assert_eq!(
            ast.roots[1].kind,
            AstNodeKind::Heading {
                level: 1,
                title: "Second Section".to_string()
            }
        );
        assert_eq!(ast.roots[1].children.len(), 1);
    }

    #[test]
    fn test_heading_with_code_and_formatting() {
        let markdown = "# The `MemexError` Type\n\nExplanation here.";
        let ast = MarkdownParser::parse(markdown).unwrap();

        assert_eq!(ast.title, Some("The `MemexError` Type".to_string()));
        assert_eq!(
            ast.roots[0].kind,
            AstNodeKind::Heading {
                level: 1,
                title: "The `MemexError` Type".to_string()
            }
        );
    }

    #[test]
    fn test_orphan_content_before_headings() {
        let markdown = r#"Introductory preamble before any header.

```bash
cargo build
```

# Main Heading
Actual content.
"#;

        let ast = MarkdownParser::parse(markdown).unwrap();
        assert_eq!(ast.title, Some("Main Heading".to_string()));
        assert_eq!(ast.roots.len(), 3); // Preamble Paragraph, CodeBlock, Heading H1

        assert_eq!(ast.roots[0].kind, AstNodeKind::Paragraph);
        assert_eq!(
            ast.roots[1].kind,
            AstNodeKind::CodeBlock {
                language: Some("bash".to_string())
            }
        );
        assert_eq!(
            ast.roots[2].kind,
            AstNodeKind::Heading {
                level: 1,
                title: "Main Heading".to_string()
            }
        );
    }
}
