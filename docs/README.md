# Memex Documentation

Welcome to the technical documentation repository for **Memex**, a fast, local, and 100% offline Model Context Protocol (MCP) server for repository documentation indexing and semantic retrieval.

---

## 📂 Documentation Directory Structure

The documentation is organized into domain-specific subdirectories:

```text
docs/
├── README.md                      # Documentation hub and index (this file)
├── architecture/                  # System architecture, database schema, and protocol specs
│   └── architecture.md            # Comprehensive Architecture Design Document (ADD)
├── roadmap/                       # Implementation phases, progress tracking, and future vision
│   ├── mvp-roadmap.md             # MVP Development Roadmap & 10-Phase Task Breakdown
│   └── post-mvp-roadmap.md        # Strategic Post-MVP Roadmap & Technical Recommendations
└── benchmarks/                    # Empirical evaluation, token efficiency, and latency reports
    └── benchmark-report.md        # Real-World Empirical Dogfooding Benchmark Report
```

---

## 📚 Section Overview

### 1. [Architecture & Technical Design](architecture/architecture.md)
Detailed specification of the entire Memex system:
- **Core Principles & Monolithic Design:** Single Rust binary, offline-first execution, zero external runtime dependencies.
- **Data Ingestion & AST Parsing:** CommonMark parsing, semantic chunking, and hierarchical heading path preservation.
- **Storage & Vector Engine:** SQLite in WAL mode, `sqlite-vec` integration for vector KNN search, schema specifications for chunks and document graphs.
- **MCP Protocol & Tools:** Specification for `search_documentation` and `traverse_graph` tools over stdio JSON-RPC.
- **Security & Integrity:** Local boundary enforcement, path traversal protection, and automated CI gates.

### 2. [MVP Development Roadmap](roadmap/mvp-roadmap.md)
The 10-phase execution plan used to build the Memex MVP:
- **Phase 1 to Phase 10 Breakdown:** Step-by-step implementation tasks from core models, AST chunking, embedding engine, MCP server, to automated test suites and release polish.
- **Task Verification Deliverables:** Granular checklists and deliverables for each milestone.

### 3. [Post-MVP Strategic Roadmap](roadmap/post-mvp-roadmap.md)
The next evolution phases transitioning Memex from MVP to an enterprise-grade platform:
- **Phase 1: Foundation Stability & Hardening:** Enhanced local observability, `memex doctor`, and structured diagnostics.
- **Phase 2: Feature Expansion & Ecosystem:** Multi-agent config management, semantic link auto-discovery, and watch-mode indexing.
- **Phase 3: Strategic Vision & Enterprise Evolution:** Scalable hybrid storage, enterprise documentation plugins, and collaborative telemetry.

### 4. [Empirical Benchmark Report](benchmarks/benchmark-report.md)
Comprehensive real-world benchmarking results validating the efficiency gains of Memex:
- **Token Efficiency:** Empirical validation demonstrating a **98.13% token reduction** across real-world developer queries.
- **Latency & Performance:** **<50ms average query latency** measured on standard hardware using local ONNX embeddings (`all-MiniLM-L6-v2`) and `sqlite-vec`.
