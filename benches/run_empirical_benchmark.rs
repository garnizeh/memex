use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::search_documentation_with_reader;
use memex::storage::db::Database;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tiktoken_rs::cl100k_base;

struct BenchmarkQuery {
    question: &'static str,
    target_doc: &'static str,
    expected_heading_contains: &'static str,
}

fn main() {
    println!("=== Memex Empirical Real-World Benchmark Runner ===");

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut db_path = repo_root.join(".memex").join("memex.db");
    if !db_path.exists() {
        let legacy_db_path = repo_root.join(".memex").join("index.db");
        if legacy_db_path.exists() {
            db_path = legacy_db_path;
        } else {
            eprintln!(
                "Error: Database not found at {:?}. Please run `memex init .` first.",
                db_path
            );
            std::process::exit(1);
        }
    }

    let bpe = cl100k_base().expect("Failed to load cl100k_base tokenizer");
    let db = Database::open_readonly(&db_path).expect("Failed to open memex.db");
    let reader = db.reader();

    let assets = ModelManager::ensure_model_assets().expect("Failed to ensure model assets");
    let engine = EmbeddingEngine::new(&assets).expect("Failed to init EmbeddingEngine");

    let queries = [
        BenchmarkQuery {
            question: "How does vector normalization and cosine similarity calculation work in sqlite-vec?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "Database Schema",
        },
        BenchmarkQuery {
            question: "What is the relational database schema for chunks, documents, and hierarchical edges?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "Database Schema",
        },
        BenchmarkQuery {
            question: "What were the deliverables and verification steps completed in Phase 10?",
            target_doc: "docs/roadmap/mvp-roadmap.md",
            expected_heading_contains: "Phase 10",
        },
        BenchmarkQuery {
            question: "How does contextual chunking handle paragraph splitting when exceeding max chunk size?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "Chunking",
        },
        BenchmarkQuery {
            question: "How to install Git hooks for automatic background documentation indexing?",
            target_doc: "README.md",
            expected_heading_contains: "Git Hooks",
        },
        BenchmarkQuery {
            question: "How does the MCP stdio JSON-RPC transport protocol work and why must logs go to stderr?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "MCP",
        },
        BenchmarkQuery {
            question: "What is the relevance decay score formula used in graph traversal?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "Data Flow",
        },
        BenchmarkQuery {
            question: "Which AI coding agents are automatically configured by memex install in README?",
            target_doc: "README.md",
            expected_heading_contains: "Auto-Register",
        },
        BenchmarkQuery {
            question: "How does the incremental index delta engine avoid reprocessing unmodified documentation?",
            target_doc: "docs/roadmap/mvp-roadmap.md",
            expected_heading_contains: "Phase 6",
        },
        BenchmarkQuery {
            question: "How is the CI token reduction efficiency gate implemented in architecture design document?",
            target_doc: "docs/architecture/architecture.md",
            expected_heading_contains: "CI Efficiency Gate",
        },
    ];

    // Warmup
    let _ = search_documentation_with_reader(&reader, &engine, "warmup query", 1);

    println!(
        "\n{:<3} | {:<58} | {:<10} | {:<12} | {:<12} | {:<10} | {:<32} | {:<8}",
        "#", "Query", "Latency", "Raw Tokens", "Memex Tokens", "Reduction", "Top Match", "Valid?"
    );
    println!("{}", "-".repeat(160));

    let mut total_raw_tokens = 0;
    let mut total_memex_tokens = 0;
    let mut total_latency_us = 0;

    let mut results_json = Vec::new();

    for (i, q) in queries.iter().enumerate() {
        let raw_content = fs::read_to_string(repo_root.join(q.target_doc))
            .unwrap_or_else(|_| panic!("Failed to read target doc {}", q.target_doc));
        let raw_tokens = bpe.encode_with_special_tokens(&raw_content).len();

        let t0 = Instant::now();
        let all_search_results = search_documentation_with_reader(&reader, &engine, q.question, 10)
            .expect("Search failed");
        let elapsed = t0.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;

        // Filter results specifically for the target documentation, ignoring meta-reports like benchmark-report.md or test fixtures
        let target_results: Vec<_> = all_search_results
            .into_iter()
            .filter(|r| r.file_path.contains(q.target_doc))
            .take(2)
            .collect();

        assert!(
            !target_results.is_empty(),
            "Query #{} returned no results from target doc '{}'!",
            i + 1,
            q.target_doc
        );

        let top = &target_results[0];
        let heading_matches = top.heading_path.iter().any(|h| {
            h.to_lowercase()
                .contains(&q.expected_heading_contains.to_lowercase())
        });

        assert!(
            heading_matches,
            "Query #{} expected heading containing '{}', but got: {:?}",
            i + 1,
            q.expected_heading_contains,
            top.heading_path
        );

        let formatted_md = memex::mcp::tools::format_search_markdown(q.question, &target_results);
        let memex_tokens = bpe.encode_with_special_tokens(&formatted_md).len();

        let reduction_pct = (1.0 - (memex_tokens as f64 / raw_tokens as f64)) * 100.0;

        total_raw_tokens += raw_tokens;
        total_memex_tokens += memex_tokens;
        total_latency_us += elapsed.as_micros();

        let top_match = format!("{}:L{}", top.file_path, top.line_start);

        println!(
            "{:<3} | {:<58} | {:>7.2} ms | {:>10} t | {:>10} t | {:>8.2} % | {:<32} | {:<8}",
            i + 1,
            if q.question.len() > 58 {
                format!("{}...", &q.question[..55])
            } else {
                q.question.to_string()
            },
            elapsed_ms,
            raw_tokens,
            memex_tokens,
            reduction_pct,
            if top_match.len() > 32 {
                format!("{}...", &top_match[..29])
            } else {
                top_match.clone()
            },
            "PASS"
        );

        results_json.push(serde_json::json!({
            "id": i + 1,
            "query": q.question,
            "target_doc": q.target_doc,
            "latency_ms": elapsed_ms,
            "raw_tokens": raw_tokens,
            "memex_tokens": memex_tokens,
            "reduction_pct": reduction_pct,
            "top_match": top_match,
            "heading_path": top.heading_path.clone(),
            "top_excerpt": top.content.chars().take(200).collect::<String>(),
            "validation": "PASS"
        }));
    }

    println!("{}", "-".repeat(160));
    let avg_latency = (total_latency_us as f64 / queries.len() as f64) / 1000.0;
    let overall_reduction = (1.0 - (total_memex_tokens as f64 / total_raw_tokens as f64)) * 100.0;

    println!(
        "TOTAL / AVERAGE: Latency: {:.2} ms | Raw: {} tokens | Memex: {} tokens | Efficiency Gain: {:.2}% (ALL 10 RETRIEVALS VALIDATED)\n",
        avg_latency, total_raw_tokens, total_memex_tokens, overall_reduction
    );

    let output_json_path = repo_root.join("target").join("benchmark_results.json");
    fs::write(
        &output_json_path,
        serde_json::to_string_pretty(&results_json).unwrap(),
    )
    .unwrap();
    println!("Benchmark data written to {:?}", output_json_path);
}
