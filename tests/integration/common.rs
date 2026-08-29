use memex::cli::init::init_project;
use memex::storage::db::Database;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

pub fn copy_fixtures(fixture_name: &str, target_dir: &Path) {
    let src = fixtures_dir().join(fixture_name);
    assert!(
        src.exists(),
        "Fixture directory does not exist: {}",
        src.display()
    );

    for entry in WalkDir::new(&src) {
        let entry = entry.unwrap();
        let rel_path = entry.path().strip_prefix(&src).unwrap();
        let dest_path = target_dir.join(rel_path);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest_path).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

pub fn setup_indexed_project(fixture_name: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_fixtures(fixture_name, tmp.path());
    init_project(tmp.path(), false, false).expect("Initialization should succeed");
    tmp
}

pub fn open_db(project_dir: &Path) -> Database {
    let db_path = project_dir.join(".memex").join("memex.db");
    Database::open_readonly(&db_path).expect("Should open database read-only")
}

pub fn count_chunks(project_dir: &Path) -> i64 {
    let db = open_db(project_dir);
    db.conn()
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap()
}
