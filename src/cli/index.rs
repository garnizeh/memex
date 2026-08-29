use crate::discovery::hash::compute_file_hash;
use crate::errors::Result;
use crate::models::Document;
use crate::storage::db::Database;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

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
}
