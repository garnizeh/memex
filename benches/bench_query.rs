//! MCP Query and Graph Traversal Latency Benchmarks for Memex.
//!
//! Measures end-to-end latency of MCP search & traversal handlers:
//! 1. Single query embedding latency [Target: < 20ms]
//! 2. search_documentation latency (limit = 5) on indexed corpus [Target: < 50ms]
//! 3. traverse_graph latency (depth = 2) on indexed corpus [Target: < 10ms]
//! 4. traverse_graph latency (depth = 5) on indexed corpus [Target: < 30ms]

mod generate_corpus;

use criterion::{Criterion, criterion_group, criterion_main};
use generate_corpus::{CorpusGenerator, CorpusPreset};
use memex::cli::init::init_project;
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::{search_documentation_with_reader, traverse_graph_with_reader};
use memex::storage::db::Database;
use std::hint::black_box;
use std::sync::Arc;
use tempfile::TempDir;

struct BenchContext {
    _tmp: TempDir,
    db: Database,
    engine: Arc<EmbeddingEngine>,
    sample_chunk_id: String,
}

fn setup_bench_context(preset: CorpusPreset) -> BenchContext {
    let tmp = TempDir::new().expect("Failed to create tempdir");
    CorpusGenerator::from_preset(preset)
        .generate(tmp.path())
        .expect("Failed to generate corpus");
    init_project(tmp.path(), false, false).expect("Failed to init project");

    let db_path = tmp.path().join(".memex").join("memex.db");
    let db = Database::open_readonly(&db_path).expect("Failed to open db readonly");

    let assets = ModelManager::ensure_model_assets().expect("Failed to ensure model assets");
    let engine = Arc::new(EmbeddingEngine::new(&assets).expect("Failed to init embedding engine"));

    // Find a valid sample chunk_id from database
    let sample_chunk_id: String = db
        .conn()
        .query_row(
            "SELECT id FROM chunks WHERE parent_chunk_id IS NOT NULL LIMIT 1",
            [],
            |row| row.get(0),
        )
        .or_else(|_| {
            db.conn()
                .query_row("SELECT id FROM chunks LIMIT 1", [], |row| row.get(0))
        })
        .expect("Database must contain at least one chunk");

    BenchContext {
        _tmp: tmp,
        db,
        engine,
        sample_chunk_id,
    }
}

/// Benchmark 1: Query embedding generation latency (< 20ms target)
fn bench_query_embedding_latency(c: &mut Criterion) {
    let assets = ModelManager::ensure_model_assets().expect("Failed to ensure model assets");
    let engine = EmbeddingEngine::new(&assets).expect("Failed to init embedding engine");

    let mut group = c.benchmark_group("query_embedding_latency");

    group.bench_function("single_query_embed", |b| {
        b.iter(|| {
            let embedding = engine
                .embed(black_box("How does OAuth2 token authentication work?"))
                .expect("embed should succeed");
            black_box(embedding)
        });
    });

    group.finish();
}

/// Benchmark 2: MCP search_documentation tool handler latency (< 50ms target)
fn bench_search_documentation_latency(c: &mut Criterion) {
    let ctx = setup_bench_context(CorpusPreset::Medium);
    let reader = ctx.db.reader();

    let mut group = c.benchmark_group("search_documentation_latency");

    const QUERIES: &[&str] = &[
        "how does authentication and token management work",
        "database schema and sqlite vector storage",
        "concurrency primitives and graph traversal",
    ];

    for (idx, &query) in QUERIES.iter().enumerate() {
        group.bench_function(format!("query_{}_{}", idx + 1, &query[..15]), |b| {
            b.iter(|| {
                let results = search_documentation_with_reader(
                    &reader,
                    &ctx.engine,
                    black_box(query),
                    black_box(5),
                )
                .expect("search_documentation should succeed");
                black_box(results)
            });
        });
    }

    group.finish();
}

/// Benchmark 3: MCP traverse_graph tool handler latency (< 10ms target for depth=2, < 30ms for depth=5)
fn bench_traverse_graph_latency(c: &mut Criterion) {
    let ctx = setup_bench_context(CorpusPreset::Medium);
    let reader = ctx.db.reader();

    let mut group = c.benchmark_group("traverse_graph_latency");

    for depth in [1, 2, 5] {
        group.bench_function(format!("depth_{}", depth), |b| {
            b.iter(|| {
                let subgraph = traverse_graph_with_reader(
                    &reader,
                    black_box(&ctx.sample_chunk_id),
                    black_box(depth),
                )
                .expect("traverse_graph should succeed");
                black_box(subgraph)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_query_embedding_latency,
    bench_search_documentation_latency,
    bench_traverse_graph_latency,
);
criterion_main!(benches);
