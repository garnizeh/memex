use memex::cli::init::init_project;
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::{
    format_search_markdown, format_subgraph_markdown, search_documentation_with_reader,
    traverse_graph_with_reader,
};
use memex::storage::db::Database;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;

#[test]
fn test_e2e_real_world_docs_validation() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path();

    // Copy memex's own docs directory and README.md
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docs_src = repo_root.join("docs");
    let docs_dest = project_dir.join("docs");
    fs::create_dir_all(&docs_dest).unwrap();

    for entry in fs::read_dir(&docs_src).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            fs::copy(entry.path(), docs_dest.join(entry.file_name())).unwrap();
        }
    }

    let readme_src = repo_root.join("README.md");
    if readme_src.exists() {
        fs::copy(&readme_src, project_dir.join("README.md")).unwrap();
    }

    // Step 1: Run memex init
    let init_start = Instant::now();
    let stats = init_project(project_dir, false, false).expect("init_project should succeed");
    let init_duration = init_start.elapsed();

    println!("E2E Init completed in {:?}", init_duration);
    println!(
        "Indexed files: {}, Chunks: {}, Vectors: {}, Edges: {}",
        stats.files_added, stats.chunks_indexed, stats.vectors_indexed, stats.edges_created
    );

    assert!(
        stats.files_added >= 2,
        "Expected at least 2 markdown files indexed"
    );
    assert!(
        stats.chunks_indexed > 50,
        "Expected >50 chunks from architecture & phases docs"
    );
    assert_eq!(stats.vectors_indexed, stats.chunks_indexed);
    assert_eq!(stats.files_failed, 0);

    // Step 2: Open DB in read-only mode & initialize embedding engine (as MCP serve does)
    let db_path = project_dir.join(".memex").join("memex.db");
    let db = Database::open_readonly(&db_path).expect("open_readonly should succeed");
    let reader = db.reader();

    let assets = match ModelManager::ensure_model_assets() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping live model query check: model assets not available: {e}");
            return;
        }
    };
    let engine = EmbeddingEngine::new(&assets).expect("EmbeddingEngine init should succeed");

    // Step 3: Run realistic queries and measure latency & verify relevance
    let queries = [
        (
            "SQLite vector indexing architecture and pragmas",
            "architecture.md",
            5,
        ),
        (
            "Phase 10 release verification and task summary",
            "phases.md",
            5,
        ),
        (
            "Contextual chunking and heading path preservation",
            "architecture.md",
            3,
        ),
        (
            "MCP stdio JSON-RPC transport protocol",
            "architecture.md",
            3,
        ),
    ];

    for (query, expected_file, limit) in queries {
        let query_start = Instant::now();
        let results = search_documentation_with_reader(&reader, &engine, query, limit)
            .expect("search_documentation should succeed");
        let query_duration = query_start.elapsed();

        println!(
            "Query '{}' returned {} results in {:?}",
            query,
            results.len(),
            query_duration
        );

        assert!(
            !results.is_empty(),
            "Query '{}' should return results",
            query
        );
        assert!(
            results.iter().any(|r| r.file_path.contains(expected_file)),
            "Expected results for '{}' to contain '{}'",
            query,
            expected_file
        );

        // Verify formatted output
        let formatted = format_search_markdown(query, &results);
        assert!(!formatted.is_empty());
        assert!(formatted.contains(expected_file));

        // Subgraph traversal on top result
        let top_chunk_id = &results[0].chunk_id;
        let traverse_start = Instant::now();
        let subgraph = traverse_graph_with_reader(&reader, top_chunk_id, 2)
            .expect("traverse_graph should succeed");
        let traverse_duration = traverse_start.elapsed();

        println!(
            "Traverse for chunk '{}' returned {} nodes in {:?}",
            top_chunk_id,
            subgraph.nodes.len(),
            traverse_duration
        );

        assert!(subgraph.root.is_some());
        assert!(!subgraph.nodes.is_empty());

        let subgraph_md = format_subgraph_markdown(&subgraph);
        assert!(!subgraph_md.is_empty());
    }
}
