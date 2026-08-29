use crate::config::MemexConfig;
use crate::discovery::{FileDiscovery, unsafe_index_root_reason};
use crate::errors::{MemexError, Result};
use crate::ingestion::embedder::{EmbeddingEngine, ModelManager};
use crate::storage::db::Database;
use crate::storage::schema::initialize_schema;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::index::{ChunkEmbedder, IndexCoordinator, IndexDelta, IndexStats};

/// Format a duration nicely for CLI output (e.g. "2.3s" or "150ms").
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 0.001 {
        format!("{:.2}ms", secs * 1000.0)
    } else {
        format!("{:.1}s", secs)
    }
}

/// Resolves and validates the target project path for initialization.
pub fn resolve_project_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(MemexError::DiscoveryError {
            path: path.display().to_string(),
            reason: "Target path does not exist".to_string(),
        });
    }

    let absolute_path = if path.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    let resolved = absolute_path.canonicalize().unwrap_or(absolute_path);

    if resolved.is_file() {
        return Err(MemexError::InvalidCommand(format!(
            "Target path '{}' is a file, but initialization requires a project directory",
            resolved.display()
        )));
    }

    Ok(resolved)
}

/// Performs initialization of a project directory using a provided [`ChunkEmbedder`].
///
/// Steps:
/// 1. Path resolution & validation
/// 2. Safety checks (rejecting `$HOME` or filesystem root unless `force = true`)
/// 3. Existing initialization verification (rejecting if `.memex/memex.db` exists)
/// 4. Directory scaffolding (`<root>/.memex/`)
/// 5. SQLite database & schema creation (`<root>/.memex/memex.db`)
/// 6. Full file discovery & complete initial indexing
pub fn init_project_with_embedder<E: ChunkEmbedder>(
    path: &Path,
    force: bool,
    verbose: bool,
    embedder: &E,
) -> Result<IndexStats> {
    let resolved_path = resolve_project_path(path)?;

    // 2. Safety check
    if !force && let Some(reason) = unsafe_index_root_reason(&resolved_path) {
        return Err(MemexError::UnsafeRoot {
            path: resolved_path.display().to_string(),
            reason,
        });
    }

    // 3. Already initialized check
    let memex_dir = resolved_path.join(".memex");
    let db_path = memex_dir.join("memex.db");
    if db_path.exists() {
        return Err(MemexError::AlreadyInitialized {
            path: resolved_path.display().to_string(),
        });
    }

    // 4. Scaffolding
    std::fs::create_dir_all(&memex_dir)?;

    // Create .memex/.gitignore so transient database and log files are ignored by default
    let gitignore_path = memex_dir.join(".gitignore");
    if !gitignore_path.exists() {
        let _ = std::fs::write(
            &gitignore_path,
            "# Memex local data files — local to each machine, not for committing.\n# Ignore everything in .memex/ except this file itself.\n*\n!.gitignore\n",
        );
    }

    // 5. Database Creation & Schema Initialization
    let mut db = Database::open(&db_path)?;
    initialize_schema(db.conn())?;

    // 6. Full File Discovery & Ingestion with multi-stage progress reporting
    let config = MemexConfig::load_or_default(&resolved_path);
    let mut reporter = crate::cli::progress::IndexProgressReporter::new(
        !verbose && !std::io::IsTerminal::is_terminal(&std::io::stderr()),
    );

    reporter.start_scan();
    let scanned_files = FileDiscovery::scan(&resolved_path, &config)?;
    let added_count = scanned_files.len();

    let delta = IndexDelta {
        added: scanned_files,
        modified: Vec::new(),
        removed: Vec::new(),
        unchanged: Vec::new(),
    };
    reporter.finish_scan(added_count, added_count);

    if verbose {
        eprintln!(
            "Discovered {} markdown file(s) for indexing in '{}'",
            delta.added.len(),
            resolved_path.display()
        );
    }

    let stats = IndexCoordinator::new(&resolved_path, &mut db).process_delta_with_reporter(
        &delta,
        embedder,
        &mut reporter,
    )?;

    Ok(stats)
}

/// Initializes a project directory and builds the initial index using the local ONNX embedding engine.
pub fn init_project(path: &Path, force: bool, verbose: bool) -> Result<IndexStats> {
    let assets = ModelManager::ensure_model_assets()?;
    let engine = EmbeddingEngine::new(&assets)?;
    init_project_with_embedder(path, force, verbose, &engine)
}

/// Executes the `init` command and formats user-facing CLI output.
pub fn run_init(path: &Path, force: bool, verbose: bool) -> Result<IndexStats> {
    let resolved_path = resolve_project_path(path)?;

    let stats = init_project(&resolved_path, force, verbose)?;

    // Print formatted success report matching architecture specification:
    // ✓ Initialized in /path/to/project
    // ✓ Indexed 47 files
    //   128 nodes, 312 edges in 2.3s
    let file_plural = if stats.files_added == 1 {
        "file"
    } else {
        "files"
    };
    println!("✓ Initialized in {}", resolved_path.display());
    println!("✓ Indexed {} {}", stats.files_added, file_plural);
    println!(
        "  {} nodes, {} edges in {}",
        stats.chunks_indexed,
        stats.edges_created,
        format_duration(stats.duration)
    );

    if stats.has_errors() {
        eprintln!(
            "⚠ Encountered {} non-fatal error(s) during indexing (see .memex/errors.log)",
            stats.files_failed
        );
    }

    if verbose {
        eprintln!("  Vector embeddings: {}", stats.vectors_indexed);
        eprintln!(
            "  Database location: {}",
            resolved_path.join(".memex").join("memex.db").display()
        );
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::embedder::EMBEDDING_DIM;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn mock_embedder() -> impl ChunkEmbedder {
        |_texts: &[&str]| -> Result<Vec<[f32; EMBEDDING_DIM]>> {
            Ok(vec![[0.05f32; EMBEDDING_DIM]; _texts.len()])
        }
    }

    fn create_file(dir: &Path, rel_path: &str, content: &str) -> PathBuf {
        let full_path = dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&full_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        full_path
    }

    #[test]
    fn test_init_success_on_new_directory() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("my_project");
        fs::create_dir(&project_dir).unwrap();

        create_file(
            &project_dir,
            "README.md",
            "# My Project\n\nWelcome to my project.\n\n## Getting Started\n\nRun the app.",
        );
        create_file(
            &project_dir,
            "docs/guide.md",
            "# User Guide\n\nRefer to [Readme](../README.md) for basics.",
        );

        let stats = init_project_with_embedder(&project_dir, false, false, &mock_embedder())
            .expect("init should succeed");

        assert_eq!(stats.files_added, 2);
        assert_eq!(stats.files_modified, 0);
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.files_failed, 0);
        assert!(stats.chunks_indexed > 0);
        assert!(stats.edges_created > 0);
        assert_eq!(stats.vectors_indexed, stats.chunks_indexed);

        // Verify .memex/memex.db exists and contains records
        let db_path = project_dir.join(".memex").join("memex.db");
        assert!(db_path.exists());

        let db = Database::open_readonly(&db_path).expect("Should open db read-only");
        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_init_already_initialized_fails() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("my_project");
        fs::create_dir(&project_dir).unwrap();

        create_file(&project_dir, "doc.md", "# Doc\nHello world");

        init_project_with_embedder(&project_dir, false, false, &mock_embedder())
            .expect("First init should succeed");

        let second_init = init_project_with_embedder(&project_dir, false, false, &mock_embedder());
        match second_init {
            Err(MemexError::AlreadyInitialized { path }) => {
                assert!(path.contains("my_project"));
            }
            other => panic!("Expected AlreadyInitialized error, got: {:?}", other),
        }
    }

    #[test]
    fn test_init_nonexistent_directory_fails() {
        let temp_dir = TempDir::new().unwrap();
        let non_existent = temp_dir.path().join("does_not_exist");

        let result = init_project_with_embedder(&non_existent, false, false, &mock_embedder());
        match result {
            Err(MemexError::DiscoveryError { reason, .. }) => {
                assert!(reason.contains("does not exist"));
            }
            other => panic!("Expected DiscoveryError, got: {:?}", other),
        }
    }

    #[test]
    fn test_init_file_target_fails() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = create_file(temp_dir.path(), "file.md", "# Heading\nContent");

        let result = init_project_with_embedder(&file_path, false, false, &mock_embedder());
        match result {
            Err(MemexError::InvalidCommand(msg)) => {
                assert!(msg.contains("is a file"));
            }
            other => panic!("Expected InvalidCommand error, got: {:?}", other),
        }
    }

    #[test]
    fn test_init_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("empty_project");
        fs::create_dir(&project_dir).unwrap();

        let stats = init_project_with_embedder(&project_dir, false, false, &mock_embedder())
            .expect("init empty directory should succeed");

        assert_eq!(stats.files_added, 0);
        assert_eq!(stats.chunks_indexed, 0);
        assert_eq!(stats.edges_created, 0);
        assert_eq!(stats.vectors_indexed, 0);

        let db_path = project_dir.join(".memex").join("memex.db");
        assert!(db_path.exists());
    }

    #[test]
    fn test_init_unsafe_root_without_force() {
        #[cfg(unix)]
        {
            let root = Path::new("/");
            let result = init_project_with_embedder(root, false, false, &mock_embedder());
            match result {
                Err(MemexError::UnsafeRoot { reason, .. }) => {
                    assert!(
                        reason.contains("filesystem root") || reason.contains("home directory")
                    );
                }
                other => panic!("Expected UnsafeRoot error, got: {:?}", other),
            }
        }
    }

    #[test]
    fn test_init_with_config_exclude() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("config_project");
        fs::create_dir(&project_dir).unwrap();

        create_file(
            &project_dir,
            "memex.json",
            r#"{"exclude": ["ignore_me/**"]}"#,
        );
        create_file(&project_dir, "keep.md", "# Keep\nThis should be indexed.");
        create_file(
            &project_dir,
            "ignore_me/skip.md",
            "# Skip\nDo not index this.",
        );

        let stats = init_project_with_embedder(&project_dir, false, false, &mock_embedder())
            .expect("init should succeed");

        assert_eq!(stats.files_added, 1);
        assert_eq!(stats.files_failed, 0);

        let db_path = project_dir.join(".memex").join("memex.db");
        let db = Database::open_readonly(&db_path).unwrap();
        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].file_path, "keep.md");
    }

    #[test]
    fn test_init_with_errors_logged() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("error_project");
        fs::create_dir(&project_dir).unwrap();

        create_file(&project_dir, "valid.md", "# Valid\nNormal markdown.");

        // Create invalid UTF-8 file
        let invalid_file = project_dir.join("invalid.md");
        fs::write(&invalid_file, [0xFF, 0xFE, 0xFD]).unwrap();

        let stats = init_project_with_embedder(&project_dir, false, false, &mock_embedder())
            .expect("init should complete with isolated file errors");

        assert_eq!(stats.files_added, 1);
        assert_eq!(stats.files_failed, 1);
        assert!(stats.has_errors());

        let err_log = project_dir.join(".memex").join("errors.log");
        assert!(err_log.exists());
        let log_content = fs::read_to_string(&err_log).unwrap();
        assert!(log_content.contains("invalid.md"));
    }

    #[test]
    fn test_format_duration_helper() {
        assert_eq!(format_duration(Duration::from_millis(500)), "0.5s");
        assert_eq!(format_duration(Duration::from_secs(2)), "2.0s");
        assert_eq!(format_duration(Duration::from_micros(200)), "0.20ms");
    }

    #[test]
    fn test_init_e2e_live_embedder() {
        let assets = match ModelManager::ensure_model_assets() {
            Ok(a) => a,
            Err(_) => return, // Skip if offline or assets missing
        };

        if EmbeddingEngine::new(&assets).is_err() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("live_init_project");
        fs::create_dir(&project_dir).unwrap();

        create_file(
            &project_dir,
            "docs/getting_started.md",
            "# Getting Started\n\nMemex is a local documentation context server.\n\n## Installation\n\nRun cargo build.",
        );

        let stats = run_init(&project_dir, false, true).expect("run_init should succeed");
        assert_eq!(stats.files_added, 1);
        assert!(stats.chunks_indexed >= 2);
        assert_eq!(stats.vectors_indexed, stats.chunks_indexed);

        // Verify database records
        let db_path = project_dir.join(".memex").join("memex.db");
        let db = Database::open_readonly(&db_path).unwrap();
        let docs = db.reader().get_all_documents().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title.as_deref(), Some("Getting Started"));
    }
}
