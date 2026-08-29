use super::common::{copy_fixtures, open_db};
use memex::cli::init::init_project;
use memex::errors::MemexError;
use tempfile::TempDir;

#[test]
fn test_full_index_creates_valid_database() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("simple", tmp.path());

    // Run init
    let stats = init_project(tmp.path(), false, false).expect("init should succeed");

    assert_eq!(stats.files_added, 3);
    assert_eq!(stats.files_failed, 0);
    assert!(stats.chunks_indexed > 0);
    assert!(stats.edges_created > 0);
    assert_eq!(stats.vectors_indexed, stats.chunks_indexed);

    // Verify .memex/memex.db exists
    assert!(tmp.path().join(".memex/memex.db").exists());

    // Open DB and verify counts
    let db = open_db(tmp.path());
    let doc_count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap();
    let chunk_count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    let edge_count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap();
    let vec_count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();

    assert_eq!(doc_count, 3, "should index 3 documents");
    assert!(chunk_count > 0, "should create chunks");
    assert!(edge_count > 0, "should create edges");
    assert_eq!(vec_count, chunk_count, "every chunk must have an embedding");

    // Verify document records
    let docs = db.reader().get_all_documents().unwrap();
    assert_eq!(docs.len(), 3);
    let paths: Vec<_> = docs.iter().map(|d| d.file_path.as_str()).collect();
    assert!(paths.contains(&"README.md"));
    assert!(paths.contains(&"api.md"));
    assert!(paths.contains(&"guide.md"));
}

#[test]
fn test_full_index_complex_fixtures() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("complex", tmp.path());

    let stats = init_project(tmp.path(), false, false).expect("init should succeed on complex fixtures");

    assert!(stats.files_added >= 20, "complex fixtures should have 20+ files");
    assert_eq!(stats.files_failed, 0);
    assert!(stats.chunks_indexed >= 20);
    assert!(stats.edges_created > 0);
    assert_eq!(stats.vectors_indexed, stats.chunks_indexed);

    let db = open_db(tmp.path());
    let doc_count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(doc_count as usize, stats.files_added);

    // Verify .gitignore was respected
    let ignored_docs: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE file_path LIKE '%ignored%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ignored_docs, 0, "ignored files must not be in the database");
}

#[test]
fn test_index_edge_cases() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("edge_cases", tmp.path());

    let stats = init_project(tmp.path(), false, false).expect("init should succeed on edge cases");

    assert!(stats.files_added >= 5);
    assert_eq!(stats.files_failed, 0);

    let db = open_db(tmp.path());
    // Check large file chunks
    let large_file_chunks: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM chunks JOIN documents ON chunks.doc_id = documents.id WHERE documents.file_path = 'large_file.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(large_file_chunks > 1000, "large_file.md should produce > 1000 chunks");

    // Check no_headings file
    let no_headings_chunks: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM chunks JOIN documents ON chunks.doc_id = documents.id WHERE documents.file_path = 'no_headings.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(no_headings_chunks > 0, "no_headings.md should produce chunks");
}

#[test]
fn test_index_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let stats = init_project(tmp.path(), false, false).expect("init should succeed on empty directory");

    assert_eq!(stats.files_added, 0);
    assert_eq!(stats.chunks_indexed, 0);
    assert_eq!(stats.edges_created, 0);
    assert_eq!(stats.vectors_indexed, 0);

    let db_path = tmp.path().join(".memex/memex.db");
    assert!(db_path.exists());
}

#[test]
fn test_init_twice_fails_already_initialized() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("simple", tmp.path());

    init_project(tmp.path(), false, false).expect("first init should succeed");
    let second = init_project(tmp.path(), false, false);
    match second {
        Err(MemexError::AlreadyInitialized { .. }) => {}
        other => panic!("Expected AlreadyInitialized, got {:?}", other),
    }
}
