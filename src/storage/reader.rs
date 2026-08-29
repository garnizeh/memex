//! Relational reader and vector KNN search operations for Memex.
//!
//! Provides read-only query capabilities for documents, chunks, graph edges,
//! and `sqlite-vec` vector similarity search (`search_knn`).

use crate::errors::Result;
use crate::models::{Chunk, Document, Edge};
use crate::storage::vec::vector_to_bytes;
use crate::storage::writer::{str_to_chunk_type, str_to_edge_type};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

type RawChunkTuple = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    u32,
    u32,
);

type RawEdgeTuple = (String, String, String, Option<String>);

/// Result of a vector semantic similarity search (KNN query).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// The documentation chunk matched by semantic similarity.
    pub chunk: Chunk,
    /// The relative file path of the document containing this chunk.
    pub file_path: String,
    /// The title of the document containing this chunk, if available.
    pub document_title: Option<String>,
    /// The raw L2 vector distance computed by `sqlite-vec` (lower is closer).
    pub distance: f32,
    /// Semantic similarity score in the range [0.0, 1.0], derived from distance.
    pub score: f32,
}

impl SearchResult {
    /// Computes a normalized similarity score in [0.0, 1.0] from an L2 distance
    /// assuming L2-normalized unit vectors where distance is in [0, 2].
    ///
    /// For unit vectors: cosine_similarity = 1 - (distance^2 / 2).
    #[inline]
    pub fn score_from_distance(distance: f32) -> f32 {
        let cos_sim = 1.0 - (distance * distance / 2.0);
        cos_sim.clamp(0.0, 1.0)
    }
}

/// Read-only storage reader for querying documents, chunks, graph edges, and vector indices.
pub struct StorageReader<'a> {
    conn: &'a Connection,
}

impl<'a> StorageReader<'a> {
    /// Creates a new [`StorageReader`] borrowing a SQLite connection.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Returns a reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        self.conn
    }

    /// Performs a K-Nearest-Neighbors (KNN) vector similarity search over chunk embeddings.
    ///
    /// Matches the query `embedding` against the `vec_chunks` virtual table using `sqlite-vec`,
    /// joining with `chunks` and `documents` to construct full [`SearchResult`] records
    /// sorted in ascending order of vector distance.
    pub fn search_knn(&self, embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        if limit == 0 || embedding.is_empty() {
            return Ok(Vec::new());
        }

        let vector_bytes = vector_to_bytes(embedding);

        let mut stmt = self.conn.prepare_cached(
            "SELECT
                v.distance,
                c.id,
                c.doc_id,
                c.parent_chunk_id,
                c.chunk_type,
                c.heading_path,
                c.content,
                c.contextual_content,
                c.line_start,
                c.line_end,
                d.file_path,
                d.title
            FROM (
                SELECT chunk_id, distance
                FROM vec_chunks
                WHERE embedding MATCH ?1
                ORDER BY distance
                LIMIT ?2
            ) v
            JOIN chunks c ON c.id = v.chunk_id
            JOIN documents d ON d.id = c.doc_id
            ORDER BY v.distance ASC;",
        )?;

        let rows = stmt.query_map(params![vector_bytes, limit as i64], |row| {
            let distance: f32 = row.get(0)?;
            let chunk_id: String = row.get(1)?;
            let doc_id: String = row.get(2)?;
            let parent_chunk_id: Option<String> = row.get(3)?;
            let chunk_type_raw: String = row.get(4)?;
            let heading_path_raw: String = row.get(5)?;
            let content: String = row.get(6)?;
            let contextual_content: String = row.get(7)?;
            let line_start: u32 = row.get(8)?;
            let line_end: u32 = row.get(9)?;
            let file_path: String = row.get(10)?;
            let document_title: Option<String> = row.get(11)?;

            Ok((
                distance,
                chunk_id,
                doc_id,
                parent_chunk_id,
                chunk_type_raw,
                heading_path_raw,
                content,
                contextual_content,
                line_start,
                line_end,
                file_path,
                document_title,
            ))
        })?;

        let mut results = Vec::new();
        for row_res in rows {
            let (
                distance,
                chunk_id,
                doc_id,
                parent_chunk_id,
                chunk_type_raw,
                heading_path_raw,
                content,
                contextual_content,
                line_start,
                line_end,
                file_path,
                document_title,
            ) = row_res?;

            let chunk_type = str_to_chunk_type(&chunk_type_raw)?;
            let heading_path: Vec<String> = serde_json::from_str(&heading_path_raw)?;

            let chunk = Chunk {
                id: chunk_id,
                doc_id,
                parent_chunk_id,
                chunk_type,
                heading_path,
                content,
                contextual_content,
                line_start,
                line_end,
            };

            let score = SearchResult::score_from_distance(distance);

            results.push(SearchResult {
                chunk,
                file_path,
                document_title,
                distance,
                score,
            });
        }

        Ok(results)
    }

    /// Retrieves a [`Document`] by its unique ID.
    pub fn get_document(&self, doc_id: &str) -> Result<Option<Document>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, content_hash, indexed_at
             FROM documents WHERE id = ?1;",
        )?;

        let mut rows = stmt.query(params![doc_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_document(row)?))
        } else {
            Ok(None)
        }
    }

    /// Retrieves a [`Document`] by its relative file path.
    pub fn get_document_by_path(&self, file_path: &str) -> Result<Option<Document>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, content_hash, indexed_at
             FROM documents WHERE file_path = ?1;",
        )?;

        let mut rows = stmt.query(params![file_path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_document(row)?))
        } else {
            Ok(None)
        }
    }

    /// Retrieves all indexed [`Document`]s ordered by file path.
    pub fn get_all_documents(&self) -> Result<Vec<Document>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, content_hash, indexed_at
             FROM documents ORDER BY file_path ASC;",
        )?;

        let rows = stmt.query_map([], Self::row_to_document)?;
        let mut docs = Vec::new();
        for doc in rows {
            docs.push(doc?);
        }
        Ok(docs)
    }

    /// Retrieves a single [`Chunk`] by its unique ID.
    pub fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, doc_id, parent_chunk_id, chunk_type, heading_path,
                    content, contextual_content, line_start, line_end
             FROM chunks WHERE id = ?1;",
        )?;

        let mut rows = stmt.query(params![chunk_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_chunk(row)?))
        } else {
            Ok(None)
        }
    }

    /// Retrieves all [`Chunk`]s belonging to a document, ordered by start line.
    pub fn get_chunks_for_document(&self, doc_id: &str) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, doc_id, parent_chunk_id, chunk_type, heading_path,
                    content, contextual_content, line_start, line_end
             FROM chunks WHERE doc_id = ?1 ORDER BY line_start ASC;",
        )?;

        let rows = stmt.query_map(params![doc_id], Self::row_to_chunk_raw)?;
        let mut chunks = Vec::new();
        for raw in rows {
            chunks.push(Self::parse_chunk_tuple(raw?)?);
        }
        Ok(chunks)
    }

    /// Retrieves all outbound [`Edge`]s from a source chunk.
    pub fn get_edges_for_source(&self, source_chunk_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT source_chunk_id, target_chunk_id, edge_type, link_text
             FROM edges WHERE source_chunk_id = ?1;",
        )?;

        let rows = stmt.query_map(params![source_chunk_id], Self::row_to_edge_raw)?;
        let mut edges = Vec::new();
        for raw in rows {
            edges.push(Self::parse_edge_tuple(raw?)?);
        }
        Ok(edges)
    }

    /// Retrieves all inbound [`Edge`]s into a target chunk.
    pub fn get_edges_for_target(&self, target_chunk_id: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT source_chunk_id, target_chunk_id, edge_type, link_text
             FROM edges WHERE target_chunk_id = ?1;",
        )?;

        let rows = stmt.query_map(params![target_chunk_id], Self::row_to_edge_raw)?;
        let mut edges = Vec::new();
        for raw in rows {
            edges.push(Self::parse_edge_tuple(raw?)?);
        }
        Ok(edges)
    }

    /// Returns the total count of documents in the database.
    pub fn count_documents(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM documents;", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Returns the total count of chunks in the database.
    pub fn count_chunks(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM chunks;", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Returns the total count of graph edges in the database.
    pub fn count_edges(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM edges;", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Returns the total count of vector embeddings in `vec_chunks`.
    pub fn count_vectors(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM vec_chunks;", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    // --- Helper deserialization methods ---

    fn row_to_document(row: &Row) -> rusqlite::Result<Document> {
        Ok(Document {
            id: row.get(0)?,
            file_path: row.get(1)?,
            title: row.get(2)?,
            content_hash: row.get(3)?,
            indexed_at: row.get(4)?,
        })
    }

    fn row_to_chunk_raw(row: &Row) -> rusqlite::Result<RawChunkTuple> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
        ))
    }

    fn parse_chunk_tuple(raw: RawChunkTuple) -> Result<Chunk> {
        let (
            id,
            doc_id,
            parent_chunk_id,
            chunk_type_raw,
            heading_path_raw,
            content,
            contextual_content,
            line_start,
            line_end,
        ) = raw;

        let chunk_type = str_to_chunk_type(&chunk_type_raw)?;
        let heading_path: Vec<String> = serde_json::from_str(&heading_path_raw)?;

        Ok(Chunk {
            id,
            doc_id,
            parent_chunk_id,
            chunk_type,
            heading_path,
            content,
            contextual_content,
            line_start,
            line_end,
        })
    }

    fn row_to_chunk(row: &Row) -> Result<Chunk> {
        let raw = Self::row_to_chunk_raw(row)?;
        Self::parse_chunk_tuple(raw)
    }

    fn row_to_edge_raw(row: &Row) -> rusqlite::Result<RawEdgeTuple> {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    }

    fn parse_edge_tuple(raw: RawEdgeTuple) -> Result<Edge> {
        let (source_chunk_id, target_chunk_id, edge_type_raw, link_text) = raw;
        let edge_type = str_to_edge_type(&edge_type_raw)?;

        Ok(Edge {
            source_chunk_id,
            target_chunk_id,
            edge_type,
            link_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChunkType, EdgeType};
    use crate::storage::db::Database;
    use crate::storage::schema::initialize_schema;

    /// Helper creating an initialized in-memory database with populated documents, chunks, edges, and vectors.
    fn setup_test_db() -> Database {
        let mut db = Database::open_in_memory().unwrap();
        initialize_schema(db.conn()).unwrap();

        let doc1 = Document {
            id: "doc-auth".to_string(),
            file_path: "docs/auth.md".to_string(),
            title: Some("Authentication Guide".to_string()),
            content_hash: "hash_auth_123".to_string(),
            indexed_at: 1700000000,
        };
        let doc2 = Document {
            id: "doc-api".to_string(),
            file_path: "docs/api.md".to_string(),
            title: Some("API Reference".to_string()),
            content_hash: "hash_api_456".to_string(),
            indexed_at: 1700000010,
        };

        // Create 10 chunks across the two documents
        let mut chunks = Vec::new();
        let mut vectors = Vec::new();

        for i in 0..10 {
            let doc_id = if i < 6 { "doc-auth" } else { "doc-api" };
            let chunk_id = format!("chunk-{i:02}");
            let heading = if i < 6 { "Authentication" } else { "API" };

            let chunk = Chunk {
                id: chunk_id.clone(),
                doc_id: doc_id.to_string(),
                parent_chunk_id: if i == 0 || i == 6 {
                    None
                } else {
                    Some(format!("chunk-{:02}", if i < 6 { 0 } else { 6 }))
                },
                chunk_type: if i == 0 || i == 6 {
                    ChunkType::Heading { level: 1 }
                } else if i % 2 == 0 {
                    ChunkType::CodeBlock {
                        language: Some("rust".to_string()),
                    }
                } else {
                    ChunkType::Paragraph
                },
                heading_path: vec![heading.to_string(), format!("Section {i}")],
                content: format!("Content for chunk {i}"),
                contextual_content: format!("[{heading} > Section {i}] Content for chunk {i}"),
                line_start: (i as u32 * 10) + 1,
                line_end: (i as u32 * 10) + 9,
            };
            chunks.push(chunk);

            // Construct 384-dimensional synthetic unit vectors:
            // Put 1.0 at index `i` so each vector is orthogonal and distance is cleanly predictable
            let mut vec = vec![0.0f32; 384];
            vec[i] = 1.0;
            vectors.push((chunk_id, vec));
        }

        let edges = vec![
            Edge {
                source_chunk_id: "chunk-00".to_string(),
                target_chunk_id: "chunk-01".to_string(),
                edge_type: EdgeType::Hierarchy,
                link_text: None,
            },
            Edge {
                source_chunk_id: "chunk-01".to_string(),
                target_chunk_id: "chunk-06".to_string(),
                edge_type: EdgeType::ExplicitLink,
                link_text: Some("See API Reference".to_string()),
            },
        ];

        let mut writer = db.writer();
        writer
            .insert_documents_batch(&[doc1, doc2])
            .expect("Failed to insert documents");
        writer
            .insert_chunks_batch(&chunks)
            .expect("Failed to insert chunks");
        writer
            .insert_edges_batch(&edges)
            .expect("Failed to insert edges");
        writer
            .insert_vectors_batch(&vectors)
            .expect("Failed to insert vectors");

        db
    }

    #[test]
    fn test_search_knn_returns_sorted_nearest_neighbors() {
        let db = setup_test_db();
        let reader = db.reader();

        // Query vector exactly matching chunk-03 (1.0 at index 3)
        let mut query_vec = vec![0.0f32; 384];
        query_vec[3] = 1.0;

        let results = reader
            .search_knn(&query_vec, 5)
            .expect("KNN search should succeed");

        assert_eq!(results.len(), 5);

        // First result must be chunk-03 with distance ~ 0.0 and score ~ 1.0
        assert_eq!(results[0].chunk.id, "chunk-03");
        assert_eq!(results[0].file_path, "docs/auth.md");
        assert_eq!(
            results[0].document_title,
            Some("Authentication Guide".to_string())
        );
        assert!(
            results[0].distance < 1e-4,
            "Distance of exact match should be ~0, got {}",
            results[0].distance
        );
        assert!(
            (results[0].score - 1.0).abs() < 1e-4,
            "Score of exact match should be ~1.0, got {}",
            results[0].score
        );

        // Subsequent results are orthogonal unit vectors, distance sqrt(2) ~ 1.4142
        for i in 1..results.len() {
            assert!(
                results[i].distance >= results[i - 1].distance,
                "Results must be in ascending distance order"
            );
            assert!(
                results[i].score <= results[i - 1].score,
                "Results must be in descending score order"
            );
        }
    }

    #[test]
    fn test_search_knn_limit_and_empty() {
        let db = setup_test_db();
        let reader = db.reader();

        let mut query_vec = vec![0.0f32; 384];
        query_vec[0] = 1.0;

        // Limit = 3
        let results_3 = reader.search_knn(&query_vec, 3).unwrap();
        assert_eq!(results_3.len(), 3);
        assert_eq!(results_3[0].chunk.id, "chunk-00");

        // Limit = 0 returns empty
        let results_0 = reader.search_knn(&query_vec, 0).unwrap();
        assert!(results_0.is_empty());

        // Empty embedding returns empty
        let results_empty = reader.search_knn(&[], 5).unwrap();
        assert!(results_empty.is_empty());
    }

    #[test]
    fn test_search_knn_on_empty_db() {
        let db = Database::open_in_memory().unwrap();
        initialize_schema(db.conn()).unwrap();
        let reader = db.reader();

        let query_vec = vec![0.5f32; 384];
        let results = reader.search_knn(&query_vec, 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_relational_queries() {
        let db = setup_test_db();
        let reader = db.reader();

        // Counts
        assert_eq!(reader.count_documents().unwrap(), 2);
        assert_eq!(reader.count_chunks().unwrap(), 10);
        assert_eq!(reader.count_edges().unwrap(), 2);
        assert_eq!(reader.count_vectors().unwrap(), 10);

        // Get document by ID
        let doc = reader.get_document("doc-auth").unwrap().expect("Doc found");
        assert_eq!(doc.file_path, "docs/auth.md");
        assert_eq!(doc.title, Some("Authentication Guide".to_string()));

        let doc_none = reader.get_document("nonexistent").unwrap();
        assert!(doc_none.is_none());

        // Get document by path
        let doc_by_path = reader
            .get_document_by_path("docs/api.md")
            .unwrap()
            .expect("Doc found by path");
        assert_eq!(doc_by_path.id, "doc-api");

        // Get all documents
        let all_docs = reader.get_all_documents().unwrap();
        assert_eq!(all_docs.len(), 2);
        assert_eq!(all_docs[0].file_path, "docs/api.md");
        assert_eq!(all_docs[1].file_path, "docs/auth.md");

        // Get chunk by ID
        let chunk_01 = reader.get_chunk("chunk-01").unwrap().expect("Chunk found");
        assert_eq!(chunk_01.doc_id, "doc-auth");
        assert_eq!(chunk_01.parent_chunk_id, Some("chunk-00".to_string()));
        assert_eq!(chunk_01.chunk_type, ChunkType::Paragraph);

        let chunk_none = reader.get_chunk("chunk-999").unwrap();
        assert!(chunk_none.is_none());

        // Get chunks for document
        let auth_chunks = reader.get_chunks_for_document("doc-auth").unwrap();
        assert_eq!(auth_chunks.len(), 6);
        for (idx, c) in auth_chunks.iter().enumerate() {
            assert_eq!(c.id, format!("chunk-{idx:02}"));
        }

        // Get edges
        let edges_src = reader.get_edges_for_source("chunk-00").unwrap();
        assert_eq!(edges_src.len(), 1);
        assert_eq!(edges_src[0].target_chunk_id, "chunk-01");
        assert_eq!(edges_src[0].edge_type, EdgeType::Hierarchy);

        let edges_target = reader.get_edges_for_target("chunk-06").unwrap();
        assert_eq!(edges_target.len(), 1);
        assert_eq!(edges_target[0].source_chunk_id, "chunk-01");
        assert_eq!(edges_target[0].edge_type, EdgeType::ExplicitLink);
        assert_eq!(
            edges_target[0].link_text,
            Some("See API Reference".to_string())
        );
    }
}
