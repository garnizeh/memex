use crate::errors::{MemexError, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Buffer size for streaming file reads when computing SHA-256 (64 KB).
const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// Computes the hex-encoded SHA-256 hash of a file's contents in a memory-efficient streaming fashion.
///
/// Returns `Ok(hex_hash)` or `Err(MemexError)` if the file cannot be opened or read.
pub fn compute_file_hash(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|e| MemexError::DiscoveryError {
        path: path.display().to_string(),
        reason: format!("failed to open file for hashing: {e}"),
    })?;

    let mut reader = BufReader::with_capacity(HASH_BUFFER_SIZE, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_SIZE];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| MemexError::DiscoveryError {
                path: path.display().to_string(),
                reason: format!("failed to read file during hashing: {e}"),
            })?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    Ok(hex::encode(result))
}

/// Computes the hex-encoded SHA-256 hash of raw byte slice.
pub fn compute_bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_compute_bytes_hash_empty() {
        // Known SHA-256 for empty byte string
        let hash = compute_bytes_hash(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_compute_bytes_hash_known_string() {
        // "Hello, World!" -> dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f
        let hash = compute_bytes_hash(b"Hello, World!");
        assert_eq!(
            hash,
            "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f"
        );
    }

    #[test]
    fn test_compute_file_hash_deterministic_and_matches_sha256sum() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = b"# Documentation Title\n\nThis is a sample markdown document for testing.\n";
        temp_file.write_all(content).unwrap();
        temp_file.flush().unwrap();

        let file_hash = compute_file_hash(temp_file.path()).unwrap();
        let direct_hash = compute_bytes_hash(content);

        assert_eq!(file_hash, direct_hash);
        assert_eq!(
            file_hash,
            "5e3b194762afcd882238a250159df750913b61b48ba9f81ec9b6ec8da0e3cbd5"
        );
    }

    #[test]
    fn test_compute_file_hash_large_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Generate >128KB content to cross multiple 64KB buffer chunks
        let chunk = "0123456789abcdef".repeat(1024); // 16KB
        for _ in 0..10 {
            temp_file.write_all(chunk.as_bytes()).unwrap();
        }
        temp_file.flush().unwrap();

        let file_hash = compute_file_hash(temp_file.path()).unwrap();
        let content = fs::read(temp_file.path()).unwrap();
        let direct_hash = compute_bytes_hash(&content);

        assert_eq!(file_hash, direct_hash);
    }

    #[test]
    fn test_compute_file_hash_nonexistent_file() {
        let non_existent = Path::new("/path/that/definitely/does/not/exist/test.md");
        let result = compute_file_hash(non_existent);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::DiscoveryError { path, reason } => {
                assert!(path.contains("test.md"));
                assert!(reason.contains("failed to open file for hashing"));
            }
            err => panic!("Unexpected error variant: {err:?}"),
        }
    }
}
