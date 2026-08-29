//! SQLite schema definitions and initialization for Memex.
//!
//! Provides the relational graph schema (`documents`, `chunks`, `edges`, indices)
//! and the `sqlite-vec` virtual table (`vec_chunks`) for vector similarity search.

use rusqlite::Connection;
use crate::errors::Result;

/// SQL schema definition containing all relational tables, indices, and vector virtual tables.
pub const SCHEMA_SQL: &str = r#"
-- Relational Tables (The Graph)
CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL UNIQUE,
    title TEXT,
    content_hash TEXT NOT NULL,
    indexed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    id TEXT PRIMARY KEY,
    doc_id TEXT NOT NULL,
    parent_chunk_id TEXT,
    chunk_type TEXT NOT NULL,
    heading_path TEXT NOT NULL,
    content TEXT NOT NULL,
    contextual_content TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    FOREIGN KEY (doc_id) REFERENCES documents(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_chunk_id) REFERENCES chunks(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS edges (
    source_chunk_id TEXT NOT NULL,
    target_chunk_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    link_text TEXT,
    PRIMARY KEY (source_chunk_id, target_chunk_id, edge_type),
    FOREIGN KEY (source_chunk_id) REFERENCES chunks(id) ON DELETE CASCADE,
    FOREIGN KEY (target_chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
);

-- Indices for fast graph traversal
CREATE INDEX IF NOT EXISTS idx_chunks_doc ON chunks(doc_id);
CREATE INDEX IF NOT EXISTS idx_chunks_parent ON chunks(parent_chunk_id);
CREATE INDEX IF NOT EXISTS idx_chunks_type ON chunks(chunk_type);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_chunk_id);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);

-- Vector Table (sqlite-vec)
CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding FLOAT[384]
);
"#;

pub use crate::storage::vec::ensure_sqlite_vec;


/// Initializes the database schema on the provided SQLite connection.
///
/// Creates all required relational tables (`documents`, `chunks`, `edges`),
/// performance indices, and the `sqlite-vec` virtual table (`vec_chunks`).
///
/// This operation is idempotent and safe to call multiple times.
pub fn initialize_schema(conn: &Connection) -> Result<()> {
    ensure_sqlite_vec(conn)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_initialize_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();

        // First initialization
        initialize_schema(&conn).expect("initial schema initialization should succeed");

        // Second initialization (idempotency check)
        initialize_schema(&conn).expect("second schema initialization should succeed without error");

        // Verify tables in sqlite_master
        let mut stmt = conn
            .prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table', 'index');")
            .unwrap();

        let entries: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let table_names: HashSet<String> = entries
            .iter()
            .filter(|(_, t)| t == "table")
            .map(|(n, _)| n.clone())
            .collect();

        assert!(table_names.contains("documents"), "documents table missing");
        assert!(table_names.contains("chunks"), "chunks table missing");
        assert!(table_names.contains("edges"), "edges table missing");
        assert!(table_names.contains("vec_chunks"), "vec_chunks table missing");

        let index_names: HashSet<String> = entries
            .iter()
            .filter(|(_, t)| t == "index")
            .map(|(n, _)| n.clone())
            .collect();

        assert!(index_names.contains("idx_chunks_doc"), "idx_chunks_doc index missing");
        assert!(index_names.contains("idx_chunks_parent"), "idx_chunks_parent index missing");
        assert!(index_names.contains("idx_chunks_type"), "idx_chunks_type index missing");
        assert!(index_names.contains("idx_edges_target"), "idx_edges_target index missing");
        assert!(index_names.contains("idx_edges_type"), "idx_edges_type index missing");
    }

    #[test]
    fn test_schema_foreign_keys_and_cascades() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        initialize_schema(&conn).unwrap();

        // Insert document
        conn.execute(
            "INSERT INTO documents (id, file_path, title, content_hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("doc-1", "docs/test.md", "Test Doc", "hash123", 1700000000),
        ).unwrap();

        // Insert chunks
        conn.execute(
            "INSERT INTO chunks (id, doc_id, parent_chunk_id, chunk_type, heading_path, content, contextual_content, line_start, line_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            ("chunk-h1", "doc-1", None::<String>, "heading:1", "[\"Title\"]", "Title", "Title", 1, 1),
        ).unwrap();

        conn.execute(
            "INSERT INTO chunks (id, doc_id, parent_chunk_id, chunk_type, heading_path, content, contextual_content, line_start, line_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            ("chunk-p1", "doc-1", Some("chunk-h1"), "paragraph", "[\"Title\"]", "Content", "[Title] Content", 2, 5),
        ).unwrap();

        // Insert edge
        conn.execute(
            "INSERT INTO edges (source_chunk_id, target_chunk_id, edge_type, link_text)
             VALUES (?1, ?2, ?3, ?4)",
            ("chunk-h1", "chunk-p1", "hierarchy", None::<String>),
        ).unwrap();

        // Verify count
        let doc_count: i64 = conn.query_row("SELECT count(*) FROM documents", [], |r| r.get(0)).unwrap();
        assert_eq!(doc_count, 1);
        let chunk_count: i64 = conn.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(chunk_count, 2);
        let edge_count: i64 = conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0)).unwrap();
        assert_eq!(edge_count, 1);

        // Delete document -> verify CASCADE deletes chunks and edges
        conn.execute("DELETE FROM documents WHERE id = 'doc-1'", []).unwrap();

        let doc_count_after: i64 = conn.query_row("SELECT count(*) FROM documents", [], |r| r.get(0)).unwrap();
        assert_eq!(doc_count_after, 0);
        let chunk_count_after: i64 = conn.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(chunk_count_after, 0);
        let edge_count_after: i64 = conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0)).unwrap();
        assert_eq!(edge_count_after, 0);
    }

    #[test]
    fn test_vec_chunks_insert_and_query() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Test inserting a 384-dimensional vector into vec_chunks
        let dummy_vec: Vec<f32> = (0..384).map(|i| i as f32 / 384.0).collect();
        let serialized_vec: Vec<u8> = dummy_vec
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?1, ?2)",
            rusqlite::params!["chunk-1", serialized_vec],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 'chunk-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
