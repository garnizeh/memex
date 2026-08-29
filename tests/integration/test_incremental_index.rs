use super::common::{copy_fixtures, count_chunks, open_db};
use memex::cli::index::index_project;
use memex::cli::init::init_project;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_incremental_index_only_processes_changes() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("simple", tmp.path());
    init_project(tmp.path(), false, false).unwrap();

    let initial_chunks: i64 = count_chunks(tmp.path());
    assert!(initial_chunks > 0);

    // 1. Modify one file
    fs::write(
        tmp.path().join("api.md"),
        "# API\n## New Section\nNew content added to api.",
    )
    .unwrap();

    // 2. Add one file
    fs::write(
        tmp.path().join("new.md"),
        "# New Doc\nHello from newly added doc.",
    )
    .unwrap();

    // 3. Remove one file
    fs::remove_file(tmp.path().join("guide.md")).unwrap();

    let result = index_project(tmp.path()).unwrap();

    assert_eq!(result.files_added, 1);
    assert_eq!(result.files_modified, 1);
    assert_eq!(result.files_removed, 1);
    assert_eq!(result.files_unchanged, 1); // README.md unchanged
    assert_eq!(result.files_failed, 0);

    // Verify the removed file's chunks and documents are gone
    let db = open_db(tmp.path());
    let removed_doc: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE file_path = 'guide.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed_doc, 0, "removed file should be gone from DB");

    let removed_chunks: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE doc_id = (SELECT id FROM documents WHERE file_path = 'guide.md')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed_chunks, 0, "removed file chunks should be gone");

    // Verify the added file is in DB
    let added_doc: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE file_path = 'new.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(added_doc, 1, "new file should be present in DB");

    // Verify re-indexing with no changes is a no-op
    let result2 = index_project(tmp.path()).unwrap();
    assert_eq!(
        result2.files_added + result2.files_modified + result2.files_removed,
        0,
        "second index run with no changes should have 0 mutations"
    );
    assert_eq!(result2.files_unchanged, 3); // README.md, new.md, and api.md -> total 3 files
}

#[test]
fn test_incremental_index_preserves_unmodified_embeddings() {
    let tmp = TempDir::new().unwrap();
    copy_fixtures("simple", tmp.path());
    init_project(tmp.path(), false, false).unwrap();

    let db = open_db(tmp.path());
    let readme_doc = db
        .conn()
        .query_row(
            "SELECT id FROM documents WHERE file_path = 'README.md'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap();

    let readme_chunk_ids: Vec<String> = {
        let mut stmt = db
            .conn()
            .prepare("SELECT id FROM chunks WHERE doc_id = ?")
            .unwrap();
        let rows = stmt
            .query_map([&readme_doc], |r| r.get::<_, String>(0))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    // Add a new file without touching README.md
    fs::write(
        tmp.path().join("extra.md"),
        "# Extra\nSome extra documentation.",
    )
    .unwrap();

    let result = index_project(tmp.path()).unwrap();
    assert_eq!(result.files_added, 1);
    assert_eq!(result.files_modified, 0);

    // Verify chunk IDs for README remain intact
    let db2 = open_db(tmp.path());
    for chunk_id in &readme_chunk_ids {
        let exists: i64 = db2
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM vec_chunks WHERE chunk_id = ?",
                [chunk_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            exists, 1,
            "vector embedding for unmodified chunk should remain"
        );
    }
}
