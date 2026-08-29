//! Synthetic Documentation Corpus Generator for Memex Benchmarks and Tests.
//!
//! Generates realistic documentation directory trees of various sizes (small, medium, large)
//! containing structured Markdown files with headings (H1-H4), code blocks (Rust, Python, JSON, Bash),
//! paragraphs, lists, tables, callouts, and inter-document cross-links.

#![allow(dead_code, unused_imports)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Preset configurations for standardized benchmark corpora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusPreset {
    /// Medium documentation tree (~500 KB, ~50 files, ~800 chunks).
    Medium,
    /// Large documentation tree (~5 MB, ~200 files, ~5,000 chunks).
    Large,
}

/// Configuration options for synthetic corpus generation.
#[derive(Debug, Clone)]
pub struct CorpusConfig {
    /// Total number of markdown files to generate.
    pub file_count: usize,
    /// Approximate target total size in bytes.
    pub target_size_bytes: usize,
    /// Whether to generate cross-document markdown links.
    pub include_cross_links: bool,
    /// Whether to include syntax-highlighted code blocks.
    pub include_code_blocks: bool,
    /// Whether to include tables and lists.
    pub include_tables_and_lists: bool,
}

impl Default for CorpusConfig {
    fn default() -> Self {
        Self::from_preset(CorpusPreset::Medium)
    }
}

impl CorpusConfig {
    /// Create configuration matching a standardized preset.
    pub fn from_preset(preset: CorpusPreset) -> Self {
        match preset {
            CorpusPreset::Medium => Self {
                file_count: 50,
                target_size_bytes: 500 * 1024, // ~500 KB
                include_cross_links: true,
                include_code_blocks: true,
                include_tables_and_lists: true,
            },
            CorpusPreset::Large => Self {
                file_count: 200,
                target_size_bytes: 5 * 1024 * 1024, // ~5 MB
                include_cross_links: true,
                include_code_blocks: true,
                include_tables_and_lists: true,
            },
        }
    }
}

/// Statistical summary of a generated corpus.
#[derive(Debug, Clone, Default)]
pub struct CorpusStats {
    /// Total number of markdown files created.
    pub total_files: usize,
    /// Total number of directories created.
    pub total_directories: usize,
    /// Total bytes written to disk.
    pub total_bytes: usize,
    /// Total number of headings generated (H1-H6).
    pub total_headings: usize,
    /// Total number of code snippets generated.
    pub total_code_blocks: usize,
    /// Total number of cross-document links generated.
    pub total_cross_links: usize,
    /// Paths to all generated files relative to corpus root.
    pub file_paths: Vec<PathBuf>,
}

/// Synthetic documentation generator.
pub struct CorpusGenerator {
    config: CorpusConfig,
}

impl CorpusGenerator {
    /// Create a generator with custom configuration.
    pub fn new(config: CorpusConfig) -> Self {
        Self { config }
    }

    /// Create a generator with a standard preset.
    pub fn from_preset(preset: CorpusPreset) -> Self {
        Self::new(CorpusConfig::from_preset(preset))
    }

    /// Generate the synthetic corpus in the target directory.
    pub fn generate(&self, root_dir: &Path) -> io::Result<CorpusStats> {
        if !root_dir.exists() {
            fs::create_dir_all(root_dir)?;
        }

        let plan = self.build_file_plan();
        let mut stats = CorpusStats::default();
        let target_bytes_per_file = self.config.target_size_bytes / self.config.file_count.max(1);

        let mut dirs_created = std::collections::HashSet::new();

        for (index, relative_path) in plan.iter().enumerate() {
            let full_path = root_dir.join(relative_path);
            if let Some(parent) = full_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
                dirs_created.insert(parent.to_path_buf());
            }

            let file_content = self.generate_file_content(
                index,
                relative_path,
                &plan,
                target_bytes_per_file,
                &mut stats,
            );

            fs::write(&full_path, &file_content)?;
            stats.total_bytes += file_content.len();
            stats.total_files += 1;
            stats.file_paths.push(relative_path.clone());
        }

        stats.total_directories = dirs_created.len();
        Ok(stats)
    }

    fn build_file_plan(&self) -> Vec<PathBuf> {
        let categories = [
            "getting_started",
            "core_concepts",
            "api_reference",
            "guides",
            "integrations",
            "tutorials",
            "internals",
            "deployment",
            "benchmarks",
            "troubleshooting",
        ];

        let mut plan = Vec::with_capacity(self.config.file_count);
        // Root overview
        plan.push(PathBuf::from("README.md"));
        plan.push(PathBuf::from("ARCHITECTURE.md"));

        let mut file_idx = 1;
        while plan.len() < self.config.file_count {
            let cat = categories[(file_idx - 1) % categories.len()];
            let topic_name = get_topic_name(file_idx);
            let file_name = format!("{}.md", topic_name);
            plan.push(PathBuf::from(cat).join(file_name));
            file_idx += 1;
        }

        plan
    }

    fn generate_file_content(
        &self,
        file_index: usize,
        relative_path: &Path,
        all_files: &[PathBuf],
        target_bytes: usize,
        stats: &mut CorpusStats,
    ) -> String {
        let stem = relative_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document");
        let title = format_title(stem);

        let mut content = String::with_capacity(target_bytes + 512);

        // H1 Document Title
        content.push_str(&format!("# {}\n\n", title));
        stats.total_headings += 1;

        // Metadata Header
        content.push_str(&format!(
            "> **Module:** `{}` | **Document ID:** `doc_{:04}` | **Status:** `Stable`\n\n",
            relative_path.display(),
            file_index
        ));

        // Executive summary paragraph
        content.push_str(&format!(
            "This document provides comprehensive technical specifications, implementation details, \
             and operational guidelines for {}. It is intended for systems engineers, AI agent architects, \
             and software developers integrating Memex local documentation context services.\n\n",
            title
        ));

        let mut section_counter = 1;

        while content.len() < target_bytes {
            let h2_topic = get_section_topic(file_index * 17 + section_counter);
            content.push_str(&format!("## {}. {}\n\n", section_counter, h2_topic));
            stats.total_headings += 1;

            content.push_str(&format!(
                "The {} module is responsible for orchestrating low-latency queries, memory caching, \
                 and transactional synchronization across distributed compute nodes. When processing high-throughput \
                 streams, deterministic execution paths ensure consistent state resolution.\n\n",
                h2_topic.to_lowercase()
            ));

            // Optional Table / List
            if self.config.include_tables_and_lists && section_counter % 2 == 1 {
                content.push_str("### Key Parameters & Performance Metrics\n\n");
                stats.total_headings += 1;
                content.push_str("| Parameter | Type | Default | Description |\n");
                content.push_str("| :--- | :--- | :--- | :--- |\n");
                content.push_str(&format!(
                    "| `batch_size_{}` | `usize` | `64` | Maximum chunk batch size for tensor inference |\n",
                    section_counter
                ));
                content.push_str(&format!(
                    "| `cache_ttl_ms` | `u64` | `{}000` | Expiration time for cached graph traversals |\n",
                    section_counter
                ));
                content.push_str("| `vector_dim` | `usize` | `384` | Embedding dimensionality (all-MiniLM-L6-v2) |\n");
                content.push_str("| `max_depth` | `u32` | `3` | Maximum BFS traversal depth for neighbor queries |\n\n");
            }

            // H3 Subsection
            content.push_str(&format!("### Implementation Details for {}\n\n", h2_topic));
            stats.total_headings += 1;

            content.push_str(
                "Consider the following sequence when initializing session handles and establishing SQLite \
                 vector connections. All operations must acquire read locks before entering the critical section:\n\n",
            );

            // Code snippet
            if self.config.include_code_blocks {
                let code_sample = get_code_sample(section_counter);
                content.push_str(&code_sample);
                content.push_str("\n\n");
                stats.total_code_blocks += 1;
            }

            // Cross links
            if self.config.include_cross_links && !all_files.is_empty() {
                let target_idx1 = (file_index + section_counter * 3) % all_files.len();
                let target_idx2 = (file_index + section_counter * 7 + 1) % all_files.len();

                let target_file1 = &all_files[target_idx1];
                let target_file2 = &all_files[target_idx2];

                let rel_link1 = compute_relative_link(relative_path, target_file1);
                let rel_link2 = compute_relative_link(relative_path, target_file2);

                content.push_str(&format!(
                    "For related architectural context, see [{} documentation]({}) and the complementary [{} guide]({}).\n\n",
                    target_file1.file_stem().and_then(|s| s.to_str()).unwrap_or("Overview"),
                    rel_link1,
                    target_file2.file_stem().and_then(|s| s.to_str()).unwrap_or("Reference"),
                    rel_link2
                ));
                stats.total_cross_links += 2;
            }

            section_counter += 1;
        }

        // Final notes & references section
        content.push_str("## Summary and Best Practices\n\n");
        stats.total_headings += 1;
        content.push_str(
            "- Always utilize pre-warmed sessions to minimize ONNX cold-start latency.\n\
             - Ensure SQLite transactions batch chunk insertions to maximize write throughput.\n\
             - Monitor vector similarity scores to dynamically calibrate relevance thresholds.\n\
             - Maintain clean Markdown headings to optimize contextual chunking boundary detection.\n",
        );

        content
    }
}

/// Helper function to generate a Medium corpus (~500 KB, ~50 files).
pub fn generate_medium_corpus(root: &Path) -> io::Result<CorpusStats> {
    CorpusGenerator::from_preset(CorpusPreset::Medium).generate(root)
}

/// Helper function to generate a Large corpus (~5 MB, ~200 files).
pub fn generate_large_corpus(root: &Path) -> io::Result<CorpusStats> {
    CorpusGenerator::from_preset(CorpusPreset::Large).generate(root)
}

/// Helper function to generate a corpus with custom configuration.
pub fn generate_corpus(root: &Path, config: &CorpusConfig) -> io::Result<CorpusStats> {
    CorpusGenerator::new(config.clone()).generate(root)
}

fn format_title(stem: &str) -> String {
    stem.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn compute_relative_link(from_path: &Path, to_path: &Path) -> String {
    let from_depth = from_path.components().count().saturating_sub(1);
    let mut prefix = String::new();
    for _ in 0..from_depth {
        prefix.push_str("../");
    }
    format!("{}{}", prefix, to_path.to_string_lossy())
}

fn get_topic_name(idx: usize) -> &'static str {
    const TOPICS: &[&str] = &[
        "authentication_flow",
        "token_cache_management",
        "database_storage_layer",
        "graph_traversal_engine",
        "vector_embeddings_pipeline",
        "mcp_stdio_transport",
        "query_optimization_strategy",
        "rest_api_specification",
        "plugin_architecture_overview",
        "cli_subcommands_reference",
        "performance_monitoring",
        "error_handling_matrix",
        "memory_layout_internals",
        "distributed_consensus_protocol",
        "websocket_session_handling",
        "incremental_delta_sync",
        "hybrid_search_scoring",
        "sqlite_vec_extension_bindings",
        "onnx_model_quantization",
        "bpe_tokenizer_workflow",
        "concurrency_control_locks",
        "telemetry_and_tracing",
        "client_auto_installer",
        "configuration_schema_v2",
        "document_ast_parser",
        "hierarchical_chunking_rules",
        "subgraph_projection_filter",
        "knn_vector_index_builder",
        "connection_pool_tuning",
        "rate_limiting_algorithms",
        "zero_copy_deserialization",
        "context_window_budgeting",
        "background_indexing_daemon",
        "edge_relationship_indexer",
        "semantic_cache_invalidation",
        "token_efficiency_gate",
        "relevance_reranking_model",
        "markdown_table_extractor",
        "relative_path_resolver",
        "checksum_content_hasher",
        "atomic_config_writer",
        "signal_handling_lifecycle",
        "security_threat_model",
        "garbage_collection_policy",
        "lockfree_ring_buffer",
        "dynamic_dispatch_router",
        "batch_inference_pipeline",
        "multi_turn_conversation_store",
        "audit_trail_logger",
        "cross_encoder_verifier",
    ];

    TOPICS[idx % TOPICS.len()]
}

fn get_section_topic(idx: usize) -> &'static str {
    const SECTIONS: &[&str] = &[
        "System Architecture and High-Level Design",
        "Request Lifecycle and State Transitions",
        "Concurrency Primitives and Synchronization",
        "Memory Footprint and Allocation Strategy",
        "Database Schema and Index Configuration",
        "Vector Cosine Similarity Computation",
        "Graph Traversal and Neighbor Expansion",
        "Token Counting and Window Truncation",
        "Failure Recovery and Fault Tolerance",
        "Security, Isolation, and Sandboxing",
        "Performance Profiling and Latency Analysis",
        "Integration Protocols and Transport Framing",
    ];

    SECTIONS[idx % SECTIONS.len()]
}

fn get_code_sample(idx: usize) -> String {
    match idx % 4 {
        0 => r#"```rust
use memex::storage::Database;
use memex::models::Chunk;
use std::sync::Arc;

pub async fn execute_batch_query(
    db: Arc<Database>,
    embedding: &[f32; 384],
    top_k: usize,
) -> anyhow::Result<Vec<Chunk>> {
    let reader = db.reader()?;
    let results = reader.search_knn(embedding, top_k)?;
    tracing::debug!("Retrieved {} nearest neighbors", results.len());
    Ok(results)
}
```"#
            .to_string(),
        1 => r#"```python
import json
import subprocess

def query_memex_mcp(query_text: str, limit: int = 5) -> dict:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "search_documentation",
            "arguments": {"query": query_text, "limit": limit}
        }
    }
    result = subprocess.run(
        ["memex", "serve", "--mcp"],
        input=json.dumps(payload).encode("utf-8"),
        capture_output=True,
    )
    return json.loads(result.stdout.decode("utf-8"))
```"#
            .to_string(),
        2 => r#"```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "MemexConfiguration",
  "type": "object",
  "properties": {
    "embedding_model": {
      "type": "string",
      "default": "all-MiniLM-L6-v2"
    },
    "max_chunk_size": {
      "type": "integer",
      "default": 512
    },
    "exclude_patterns": {
      "type": "array",
      "items": { "type": "string" }
    }
  },
  "required": ["embedding_model", "max_chunk_size"]
}
```"#
            .to_string(),
        _ => r#"```bash
# Initialize and index project documentation
memex init ./docs --verbose

# Inspect generated database and schema integrity
sqlite3 .memex/memex.db "SELECT count(*) FROM chunks;"

# Run benchmark query verification
memex serve --mcp < query_payload.json
```"#
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_medium_corpus() {
        let temp = tempdir().unwrap();
        let stats = generate_medium_corpus(temp.path()).unwrap();

        assert_eq!(stats.total_files, 50, "Medium corpus should have 50 files");
        assert!(
            stats.total_bytes >= 400 * 1024 && stats.total_bytes <= 700 * 1024,
            "Medium corpus total size should be ~500 KB, got {} bytes",
            stats.total_bytes
        );
        assert!(
            stats.total_headings >= 150,
            "Medium corpus should contain headings"
        );
        assert!(
            stats.total_code_blocks >= 50,
            "Medium corpus should contain code blocks"
        );
        assert!(
            stats.total_cross_links >= 50,
            "Medium corpus should contain cross links"
        );
        assert!(
            stats.total_directories >= 5,
            "Medium corpus should create subdirectories"
        );

        // Verify root files exist
        assert!(temp.path().join("README.md").exists());
        assert!(temp.path().join("ARCHITECTURE.md").exists());
    }

    #[test]
    fn test_generate_large_corpus() {
        let temp = tempdir().unwrap();
        let stats = generate_large_corpus(temp.path()).unwrap();

        assert_eq!(stats.total_files, 200, "Large corpus should have 200 files");
        assert!(
            stats.total_bytes >= 4 * 1024 * 1024 && stats.total_bytes <= 7 * 1024 * 1024,
            "Large corpus total size should be ~5 MB, got {} bytes",
            stats.total_bytes
        );
        assert!(
            stats.total_headings >= 600,
            "Large corpus should contain headings"
        );
        assert!(
            stats.total_code_blocks >= 200,
            "Large corpus should contain code blocks"
        );
        assert!(
            stats.total_cross_links >= 200,
            "Large corpus should contain cross links"
        );
        assert!(
            stats.total_directories >= 8,
            "Large corpus should create subdirectories"
        );
    }

    #[test]
    fn test_generated_corpus_markdown_parsable_and_chunkable() {
        use memex::config::MemexConfig;
        use memex::discovery::FileDiscovery;
        use memex::ingestion::chunker::ContextualChunker;
        use memex::ingestion::parser::MarkdownParser;

        let temp = tempdir().unwrap();
        let stats = generate_medium_corpus(temp.path()).unwrap();

        let config = MemexConfig::default();
        let discovered = FileDiscovery::scan(temp.path(), &config).unwrap();
        assert_eq!(discovered.len(), stats.total_files);

        let mut total_chunks = 0;
        for file_path in &discovered {
            let content = fs::read_to_string(file_path).unwrap();
            let doc_ast =
                MarkdownParser::parse(&content).expect("Generated markdown must parse cleanly");
            let chunks = ContextualChunker::chunk_document(&file_path.to_string_lossy(), &doc_ast);
            assert!(
                !chunks.is_empty(),
                "Each generated file must produce chunks"
            );
            total_chunks += chunks.len();
        }

        // Medium corpus should produce ~800 chunks (per architecture spec)
        assert!(
            total_chunks >= 300,
            "Expected substantial chunk count, got {}",
            total_chunks
        );
    }
}
