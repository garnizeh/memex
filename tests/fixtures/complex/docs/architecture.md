# System Architecture

Detailed system overview and component interactions.

## Core Components

The system consists of ingestion, indexing, and serving layers.
Refer to [Quickstart Guide](../guides/quickstart.md) for running the components.

### Ingestion Engine

Responsible for AST parsing, section hierarchy building, and semantic chunking.

### Storage Subsystem

Manages SQLite database, relational tables, and vector virtual tables.

### MCP Interface

Provides JSON-RPC stdio protocol access. Check [Endpoints](../api/endpoints.md) and [Authentication](../api/auth.md).
