#[path = "../../benches/generate_corpus.rs"]
pub mod generate_corpus;

use generate_corpus::{
    generate_corpus, generate_large_corpus, generate_medium_corpus, CorpusConfig,
};
use memex::config::MemexConfig;
use memex::discovery::FileDiscovery;
use memex::ingestion::chunker::ContextualChunker;
use memex::ingestion::parser::MarkdownParser;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_generate_medium_corpus_structure_and_stats() {
    let temp = tempdir().unwrap();
    let stats = generate_medium_corpus(temp.path()).expect("Failed to generate medium corpus");

    assert_eq!(stats.total_files, 50);
    assert_eq!(stats.file_paths.len(), 50);
    assert!(
        stats.total_bytes >= 400 * 1024 && stats.total_bytes <= 700 * 1024,
        "Expected ~500 KB, got {} bytes",
        stats.total_bytes
    );
    assert!(stats.total_headings >= 150);
    assert!(stats.total_code_blocks >= 50);
    assert!(stats.total_cross_links >= 50);
    assert!(stats.total_directories >= 5);

    // Verify all generated files exist on disk
    for rel_path in &stats.file_paths {
        let full_path = temp.path().join(rel_path);
        assert!(
            full_path.exists(),
            "Generated file must exist: {:?}",
            full_path
        );
        let content = fs::read_to_string(&full_path).unwrap();
        assert!(!content.is_empty());
    }
}

#[test]
fn test_generate_large_corpus_structure_and_stats() {
    let temp = tempdir().unwrap();
    let stats = generate_large_corpus(temp.path()).expect("Failed to generate large corpus");

    assert_eq!(stats.total_files, 200);
    assert_eq!(stats.file_paths.len(), 200);
    assert!(
        stats.total_bytes >= 4 * 1024 * 1024 && stats.total_bytes <= 7 * 1024 * 1024,
        "Expected ~5 MB, got {} bytes",
        stats.total_bytes
    );
    assert!(stats.total_headings >= 600);
    assert!(stats.total_code_blocks >= 200);
    assert!(stats.total_cross_links >= 200);
    assert!(stats.total_directories >= 8);
}

#[test]
fn test_generate_corpus_custom_config() {
    let temp = tempdir().unwrap();
    let config = CorpusConfig {
        file_count: 10,
        target_size_bytes: 50 * 1024,
        include_cross_links: true,
        include_code_blocks: true,
        include_tables_and_lists: true,
    };
    let stats = generate_corpus(temp.path(), &config).unwrap();
    assert_eq!(stats.total_files, 10);
    assert!(stats.total_bytes >= 35 * 1024 && stats.total_bytes <= 75 * 1024);
}

#[test]
fn test_generated_medium_corpus_discovery_and_chunking() {
    let temp = tempdir().unwrap();
    let stats = generate_medium_corpus(temp.path()).unwrap();

    let config = MemexConfig::default();
    let discovered = FileDiscovery::scan(temp.path(), &config).unwrap();
    assert_eq!(discovered.len(), stats.total_files);

    let mut total_chunks = 0;
    for file_path in &discovered {
        let content = fs::read_to_string(file_path).unwrap();
        let doc_ast = MarkdownParser::parse(&content).expect("MarkdownParser must succeed");
        let chunks = ContextualChunker::chunk_document(&file_path.to_string_lossy(), &doc_ast);
        assert!(!chunks.is_empty());
        total_chunks += chunks.len();
    }

    assert!(
        total_chunks >= 300,
        "Expected realistic chunk count for medium corpus, got {}",
        total_chunks
    );
}
