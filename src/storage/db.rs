use crate::errors::Result;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// Wrapper around a SQLite connection configured for Memex with optimal pragmas.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    readonly: bool,
}

impl Database {
    /// Opens or creates a SQLite database in read-write mode at the specified path,
    /// applying performance and safety pragmas (`WAL`, `synchronous = NORMAL`,
    /// `foreign_keys = ON`, `cache_size = -64000`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let conn = Connection::open_with_flags(path, flags)?;
        Self::apply_pragmas(&conn, false)?;
        crate::storage::vec::ensure_sqlite_vec(&conn)?;

        Ok(Self {
            conn,
            readonly: false,
        })
    }

    /// Opens an existing SQLite database in read-only mode, applying read pragmas.
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let conn = Connection::open_with_flags(path, flags)?;
        Self::apply_pragmas(&conn, true)?;
        crate::storage::vec::ensure_sqlite_vec(&conn)?;

        Ok(Self {
            conn,
            readonly: true,
        })
    }

    /// Opens an in-memory database configured with Memex pragmas (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::apply_pragmas(&conn, false)?;
        crate::storage::vec::ensure_sqlite_vec(&conn)?;

        Ok(Self {
            conn,
            readonly: false,
        })
    }

    /// Applies Memex PRAGMA configurations to a connection.
    fn apply_pragmas(conn: &Connection, readonly: bool) -> Result<()> {
        if !readonly {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA cache_size = -64000;",
            )?;
        } else {
            conn.execute_batch(
                "PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA cache_size = -64000;",
            )?;
        }
        Ok(())
    }

    /// Validates `sqlite-vec` vector extension support on this database connection.
    pub fn validate_vector_support(&self) -> Result<String> {
        crate::storage::vec::validate_vector_support(&self.conn)
    }

    /// Retrieves the loaded `sqlite-vec` extension version.
    pub fn vec_version(&self) -> Result<String> {
        crate::storage::vec::sqlite_vec_version(&self.conn)
    }

    /// Returns a reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Returns a mutable reference to the underlying SQLite connection.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Consumes the wrapper and returns the underlying SQLite connection.
    pub fn into_inner(self) -> Connection {
        self.conn
    }

    /// Returns a [`StorageWriter`] wrapping the database connection.
    pub fn writer(&mut self) -> crate::storage::writer::StorageWriter<'_> {
        crate::storage::writer::StorageWriter::new(&mut self.conn)
    }

    /// Checks whether the connection was opened in read-only mode.
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_read_write_pragmas() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join(".memex").join("memex.db");

        let db = Database::open(&db_path).unwrap();
        assert!(!db.is_readonly());
        assert!(db_path.exists());

        // Check journal_mode = wal
        let journal_mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");

        // Check synchronous = 1 (NORMAL)
        let synchronous: i32 = db
            .conn()
            .query_row("PRAGMA synchronous;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1);

        // Check foreign_keys = 1 (ON)
        let foreign_keys: i32 = db
            .conn()
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        // Check cache_size = -64000
        let cache_size: i32 = db
            .conn()
            .query_row("PRAGMA cache_size;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache_size, -64000);
    }

    #[test]
    fn test_open_readonly() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        // First create the database in read-write mode
        {
            let db = Database::open(&db_path).unwrap();
            db.conn()
                .execute("CREATE TABLE test (id INTEGER PRIMARY KEY, val TEXT);", [])
                .unwrap();
            db.conn()
                .execute("INSERT INTO test (val) VALUES ('hello');", [])
                .unwrap();
        }

        // Open in read-only mode
        let db_ro = Database::open_readonly(&db_path).unwrap();
        assert!(db_ro.is_readonly());

        // Assert we can read
        let val: String = db_ro
            .conn()
            .query_row("SELECT val FROM test WHERE id = 1;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(val, "hello");

        // Assert write fails
        let write_res = db_ro
            .conn()
            .execute("INSERT INTO test (val) VALUES ('fail');", []);
        assert!(write_res.is_err());

        // Check read-only pragmas
        let cache_size: i32 = db_ro
            .conn()
            .query_row("PRAGMA cache_size;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache_size, -64000);

        let foreign_keys: i32 = db_ro
            .conn()
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_readonly());

        let foreign_keys: i32 = db
            .conn()
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        let cache_size: i32 = db
            .conn()
            .query_row("PRAGMA cache_size;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache_size, -64000);
    }

    #[test]
    fn test_open_readonly_nonexistent_fails() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("does_not_exist.db");
        let res = Database::open_readonly(&db_path);
        assert!(res.is_err());
    }

    #[test]
    fn test_database_vector_extension_loaded_and_validated() {
        let db = Database::open_in_memory().unwrap();
        let version = db
            .vec_version()
            .expect("vec_version should succeed on Database");
        assert!(!version.is_empty());

        let validated_version = db
            .validate_vector_support()
            .expect("validate_vector_support should succeed on Database");
        assert_eq!(version, validated_version);
    }
}
