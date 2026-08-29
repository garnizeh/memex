use crate::discovery::hash::{compute_bytes_hash, compute_file_hash};
use crate::errors::{MemexError, Result};
use crate::ingestion::chunker::ContextualChunker;
use crate::ingestion::embedder::{EmbeddingEngine, EMBEDDING_DIM};
use crate::ingestion::parser::MarkdownParser;
use crate::models::{Chunk, Document, Edge};
use crate::storage::db::Database;
use crate::storage::writer::StorageWriter;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Represents the categorized differences between files discovered on disk
/// and documents previously stored in the database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexDelta {
    /// Newly discovered files on disk that do not exist in the database.
    pub added: Vec<PathBuf>,
    /// Files that exist both on disk and in the database, but whose content hash differs.
    pub modified: Vec<PathBuf>,
    /// Documents previously stored in the database whose files no longer exist on disk.
    pub removed: Vec<Document>,
    /// Files that exist both on disk and in the database with matching content hashes.
    pub unchanged: Vec<PathBuf>,
}

impl IndexDelta {
    /// Creates a new empty [`IndexDelta`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if there are any changes (added, modified, or removed files).
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.modified.is_empty() || !self.removed.is_empty()
    }

    /// Returns the total count of scanned files currently present on disk.
    pub fn total_scanned(&self) -> usize {
        self.added.len() + self.modified.len() + self.unchanged.len()
    }

    /// Returns the total count of files that require database mutations.
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }

    /// Returns `true` if all categorized buckets are empty.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.removed.is_empty()
            && self.unchanged.is_empty()
    }
}

/// Execution statistics for an indexing pass executed by [`IndexCoordinator`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexStats {
    /// Number of newly added files indexed into the database.
    pub files_added: usize,
    /// Number of modified files re-indexed.
    pub files_modified: usize,
    /// Number of deleted files removed from the database.
    pub files_removed: usize,
    /// Number of existing files that remained unchanged and were skipped.
    pub files_unchanged: usize,
    /// Total documentation chunks parsed and written to the database.
    pub chunks_indexed: usize,
    /// Total hierarchy and explicit link edges inserted into the knowledge graph.
    pub edges_created: usize,
    /// Total vector embeddings generated and stored in `vec_chunks`.
    pub vectors_indexed: usize,
    /// Elapsed duration of the indexing execution.
    pub duration: Duration,
}

impl IndexStats {
    /// Returns `true` if any file additions, modifications, or removals were processed.
    pub fn has_changes(&self) -> bool {
        self.files_added > 0 || self.files_modified > 0 || self.files_removed > 0
    }

    /// Returns the total number of files discovered and processed.
    pub fn total_files(&self) -> usize {
        self.files_added + self.files_modified + self.files_removed + self.files_unchanged
    }

    /// Returns the total number of file mutations applied.
    pub fn total_mutations(&self) -> usize {
        self.files_added + self.files_modified + self.files_removed
    }
}

/// Trait for generating vector embeddings for text chunks during document indexing.
pub trait ChunkEmbedder {
    /// Computes normalized 384-dimensional embedding vectors for a batch of text slices.
    fn embed_chunks(&self, texts: &[&str]) -> Result<Vec<[f32; EMBEDDING_DIM]>>;
}

impl ChunkEmbedder for EmbeddingEngine {
    fn embed_chunks(&self, texts: &[&str]) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
        self.embed_batch_str(texts)
    }
}

impl<F> ChunkEmbedder for F
where
    F: Fn(&[&str]) -> Result<Vec<[f32; EMBEDDING_DIM]>>,
{
    fn embed_chunks(&self, texts: &[&str]) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
        (self)(texts)
    }
}

/// Normalizes a path into a canonical relative string representation using forward slashes (`/`).
pub fn normalize_relative_path(path: &Path, root: Option<&Path>) -> String {
    let rel_path = match root {
        Some(r) => path.strip_prefix(r).unwrap_or(path),
        None => path,
    };

    let mut parts = Vec::new();
    for component in rel_path.components() {
        match component {
            Component::Normal(s) => {
                if let Some(s_str) = s.to_str() {
                    parts.push(s_str);
                }
            }
            Component::CurDir => {}
            _ => {}
        }
    }

    if parts.is_empty() {
        rel_path
            .to_str()
            .unwrap_or_default()
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string()
    } else {
        parts.join("/")
    }
}

/// Classifier that determines incremental index deltas by comparing on-disk files against the database.
pub struct DeltaClassifier;

impl DeltaClassifier {
    /// Computes the [`IndexDelta`] comparing `scanned` disk file paths against the stored documents in `db`.
    pub fn compute(scanned: &[PathBuf], db: &Database) -> Result<IndexDelta> {
        Self::compute_with_root_opt(None, scanned, db)
    }

    /// Computes the [`IndexDelta`] using an explicit project `root` directory for path normalization.
    pub fn compute_with_root(
        root: &Path,
        scanned: &[PathBuf],
        db: &Database,
    ) -> Result<IndexDelta> {
        Self::compute_with_root_opt(Some(root), scanned, db)
    }

    fn compute_with_root_opt(
        root: Option<&Path>,
        scanned: &[PathBuf],
        db: &Database,
    ) -> Result<IndexDelta> {
        let stored_docs = db.reader().get_all_documents()?;
        let mut docs_by_path: HashMap<String, Document> = stored_docs
            .into_iter()
            .map(|doc| (doc.file_path.clone(), doc))
            .collect();

        let mut delta = IndexDelta::new();

        for file_path in scanned {
            let current_hash = compute_file_hash(file_path)?;
            let rel_path_str = normalize_relative_path(file_path, root);

            // If not found by direct normalized relative path, check if any stored path matches as suffix
            let matching_key = if docs_by_path.contains_key(&rel_path_str) {
                Some(rel_path_str)
            } else {
                docs_by_path
                    .keys()
                    .find(|k| {
                        let k_path = Path::new(k.as_str());
                        file_path.ends_with(k_path)
                    })
                    .cloned()
            };

            if let Some(key) = matching_key {
                let stored_doc = docs_by_path.remove(&key).unwrap();
                if stored_doc.content_hash == current_hash {
                    delta.unchanged.push(file_path.clone());
                } else {
                    delta.modified.push(file_path.clone());
                }
            } else {
                delta.added.push(file_path.clone());
            }
        }

        // Remaining documents in DB were not found during scan -> Removed
        let mut removed_docs: Vec<Document> = docs_by_path.into_values().collect();
        removed_docs.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        delta.removed = removed_docs;

        delta.added.sort();
        delta.modified.sort();
        delta.unchanged.sort();

        Ok(delta)
    }
}

/// Coordinates the end-to-end document indexing pipeline:
/// 1. Markdown Parsing (`MarkdownParser`)
/// 2. Structural & Contextual Chunking (`ContextualChunker`)
/// 3. Graph Edge Generation (Hierarchy & Explicit Links)
/// 4. Batched Vector Embedding (`ChunkEmbedder` / `EmbeddingEngine`)
/// 5. Atomic Database Mutation & Deletion within a single SQLite Transaction (`StorageWriter`)
pub struct IndexCoordinator<'a> {
    root: &'a Path,
    db: &'a mut Database,
}

impl<'a> IndexCoordinator<'a> {
    /// Creates a new [`IndexCoordinator`] for the given project `root` and mutable [`Database`].
    pub fn new(root: &'a Path, db: &'a mut Database) -> Self {
        Self { root, db }
    }

    /// Convenience static method to process a delta on a given root path.
    pub fn process_delta_with_root<E: ChunkEmbedder>(
        root: &Path,
        delta: &IndexDelta,
        db: &mut Database,
        embedder: &E,
    ) -> Result<IndexStats> {
        let mut coordinator = IndexCoordinator::new(root, db);
        coordinator.process_delta(delta, embedder)
    }

    /// Coordinates end-to-end ingestion, delta application, embedding, and atomic database persistence.
    pub fn process_delta<E: ChunkEmbedder>(
        &mut self,
        delta: &IndexDelta,
        embedder: &E,
    ) -> Result<IndexStats> {
        let start_time = Instant::now();

        // If there are no changes, return early with accurate unchanged stats
        if !delta.has_changes() {
            return Ok(IndexStats {
                files_added: 0,
                files_modified: 0,
                files_removed: 0,
                files_unchanged: delta.unchanged.len(),
                chunks_indexed: 0,
                edges_created: 0,
                vectors_indexed: 0,
                duration: start_time.elapsed(),
            });
        }

        // 1. Gather all document path mappings (existing unmodified documents + newly added/modified)
        // to enable full cross-document explicit link resolution.
        let mut all_doc_paths: Vec<(String, String)> = Vec::new();
        if let Ok(existing_docs) = self.db.reader().get_all_documents() {
            for d in existing_docs {
                let is_removed = delta.removed.iter().any(|r| r.id == d.id);
                let is_modified = delta.modified.iter().any(|m| {
                    let rel = normalize_relative_path(m, Some(self.root));
                    d.file_path == rel || compute_bytes_hash(rel.as_bytes()) == d.id
                });
                if !is_removed && !is_modified {
                    all_doc_paths.push((d.id, d.file_path));
                }
            }
        }

        // 2. Parse, chunk, and extract edges for all added and modified files
        let mut files_to_process = Vec::new();
        files_to_process.extend(delta.added.iter().map(|p| (p, true))); // (path, is_added)
        files_to_process.extend(delta.modified.iter().map(|p| (p, false))); // (path, is_modified)

        let mut new_docs: Vec<Document> = Vec::new();
        let mut all_chunks: Vec<Chunk> = Vec::new();
        let mut all_hierarchy_edges: Vec<Edge> = Vec::new();

        let now_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        for (file_path, _) in &files_to_process {
            let full_path = if file_path.is_absolute() {
                file_path.to_path_buf()
            } else {
                self.root.join(file_path)
            };

            let content = std::fs::read_to_string(&full_path).map_err(|e| {
                MemexError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read file '{}': {e}", full_path.display()),
                ))
            })?;

            let content_hash = compute_bytes_hash(content.as_bytes());
            let rel_path_str = normalize_relative_path(file_path, Some(self.root));
            let doc_id = compute_bytes_hash(rel_path_str.as_bytes());

            let ast = MarkdownParser::parse(&content)?;
            let doc = Document {
                id: doc_id.clone(),
                file_path: rel_path_str.clone(),
                title: ast.title.clone(),
                content_hash,
                indexed_at: now_timestamp,
            };

            let doc_chunks = ContextualChunker::chunk_document(&doc_id, &ast);
            let doc_h_edges = ContextualChunker::build_hierarchy_edges(&doc_chunks);

            all_doc_paths.push((doc_id.clone(), rel_path_str));
            new_docs.push(doc);
            all_chunks.extend(doc_chunks);
            all_hierarchy_edges.extend(doc_h_edges);
        }

        // 3. Resolve explicit cross-document and anchor links across all chunks
        let all_explicit_edges =
            ContextualChunker::resolve_explicit_links(&all_chunks, Some(&all_doc_paths));

        let mut all_edges = all_hierarchy_edges;
        all_edges.extend(all_explicit_edges);

        // 4. Batched vector embeddings for all chunks
        let embeddings = if all_chunks.is_empty() {
            Vec::new()
        } else {
            let texts: Vec<&str> = all_chunks
                .iter()
                .map(|c| c.contextual_content.as_str())
                .collect();
            embedder.embed_chunks(&texts)?
        };

        if embeddings.len() != all_chunks.len() && !all_chunks.is_empty() {
            return Err(MemexError::EmbeddingError {
                chunk_id: "coordinator".to_string(),
                message: format!(
                    "Embedding count mismatch: expected {} embeddings for {} chunks, got {}",
                    all_chunks.len(),
                    all_chunks.len(),
                    embeddings.len()
                ),
            });
        }

        // 5. Execute all mutations in a single atomic SQLite transaction
        let tx = self.db.conn_mut().transaction()?;

        // 5a. Delete removed documents (cascade deletes chunks, edges, vec_chunks)
        for doc in &delta.removed {
            StorageWriter::delete_document_tx(&tx, &doc.id)?;
        }

        // 5b. Delete modified documents (cascade deletes previous chunks, edges, vec_chunks)
        for path in &delta.modified {
            let rel_path_str = normalize_relative_path(path, Some(self.root));
            let doc_id = compute_bytes_hash(rel_path_str.as_bytes());
            StorageWriter::delete_document_tx(&tx, &doc_id)?;
        }

        // 5c. Insert / upsert new and updated documents
        for doc in &new_docs {
            StorageWriter::insert_document_tx(&tx, doc)?;
        }

        // 5d. Insert all chunks
        StorageWriter::insert_chunks_batch_tx(&tx, &all_chunks)?;

        // 5e. Insert all edges
        StorageWriter::insert_edges_batch_tx(&tx, &all_edges)?;

        // 5f. Insert all vector embeddings
        let vectors: Vec<(&str, &[f32; EMBEDDING_DIM])> = all_chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(c, emb)| (c.id.as_str(), emb))
            .collect();
        StorageWriter::insert_vectors_batch_tx(&tx, &vectors)?;

        // 5g. Commit transaction atomically
        tx.commit()?;

        let stats = IndexStats {
            files_added: delta.added.len(),
            files_modified: delta.modified.len(),
            files_removed: delta.removed.len(),
            files_unchanged: delta.unchanged.len(),
            chunks_indexed: all_chunks.len(),
            edges_created: all_edges.len(),
            vectors_indexed: vectors.len(),
            duration: start_time.elapsed(),
        };

        Ok(stats)
    }
}

/// Executes the `index` command.
pub fn run_index(path: &Path, quiet: bool, verbose: bool) -> Result<()> {
    if !quiet {
        eprintln!("Running index at {:?} (verbose: {})", path, verbose);
    }
    // Will be fully orchestrated in subsequent Phase 6 tasks
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::initialize_schema;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_test_db() -> Database {
        let db = Database::open_in_memory().expect("Failed to create test db");
        initialize_schema(db.conn()).expect("Failed to initialize schema");
        db
    }

    fn create_test_file(dir: &Path, rel_path: &str, content: &[u8]) -> PathBuf {
        let full_path = dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&full_path).unwrap();
        file.write_all(content).unwrap();
        file.flush().unwrap();
        full_path
    }

    #[test]
    fn test_delta_all_added_on_empty_db() {
        let temp_dir = TempDir::new().unwrap();
        let db = setup_test_db();

        let file1 = create_test_file(temp_dir.path(), "docs/intro.md", b"# Intro\nWelcome!");
        let file2 = create_test_file(temp_dir.path(), "README.md", b"# Memex\nKnowledge engine.");

        let scanned = vec![file1.clone(), file2.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added.len(), 2);
        assert_eq!(delta.modified.len(), 0);
        assert_eq!(delta.removed.len(), 0);
        assert_eq!(delta.unchanged.len(), 0);
        assert!(delta.has_changes());
        assert_eq!(delta.total_scanned(), 2);
        assert_eq!(delta.total_changes(), 2);
        assert!(!delta.is_empty());
    }

    #[test]
    fn test_delta_all_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(temp_dir.path(), "docs/guide.md", b"# Guide\nStep 1");
        let hash1 = compute_file_hash(&file1).unwrap();

        let doc = Document {
            id: "doc_guide".to_string(),
            file_path: "docs/guide.md".to_string(),
            title: Some("Guide".to_string()),
            content_hash: hash1,
            indexed_at: 1700000000,
        };
        db.writer().insert_document(&doc).unwrap();

        let scanned = vec![file1.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added.len(), 0);
        assert_eq!(delta.modified.len(), 0);
        assert_eq!(delta.removed.len(), 0);
        assert_eq!(delta.unchanged, vec![file1]);
        assert!(!delta.has_changes());
        assert_eq!(delta.total_scanned(), 1);
        assert_eq!(delta.total_changes(), 0);
    }

    #[test]
    fn test_delta_modified_when_content_changes() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(temp_dir.path(), "docs/arch.md", b"# Old Architecture");
        let doc = Document {
            id: "doc_arch".to_string(),
            file_path: "docs/arch.md".to_string(),
            title: Some("Architecture".to_string()),
            content_hash: "old_outdated_sha256_hash".to_string(),
            indexed_at: 1700000000,
        };
        db.writer().insert_document(&doc).unwrap();

        let scanned = vec![file1.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added.len(), 0);
        assert_eq!(delta.modified, vec![file1]);
        assert_eq!(delta.removed.len(), 0);
        assert_eq!(delta.unchanged.len(), 0);
        assert!(delta.has_changes());
    }

    #[test]
    fn test_delta_removed_when_file_deleted() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let doc = Document {
            id: "doc_deleted".to_string(),
            file_path: "docs/deleted.md".to_string(),
            title: Some("Deleted Doc".to_string()),
            content_hash: "dummy_hash".to_string(),
            indexed_at: 1700000000,
        };
        db.writer().insert_document(&doc).unwrap();

        // Scanned is empty (file was deleted on disk)
        let scanned: Vec<PathBuf> = Vec::new();
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added.len(), 0);
        assert_eq!(delta.modified.len(), 0);
        assert_eq!(delta.removed.len(), 1);
        assert_eq!(delta.removed[0].file_path, "docs/deleted.md");
        assert_eq!(delta.unchanged.len(), 0);
        assert!(delta.has_changes());
        assert_eq!(delta.total_scanned(), 0);
        assert_eq!(delta.total_changes(), 1);
    }

    #[test]
    fn test_delta_mixed_all_four_states() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        // 1. Unchanged file
        let unchanged_file = create_test_file(temp_dir.path(), "unchanged.md", b"# Unchanged");
        let unchanged_hash = compute_file_hash(&unchanged_file).unwrap();
        db.writer()
            .insert_document(&Document {
                id: "doc_unchanged".to_string(),
                file_path: "unchanged.md".to_string(),
                title: Some("Unchanged".to_string()),
                content_hash: unchanged_hash,
                indexed_at: 1700000000,
            })
            .unwrap();

        // 2. Modified file
        let modified_file = create_test_file(temp_dir.path(), "modified.md", b"# New Content");
        db.writer()
            .insert_document(&Document {
                id: "doc_modified".to_string(),
                file_path: "modified.md".to_string(),
                title: Some("Modified".to_string()),
                content_hash: "old_different_hash".to_string(),
                indexed_at: 1700000000,
            })
            .unwrap();

        // 3. Removed file in DB (not created on disk)
        db.writer()
            .insert_document(&Document {
                id: "doc_removed".to_string(),
                file_path: "removed.md".to_string(),
                title: Some("Removed".to_string()),
                content_hash: "removed_hash".to_string(),
                indexed_at: 1700000000,
            })
            .unwrap();

        // 4. Added file on disk
        let added_file = create_test_file(temp_dir.path(), "added.md", b"# Fresh File");

        let scanned = vec![
            unchanged_file.clone(),
            modified_file.clone(),
            added_file.clone(),
        ];

        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added, vec![added_file]);
        assert_eq!(delta.modified, vec![modified_file]);
        assert_eq!(delta.unchanged, vec![unchanged_file]);
        assert_eq!(delta.removed.len(), 1);
        assert_eq!(delta.removed[0].file_path, "removed.md");

        assert!(delta.has_changes());
        assert_eq!(delta.total_scanned(), 3);
        assert_eq!(delta.total_changes(), 3);
    }

    #[test]
    fn test_delta_compute_without_root_and_relative_paths() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file = create_test_file(temp_dir.path(), "docs/test.md", b"# Content");
        let hash = compute_file_hash(&file).unwrap();

        db.writer()
            .insert_document(&Document {
                id: "doc_test".to_string(),
                file_path: "docs/test.md".to_string(),
                title: Some("Test".to_string()),
                content_hash: hash,
                indexed_at: 1700000000,
            })
            .unwrap();

        // Scanned passing the file directly
        let scanned = vec![file.clone()];
        let delta = DeltaClassifier::compute(&scanned, &db).unwrap();

        assert_eq!(delta.unchanged, vec![file]);
        assert_eq!(delta.added.len(), 0);
        assert_eq!(delta.modified.len(), 0);
        assert_eq!(delta.removed.len(), 0);
    }

    #[test]
    fn test_delta_error_on_nonexistent_scanned_file() {
        let db = setup_test_db();
        let bad_path = PathBuf::from("/nonexistent/path/docs/ghost.md");

        let scanned = vec![bad_path];
        let result = DeltaClassifier::compute(&scanned, &db);
        assert!(result.is_err());
    }

    #[test]
    fn test_index_delta_empty_helpers() {
        let delta = IndexDelta::new();
        assert!(delta.is_empty());
        assert!(!delta.has_changes());
        assert_eq!(delta.total_scanned(), 0);
        assert_eq!(delta.total_changes(), 0);
    }

    fn dummy_embedder() -> impl ChunkEmbedder {
        |_texts: &[&str]| -> Result<Vec<[f32; EMBEDDING_DIM]>> {
            let count = _texts.len();
            let mut embeddings = Vec::with_capacity(count);
            for i in 0..count {
                let mut vec = [0.0f32; EMBEDDING_DIM];
                vec[0] = 1.0;
                vec[1] = (i as f32) * 0.01;
                embeddings.push(vec);
            }
            Ok(embeddings)
        }
    }

    #[test]
    fn test_coordinator_process_delta_added_files() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(
            temp_dir.path(),
            "docs/intro.md",
            b"# Introduction\n\nWelcome to Memex!\n\n## Getting Started\n\nFollow these steps.\n",
        );
        let file2 = create_test_file(
            temp_dir.path(),
            "README.md",
            b"# Project Readme\n\nThis is the root project file.\n",
        );

        let scanned = vec![file1.clone(), file2.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        assert_eq!(delta.added.len(), 2);

        let stats = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta,
            &mut db,
            &dummy_embedder(),
        )
        .expect("process_delta failed");

        assert_eq!(stats.files_added, 2);
        assert_eq!(stats.files_modified, 0);
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.files_unchanged, 0);
        assert!(stats.chunks_indexed >= 4);
        assert!(stats.edges_created >= 2);
        assert_eq!(stats.vectors_indexed, stats.chunks_indexed);
        assert!(stats.has_changes());
        assert_eq!(stats.total_files(), 2);
        assert_eq!(stats.total_mutations(), 2);

        // Verify DB contents
        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 2);

        let doc1 = docs
            .iter()
            .find(|d| d.file_path == "docs/intro.md")
            .unwrap();
        assert_eq!(doc1.title, Some("Introduction".to_string()));

        let doc2 = docs.iter().find(|d| d.file_path == "README.md").unwrap();
        assert_eq!(doc2.title, Some("Project Readme".to_string()));

        let chunks1 = db.reader().get_chunks_for_document(&doc1.id).unwrap();
        assert!(!chunks1.is_empty());

        let chunks2 = db.reader().get_chunks_for_document(&doc2.id).unwrap();
        assert!(!chunks2.is_empty());
    }

    #[test]
    fn test_coordinator_process_delta_unchanged_files() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file = create_test_file(
            temp_dir.path(),
            "docs/guide.md",
            b"# User Guide\n\nRead this guide carefully.\n",
        );

        let scanned = vec![file.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        let stats1 = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();
        assert_eq!(stats1.files_added, 1);

        // Second run without touching files
        let delta2 = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        assert_eq!(delta2.unchanged.len(), 1);
        assert!(!delta2.has_changes());

        let stats2 = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta2,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();
        assert_eq!(stats2.files_added, 0);
        assert_eq!(stats2.files_modified, 0);
        assert_eq!(stats2.files_removed, 0);
        assert_eq!(stats2.files_unchanged, 1);
        assert_eq!(stats2.chunks_indexed, 0);
        assert_eq!(stats2.edges_created, 0);
        assert_eq!(stats2.vectors_indexed, 0);
        assert!(!stats2.has_changes());
    }

    #[test]
    fn test_coordinator_process_delta_modified_file() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(
            temp_dir.path(),
            "doc1.md",
            b"# Version 1\n\nInitial version paragraph.\n",
        );
        let file2 = create_test_file(
            temp_dir.path(),
            "doc2.md",
            b"# Unchanged Doc\n\nStays the same.\n",
        );

        let scanned = vec![file1.clone(), file2.clone()];
        let delta1 = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta1,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();

        // Modify file1
        create_test_file(
            temp_dir.path(),
            "doc1.md",
            b"# Version 2 Updated\n\nUpdated new content line.\n\n## New Subsection\n\nAdditional details.\n",
        );

        let delta2 = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        assert_eq!(delta2.modified.len(), 1);
        assert_eq!(delta2.unchanged.len(), 1);

        let stats2 = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta2,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();
        assert_eq!(stats2.files_added, 0);
        assert_eq!(stats2.files_modified, 1);
        assert_eq!(stats2.files_removed, 0);
        assert_eq!(stats2.files_unchanged, 1);

        // Verify updated title and chunks in DB
        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 2);
        let updated_doc1 = docs.iter().find(|d| d.file_path == "doc1.md").unwrap();
        assert_eq!(updated_doc1.title, Some("Version 2 Updated".to_string()));

        let doc1_chunks = db
            .reader()
            .get_chunks_for_document(&updated_doc1.id)
            .unwrap();
        assert!(doc1_chunks
            .iter()
            .any(|c| c.content.contains("New Subsection")));
    }

    #[test]
    fn test_coordinator_process_delta_removed_file() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(temp_dir.path(), "keep.md", b"# Keep Me\n\nContent.\n");
        let file2 = create_test_file(temp_dir.path(), "delete.md", b"# Delete Me\n\nContent.\n");

        let scanned = vec![file1.clone(), file2.clone()];
        let delta1 = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta1,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();

        // Delete file2 from disk and scan only file1
        fs::remove_file(&file2).unwrap();
        let scanned2 = vec![file1.clone()];
        let delta2 = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned2, &db).unwrap();
        assert_eq!(delta2.removed.len(), 1);
        assert_eq!(delta2.unchanged.len(), 1);

        let stats2 = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta2,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();
        assert_eq!(stats2.files_removed, 1);
        assert_eq!(stats2.files_unchanged, 1);

        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].file_path, "keep.md");
    }

    #[test]
    fn test_coordinator_cross_document_explicit_links() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file1 = create_test_file(
            temp_dir.path(),
            "guide.md",
            b"# Guide\n\nSee the [API Reference](api.md#endpoints) for more details.\n",
        );
        let file2 = create_test_file(
            temp_dir.path(),
            "api.md",
            b"# API Reference\n\n## Endpoints\n\nGET /users\n",
        );

        let scanned = vec![file1.clone(), file2.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();
        let stats = IndexCoordinator::process_delta_with_root(
            temp_dir.path(),
            &delta,
            &mut db,
            &dummy_embedder(),
        )
        .unwrap();

        assert!(stats.edges_created >= 2);
        assert!(db.reader().count_edges().unwrap() >= 2);

        let doc1 = db
            .reader()
            .get_document_by_path("guide.md")
            .unwrap()
            .unwrap();
        let chunks1 = db.reader().get_chunks_for_document(&doc1.id).unwrap();
        let chunk_with_link = chunks1
            .iter()
            .find(|c| c.content.contains("[API Reference]"))
            .unwrap();
        let edges = db
            .reader()
            .get_edges_for_source(&chunk_with_link.id)
            .unwrap();
        let explicit_edge = edges
            .iter()
            .find(|e| e.edge_type == crate::models::EdgeType::ExplicitLink);
        assert!(
            explicit_edge.is_some(),
            "Explicit link edge should be created across docs"
        );
    }

    #[test]
    fn test_coordinator_with_live_embedding_engine() {
        use crate::ingestion::embedder::ModelManager;

        let assets = match ModelManager::ensure_model_assets() {
            Ok(assets) => assets,
            Err(_) => return, // Skip if offline/no assets
        };

        let engine = match EmbeddingEngine::new(&assets) {
            Ok(e) => e,
            Err(_) => return,
        };

        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db();

        let file = create_test_file(
            temp_dir.path(),
            "docs/auth.md",
            b"# Authentication Guide\n\nUse Bearer tokens to authenticate HTTP requests.\n\n## Token Expiry\n\nTokens expire after 1 hour.\n",
        );

        let scanned = vec![file.clone()];
        let delta = DeltaClassifier::compute_with_root(temp_dir.path(), &scanned, &db).unwrap();

        let stats =
            IndexCoordinator::process_delta_with_root(temp_dir.path(), &delta, &mut db, &engine)
                .expect("Failed live indexing with EmbeddingEngine");

        assert_eq!(stats.files_added, 1);
        assert!(stats.vectors_indexed >= 3);

        // Perform vector search in DB to verify embeddings were actually inserted in vec_chunks
        let query_emb = engine.embed("how do I authenticate requests?").unwrap();
        let results = db.reader().search_knn(&query_emb, 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk.doc_id, compute_bytes_hash(b"docs/auth.md"));
    }
}
