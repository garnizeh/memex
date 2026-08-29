# Memex Post-MVP Strategic Roadmap & Technical Recommendations

**Version:** 1.1  
**Status:** Post-MVP (v0.3.0) Planning Document  
**Audience:** Core Engineering Team, Product Stakeholders  

---

## 1. Executive Summary

The Memex MVP (v0.3.0) has successfully delivered a robust, offline-first semantic search engine for AI coding assistants. With **47/47 core tasks completed**, the foundation is solid. The system achieves a validated **98.13% token reduction** and **<50ms query latency**.

This document outlines a detailed technical roadmap to transition Memex from a functional MVP to a scalable, enterprise-grade platform. Recommendations are organized into sequential milestone phases: **Phase 1: Foundation Stability & Hardening**, **Phase 2: Feature Expansion & Ecosystem**, and **Phase 3: Strategic Vision & Enterprise Evolution**, with special emphasis on **Observability, Diagnostics, and Privacy-Preserving Telemetry** to ensure system reliability, performance governance, and user transparency.

---

## 2. Foundation Stability & Hardening (Phase 1)

*Focus: Reliability, User Feedback Loops, Local Observability, and Edge Case Resolution.*

### 2.1. Enhanced Local Observability & Diagnostics
**Current State:** Basic `tracing` setup; errors logged to `.memex/errors.log`.
**Problem:** Debugging user-specific indexing failures or model loading issues in the wild is difficult without configurable verbosity, structured error codes, and strict separation between diagnostic output and MCP JSON-RPC protocol transport.

**Recommendations:**
1. **Dynamic Log Levels & Format via CLI:**
   - Implement verbosity flags (`-v`, `-vv`, `--quiet`, `--log-json`) in the CLI parser (`clap`).
   - Map these to `tracing_subscriber` filters at runtime initialization.
   - Support `RUST_LOG=memex=debug` filtering.
   - **Strict Stderr Isolation:** Ensure all logs, diagnostic output, and progress indicators are routed exclusively to `stderr` to prevent JSON-RPC stream pollution on `stdout` during `serve --mcp` mode.
2. **Diagnostic Command (`memex doctor`):**
   - Implement `memex doctor` for automated environment validation.
   - **Outputs & Checks:**
     - ONNX Runtime version & model SHA-256 integrity verification.
     - SQLite WAL status, vector extension (`sqlite-vec`) loading, and database integrity check (`PRAGMA integrity_check`).
     - Configuration file paths, permissions, and schema compliance.
     - System resource availability (RAM, disk space, CPU capabilities like AVX2/NEON).
     - MCP agent integration status (detect active configurations in Claude Code, Cursor, Windsurf, Zed).
3. **Local Index Diagnostics (`memex stats`):**
   - Provide an operational overview of the local index: total files tracked, chunks indexed, embedding vector dimensions, database file size, SQLite page fragmentation, and top-5 largest indexed files.
4. **Structured Error Reporting:**
   - Enhance `errors.rs` to include unique, machine-readable error codes (e.g., `MEMEX-E-1042` for model load failure, `MEMEX-E-2010` for SQLite lock timeout).
   - Generate contextual triage links pointing to offline/online documentation for each specific error code.

### 2.2. Model Management & Updates
**Current State:** Model (`all-MiniLM-L6-v2`) is bundled/downloaded once and static.
**Problem:** Users cannot update the embedding model if a better small-model becomes available, nor can they verify model integrity if the file gets corrupted.

**Recommendations:**
1. **Model Integrity Verification:**
   - Store the SHA-256 hash of the expected model in `config.rs`.
   - On startup, verify the local `.onnx` file hash. If mismatched, auto-trigger re-download or alert the user.
2. **Explicit Model Update Command:**
   - Implement `memex model update [--force]`.
   - Logic: Check a remote manifest (hosted on GitHub Releases) for a newer model version/hash.
   - **Safety:** Download to a temp file -> Verify Hash -> Atomic Rename -> Reload Session.
3. **Configurable Model Path:**
   - Allow `memex.json` to specify a custom path: `"embedding_model": "/custom/path/model.onnx"`.
   - Enables power users to test experimental quantizations (e.g., INT8 vs FP32).

### 2.3. Windows Parity & Robustness
**Current State:** Some tests gated to Unix; file path handling relies on standard libs but edge cases (long paths, special chars) need stress testing.

**Recommendations:**
1. **Enable Full Test Suite on Windows:**
   - Remove `#[cfg(unix)]` gates from `test_git_hooks` and file discovery tests.
   - Implement Windows-specific mock implementations for any POSIX-only assumptions (e.g., executable bits).
2. **Long Path Support:**
   - Ensure all file I/O uses the `\\?\` prefix on Windows when paths exceed 260 characters.
   - Use `dunce` crate to normalize Windows paths safely before processing.
3. **PowerShell Installer Hardening:**
   - Avoid executing unverified remote scripts via shell piping (`irm ... | iex`). Instead, download the installer to a staging file, verify its Authenticode signature or pinned SHA-256 hash, and then invoke execution.
   - Validate the downloaded `memex.exe` binary digest before final installation and implement an automatic rollback mechanism on failure.

---

## 3. Feature Expansion & Ecosystem (Phase 2)

*Focus: Flexibility, Customization, Ecosystem Support, and Observability & Telemetry Infrastructure.*

### 3.1. Advanced Chunking Strategies
**Current State:** Fixed-size chunking (~512 tokens) with simple header inheritance.
**Problem:** Fixed sizes may split coherent logical units (e.g., a function definition + its docstring) or create too much noise for very small files.

**Recommendations:**
1. **Semantic Chunking:**
   - Integrate a lightweight rule-based splitter that respects Markdown headers (`#`, `##`) and code blocks.
   - *Algorithm:* Do not break chunks in the middle of a fenced code block (` ``` `).
2. **Configurable Parameters:**
   - Expose in `memex.json`:
     ```json
     "chunking": {
       "strategy": "semantic",
       "max_tokens": 512,
       "overlap_tokens": 50,
       "min_chunk_size": 32
     }
     ```
3. **Parent-Child Indexing:**
   - Store small "child" chunks for precise retrieval but return the larger "parent" context window to the LLM.
   - *Schema Change:* Add `parent_id` to the `chunks` table.

### 3.2. Extended File Format Support
**Current State:** Markdown (`.md`) only.
**Problem:** Developers need context from source code (`.rs`, `.py`, `.ts`), config files (`.yaml`, `.toml`), and documentation generators (JSDoc, Rustdoc).

**Recommendations:**
1. **Pluggable Parser Architecture:**
   - Refactor `ingestion` module to use a trait-based parser system:
     ```rust
     pub trait DocParser: Send + Sync {
         fn parse(&self, content: &str, path: &Path) -> Result<Vec<Document>>;
         fn supported_extensions(&self) -> &[&str];
     }
     ```
2. **Priority Implementations:**
   - **Source Code Parser:** Extract doc comments (`///`, `/** */`) and top-level structure (modules, classes, functions) as hierarchical nodes.
   - **ReStructuredText/Sphinx:** For Python ecosystem compatibility.
   - **AsciiDoc:** For enterprise documentation.
3. **Language-Specific Metadata:**
   - When parsing code, inject metadata tags: `lang: rust`, `scope: public`, `type: trait`.
   - Enable filtering in MCP tools: `search_documentation(query, lang="rust")`.

### 3.3. Agent Ecosystem Expansion
**Current State:** Supports Claude Code, Cursor, Antigravity.
**Problem:** Rapidly growing market of AI agents (Windsurf, Zed, VS Code Copilot Chat).

**Recommendations:**
1. **Universal MCP Config Generator:**
   - Create a generic installer for any agent supporting the standard `mcp.json` spec location.
2. **Specific Integrations:**
   - **Windsurf (Codeium):** Detect `~/.codeium/windsurf/config.json`.
   - **Zed Editor:** Detect `~/.config/zed/settings.json` (MCP section).
   - **VS Code:** Support the official "MCP Server" extension configuration.
3. **Runtime Connection Testing:**
   - Add `memex test-connection <agent_name>` to verify the agent can successfully handshake with the Memex stdio server.

### 3.4. Query Enhancement & Reranking
**Current State:** Pure vector similarity (KNN) via `sqlite-vec`.
**Problem:** Vector search retrieves semantically similar text but might miss exact keyword matches (e.g., specific error codes, variable names).

**Recommendations:**
1. **Hybrid Search (BM25 + Vector):**
   - Enable SQLite FTS5 (Full Text Search) on the `content` column alongside `sqlite-vec`.
   - Implement a scoring fusion algorithm (e.g., Reciprocal Rank Fusion) to combine vector scores and keyword match scores.
2. **Local Cross-Encoder Reranker (Optional):**
   - Offer an optional, heavier model (e.g., `ms-marco-MiniLM-L-6-v2` cross-encoder) for reranking the top-20 results.
   - *Trade-off:* Higher latency (~100ms extra) but significantly higher precision. Make this configurable via `--precise` flag.

### 3.5. Observability & Privacy-Preserving Telemetry
**Current State:** Local error logging only; no distributed tracing or standardized usage/performance analytics.  
**Problem:** Without structured observability and telemetry, it is impossible to detect systemic performance regressions, monitor model inference bottlenecks across heterogeneous hardware, or understand token savings efficacy in real-world workloads.

```text
┌───────────────────────────────────────────────────────────────────────────────────┐
│                           Memex Observability Architecture                         │
└───────────────────────────────────────────────────────────────────────────────────┘
          │                                                    │
          ▼                                                    ▼
┌───────────────────────────────┐                    ┌──────────────────────────────┐
│       Local Diagnostics       │                    │  Privacy-Preserving Telemetry│
│ ───────────────────────────── │                    │ ──────────────────────────── │
│ • tracing-subscriber (stderr) │                    │ • Opt-in / Opt-out Controls  │
│ • memex doctor / stats        │                    │ • Cryptographic Pseudonym ID │
│ • OpenTelemetry Spans         │                    │ • Zero Code/Query Retention  │
│ • Local SQLite Audit Log      │                    │ • Offline SQLite Queue       │
└───────────────────────────────┘                    └──────────────────────────────┘
          │                                                    │
          ▼                                                    ▼
┌───────────────────────────────┐                    ┌──────────────────────────────┐
│  Enterprise OTLP Exporters    │                    │ Aggregate Telemetry Endpoint │
│  (Jaeger, Prometheus, Datadog)│                    │ (Crash, Latency, Token % KPIs│
└───────────────────────────────┘                    └──────────────────────────────┘
```

**Recommendations:**

#### 3.5.1. Privacy-First Telemetry Architecture
1. **Strict Privacy Principles:**
   - **Local-First & Zero PII Guarantee:** Never collect or transmit source code, query strings, file paths, repository names, author names, or environment variables.
   - **Pseudonymous Machine Identity:** Generate an anonymized, cryptographically salted SHA-256 identifier stored in `~/.memex/telemetry_id`.
   - **Transparent Opt-In / Opt-Out Controls:**
     - Prompt user explicitly during initial `memex init`.
     - Support standard environment toggles: `MEMEX_TELEMETRY=0`, `DO_NOT_TRACK=1`.
     - Expose configuration setting in `memex.json`:
       ```json
       "telemetry": {
         "enabled": false,
         "endpoint": "https://telemetry.memex.dev/v1/metrics",
         "flush_interval_secs": 300
       }
       ```
2. **Telemetry Payload & Event Types:**
   - **Session Lifecycle:** Startup time, shutdown reason, CLI command invoked, active MCP client name (`claude-code`, `cursor`, `antigravity`).
   - **Performance & Efficiency Metrics:**
     - Aggregate token reduction percentage achieved (`naive_tokens` vs `memex_tokens`).
     - Query latency percentiles (P50, P95, P99).
     - Indexing throughput (chunks per second, total time elapsed).
     - System hardware profile (OS, CPU architecture, core count, available memory, ONNX execution provider).
   - **Sanitized Error & Panic Reporting:** Crash traces with error codes (`MEMEX-E-XXXX`) stripped of local filesystem paths and source snippet contents.
3. **Offline-First Asynchronous Batching:**
   - Persist telemetry events to a lightweight local SQLite table (`.memex/telemetry_queue.db`).
   - Flush batches asynchronously on a background worker thread with exponential backoff.
   - **Zero User Impact:** Ensure telemetry never blocks query execution or MCP request/response loops.

#### 3.5.2. Distributed Tracing & OpenTelemetry (OTel)
1. **Tracing Layer Integration:**
   - Integrate `tracing-opentelemetry` to emit standard OpenTelemetry spans across key critical path operations:
     - `mcp.request`: MCP JSON-RPC protocol round-trip.
     - `ingest.tokenize`: Text tokenization and chunk splitting.
     - `model.embed`: ONNX Runtime inference batch latency.
     - `db.vector_knn`: `sqlite-vec` vector similarity search execution.
     - `db.fts_search`: SQLite FTS5 keyword matching and rank fusion.
     - `graph.traverse`: Multi-hop document relationship traversal.
2. **Enterprise OTLP Exporter:**
   - Provide an optional `--otlp-endpoint <url>` flag or `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable to export traces and metrics to OpenTelemetry collectors (Jaeger, Prometheus, Datadog, Grafana Tempo).

#### 3.5.3. Core Metrics Catalog
The following standard metrics will be instrumented across the engine:

| Metric Name | Type | Unit | Description | Target |
| :--- | :--- | :--- | :--- | :---: |
| `memex_query_duration_seconds` | Histogram | Seconds | End-to-end latency for `search_documentation` | P95 < 0.050s (< 50ms) |
| `memex_token_reduction_ratio` | Histogram | Ratio (0.0-1.0) | Token reduction percentage achieved per query | > 0.90 (> 90%) |
| `memex_onnx_inference_duration_seconds` | Histogram | Seconds | Time spent generating embeddings in ONNX session | P95 < 0.025s (< 25ms) |
| `memex_db_query_duration_seconds` | Histogram | Seconds | SQLite + sqlite-vec query execution time | P95 < 0.010s (< 10ms) |
| `memex_indexing_throughput_chunks_per_sec`| Gauge | Chunks/s | Rate of chunks parsed, embedded, and indexed | > 100/s (CPU) |
| `memex_mcp_requests_total` | Counter | Requests | Count of MCP requests partitioned by tool and status | — |
| `memex_cache_hit_ratio` | Gauge | Ratio (0.0-1.0) | Hit ratio for query embeddings and chunk cache | > 0.40 |
| `memex_memory_resident_bytes` | Gauge | Bytes | Peak Resident Set Size (RSS) memory consumption | < 150MB |

---

## 4. Strategic Vision & Enterprise Evolution (Phase 3)

*Focus: Scalability, Collaboration, Enterprise Governance, and AI Evolution.*

### 4.1. Team & Enterprise Features
**Current State:** Single-user, local-only index.  
**Vision:** Shared knowledge bases for engineering teams.

**Recommendations:**
1. **Encrypted Cloud Sync (Optional):**
   - Implement a sync protocol using WebRTC or a relay server.
   - **Security:** End-to-End Encryption (E2EE). The server stores only encrypted blobs; keys never leave the client.
   - Use CRDTs (Conflict-free Replicated Data Types) to handle concurrent index updates from multiple team members.
2. **Organization Indexes:**
   - Allow merging multiple local indexes into a "Team Index" stored in a shared network volume or S3 bucket (read-only for most, write-access for maintainers).
3. **Access Control Lists (ACLs):**
   - Tag chunks with visibility levels (`public`, `internal`, `confidential`) within encrypted index manifests.
   - Maintain client-side query-level filtering after local blob decryption using the authenticated user's local key hierarchy, preventing the relay server from inspecting plaintext metadata or violating E2EE guarantees.

### 4.2. Advanced AI Capabilities
**Vision:** Moving from "Search" to "Reasoning".

**Recommendations:**
1. **Domain-Specific Fine-Tuning:**
   - Build a pipeline to export high-quality chunk pairs (query, relevant_doc) from user interaction logs (opt-in).
   - Provide a script to fine-tune a small local LLM (e.g., Phi-3, Llama-3-8B) on the project's specific terminology.
2. **Graph-Based Reasoning:**
   - Leverage the existing graph structure (`edges` table) for multi-hop reasoning.
   - Implement "Path Finding" queries: *"How does the Authentication module connect to the Database Migration script?"*
   - Expose a new MCP tool: `find_path(start_node, end_node)`.
3. **Automated Documentation Healing:**
   - Detect "orphaned" chunks (no incoming links) or outdated references.
   - Suggest documentation updates to the user: *"This function was modified in recent commits, but the linked documentation chunk hasn't changed. Review?"*

### 4.3. Plugin System & Extensibility
**Vision:** A community-driven ecosystem.

**Recommendations:**
1. **WASM Plugin Sandbox:**
   - Allow users to write custom parsers or pre-processors in Rust/Go/TypeScript, compiled to WASM.
   - Load these dynamically at runtime to handle proprietary file formats without modifying Memex core.
2. **Custom Tool Definitions:**
   - Allow `memex.json` to define custom MCP tools that execute specific SQL queries or graph traversals predefined by the user.

### 4.4. Enterprise Fleet Observability & Autonomous Optimization
**Vision:** Centralized health governance and self-optimizing search engines.

**Recommendations:**
1. **Enterprise Fleet Telemetry Aggregator:**
   - Centralized observability dashboard for engineering leaders to monitor repository documentation coverage, query precision, and developer token savings across the engineering organization.
   - Identification of "knowledge dead zones" (areas in codebases where LLM queries consistently yield low semantic similarity scores).
2. **Autonomous Self-Healing & Adaptive Compaction:**
   - Telemetry-driven database maintenance: automatically schedule `VACUUM` and vector index rebuilding when query latency drifts beyond P95 thresholds due to SQLite page fragmentation.
   - Embedding drift monitoring: detect when codebase changes diverge significantly from the embedding model's semantic cluster space.

---

## 5. Technical Debt & Refactoring Priorities

While the codebase is high quality, the following areas should be addressed during the expansion phases:

| Area | Current Implementation | Recommended Refactor | Priority |
| :--- | :--- | :--- | :---: |
| **Concurrency** | `Arc<Session>` with sequential batch processing | Implement a worker pool pattern (`rayon` / actor model) for parallel embedding generation across CPU cores. | High |
| **Observability Overhead** | Basic `tracing` logging | Adopt zero-allocation tracing spans and adaptive sampling to ensure telemetry overhead remains < 1ms per query. | High |
| **Memory Mgmt** | Loads full file content into memory | Switch to streaming parsers for files > 1MB to reduce peak RAM usage. | Medium |
| **Telemetry Dispatcher** | Synchronous error writes | Implement a non-blocking, bounded SQLite telemetry buffer with background batch worker and backpressure control. | Medium |
| **Config Validation** | Basic Serde deserialization | Integrate `jsonschema` to provide detailed validation errors for `memex.json`. | Low |
| **Binary Size** | ~15-20MB (static linked) | Investigate `cargo-strip` and dynamic linking options for distro-packaged versions. | Low |

---

## 6. Success Metrics (KPIs) for Next Phase

To measure the success of these recommendations, track the following:

1. **Adoption & Engagement:** Growth in active installs and aggregate MCP session duration.
2. **Index Health & Reliability:** % of successful indexing runs without errors (Target: >99.5%).
3. **Crash-Free Session Rate:** Telemetry-verified crash-free execution rate (Target: >99.9%).
4. **Query Precision & Token Savings:** Maintain >90% (>0.90) validated token reduction and >0.85 Precision@5 across supported file formats.
5. **Performance Governance:** Maintain P95 query latency < 0.050s (< 50ms) and telemetry dispatch overhead < 0.001s (< 1ms).
6. **Telemetry Transparency:** 100% compliance with privacy-first standards (zero PII, zero code retention, full opt-out support).
7. **Ecosystem Breadth:** Expand native support to >= 5 AI agents and >= 6 file formats.

---

## 7. Conclusion

Memex has achieved a remarkable MVP launch. The transition from v0.3.0 to v1.0 requires shifting focus from **feature completion** to **ecosystem integration, operational observability, and system hardening**. 

By implementing hybrid search, expanding file support, introducing enterprise-grade OpenTelemetry tracing, and deploying privacy-first telemetry, Memex will solidify its position as the indispensable, reliable "second brain" for AI-assisted software development.

**Next Immediate Step:** Prioritize the **"Enhanced Local Observability & `memex doctor`"** and **"Model Management"** implementations for the v0.4.0 milestone.
