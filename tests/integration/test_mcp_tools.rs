use super::common::{open_db, setup_indexed_project};
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::{
    handle_search_documentation_json, handle_traverse_graph_json, search_documentation_with_reader,
    traverse_graph_with_reader,
};
use memex::mcp::types::ToolContent;
use memex::models::ChunkType;
use serde_json::json;

#[test]
fn test_search_documentation_returns_relevant_results() {
    let tmp = setup_indexed_project("complex");
    let db = open_db(tmp.path());
    let reader = db.reader();

    let assets = ModelManager::ensure_model_assets().unwrap();
    let engine = EmbeddingEngine::new(&assets).unwrap();

    // Query for OAuth2 authentication
    let results = search_documentation_with_reader(
        &reader,
        &engine,
        "OAuth2 authentication flow and token management",
        5,
    )
    .unwrap();

    assert!(!results.is_empty(), "should return results");
    assert!(
        results[0].file_path.contains("auth.md"),
        "top result should be from auth.md, got: {}",
        results[0].file_path
    );
    assert!(
        results[0]
            .heading_path
            .iter()
            .any(|h| h.to_lowercase().contains("oauth") || h.to_lowercase().contains("auth")),
        "top result should be in the authentication section, got heading_path: {:?}",
        results[0].heading_path
    );
    assert!(
        results[0].similarity_score > 0.4,
        "top result should have high similarity score, got: {}",
        results[0].similarity_score
    );
}

#[test]
fn test_traverse_graph_returns_ancestors_and_children() {
    let tmp = setup_indexed_project("complex");
    let db = open_db(tmp.path());
    let reader = db.reader();

    let assets = ModelManager::ensure_model_assets().unwrap();
    let engine = EmbeddingEngine::new(&assets).unwrap();

    let search_results =
        search_documentation_with_reader(&reader, &engine, "OAuth2 Flow", 1).unwrap();
    assert!(!search_results.is_empty());
    let chunk_id = &search_results[0].chunk_id;

    let subgraph = traverse_graph_with_reader(&reader, chunk_id, 2).unwrap();

    // Should have the chunk itself, at least one ancestor or linked node
    assert!(subgraph.root.is_some(), "root chunk must be found");
    assert!(!subgraph.nodes.is_empty(), "subgraph should contain nodes");
    assert!(
        subgraph
            .nodes
            .iter()
            .any(|n| matches!(n.chunk_type, ChunkType::Heading { .. })),
        "subgraph should contain heading ancestors or parent nodes"
    );
}

#[test]
fn test_search_empty_query_returns_graceful_response() {
    let tmp = setup_indexed_project("simple");
    let db = open_db(tmp.path());
    let reader = db.reader();

    let assets = ModelManager::ensure_model_assets().unwrap();
    let engine = EmbeddingEngine::new(&assets).unwrap();

    let results = search_documentation_with_reader(&reader, &engine, "", 5).unwrap();
    assert!(
        results.is_empty(),
        "empty query should return empty results"
    );

    let results_spaces = search_documentation_with_reader(&reader, &engine, "   ", 5).unwrap();
    assert!(
        results_spaces.is_empty(),
        "whitespace query should return empty results"
    );
}

#[test]
fn test_traverse_nonexistent_chunk_returns_empty_subgraph() {
    let tmp = setup_indexed_project("simple");
    let db = open_db(tmp.path());
    let reader = db.reader();

    let subgraph = traverse_graph_with_reader(&reader, "nonexistent_id_abc123", 2).unwrap();
    assert!(
        subgraph.root.is_none(),
        "root should be None for missing chunk"
    );
    assert!(subgraph.nodes.is_empty(), "nodes should be empty");
    assert!(subgraph.edges.is_empty(), "edges should be empty");
}

#[test]
fn test_mcp_json_tool_handlers_roundtrip() {
    let tmp = setup_indexed_project("complex");
    let db = open_db(tmp.path());
    let reader = db.reader();

    let assets = ModelManager::ensure_model_assets().unwrap();
    let engine = EmbeddingEngine::new(&assets).unwrap();

    // 1. search_documentation via JSON
    let search_res = handle_search_documentation_json(
        &reader,
        &engine,
        Some(json!({
            "query": "curl -X POST /api/auth/token",
            "limit": 3
        })),
    );
    assert_ne!(search_res.is_error, Some(true));
    let text = match &search_res.content[0] {
        ToolContent::Text { text } => text,
    };
    assert!(
        text.contains("auth.md") || text.contains("OAuth2"),
        "search response should contain auth doc details: {}",
        text
    );

    // 2. traverse_graph via JSON with invalid args
    let bad_traverse = handle_traverse_graph_json(&reader, Some(json!({})));
    assert_eq!(
        bad_traverse.is_error,
        Some(true),
        "missing chunk_id parameter should produce error response"
    );

    // 3. search_documentation with missing query
    let bad_search = handle_search_documentation_json(&reader, &engine, None);
    assert_eq!(bad_search.is_error, Some(true));
}
