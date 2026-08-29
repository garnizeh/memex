//! Ingestion Performance Benchmark for Memex.
//!
//! Measures throughput of the full indexing pipeline:
//! 1. Full indexing throughput on medium corpus (~500 KB, ~50 files) [Target: < 5s]
//! 2. Incremental re-index throughput on modified files [Target: < 1s]
//! 3. Embedding throughput for batches of chunks [Target: > 100 chunks/sec]
//! 4. Database write throughput for batch chunk / edge / vector insertion.

mod generate_corpus;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use generate_corpus::{generate_medium_corpus, CorpusGenerator, CorpusPreset};
use memex::cli::index::run_index;
use memex::cli::init::init_project;
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager, EMBEDDING_DIM};
use memex::models::Chunk;
use memex::storage::db::Database;
use memex::storage::schema::initialize_schema;
use memex::storage::writer::StorageWriter;
use std::fs;
use std::hint::black_box;
use std::sync::Arc;
use tempfile::TempDir;

/// Helper to set up an initialized and indexed project using the synthetic corpus.
fn setup_synthetic_project(preset: CorpusPreset) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create tempdir");
    CorpusGenerator::from_preset(preset)
        .generate(tmp.path())
        .expect("Failed to generate corpus");
    init_project(tmp.path(), false, false).expect("Failed to init project");
    tmp
}

/// Benchmark 1: Full indexing throughput on a medium corpus (~500 KB, 50 files)
fn bench_full_index_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingestion_full_index");
    group.sample_size(10); // End-to-end indexing includes ONNX inference, keep sample size reasonable

    group.bench_function("medium_50_files", |b| {
        b.iter_with_setup(
            || {
                let tmp = TempDir::new().expect("Failed to create tempdir");
                generate_medium_corpus(tmp.path()).expect("Failed to generate medium corpus");
                tmp
            },
            |tmp| {
                let stats = init_project(tmp.path(), false, false).expect("init should succeed");
                black_box(stats);
            },
        );
    });

    group.finish();
}

/// Benchmark 2: Incremental re-indexing latency when a subset of files are modified
fn bench_incremental_index_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingestion_incremental_index");
    group.sample_size(10);

    group.bench_function("modify_3_files", |b| {
        b.iter_with_setup(
            || {
                let tmp = setup_synthetic_project(CorpusPreset::Medium);

                // Modify 3 markdown files
                let file1 = tmp.path().join("README.md");
                let file2 = tmp.path().join("ARCHITECTURE.md");
                let file3 = tmp.path().join("getting_started").join("doc_0001.md");

                if file1.exists() {
                    let mut content = fs::read_to_string(&file1).unwrap();
                    content.push_str("\n\n## Incremental Update 1\nAdded benchmark line.");
                    fs::write(&file1, content).unwrap();
                }

                if file2.exists() {
                    let mut content = fs::read_to_string(&file2).unwrap();
                    content.push_str("\n\n## Incremental Update 2\nAdded benchmark line.");
                    fs::write(&file2, content).unwrap();
                }

                if file3.exists() {
                    let mut content = fs::read_to_string(&file3).unwrap();
                    content.push_str("\n\n## Incremental Update 3\nAdded benchmark line.");
                    fs::write(&file3, content).unwrap();
                }

                tmp
            },
            |tmp| {
                let stats = run_index(tmp.path(), true, false).expect("run_index should succeed");
                black_box(stats);
            },
        );
    });

    group.bench_function("no_changes_noop", |b| {
        let tmp = setup_synthetic_project(CorpusPreset::Medium);
        b.iter(|| {
            let stats = run_index(tmp.path(), true, false).expect("run_index should succeed");
            black_box(stats);
        });
    });

    group.finish();
}

/// Benchmark 3: Embedding throughput (chunks / sec) using the ONNX engine
fn bench_embedding_throughput(c: &mut Criterion) {
    let assets = ModelManager::ensure_model_assets().expect("Failed to ensure model assets");
    let engine = Arc::new(EmbeddingEngine::new(&assets).expect("Failed to initialize embedder"));

    let texts_64: Vec<String> = (0..64)
        .map(|i| {
            format!(
                "This is sample document chunk {} containing technical details about SQLite vector \
                 indexing, ONNX embeddings, and MCP stdio JSON-RPC transport layers.",
                i
            )
        })
        .collect();

    let mut group = c.benchmark_group("embedding_throughput");
    group.throughput(Throughput::Elements(64));

    group.bench_function("embed_batch_64", |b| {
        b.iter(|| {
            let embeddings = engine
                .embed_batch(black_box(&texts_64))
                .expect("Embedding batch should succeed");
            black_box(embeddings);
        });
    });

    group.finish();
}

/// Benchmark 4: Database write throughput for batch chunk & vector insertion
fn bench_db_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("db_write_throughput");
    group.sample_size(10);

    let dummy_embedding: [f32; EMBEDDING_DIM] = [0.05; EMBEDDING_DIM];

    for chunk_count in [100, 500] {
        group.throughput(Throughput::Elements(chunk_count as u64));
        group.bench_with_input(
            BenchmarkId::new("insert_batch_chunks_and_vectors", chunk_count),
            &chunk_count,
            |b, &count| {
                b.iter_with_setup(
                    || {
                        let tmp = TempDir::new().expect("Failed to create tempdir");
                        let db_path = tmp.path().join("bench.db");
                        let mut db = Database::open(&db_path).expect("Failed to open DB");
                        initialize_schema(db.conn_mut()).expect("Failed to init schema");

                        // Create dummy chunks
                        let chunks: Vec<Chunk> = (0..count)
                            .map(|i| Chunk {
                                id: format!("chunk_{:06}", i),
                                doc_id: "doc_test".to_string(),
                                chunk_type: memex::models::ChunkType::Paragraph,
                                heading_path: vec!["Root".to_string(), "Section".to_string()],
                                content: format!("Benchmark chunk content {}", i),
                                contextual_content: format!(
                                    "Root > Section\nBenchmark chunk content {}",
                                    i
                                ),
                                line_start: (i + 1) as u32,
                                line_end: (i + 5) as u32,
                                parent_chunk_id: None,
                            })
                            .collect();

                        (tmp, db, chunks)
                    },
                    |(_tmp, mut db, chunks)| {
                        let tx = db.conn_mut().transaction().unwrap();
                        StorageWriter::insert_chunks_batch_tx(&tx, &chunks).unwrap();

                        let vectors: Vec<(&str, &[f32; EMBEDDING_DIM])> = chunks
                            .iter()
                            .map(|c| (c.id.as_str(), &dummy_embedding))
                            .collect();
                        StorageWriter::insert_vectors_batch_tx(&tx, &vectors).unwrap();

                        tx.commit().unwrap();
                        black_box(())
                    },
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_full_index_throughput,
    bench_incremental_index_throughput,
    bench_embedding_throughput,
    bench_db_write_throughput,
);
criterion_main!(benches);
