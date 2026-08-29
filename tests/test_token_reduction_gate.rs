//! CI Token Reduction Efficiency Gate
//!
//! Verifies that Memex semantic chunk retrieval reduces LLM token consumption
//! by at least 70% compared to naive full-file ingestion across representative queries.
//!
//! Run via: `cargo test --test test_token_reduction_gate -- --ignored`

#[path = "../benches/generate_corpus.rs"]
mod generate_corpus;

use generate_corpus::{CorpusGenerator, CorpusPreset};
use memex::cli::init::init_project;
use memex::ingestion::embedder::{EmbeddingEngine, ModelManager};
use memex::mcp::tools::{format_search_markdown, search_documentation_with_reader, DocSearchResult};
use memex::storage::db::Database;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tiktoken_rs::cl100k_base;
use walkdir::WalkDir;

/// Representative query suite matching Section 13.4.2 / 13.5.1 of architecture specification.
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

/// List of generic English stopwords to filter when extracting keyword stems for naive grep simulation.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he", "in", "is", "it",
    "its", "of", "on", "that", "the", "to", "was", "were", "will", "with", "how", "what", "why",
    "when", "where", "which", "who", "does", "do",
];

/// Context for the efficiency gate test with an indexed medium corpus.
struct GateContext {
    _tmp: TempDir,
    project_dir: std::path::PathBuf,
    db: Database,
    engine: EmbeddingEngine,
}

fn setup_gate_context() -> GateContext {
    let tmp = TempDir::new().expect("Failed to create temporary directory for gate test");
    CorpusGenerator::from_preset(CorpusPreset::Medium)
        .generate(tmp.path())
        .expect("Failed to generate synthetic medium corpus");

    init_project(tmp.path(), false, false).expect("Failed to initialize and index project");

    let db_path = tmp.path().join(".memex").join("memex.db");
    let db = Database::open_readonly(&db_path).expect("Failed to open memex database");

    let assets = ModelManager::ensure_model_assets().expect("Failed to ensure model assets");
    let engine = EmbeddingEngine::new(&assets).expect("Failed to initialize embedding engine");

    let project_dir = tmp.path().to_path_buf();

    GateContext {
        _tmp: tmp,
        project_dir,
        db,
        engine,
    }
}

/// Simulates naive full-file ingestion (e.g. naive keyword grep loading full matching documents).
///
/// If keyword matches are found across documents, returns the sum of tokens of all matching documents.
/// If no specific keyword match is found, falls back to total corpus documentation tokens.
fn count_naive_tokens(
    project_dir: &Path,
    query: &str,
    tokenizer: &tiktoken_rs::CoreBPE,
) -> usize {
    let keywords: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .map(|s| s.to_lowercase())
        .filter(|s| s.len() >= 3 && !STOP_WORDS.contains(&s.as_str()))
        .collect();

    let mut matched_files_text = String::new();
    let mut all_files_text = String::new();
    let mut match_count = 0;

    for entry in WalkDir::new(project_dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        // Ignore internal .memex directory files
        if path.components().any(|c| c.as_os_str() == ".memex") {
            continue;
        }

        if let Ok(content) = fs::read_to_string(path) {
            all_files_text.push_str(&content);
            all_files_text.push('\n');

            let content_lower = content.to_lowercase();
            let matches = keywords.iter().any(|kw| content_lower.contains(kw));

            if matches {
                matched_files_text.push_str(&content);
                matched_files_text.push('\n');
                match_count += 1;
            }
        }
    }

    let text_to_count = if match_count > 0 {
        matched_files_text
    } else {
        all_files_text
    };

    tokenizer.encode_with_special_tokens(&text_to_count).len()
}

/// Counts tokens in the Memex search result response (formatted markdown representation).
fn count_result_tokens(
    query: &str,
    results: &[DocSearchResult],
    tokenizer: &tiktoken_rs::CoreBPE,
) -> usize {
    let formatted_output = format_search_markdown(query, results);
    tokenizer.encode_with_special_tokens(&formatted_output).len()
}

/// CI Token Reduction Efficiency Gate
///
/// Marked `#[ignore]` so standard unit / integration test runs stay fast (~seconds).
/// Executed explicitly in CI via:
/// `cargo test --test test_token_reduction_gate -- --ignored`
///
/// Fails the build if token reduction drops below 70%.
#[test]
#[ignore]
fn gate_token_reduction_minimum_70_percent() {
    let ctx = setup_gate_context();
    let reader = ctx.db.reader();
    let tokenizer = cl100k_base().expect("Failed to load cl100k_base tokenizer");

    let mut total_naive = 0usize;
    let mut total_memex = 0usize;

    println!("\n{:=^85}", " CI TOKEN REDUCTION EFFICIENCY GATE ");
    println!(
        "{:<42} | {:>10} | {:>10} | {:>12}",
        "Query", "Naive Tok", "Memex Tok", "Reduction"
    );
    println!("{:-^85}", "");

    for &query in QUERIES {
        let naive_tokens = count_naive_tokens(&ctx.project_dir, query, &tokenizer);
        let search_results =
            search_documentation_with_reader(&reader, &ctx.engine, query, 5)
                .expect("search_documentation should succeed");

        let memex_tokens = count_result_tokens(query, &search_results, &tokenizer);

        let query_reduction = if naive_tokens > 0 {
            (1.0 - (memex_tokens as f64 / naive_tokens as f64)) * 100.0
        } else {
            0.0
        };

        let truncated_query = if query.len() > 40 {
            format!("{}...", &query[..37])
        } else {
            query.to_string()
        };

        println!(
            "{:<42} | {:>10} | {:>10} | {:>11.1}%",
            truncated_query, naive_tokens, memex_tokens, query_reduction
        );

        total_naive += naive_tokens;
        total_memex += memex_tokens;
    }

    let reduction_pct = if total_naive > 0 {
        (1.0 - (total_memex as f64 / total_naive as f64)) * 100.0
    } else {
        0.0
    };

    println!("{:-^85}", "");
    println!(
        "{:<42} | {:>10} | {:>10} | {:>11.1}%",
        "OVERALL TOTAL", total_naive, total_memex, reduction_pct
    );
    println!("{:=^85}\n", "");

    assert!(
        reduction_pct >= 70.0,
        "Token reduction is {:.1}%, expected >= 70.0%. \
         Naive: {} tokens, Memex: {} tokens. \
         This gate ensures retrieval quality and efficiency haven't regressed.",
        reduction_pct,
        total_naive,
        total_memex
    );

    eprintln!(
        "✓ Efficiency gate passed: {:.1}% token reduction ({} → {} tokens)",
        reduction_pct, total_naive, total_memex
    );
}
