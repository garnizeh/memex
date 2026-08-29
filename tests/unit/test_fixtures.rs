use std::path::PathBuf;
use memex::config::MemexConfig;
use memex::discovery::FileDiscovery;
use memex::ingestion::chunker::ContextualChunker;
use memex::ingestion::parser::MarkdownParser;

fn get_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

#[test]
fn test_simple_fixtures_exist_and_parse() {
    let simple_dir = get_fixtures_dir().join("simple");
    assert!(simple_dir.exists(), "simple fixtures directory must exist");

    let config = MemexConfig::default();
    let discovered = FileDiscovery::scan(&simple_dir, &config).unwrap();
    assert_eq!(discovered.len(), 3, "simple/ should contain exactly 3 Markdown files");

    for path in &discovered {
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.is_empty(), "simple file should not be empty: {:?}", path);
        let doc_ast = MarkdownParser::parse(&content).unwrap();
        let chunks = ContextualChunker::chunk_document(&path.to_string_lossy(), &doc_ast);
        assert!(!chunks.is_empty(), "simple file should produce chunks: {:?}", path);
    }
}

#[test]
fn test_complex_fixtures_discovery_and_gitignore() {
    let complex_dir = get_fixtures_dir().join("complex");
    assert!(complex_dir.exists(), "complex fixtures directory must exist");

    let config = MemexConfig::default();
    let discovered = FileDiscovery::scan(&complex_dir, &config).unwrap();

    // Verify .gitignore inside complex/ ignores ignored_dir/ignored.md
    assert!(
        !discovered.iter().any(|p| p.to_string_lossy().contains("ignored_dir")),
        "ignored_dir should be excluded by .gitignore"
    );

    // Verify we have 20+ nested markdown files
    assert!(
        discovered.len() >= 20,
        "complex/ should contain 20+ Markdown files, found {}",
        discovered.len()
    );

    for path in &discovered {
        let content = std::fs::read_to_string(path).unwrap();
        let doc_ast = MarkdownParser::parse(&content).unwrap();
        let chunks = ContextualChunker::chunk_document(&path.to_string_lossy(), &doc_ast);
        assert!(!chunks.is_empty(), "complex file should produce chunks: {:?}", path);
    }
}

#[test]
fn test_edge_cases_fixtures() {
    let edge_dir = get_fixtures_dir().join("edge_cases");
    assert!(edge_dir.exists(), "edge_cases fixtures directory must exist");

    // 1. empty.md
    let empty_path = edge_dir.join("empty.md");
    let empty_content = std::fs::read_to_string(&empty_path).unwrap();
    assert_eq!(empty_content.len(), 0);
    let doc_ast = MarkdownParser::parse(&empty_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&empty_path.to_string_lossy(), &doc_ast);
    assert!(chunks.is_empty(), "empty.md should produce 0 chunks");

    // 2. heading_only.md
    let heading_path = edge_dir.join("heading_only.md");
    let heading_content = std::fs::read_to_string(&heading_path).unwrap();
    let doc_ast = MarkdownParser::parse(&heading_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&heading_path.to_string_lossy(), &doc_ast);
    assert!(!chunks.is_empty(), "heading_only.md should produce chunks");

    // 3. deeply_nested.md (H1-H6)
    let nested_path = edge_dir.join("deeply_nested.md");
    let nested_content = std::fs::read_to_string(&nested_path).unwrap();
    let doc_ast = MarkdownParser::parse(&nested_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&nested_path.to_string_lossy(), &doc_ast);
    assert!(chunks.len() >= 6, "deeply_nested.md should parse H1 through H6 hierarchy");

    // 4. unicode_heavy.md
    let unicode_path = edge_dir.join("unicode_heavy.md");
    let unicode_content = std::fs::read_to_string(&unicode_path).unwrap();
    let doc_ast = MarkdownParser::parse(&unicode_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&unicode_path.to_string_lossy(), &doc_ast);
    assert!(!chunks.is_empty(), "unicode_heavy.md should produce chunks");

    // 5. broken_links.md
    let broken_path = edge_dir.join("broken_links.md");
    let broken_content = std::fs::read_to_string(&broken_path).unwrap();
    let doc_ast = MarkdownParser::parse(&broken_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&broken_path.to_string_lossy(), &doc_ast);
    assert!(!chunks.is_empty(), "broken_links.md should parse without panic");

    // 6. no_headings.md
    let no_headings_path = edge_dir.join("no_headings.md");
    let no_headings_content = std::fs::read_to_string(&no_headings_path).unwrap();
    let doc_ast = MarkdownParser::parse(&no_headings_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&no_headings_path.to_string_lossy(), &doc_ast);
    assert!(!chunks.is_empty(), "no_headings.md should produce fallback chunks");

    // 7. large_file.md (~1MB)
    let large_path = edge_dir.join("large_file.md");
    let metadata = std::fs::metadata(&large_path).unwrap();
    assert!(metadata.len() > 500_000, "large_file.md should be substantial in size (~1MB)");
    let large_content = std::fs::read_to_string(&large_path).unwrap();
    let doc_ast = MarkdownParser::parse(&large_content).unwrap();
    let chunks = ContextualChunker::chunk_document(&large_path.to_string_lossy(), &doc_ast);
    assert!(chunks.len() > 1000, "large_file.md should produce thousands of chunks");
}
