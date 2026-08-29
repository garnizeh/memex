//! `sqlite-vec` extension loading, self-test validation, and vector conversion utilities.

use std::sync::Once;
use rusqlite::Connection;
use crate::errors::{MemexError, Result};

static VEC_EXTENSION_INIT: Once = Once::new();

type SqliteVecInit = unsafe extern "C" fn(
    db: *mut rusqlite::ffi::sqlite3,
    err_msg: *mut *mut std::os::raw::c_char,
    api: *const rusqlite::ffi::sqlite3_api_routines,
) -> std::os::raw::c_int;

/// Ensures the `sqlite-vec` extension is registered globally via `sqlite3_auto_extension`
/// and explicitly initialized on the given connection.
pub fn ensure_sqlite_vec(conn: &Connection) -> Result<()> {
    VEC_EXTENSION_INIT.call_once(|| unsafe {
        let auto_init_fn: Option<SqliteVecInit> = Some(std::mem::transmute::<
            *const (),
            SqliteVecInit,
        >(
            sqlite_vec::sqlite3_vec_init as *const (),
        ));
        let _ = rusqlite::ffi::sqlite3_auto_extension(auto_init_fn);
    });

    unsafe {
        let init_fn: SqliteVecInit = std::mem::transmute::<*const (), SqliteVecInit>(
            sqlite_vec::sqlite3_vec_init as *const (),
        );
        let mut err_msg: *mut std::os::raw::c_char = std::ptr::null_mut();
        let rc = init_fn(conn.handle(), &mut err_msg, std::ptr::null());
        if rc != rusqlite::ffi::SQLITE_OK {
            let msg = if !err_msg.is_null() {
                let s = std::ffi::CStr::from_ptr(err_msg).to_string_lossy().into_owned();
                rusqlite::ffi::sqlite3_free(err_msg as *mut std::ffi::c_void);
                s
            } else {
                "Failed to initialize sqlite-vec extension".to_string()
            };
            return Err(MemexError::VecExtension(msg));
        }
    }

    Ok(())
}

/// Retrieves the version string of the loaded `sqlite-vec` extension.
pub fn sqlite_vec_version(conn: &Connection) -> Result<String> {
    ensure_sqlite_vec(conn)?;
    let version: String = conn.query_row("SELECT vec_version();", [], |row| row.get(0))?;
    Ok(version)
}

/// Performs a self-test validation on the connection to verify `sqlite-vec` and `vec0`
/// virtual table capabilities.
///
/// Returns the version string on success, or an error if vector operations fail.
pub fn validate_vector_support(conn: &Connection) -> Result<String> {
    ensure_sqlite_vec(conn)?;

    let version: String = conn.query_row("SELECT vec_version();", [], |row| row.get(0))?;

    // Validate vec0 virtual table functionality using a temporary table
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp._memex_vec_selftest USING vec0(
            embedding FLOAT[4]
        );",
        [],
    )?;

    let test_embedding = [1.0f32, 0.0f32, 0.0f32, 0.0f32];
    let bytes = vector_to_bytes(&test_embedding);

    conn.execute(
        "INSERT INTO temp._memex_vec_selftest (rowid, embedding) VALUES (1, ?1);",
        rusqlite::params![bytes],
    )?;

    let distance: f32 = conn.query_row(
        "SELECT distance FROM temp._memex_vec_selftest WHERE embedding MATCH ?1 ORDER BY distance LIMIT 1;",
        rusqlite::params![bytes],
        |row| row.get(0),
    )?;

    // Clean up temporary test table
    let _ = conn.execute("DROP TABLE IF EXISTS temp._memex_vec_selftest;", []);

    if distance > 1e-5 {
        return Err(MemexError::VecExtension(format!(
            "sqlite-vec self-test failed: distance between identical vectors is {distance}, expected ~0.0"
        )));
    }

    Ok(version)
}

/// Converts a slice of `f32` vector components into little-endian bytes for `sqlite-vec`.
pub fn vector_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for &val in vector {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Converts little-endian raw bytes back into a `Vec<f32>`.
pub fn bytes_to_vector(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return Err(MemexError::VecExtension(format!(
            "Invalid vector byte length {}; must be a multiple of 4",
            bytes.len()
        )));
    }

    let mut vector = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
        vector.push(f32::from_le_bytes(arr));
    }

    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_sqlite_vec_and_version() {
        let conn = Connection::open_in_memory().unwrap();
        let version = sqlite_vec_version(&conn).expect("sqlite-vec version check should succeed");
        assert!(!version.is_empty(), "sqlite-vec version should not be empty");
    }

    #[test]
    fn test_validate_vector_support() {
        let conn = Connection::open_in_memory().unwrap();
        let version = validate_vector_support(&conn).expect("sqlite-vec self-test should succeed");
        assert!(!version.is_empty());
    }

    #[test]
    fn test_vector_bytes_roundtrip() {
        let original: Vec<f32> = vec![0.0, -1.5, 3.14159, 100.25];
        let bytes = vector_to_bytes(&original);
        assert_eq!(bytes.len(), original.len() * 4);

        let restored = bytes_to_vector(&bytes).expect("deserialization should succeed");
        assert_eq!(original, restored);
    }

    #[test]
    fn test_bytes_to_vector_invalid_length() {
        let invalid_bytes = vec![0u8, 1u8, 2u8]; // not a multiple of 4
        let res = bytes_to_vector(&invalid_bytes);
        assert!(res.is_err());
    }

    #[test]
    fn test_dummy_384_dim_vector_knn_query() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_sqlite_vec(&conn).unwrap();

        // Create 384-dimensional vec0 virtual table
        conn.execute(
            "CREATE VIRTUAL TABLE vec_test USING vec0(
                chunk_id TEXT PRIMARY KEY,
                embedding FLOAT[384]
            );",
            [],
        ).unwrap();

        // Generate synthetic 384-dim vectors
        let vec_a: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
        let mut vec_b: Vec<f32> = (0..384).map(|i| ((384 - i) as f32) / 384.0).collect();
        // Normalize slightly to have distinct vectors
        vec_b[0] += 0.5;

        let bytes_a = vector_to_bytes(&vec_a);
        let bytes_b = vector_to_bytes(&vec_b);

        conn.execute(
            "INSERT INTO vec_test (chunk_id, embedding) VALUES (?1, ?2);",
            rusqlite::params!["chunk-a", bytes_a],
        ).unwrap();

        conn.execute(
            "INSERT INTO vec_test (chunk_id, embedding) VALUES (?1, ?2);",
            rusqlite::params!["chunk-b", bytes_b],
        ).unwrap();

        // Query KNN with exact vec_a matching
        let mut stmt = conn
            .prepare("SELECT chunk_id, distance FROM vec_test WHERE embedding MATCH ?1 ORDER BY distance LIMIT 2;")
            .unwrap();

        let results: Vec<(String, f32)> = stmt
            .query_map(rusqlite::params![bytes_a], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "chunk-a");
        assert!(results[0].1 < 1e-5, "Exact match distance should be ~0, got {}", results[0].1);
        assert_eq!(results[1].0, "chunk-b");
        assert!(results[1].1 > results[0].1);
    }
}
