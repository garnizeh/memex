//! Transactional storage writer for Memex.
//!
//! Provides atomic, transactional batch writes for documents, chunks,
//! graph edges, and `sqlite-vec` vector embeddings, as well as cascade deletions.

use rusqlite::{params, Connection, Transaction};
use crate::errors::{MemexError, Result};
use crate::models::{Chunk, ChunkType, Document, Edge, EdgeType};
use crate::storage::vec::vector_to_bytes;

/// Converts a [`ChunkType`] into its database string representation.
pub fn chunk_type_to_str(chunk_type: &ChunkType) -> String {
    match chunk_type {
        ChunkType::Heading { level } => format!("heading:{level}"),
        ChunkType::Paragraph => "paragraph".to_string(),
        ChunkType::CodeBlock { language: Some(lang) } => format!("code_block:{lang}"),
        ChunkType::CodeBlock { language: None } => "code_block".to_string(),
        ChunkType::List => "list".to_string(),
    }
}

/// Parses a database string representation into a [`ChunkType`].
pub fn str_to_chunk_type(s: &str) -> Result<ChunkType> {
    if let Some(level_str) = s.strip_prefix("heading:") {
        let level = level_str
            .parse::<u8>()
            .map_err(|_| MemexError::TransactionError(format!("Invalid heading level in chunk_type: {s}")))?;
        return Ok(ChunkType::Heading { level });
    }
    if s == "heading" {
        return Ok(ChunkType::Heading { level: 1 });
    }
    if s == "paragraph" {
        return Ok(ChunkType::Paragraph);
    }
    if let Some(lang) = s.strip_prefix("code_block:") {
        return Ok(ChunkType::CodeBlock {
            language: Some(lang.to_string()),
        });
    }
    if s == "code_block" {
        return Ok(ChunkType::CodeBlock { language: None });
    }
    if s == "list" {
        return Ok(ChunkType::List);
    }

    // Attempt fallback to JSON deserialization
    serde_json::from_str::<ChunkType>(s)
        .map_err(|_| MemexError::TransactionError(format!("Unknown chunk_type: {s}")))
}

/// Converts an [`EdgeType`] into its database string representation.
pub fn edge_type_to_str(edge_type: &EdgeType) -> &'static str {
    match edge_type {
        EdgeType::Hierarchy => "hierarchy",
        EdgeType::ExplicitLink => "explicit_link",
    }
}

/// Parses a database string representation into an [`EdgeType`].
pub fn str_to_edge_type(s: &str) -> Result<EdgeType> {
    match s {
        "hierarchy" => Ok(EdgeType::Hierarchy),
        "explicit_link" | "explicit" => Ok(EdgeType::ExplicitLink),
        other => serde_json::from_str::<EdgeType>(other)
            .map_err(|_| MemexError::TransactionError(format!("Unknown edge_type: {other}"))),
    }
}

/// Transactional writer for Memex database operations.
///
/// Wraps a SQLite connection and executes atomic batch operations for documents,
/// chunks, graph edges, and vector embeddings.
pub struct StorageWriter<'a> {
    conn: &'a mut Connection,
}

impl<'a> StorageWriter<'a> {
    /// Creates a new [`StorageWriter`] borrowing a mutable SQLite connection.
    pub fn new(conn: &'a mut Connection) -> Self {
        Self { conn }
    }

    /// Returns a reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        self.conn
    }

    /// Returns a mutable reference to the underlying SQLite connection.
    pub fn conn_mut(&mut self) -> &mut Connection {
        self.conn
    }

    /// Inserts or updates a single [`Document`] in the database within a transaction.
    pub fn insert_document(&mut self, doc: &Document) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::insert_document_tx(&tx, doc)?;
        tx.commit()?;
        Ok(())
    }

    /// Inserts or updates a batch of [`Document`]s within a single transaction.
    pub fn insert_documents_batch(&mut self, docs: &[Document]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for doc in docs {
            Self::insert_document_tx(&tx, doc)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Inserts or updates a batch of [`Chunk`]s within a single transaction.
    pub fn insert_chunks_batch(&mut self, chunks: &[Chunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        Self::insert_chunks_batch_tx(&tx, chunks)?;
        tx.commit()?;
        Ok(())
    }

    /// Inserts or updates a batch of graph [`Edge`]s within a single transaction.
    pub fn insert_edges_batch(&mut self, edges: &[Edge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        Self::insert_edges_batch_tx(&tx, edges)?;
        tx.commit()?;
        Ok(())
    }

    /// Inserts or replaces a batch of vector embeddings in the `vec_chunks` virtual table.
    ///
    /// Each entry consists of `(chunk_id, embedding_slice)`.
    pub fn insert_vectors_batch<S: AsRef<str>, V: AsRef<[f32]>>(
        &mut self,
        vectors: &[(S, V)],
    ) -> Result<()> {
        if vectors.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        Self::insert_vectors_batch_tx(&tx, vectors)?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically writes a complete document bundle: document record, chunks, graph edges,
    /// and vector embeddings in a single atomic transaction.
    ///
    /// If the document already exists, any obsolete chunks/edges/vectors belonging to it
    /// are replaced.
    pub fn save_document_bundle<S: AsRef<str>, V: AsRef<[f32]>>(
        &mut self,
        doc: &Document,
        chunks: &[Chunk],
        edges: &[Edge],
        vectors: &[(S, V)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        // Clean up any existing vector records for chunks belonging to this document
        Self::delete_vectors_for_doc_tx(&tx, &doc.id)?;

        Self::insert_document_tx(&tx, doc)?;
        Self::insert_chunks_batch_tx(&tx, chunks)?;
        Self::insert_edges_batch_tx(&tx, edges)?;
        Self::insert_vectors_batch_tx(&tx, vectors)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes a document and ensures all related relational rows (chunks, edges) and
    /// vector embeddings in `vec_chunks` are cleaned up.
    ///
    /// Returns the number of documents deleted (0 or 1).
    pub fn delete_document(&mut self, doc_id: &str) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let deleted = Self::delete_document_tx(&tx, doc_id)?;
        tx.commit()?;
        Ok(deleted)
    }

    /// Deletes a batch of documents and ensures all associated relational rows and
    /// vector embeddings are completely removed.
    ///
    /// Returns the total number of documents deleted.
    pub fn delete_documents_batch(&mut self, doc_ids: &[&str]) -> Result<usize> {
        if doc_ids.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        let mut total_deleted = 0;
        for &doc_id in doc_ids {
            total_deleted += Self::delete_document_tx(&tx, doc_id)?;
        }
        tx.commit()?;
        Ok(total_deleted)
    }

    // --- Internal Transaction-scoped Operations ---

    /// Internal helper to insert/replace a document within an active transaction.
    pub fn insert_document_tx(tx: &Transaction, doc: &Document) -> Result<()> {
        tx.execute(
            "INSERT INTO documents (id, file_path, title, content_hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 file_path = excluded.file_path,
                 title = excluded.title,
                 content_hash = excluded.content_hash,
                 indexed_at = excluded.indexed_at;",
            params![
                doc.id,
                doc.file_path,
                doc.title,
                doc.content_hash,
                doc.indexed_at,
            ],
        )?;
        Ok(())
    }

    /// Internal helper to insert/replace a batch of chunks within an active transaction.
    pub fn insert_chunks_batch_tx(tx: &Transaction, chunks: &[Chunk]) -> Result<()> {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO chunks (
                id, doc_id, parent_chunk_id, chunk_type, heading_path,
                content, contextual_content, line_start, line_end
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                 doc_id = excluded.doc_id,
                 parent_chunk_id = excluded.parent_chunk_id,
                 chunk_type = excluded.chunk_type,
                 heading_path = excluded.heading_path,
                 content = excluded.content,
                 contextual_content = excluded.contextual_content,
                 line_start = excluded.line_start,
                 line_end = excluded.line_end;",
        )?;

        for chunk in chunks {
            let chunk_type_str = chunk_type_to_str(&chunk.chunk_type);
            let heading_path_json = serde_json::to_string(&chunk.heading_path)?;

            stmt.execute(params![
                chunk.id,
                chunk.doc_id,
                chunk.parent_chunk_id,
                chunk_type_str,
                heading_path_json,
                chunk.content,
                chunk.contextual_content,
                chunk.line_start,
                chunk.line_end,
            ])?;
        }

        Ok(())
    }

    /// Internal helper to insert/replace a batch of graph edges within an active transaction.
    pub fn insert_edges_batch_tx(tx: &Transaction, edges: &[Edge]) -> Result<()> {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO edges (source_chunk_id, target_chunk_id, edge_type, link_text)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source_chunk_id, target_chunk_id, edge_type) DO UPDATE SET
                 link_text = excluded.link_text;",
        )?;

        for edge in edges {
            let edge_type_str = edge_type_to_str(&edge.edge_type);
            stmt.execute(params![
                edge.source_chunk_id,
                edge.target_chunk_id,
                edge_type_str,
                edge.link_text,
            ])?;
        }

        Ok(())
    }

    /// Internal helper to insert/replace a batch of vector embeddings within an active transaction.
    pub fn insert_vectors_batch_tx<S: AsRef<str>, V: AsRef<[f32]>>(
        tx: &Transaction,
        vectors: &[(S, V)],
    ) -> Result<()> {
        let mut del_stmt = tx.prepare_cached(
            "DELETE FROM vec_chunks WHERE chunk_id = ?1;",
        )?;
        let mut ins_stmt = tx.prepare_cached(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?1, ?2);",
        )?;

        for (chunk_id, embedding) in vectors {
            let id = chunk_id.as_ref();
            let emb_slice = embedding.as_ref();
            let bytes = vector_to_bytes(emb_slice);

            // Delete any existing vector row for this chunk_id before inserting
            del_stmt.execute(params![id])?;
            ins_stmt.execute(params![id, bytes])?;
        }

        Ok(())
    }

    /// Internal helper to delete vector rows for all chunks belonging to a document.
    pub fn delete_vectors_for_doc_tx(tx: &Transaction, doc_id: &str) -> Result<()> {
        tx.execute(
            "DELETE FROM vec_chunks WHERE chunk_id IN (SELECT id FROM chunks WHERE doc_id = ?1);",
            params![doc_id],
        )?;
        Ok(())
    }

    /// Internal helper to delete a document and cascade clean chunks, edges, and vectors.
    pub fn delete_document_tx(tx: &Transaction, doc_id: &str) -> Result<usize> {
        // 1. Delete vector records in vec0 virtual table
        Self::delete_vectors_for_doc_tx(tx, doc_id)?;

        // 2. Delete edges referencing chunks of this doc (in case FK cascades are disabled or for safety)
        tx.execute(
            "DELETE FROM edges WHERE source_chunk_id IN (SELECT id FROM chunks WHERE doc_id = ?1)
             OR target_chunk_id IN (SELECT id FROM chunks WHERE doc_id = ?1);",
            params![doc_id],
        )?;

        // 3. Delete chunks belonging to this doc
        tx.execute(
            "DELETE FROM chunks WHERE doc_id = ?1;",
            params![doc_id],
        )?;

        // 4. Delete the document record
        let deleted = tx.execute(
            "DELETE FROM documents WHERE id = ?1;",
            params![doc_id],
        )?;

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::Database;
    use crate::storage::schema::initialize_schema;

    fn setup_test_db() -> Database {
        let db = Database::open_in_memory().expect("failed to open in-memory db");
        initialize_schema(db.conn()).expect("failed to initialize schema");
        db
    }

    #[test]
    fn test_chunk_type_conversion_roundtrip() {
        let types = vec![
            ChunkType::Heading { level: 1 },
            ChunkType::Heading { level: 4 },
            ChunkType::Paragraph,
            ChunkType::CodeBlock {
                language: Some("rust".to_string()),
            },
            ChunkType::CodeBlock { language: None },
            ChunkType::List,
        ];

        for ct in types {
            let s = chunk_type_to_str(&ct);
            let parsed = str_to_chunk_type(&s).expect("failed to parse chunk type");
            assert_eq!(ct, parsed);
        }
    }

    #[test]
    fn test_edge_type_conversion_roundtrip() {
        let types = vec![EdgeType::Hierarchy, EdgeType::ExplicitLink];

        for et in types {
            let s = edge_type_to_str(&et);
            let parsed = str_to_edge_type(s).expect("failed to parse edge type");
            assert_eq!(et, parsed);
        }
    }

    #[test]
    fn test_insert_document_and_update() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        let doc = Document {
            id: "doc-1".to_string(),
            file_path: "docs/architecture.md".to_string(),
            title: Some("Architecture".to_string()),
            content_hash: "hash-v1".to_string(),
            indexed_at: 1700000000,
        };

        writer.insert_document(&doc).expect("insert document failed");

        let count: i64 = writer
            .conn()
            .query_row("SELECT count(*) FROM documents WHERE id = 'doc-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        // Update same document with new content hash
        let updated_doc = Document {
            id: "doc-1".to_string(),
            file_path: "docs/architecture.md".to_string(),
            title: Some("Architecture V2".to_string()),
            content_hash: "hash-v2".to_string(),
            indexed_at: 1700000100,
        };

        writer
            .insert_document(&updated_doc)
            .expect("update document failed");

        let (title, content_hash): (String, String) = writer
            .conn()
            .query_row(
                "SELECT title, content_hash FROM documents WHERE id = 'doc-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Architecture V2");
        assert_eq!(content_hash, "hash-v2");
    }

    #[test]
    fn test_insert_chunks_batch() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        let doc = Document {
            id: "doc-1".to_string(),
            file_path: "docs/test.md".to_string(),
            title: Some("Test".to_string()),
            content_hash: "hash-1".to_string(),
            indexed_at: 1700000000,
        };
        writer.insert_document(&doc).unwrap();

        let chunks = vec![
            Chunk {
                id: "c-1".to_string(),
                doc_id: "doc-1".to_string(),
                parent_chunk_id: None,
                chunk_type: ChunkType::Heading { level: 1 },
                heading_path: vec!["Test".to_string()],
                content: "# Test".to_string(),
                contextual_content: "# Test".to_string(),
                line_start: 1,
                line_end: 1,
            },
            Chunk {
                id: "c-2".to_string(),
                doc_id: "doc-1".to_string(),
                parent_chunk_id: Some("c-1".to_string()),
                chunk_type: ChunkType::Paragraph,
                heading_path: vec!["Test".to_string()],
                content: "Paragraph content".to_string(),
                contextual_content: "[Test] Paragraph content".to_string(),
                line_start: 3,
                line_end: 5,
            },
        ];

        writer.insert_chunks_batch(&chunks).expect("insert chunks failed");

        let count: i64 = writer
            .conn()
            .query_row("SELECT count(*) FROM chunks WHERE doc_id = 'doc-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_insert_edges_batch() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        let doc = Document {
            id: "doc-1".to_string(),
            file_path: "docs/test.md".to_string(),
            title: Some("Test".to_string()),
            content_hash: "hash-1".to_string(),
            indexed_at: 1700000000,
        };
        writer.insert_document(&doc).unwrap();

        let chunks = vec![
            Chunk {
                id: "c-1".to_string(),
                doc_id: "doc-1".to_string(),
                parent_chunk_id: None,
                chunk_type: ChunkType::Heading { level: 1 },
                heading_path: vec!["Test".to_string()],
                content: "# Test".to_string(),
                contextual_content: "# Test".to_string(),
                line_start: 1,
                line_end: 1,
            },
            Chunk {
                id: "c-2".to_string(),
                doc_id: "doc-1".to_string(),
                parent_chunk_id: Some("c-1".to_string()),
                chunk_type: ChunkType::Paragraph,
                heading_path: vec!["Test".to_string()],
                content: "Paragraph".to_string(),
                contextual_content: "[Test] Paragraph".to_string(),
                line_start: 2,
                line_end: 3,
            },
        ];
        writer.insert_chunks_batch(&chunks).unwrap();

        let edges = vec![Edge {
            source_chunk_id: "c-1".to_string(),
            target_chunk_id: "c-2".to_string(),
            edge_type: EdgeType::Hierarchy,
            link_text: None,
        }];

        writer.insert_edges_batch(&edges).expect("insert edges failed");

        let count: i64 = writer
            .conn()
            .query_row("SELECT count(*) FROM edges WHERE source_chunk_id = 'c-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_vectors_batch() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        let vec1: Vec<f32> = vec![0.1; 384];
        let vec2: Vec<f32> = vec![0.2; 384];

        let vectors = vec![
            ("c-1", &vec1[..]),
            ("c-2", &vec2[..]),
        ];

        writer.insert_vectors_batch(&vectors).expect("insert vectors failed");

        let count: i64 = writer
            .conn()
            .query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // Test replacing an existing vector
        let updated_vec1: Vec<f32> = vec![0.9; 384];
        let update_vectors = vec![("c-1", &updated_vec1[..])];
        writer.insert_vectors_batch(&update_vectors).unwrap();

        let count_after: i64 = writer
            .conn()
            .query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_after, 2);
    }

    #[test]
    fn test_cascade_deletion_of_chunks_edges_and_vectors() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        // 1. Insert Document
        let doc = Document {
            id: "doc-to-delete".to_string(),
            file_path: "docs/delete_me.md".to_string(),
            title: Some("Delete Me".to_string()),
            content_hash: "hash-del".to_string(),
            indexed_at: 1700000000,
        };
        writer.insert_document(&doc).unwrap();

        // 2. Insert Chunks
        let chunks = vec![
            Chunk {
                id: "del-c1".to_string(),
                doc_id: "doc-to-delete".to_string(),
                parent_chunk_id: None,
                chunk_type: ChunkType::Heading { level: 1 },
                heading_path: vec!["Delete Me".to_string()],
                content: "# Delete Me".to_string(),
                contextual_content: "# Delete Me".to_string(),
                line_start: 1,
                line_end: 1,
            },
            Chunk {
                id: "del-c2".to_string(),
                doc_id: "doc-to-delete".to_string(),
                parent_chunk_id: Some("del-c1".to_string()),
                chunk_type: ChunkType::Paragraph,
                heading_path: vec!["Delete Me".to_string()],
                content: "Content to delete".to_string(),
                contextual_content: "[Delete Me] Content to delete".to_string(),
                line_start: 2,
                line_end: 4,
            },
        ];
        writer.insert_chunks_batch(&chunks).unwrap();

        // 3. Insert Edges
        let edges = vec![Edge {
            source_chunk_id: "del-c1".to_string(),
            target_chunk_id: "del-c2".to_string(),
            edge_type: EdgeType::Hierarchy,
            link_text: None,
        }];
        writer.insert_edges_batch(&edges).unwrap();

        // 4. Insert Vectors
        let dummy_vec: Vec<f32> = vec![0.5; 384];
        let vectors = vec![
            ("del-c1", &dummy_vec[..]),
            ("del-c2", &dummy_vec[..]),
        ];
        writer.insert_vectors_batch(&vectors).unwrap();

        // Verify initial counts
        let doc_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM documents WHERE id = 'doc-to-delete'", [], |r| r.get(0)).unwrap();
        let chunk_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM chunks WHERE doc_id = 'doc-to-delete'", [], |r| r.get(0)).unwrap();
        let edge_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM edges WHERE source_chunk_id = 'del-c1'", [], |r| r.get(0)).unwrap();
        let vec_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id IN ('del-c1', 'del-c2')", [], |r| r.get(0)).unwrap();

        assert_eq!(doc_cnt, 1);
        assert_eq!(chunk_cnt, 2);
        assert_eq!(edge_cnt, 1);
        assert_eq!(vec_cnt, 2);

        // 5. Delete document
        let deleted = writer.delete_document("doc-to-delete").expect("delete_document failed");
        assert_eq!(deleted, 1);

        // Assert all cascading records are gone
        let doc_cnt_after: i64 = writer.conn().query_row("SELECT count(*) FROM documents WHERE id = 'doc-to-delete'", [], |r| r.get(0)).unwrap();
        let chunk_cnt_after: i64 = writer.conn().query_row("SELECT count(*) FROM chunks WHERE doc_id = 'doc-to-delete'", [], |r| r.get(0)).unwrap();
        let edge_cnt_after: i64 = writer.conn().query_row("SELECT count(*) FROM edges WHERE source_chunk_id = 'del-c1' OR target_chunk_id = 'del-c2'", [], |r| r.get(0)).unwrap();
        let vec_cnt_after: i64 = writer.conn().query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id IN ('del-c1', 'del-c2')", [], |r| r.get(0)).unwrap();

        assert_eq!(doc_cnt_after, 0, "document should be deleted");
        assert_eq!(chunk_cnt_after, 0, "chunks should be cascade deleted");
        assert_eq!(edge_cnt_after, 0, "edges should be cascade deleted");
        assert_eq!(vec_cnt_after, 0, "vectors in vec_chunks should be deleted");
    }

    #[test]
    fn test_save_document_bundle() {
        let mut db = setup_test_db();
        let mut writer = StorageWriter::new(db.conn_mut());

        let doc = Document {
            id: "bundle-doc".to_string(),
            file_path: "docs/bundle.md".to_string(),
            title: Some("Bundle".to_string()),
            content_hash: "bundle-hash".to_string(),
            indexed_at: 1700000000,
        };

        let chunks = vec![Chunk {
            id: "b-c1".to_string(),
            doc_id: "bundle-doc".to_string(),
            parent_chunk_id: None,
            chunk_type: ChunkType::Heading { level: 1 },
            heading_path: vec!["Bundle".to_string()],
            content: "# Bundle".to_string(),
            contextual_content: "# Bundle".to_string(),
            line_start: 1,
            line_end: 1,
        }];

        let edges = vec![];
        let vec_emb = vec![0.3f32; 384];
        let vectors = vec![("b-c1", &vec_emb[..])];

        writer
            .save_document_bundle(&doc, &chunks, &edges, &vectors)
            .expect("save_document_bundle should succeed");

        let doc_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM documents WHERE id = 'bundle-doc'", [], |r| r.get(0)).unwrap();
        let chunk_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM chunks WHERE doc_id = 'bundle-doc'", [], |r| r.get(0)).unwrap();
        let vec_cnt: i64 = writer.conn().query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 'b-c1'", [], |r| r.get(0)).unwrap();

        assert_eq!(doc_cnt, 1);
        assert_eq!(chunk_cnt, 1);
        assert_eq!(vec_cnt, 1);
    }
}
