use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::discovery::hash::compute_file_hash;
use crate::errors::{MemexError, Result};

/// Default model name for local embeddings.
pub const DEFAULT_MODEL_NAME: &str = "all-MiniLM-L6-v2";

/// Standard ONNX model file name.
pub const MODEL_FILE_NAME: &str = "model.onnx";

/// Standard tokenizer definition file name.
pub const TOKENIZER_FILE_NAME: &str = "tokenizer.json";

/// Default Hugging Face repository URL for all-MiniLM-L6-v2 ONNX model weights (~80MB).
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";

/// Default Hugging Face repository URL for all-MiniLM-L6-v2 tokenizer config.
pub const DEFAULT_TOKENIZER_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

/// Resolved local filesystem paths to the embedding model and tokenizer assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAssets {
    /// Absolute path to the ONNX model file (`model.onnx`).
    pub model_path: PathBuf,
    /// Absolute path to the tokenizer configuration file (`tokenizer.json`).
    pub tokenizer_path: PathBuf,
}

/// Manages the resolution, downloading, caching, and integrity verification of embedding model assets.
#[derive(Debug, Clone)]
pub struct ModelManager {
    /// Root directory where model assets are cached.
    pub cache_dir: PathBuf,
    /// Remote URL to fetch the ONNX model file from if missing.
    pub model_url: String,
    /// Remote URL to fetch the tokenizer configuration from if missing.
    pub tokenizer_url: String,
    /// Optional expected SHA-256 hash for `model.onnx`.
    pub expected_model_hash: Option<String>,
    /// Optional expected SHA-256 hash for `tokenizer.json`.
    pub expected_tokenizer_hash: Option<String>,
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelManager {
    /// Creates a new `ModelManager` targeting the default user cache directory and canonical asset URLs.
    pub fn new() -> Self {
        Self {
            cache_dir: Self::default_cache_dir(),
            model_url: DEFAULT_MODEL_URL.to_string(),
            tokenizer_url: DEFAULT_TOKENIZER_URL.to_string(),
            expected_model_hash: None,
            expected_tokenizer_hash: None,
        }
    }

    /// Creates a `ModelManager` with a custom cache directory.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            model_url: DEFAULT_MODEL_URL.to_string(),
            tokenizer_url: DEFAULT_TOKENIZER_URL.to_string(),
            expected_model_hash: None,
            expected_tokenizer_hash: None,
        }
    }

    /// Sets custom remote URLs for model and tokenizer download.
    pub fn with_urls(
        mut self,
        model_url: impl Into<String>,
        tokenizer_url: impl Into<String>,
    ) -> Self {
        self.model_url = model_url.into();
        self.tokenizer_url = tokenizer_url.into();
        self
    }

    /// Sets expected SHA-256 hashes for integrity verification.
    pub fn with_expected_hashes(
        mut self,
        model_hash: Option<String>,
        tokenizer_hash: Option<String>,
    ) -> Self {
        self.expected_model_hash = model_hash;
        self.expected_tokenizer_hash = tokenizer_hash;
        self
    }

    /// Resolves the default cache directory for Memex models.
    ///
    /// Respects the `MEMEX_CACHE_DIR` environment variable if set, otherwise defaults to
    /// standard user cache directory (`~/.cache/memex/models/` on Linux).
    pub fn default_cache_dir() -> PathBuf {
        if let Ok(env_dir) = std::env::var("MEMEX_CACHE_DIR") {
            return PathBuf::from(env_dir);
        }

        if let Some(base_dirs) = directories::BaseDirs::new() {
            base_dirs.cache_dir().join("memex").join("models")
        } else {
            PathBuf::from(".cache").join("memex").join("models")
        }
    }

    /// Global convenience entrypoint: ensures model and tokenizer assets are present in the default cache dir.
    pub fn ensure_model_assets() -> Result<ModelAssets> {
        Self::new().ensure_assets()
    }

    /// Ensures that both `model.onnx` and `tokenizer.json` exist in the cache directory,
    /// downloading them if absent and validating their SHA-256 checksums if specified.
    pub fn ensure_assets(&self) -> Result<ModelAssets> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed to create model cache directory '{}': {e}",
                    self.cache_dir.display()
                ))
            })?;
        }

        let model_path = self.cache_dir.join(MODEL_FILE_NAME);
        let tokenizer_path = self.cache_dir.join(TOKENIZER_FILE_NAME);

        self.ensure_asset_file(
            &model_path,
            &self.model_url,
            self.expected_model_hash.as_deref(),
            "ONNX model",
        )?;

        self.ensure_asset_file(
            &tokenizer_path,
            &self.tokenizer_url,
            self.expected_tokenizer_hash.as_deref(),
            "Tokenizer configuration",
        )?;

        Ok(ModelAssets {
            model_path,
            tokenizer_path,
        })
    }

    /// Ensures a single asset file is present and valid.
    fn ensure_asset_file(
        &self,
        target_path: &Path,
        download_url: &str,
        expected_hash: Option<&str>,
        description: &str,
    ) -> Result<()> {
        if target_path.exists() {
            if let Some(expected) = expected_hash {
                if let Err(e) = Self::verify_file_integrity(target_path, expected) {
                    warn!(
                        path = %target_path.display(),
                        error = %e,
                        "Existing {description} asset failed SHA-256 verification, re-downloading..."
                    );
                    let _ = fs::remove_file(target_path);
                    self.download_asset_file(
                        target_path,
                        download_url,
                        expected_hash,
                        description,
                    )?;
                }
            }
            return Ok(());
        }

        self.download_asset_file(target_path, download_url, expected_hash, description)
    }

    /// Downloads an asset from a URL into a temporary file and atomically renames it after verification.
    fn download_asset_file(
        &self,
        target_path: &Path,
        download_url: &str,
        expected_hash: Option<&str>,
        description: &str,
    ) -> Result<()> {
        info!(
            url = %download_url,
            target = %target_path.display(),
            "Downloading {description}..."
        );

        let temp_path = target_path.with_extension(format!("tmp.{}", std::process::id()));

        // Attempt download using ureq
        let download_result = (|| -> Result<()> {
            let response = ureq::get(download_url).call().map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed to download {description} from '{download_url}': {e}"
                ))
            })?;

            let mut reader = response.into_body().into_reader();
            let mut file = File::create(&temp_path).map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed to create temporary file '{}': {e}",
                    temp_path.display()
                ))
            })?;

            io::copy(&mut reader, &mut file).map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed writing downloaded {description} to '{}': {e}",
                    temp_path.display()
                ))
            })?;
            file.flush().map_err(MemexError::Io)?;

            if let Some(expected) = expected_hash {
                Self::verify_file_integrity(&temp_path, expected)?;
            }

            fs::rename(&temp_path, target_path).map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed to finalize downloaded file '{}': {e}",
                    target_path.display()
                ))
            })?;

            Ok(())
        })();

        if download_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }

        download_result
    }

    /// Verifies the SHA-256 hash of a file matches the expected hex-encoded checksum.
    pub fn verify_file_integrity(path: &Path, expected_hash: &str) -> Result<()> {
        let computed = compute_file_hash(path).map_err(|e| {
            MemexError::ModelLoadError(format!(
                "Failed to compute SHA-256 for asset '{}': {e}",
                path.display()
            ))
        })?;

        if !computed.eq_ignore_ascii_case(expected_hash) {
            return Err(MemexError::ModelLoadError(format!(
                "SHA-256 integrity verification failed for '{}': expected {}, computed {}",
                path.display(),
                expected_hash,
                computed
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::hash::compute_bytes_hash;
    use tempfile::tempdir;

    #[test]
    fn test_default_cache_dir_resolution() {
        let cache_dir = ModelManager::default_cache_dir();
        assert!(cache_dir.to_string_lossy().contains("memex"));
        assert!(cache_dir.to_string_lossy().contains("models"));
    }

    #[test]
    fn test_custom_cache_dir_and_builder() {
        let temp = tempdir().expect("failed to create temp dir");
        let custom_dir = temp.path().join("custom_models");

        let manager = ModelManager::with_cache_dir(custom_dir.clone())
            .with_urls("https://example.com/m.onnx", "https://example.com/t.json")
            .with_expected_hashes(
                Some("abc123hash".to_string()),
                Some("def456hash".to_string()),
            );

        assert_eq!(manager.cache_dir, custom_dir);
        assert_eq!(manager.model_url, "https://example.com/m.onnx");
        assert_eq!(manager.tokenizer_url, "https://example.com/t.json");
        assert_eq!(manager.expected_model_hash, Some("abc123hash".to_string()));
        assert_eq!(
            manager.expected_tokenizer_hash,
            Some("def456hash".to_string())
        );
    }

    #[test]
    fn test_verify_file_integrity_success() {
        let temp = tempdir().expect("temp dir failed");
        let test_file = temp.path().join("sample_asset.bin");
        let content = b"Sample model weights content for testing integrity";
        fs::write(&test_file, content).expect("write failed");

        let expected_hash = compute_bytes_hash(content);
        let result = ModelManager::verify_file_integrity(&test_file, &expected_hash);
        assert!(result.is_ok());

        // Upper case hex should also be accepted
        let upper_expected = expected_hash.to_uppercase();
        let result_upper = ModelManager::verify_file_integrity(&test_file, &upper_expected);
        assert!(result_upper.is_ok());
    }

    #[test]
    fn test_verify_file_integrity_mismatch() {
        let temp = tempdir().expect("temp dir failed");
        let test_file = temp.path().join("corrupted.bin");
        fs::write(&test_file, b"corrupted bytes").expect("write failed");

        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = ModelManager::verify_file_integrity(&test_file, wrong_hash);
        assert!(result.is_err());

        match result.unwrap_err() {
            MemexError::ModelLoadError(msg) => {
                assert!(msg.contains("SHA-256 integrity verification failed"));
                assert!(msg.contains(wrong_hash));
            }
            err => panic!("Unexpected error variant: {err:?}"),
        }
    }

    #[test]
    fn test_ensure_assets_when_files_already_exist() {
        let temp = tempdir().expect("temp dir failed");
        let cache_dir = temp.path().join("models");
        fs::create_dir_all(&cache_dir).expect("failed to create cache dir");

        let model_path = cache_dir.join(MODEL_FILE_NAME);
        let tokenizer_path = cache_dir.join(TOKENIZER_FILE_NAME);

        let model_content = b"fake onnx model bytes";
        let tokenizer_content = b"fake tokenizer json content";

        fs::write(&model_path, model_content).expect("write model failed");
        fs::write(&tokenizer_path, tokenizer_content).expect("write tokenizer failed");

        let model_hash = compute_bytes_hash(model_content);
        let tokenizer_hash = compute_bytes_hash(tokenizer_content);

        let manager = ModelManager::with_cache_dir(cache_dir.clone())
            .with_expected_hashes(Some(model_hash), Some(tokenizer_hash));

        let assets = manager.ensure_assets().expect("ensure_assets failed");
        assert_eq!(assets.model_path, model_path);
        assert_eq!(assets.tokenizer_path, tokenizer_path);
        assert!(assets.model_path.exists());
        assert!(assets.tokenizer_path.exists());
    }

    #[test]
    fn test_ensure_assets_offline_missing_file_error() {
        let temp = tempdir().expect("temp dir failed");
        let cache_dir = temp.path().join("empty_cache");

        // Invalid unreachable URL
        let manager = ModelManager::with_cache_dir(cache_dir).with_urls(
            "http://127.0.0.1:9/nonexistent.onnx",
            "http://127.0.0.1:9/nonexistent.json",
        );

        let result = manager.ensure_assets();
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::ModelLoadError(msg) => {
                assert!(msg.contains("Failed to download ONNX model"));
            }
            err => panic!("Unexpected error variant: {err:?}"),
        }
    }
}
