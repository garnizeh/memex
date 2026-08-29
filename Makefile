.PHONY: help build check test test-unit test-integration bench fmt clippy clean run

help: ## Show this help message
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

build: ## Compile binary in release mode
	cargo build --release

check: ## Fast compilation check
	cargo check --all-targets

test: ## Run unit and integration tests
	cargo test --workspace

test-unit: ## Run only unit tests
	cargo test --lib

test-integration: ## Run only integration tests
	cargo test --test '*'

test-efficiency-gate: ## Run token reduction efficiency gate verification
	cargo test --test test_token_reduction_gate -- --ignored

bench: ## Run Criterion performance and efficiency benchmarks
	cargo bench

fmt: ## Format Rust code
	cargo fmt

fmt-check: ## Check code formatting without applying changes
	cargo fmt --check

clippy: ## Run Clippy linter
	cargo clippy --all-targets -- -D warnings

clean: ## Clean build artifacts
	cargo clean

install-hooks: ## Install Git hooks for automatic background indexing
	./scripts/install-git-hooks.sh

run: ## Run Memex CLI locally
	cargo run --
