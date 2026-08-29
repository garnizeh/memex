# Changelog

## [0.4.0](https://github.com/garnizeh/memex/compare/memex-v0.3.0...memex-v0.4.0) (2026-08-29)


### Features

* migrate to Rust 2024 edition and initialize agent harness ([e6dfb74](https://github.com/garnizeh/memex/commit/e6dfb744439528d213614b514379d2ef6c3396cd))

## [0.3.0](https://github.com/garnizeh/memex/compare/memex-v0.2.0...memex-v0.3.0) (2026-08-29)


### Features

* **installer:** add one-line curl and powershell installers and validate benchmark retrieval quality ([9451bc6](https://github.com/garnizeh/memex/commit/9451bc61652c90cec5c4a98d9bfeb56340ab3a04))


### Bug Fixes

* **bench:** support index.db fallback in run_empirical_benchmark ([76e7034](https://github.com/garnizeh/memex/commit/76e7034e957a56592a69a4b38e61f17a9d7786c2))

## [0.2.0](https://github.com/garnizeh/memex/compare/memex-v0.1.0...memex-v0.2.0) (2026-08-29)


### Features

* **benchmarks:** implement synthetic corpus generator (TASK-9.2) ([3c47f70](https://github.com/garnizeh/memex/commit/3c47f70dac8cd01aa1dc16d2f1cc789a0a357918))
* **cli:** implement memex index command and project root discovery (TASK-6.5) ([fcce8dd](https://github.com/garnizeh/memex/commit/fcce8dd67c6960487a9e1b9ca739974efddc5a2e))
* **cli:** implement memex init command and full project initialization (TASK-6.4) ([32628e6](https://github.com/garnizeh/memex/commit/32628e61b5c3b81821643bd8e3a359544876f51b))
* **cli:** implement memex install command (TASK-8.5) ([448d0d8](https://github.com/garnizeh/memex/commit/448d0d8f058e7e577cf7904ae9f79a009eaae7d5))
* **cli:** implement memex serve --mcp command and server loop (TASK-7.5) ([bc5f424](https://github.com/garnizeh/memex/commit/bc5f424599034126d29ac74f2d34bd69e231b33e))
* complete MVP implementation, CI/CD release automation and Phase 10 validation ([fb729d3](https://github.com/garnizeh/memex/commit/fb729d36a80211d136ebe69d500d516c214317bb))
* complete MVP implementation, CI/CD release automation and Phase 10 validation ([22d32c7](https://github.com/garnizeh/memex/commit/22d32c701bece18f3aa72ff8ac8806db4ee4d173))
* **config:** implement MemexConfig parser for memex.json (TASK-1.4) ([31846dd](https://github.com/garnizeh/memex/commit/31846ddcc1b9b7437e38064b15309c937e693bed))
* **discovery:** implement gitignore, custom exclude/include filtering, and path filter chain (TASK-3.3) ([d223d1b](https://github.com/garnizeh/memex/commit/d223d1b1ab5b3c916c7a540431c84bcab5bc124f))
* **discovery:** implement recursive file walker (TASK-3.2) ([860361c](https://github.com/garnizeh/memex/commit/860361cd10328c8d9f972e8ce2cad936c1f4e8bd))
* **discovery:** implement root safety validator and tests (TASK-3.1) ([548ebea](https://github.com/garnizeh/memex/commit/548ebea95e14bcd5c224f6c118315a3aeb28e92a))
* **discovery:** implement SHA-256 content hash calculation utility (TASK-3.4) ([4e24460](https://github.com/garnizeh/memex/commit/4e244604a24ffbf98bda320f31e13c075e717cbc))
* **embedder:** implement batched inference and L2 normalization (TASK-5.4) ([215a325](https://github.com/garnizeh/memex/commit/215a3259307d804dfa78631fe599d27bbce8d712))
* **embedder:** implement model and tokenizer asset loader with integrity verification (TASK-5.1) ([ab4fe53](https://github.com/garnizeh/memex/commit/ab4fe53f675faabddd8e44ef53653ab5b7291c63))
* **embedder:** implement ONNX Runtime session management (TASK-5.2) ([e8b6ec8](https://github.com/garnizeh/memex/commit/e8b6ec8f26e5303572af3d29f185dcfc4ccb5d81))
* **embedder:** implement tokenization and attention mask pipeline (TASK-5.3) ([2cac601](https://github.com/garnizeh/memex/commit/2cac6016eff9a9f7404c74fa9f0e664e4bd4eb40))
* **errors:** implement comprehensive error type hierarchy (TASK-1.3) ([26cacee](https://github.com/garnizeh/memex/commit/26cacee3478af2a8d2065ab9196c0c0867f745f8))
* implement criterion performance benchmarks (task-9.4) ([8c7f009](https://github.com/garnizeh/memex/commit/8c7f0099bdbcb7ad2d6edc0395aa8a943818fce6))
* **index:** implement end-to-end document indexing coordinator (TASK-6.2) ([453f743](https://github.com/garnizeh/memex/commit/453f743c01a74b2e1f80dcb7889290058c83842f))
* **index:** implement error log recorder and error isolation (TASK-6.3) ([a43b3b5](https://github.com/garnizeh/memex/commit/a43b3b5f24567526997d74152b95cd09fc8a59b9))
* **index:** implement incremental delta engine (TASK-6.1) ([3d4a3dc](https://github.com/garnizeh/memex/commit/3d4a3dc25edb41f67b6951cca3b5cc8a433cca46))
* **ingestion:** implement chunk size guardrail and splitting (TASK-4.3) ([c10c52d](https://github.com/garnizeh/memex/commit/c10c52d9c28dd732e86a0af43f51f65f110da96b))
* **ingestion:** implement contextual prefix generator and chunking (TASK-4.2) ([cc4f900](https://github.com/garnizeh/memex/commit/cc4f9004c87a6d525171454792440aba89447dbf))
* **ingestion:** implement explicit markdown link resolver (TASK-4.5) ([bfbeb61](https://github.com/garnizeh/memex/commit/bfbeb61b645d855bce719984c6d1d433d728b27b))
* **ingestion:** implement graph hierarchy edge builder (TASK-4.4) ([8f7179e](https://github.com/garnizeh/memex/commit/8f7179e220bbb628eb80c4b52685773b68caf28b))
* **ingestion:** implement markdown event-to-AST parser (TASK-4.1) ([396f80d](https://github.com/garnizeh/memex/commit/396f80d77ac343c2e4f11d13222c63a2367c1b1a))
* initial commit for memex ([39b90b6](https://github.com/garnizeh/memex/commit/39b90b61897d2a2b8558c11fca5f8b8230dfdc8e))
* **installer:** implement agent target trait and registry (TASK-8.2) ([fda3880](https://github.com/garnizeh/memex/commit/fda3880ccddb41e157b361cae976fcfee1f64cb5))
* **installer:** implement atomic file mutation and json config helpers (TASK-8.1) ([7c57944](https://github.com/garnizeh/memex/commit/7c57944d49e2f28af476f19668af61e3679dab19))
* **installer:** implement Claude Code installer target (TASK-8.3) ([eaa74cc](https://github.com/garnizeh/memex/commit/eaa74ccdf7c0f951e937b6117a64e21c084571a6))
* **mcp:** implement JSON-RPC 2.0 stdio framing and transport (TASK-7.1) ([e6f39f4](https://github.com/garnizeh/memex/commit/e6f39f4d72adf293758872a4555c7f200c41ba17))
* **mcp:** implement MCP handshake and tool schemas (TASK-7.2) ([9d8937e](https://github.com/garnizeh/memex/commit/9d8937e855855a9c4c10694f0ef09c0b0b9c59e0))
* **mcp:** implement search_documentation tool handler and markdown formatter (TASK-7.3) ([c69d371](https://github.com/garnizeh/memex/commit/c69d37139dcca3146fc433c7d06e5154253b3c8d))
* **mcp:** implement traverse_graph tool handler and markdown formatting (TASK-7.4) ([bfdc7fe](https://github.com/garnizeh/memex/commit/bfdc7fe17a10705e2301269c0aac77ccfbaade0f))
* **models:** implement core domain models and unit tests (TASK-1.2) ([f2530b4](https://github.com/garnizeh/memex/commit/f2530b4e30f8318e7e05e20e4a876dc030b15d38))
* **phase-1:** complete TASK-1.1 module directory scaffolding ([830f23e](https://github.com/garnizeh/memex/commit/830f23eb7e4eb0c57f6ed15e7441bd772a99d346))
* **storage:** implement Database connection helper and pragmas (TASK-2.1) ([354c3de](https://github.com/garnizeh/memex/commit/354c3dea28d311c4cdf362bca8601a51672ff47d))
* **storage:** implement database schema initialization and migrations (TASK-2.2) ([e3440f2](https://github.com/garnizeh/memex/commit/e3440f2447c4f1558c0b5b9f3b7acf4d4ff3ab51))
* **storage:** implement graph traversal engine (TASK-2.6) ([5bbdffc](https://github.com/garnizeh/memex/commit/5bbdffc73e7244247b65ef5fdc036effdccebed5))
* **storage:** implement sqlite-vec extension loading and validation (TASK-2.3) ([d0e15f1](https://github.com/garnizeh/memex/commit/d0e15f13f69be66a6d7adb24eec00802500c6b47))
* **storage:** implement StorageReader with vector KNN search and relational queries (TASK-2.5) ([06006f2](https://github.com/garnizeh/memex/commit/06006f296a381c2f552f62f256aec440bdf5f61f))
* **storage:** implement transactional StorageWriter and cascade deletions (TASK-2.4) ([6e40422](https://github.com/garnizeh/memex/commit/6e404229f67b8b3a53140ec3f30a3b2da25aeaf9))
* **tests:** implement CI token reduction efficiency gate (TASK-9.5) ([ee74bf0](https://github.com/garnizeh/memex/commit/ee74bf0950c142b3628884baac9ebab7a8ea289f))
* **tests:** implement integration test suite (TASK-9.3) ([6bfd1be](https://github.com/garnizeh/memex/commit/6bfd1be2fd7f8fd44df8ff8fb0f21fff146cb2e6))
* **tests:** setup static test fixture corpus and test runner (TASK-9.1) ([11dabfa](https://github.com/garnizeh/memex/commit/11dabfa9201caaeb6b5c57a27b6abda6f6975e39))


### Bug Fixes

* **chunker:** resolve collapsible-if clippy warning in slugify ([11fe250](https://github.com/garnizeh/memex/commit/11fe2509ed1c1d0ef3d1993b1bc82c1afb167a56))
* **ci:** add always() guarded trigger condition to publish-release job ([362f626](https://github.com/garnizeh/memex/commit/362f6261063a0a6c991697a763402b25db641f45))
* **ci:** address review comments on actions versions, runner images, git hooks and tests ([41a9e9e](https://github.com/garnizeh/memex/commit/41a9e9e87bfa0659d44236f93f35d29c729be640))
* **ci:** integrate release-please with automated multi-platform binary compilation and publication ([53dc9b3](https://github.com/garnizeh/memex/commit/53dc9b3395aa5cd6041051ce386a96340a5c39e7))
* **cli:** remove unused duplex import in serve tests ([7dda9dd](https://github.com/garnizeh/memex/commit/7dda9ddc55a15c99d1ed0c491549346e967a19ef))
* **embedder:** synchronize concurrent model asset downloads and handle Windows file locking ([a43c8fd](https://github.com/garnizeh/memex/commit/a43c8fd77e2c481a2ab649d3f3847a511207482d))
* **installer:** use unit struct constructors directly to satisfy clippy ([681ada5](https://github.com/garnizeh/memex/commit/681ada506cc8bb43fb2a0e446b8e8b0c8815e5b9))
* **scripts:** eliminate sed -i call in install-git-hooks.sh for cross-platform BSD/macOS compatibility ([0559f29](https://github.com/garnizeh/memex/commit/0559f294ba22fc76005b92a25326ae62bd54c155))
* **tests:** gate test_git_hooks to unix targets to fix Windows runner execution ([dfacdf7](https://github.com/garnizeh/memex/commit/dfacdf7917564e6cdb39a176abcd171584b353fb))
