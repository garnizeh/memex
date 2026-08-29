//! Token Reduction Efficiency Benchmark for Memex.
//!
//! Benchmarks token savings and retrieval performance against representative queries.
//! Measures token reduction ratio between Memex semantic search results (top-5 chunks)
//! and naive whole-file ingestion.

mod generate_corpus;

use criterion::{Criterion, criterion_group, criterion_main};
use generate_corpus::{CorpusGenerator, CorpusPreset};
use memex::cli::init::init_project;
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::{format_search_markdown, search_documentation_with_reader};
use memex::storage::db::Database;
use std::hint::black_box;
use std::sync::Arc;
use tempfile::TempDir;
use tiktoken_rs::cl100k_base;

const QUERIES: &[&str] = &[
    "How does OAuth2 authentication work?",
    "What is the database schema?",
    "How to configure logging?",
    "Error handling best practices",
    "API rate limiting strategy",
    "How to run integration tests?",
    "Deployment to production",
    "WebSocket connection lifecycle",
    "User permission model",
    "Caching strategy and invalidation",
];

struct BenchContext {
    _tmp: TempDir,
    db: Database,
    engine: Arc<EmbeddingEngine>,
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

    BenchContext {
        _tmp: tmp,
        db,
        engine,
    }
}

fn bench_token_efficiency(c: &mut Criterion) {
    let ctx = setup_bench_context(CorpusPreset::Medium);
    let reader = ctx.db.reader();
    let tokenizer = cl100k_base().expect("Failed to load tokenizer");

    let mut group = c.benchmark_group("token_efficiency");

    for (idx, &query) in QUERIES.iter().enumerate() {
        let label = format!("query_{:02}", idx + 1);
        group.bench_function(label, |b| {
            b.iter(|| {
                let results = search_documentation_with_reader(
                    &reader,
                    &ctx.engine,
                    black_box(query),
                    black_box(5),
                )
                .expect("search_documentation should succeed");

                let formatted = format_search_markdown(query, &results);
                let token_count = tokenizer.encode_with_special_tokens(&formatted).len();
                black_box(token_count)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_token_efficiency);
criterion_main!(benches);
