# Agent Guidelines & Harness for Memex

## 1. Primary Language Directive
> **CRITICAL REQUIREMENT**: All agent-produced artifacts, commits, pull requests, issues, comments, documentation, code docstrings, and messages must ALWAYS be written strictly in **English**.
>
> - **Commits**: Follow Conventional Commits in English (e.g. `feat: add ...`, `fix: resolve ...`, `docs: update ...`).
> - **Pull Requests**: Titles, descriptions, checklists, and summaries must be entirely in English.
> - **Code & Documentation**: All comments, rustdoc documentation, guides, design docs, and release notes must be in English.
> - **GitHub Issues**: Titles and bodies must be formatted in English.

## 2. Project Architecture & Context
- **Name**: `memex`
- **Language**: Rust (edition 2024)
- **Description**: Fast, local code search & indexing engine combining lexical (BM25/tantivy), vector embeddings, and AST parsing for contextual retrieval.
- **Documentation Directory Structure**:
  - `docs/README.md`: Index and overview of all documentation.
  - `docs/architecture/`: Architecture diagrams, technical design, and core systems.
  - `docs/benchmarks/`: Benchmark methodology, metrics, and empirical reports.
  - `docs/roadmap/`: Roadmap, milestones, tasks, and future plans.

## 3. Development Workflow & Quality Standards
> **MANDATORY PRE-COMMIT & PRE-PUBLISH DIRECTIVE**:
> Before committing (`git commit`), pushing (`git push`), or publishing a PR, agents **MUST ALWAYS** run:
> 1. `cargo fmt --all` (ensure zero formatting diffs)
> 2. `cargo clippy --all-targets -- -D warnings` (must pass with 0 warnings)
> 3. `cargo test --lib --bins` (all tests must pass)
>
> Committing or pushing without executing and verifying these three steps is **strictly prohibited**.

- **Formatting**: Always format Rust code using `cargo fmt --all` before committing or pushing.
- **Linting**: Ensure code passes `cargo clippy --all-targets -- -D warnings`.
- **Testing**:
  - Run unit and integration tests with `cargo test`.
  - Validate benchmarks when touching indexing/retrieval paths: `cargo bench` or empirical benchmark scripts.
- **Git & PR Workflow**:
  - Create small, focused feature or chore branches (`feat/*`, `fix/*`, `docs/*`, `refactor/*`, `harness/*`).
  - Keep commits clean, granular, and properly formatted.
  - Push branch and create PR via `gh pr create` with descriptive English title and body.
