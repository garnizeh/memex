use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::discovery::hash::compute_file_hash;
use crate::errors::{MemexError, Result};

/// Default model name for local embeddings.
pub const DEFAULT_MODEL_NAME: &str = "all-MiniLM-L6-v2";

/// Maximum token sequence length bounded for the embedding model (all-MiniLM-L6-v2 max sequence length is 256 tokens).
pub const MAX_SEQUENCE_LENGTH: usize = 256;

/// Default embedding batch size.
pub const DEFAULT_BATCH_SIZE: usize = 64;

/// Standard ONNX model file name.
pub const MODEL_FILE_NAME: &str = "model.onnx";

/// Standard tokenizer definition file name.
pub const TOKENIZER_FILE_NAME: &str = "tokenizer.json";

/// Embedding vector dimensionality for all-MiniLM-L6-v2 (384 float32 components).
pub const EMBEDDING_DIM: usize = 384;

/// Default Hugging Face repository URL for all-MiniLM-L6-v2 ONNX model weights (~80MB).
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";

/// Default Hugging Face repository URL for all-MiniLM-L6-v2 tokenizer config.
pub const DEFAULT_TOKENIZER_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

/// Tokenized batch containing 1D flattened vectors and sequence dimensions suitable for ONNX tensor inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizedBatch {
    /// Batch size (number of input sequences).
    pub batch_size: usize,
    /// Sequence length after padding/truncation.
    pub seq_len: usize,
    /// Flattened input token IDs of shape `[batch_size, seq_len]`.
    pub input_ids: Vec<i64>,
    /// Flattened attention mask of shape `[batch_size, seq_len]`, where 1 indicates an active token and 0 indicates padding.
    pub attention_mask: Vec<i64>,
    /// Flattened token type IDs of shape `[batch_size, seq_len]` (all zeros for single-sequence BERT/MiniLM).
    pub token_type_ids: Vec<i64>,
}

impl TokenizedBatch {
    /// Returns the shape tuple `[batch_size, seq_len]`.
    pub fn shape(&self) -> (usize, usize) {
        (self.batch_size, self.seq_len)
    }

    /// Returns `true` if the batch is empty (`batch_size == 0`).
    pub fn is_empty(&self) -> bool {
        self.batch_size == 0
    }
}

/// Thread-safe wrapper over `tokenizers::Tokenizer` providing batched tokenization, padding, and truncation.
#[derive(Debug, Clone)]
pub struct TokenizerWrapper {
    /// Inner fast tokenizer instance.
    pub tokenizer: Arc<Tokenizer>,
    /// Maximum allowed sequence length (defaults to 256).
    pub max_seq_len: usize,
}

impl TokenizerWrapper {
    /// Creates a new `TokenizerWrapper` wrapping an `Arc<Tokenizer>` with default max sequence length (256).
    pub fn new(tokenizer: Arc<Tokenizer>) -> Self {
        Self {
            tokenizer,
            max_seq_len: MAX_SEQUENCE_LENGTH,
        }
    }

    /// Creates a new `TokenizerWrapper` with a custom maximum sequence length.
    pub fn with_max_seq_len(tokenizer: Arc<Tokenizer>, max_seq_len: usize) -> Self {
        Self {
            tokenizer,
            max_seq_len,
        }
    }

    /// Encodes a batch of string slices into `TokenizedBatch`, handling truncation, batch-wide padding,
    /// and generating `input_ids`, `attention_mask`, and `token_type_ids`.
    pub fn encode_batch(&self, texts: &[&str]) -> Result<TokenizedBatch> {
        let batch_size = texts.len();
        if batch_size == 0 {
            return Ok(TokenizedBatch {
                batch_size: 0,
                seq_len: 0,
                input_ids: Vec::new(),
                attention_mask: Vec::new(),
                token_type_ids: Vec::new(),
            });
        }

        // Tokenize all texts in parallel or sequence using the underlying Tokenizer
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| MemexError::TokenizerError(format!("Failed to encode batch: {e}")))?;

        // Find max sequence length in this batch, bounded by self.max_seq_len and at least 1
        let max_batch_len = encodings
            .iter()
            .map(|e| e.get_ids().len().min(self.max_seq_len))
            .max()
            .unwrap_or(0)
            .max(1);

        let total_elements = batch_size * max_batch_len;
        let mut input_ids = vec![0i64; total_elements];
        let mut attention_mask = vec![0i64; total_elements];
        let mut token_type_ids = vec![0i64; total_elements];

        for (row_idx, encoding) in encodings.iter().enumerate() {
            let ids = encoding.get_ids();
            let masks = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();

            let token_count = ids.len().min(self.max_seq_len).min(max_batch_len);
            let row_offset = row_idx * max_batch_len;

            for i in 0..token_count {
                input_ids[row_offset + i] = ids[i] as i64;
                attention_mask[row_offset + i] = masks[i] as i64;
                if i < type_ids.len() {
                    token_type_ids[row_offset + i] = type_ids[i] as i64;
                }
            }
        }

        Ok(TokenizedBatch {
            batch_size,
            seq_len: max_batch_len,
            input_ids,
            attention_mask,
            token_type_ids,
        })
    }

    /// Encodes a single string slice into a `TokenizedBatch` with batch_size = 1.
    pub fn encode(&self, text: &str) -> Result<TokenizedBatch> {
        self.encode_batch(&[text])
    }
}

/// Resolved local filesystem paths to the embedding model and tokenizer assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAssets {
    /// Absolute path to the ONNX model file (`model.onnx`).
    pub model_path: PathBuf,
    /// Absolute path to the tokenizer configuration file (`tokenizer.json`).
    pub tokenizer_path: PathBuf,
}

/// Thread-safe local embedding engine executing inference via ONNX Runtime and tokenizers.
#[derive(Debug, Clone)]
pub struct EmbeddingEngine {
    /// ONNX Runtime session wrapped in `Arc<Mutex>` for thread-safe concurrent reuse.
    pub session: Arc<Mutex<Session>>,
    /// Fast tokenizer instance wrapped in `Arc`.
    pub tokenizer: Arc<Tokenizer>,
    /// Tokenizer wrapper for batched tensor encoding.
    pub tokenizer_wrapper: TokenizerWrapper,
    /// Model assets used to initialize this engine.
    pub assets: ModelAssets,
}

impl EmbeddingEngine {
    /// Initializes the ONNX Runtime session and tokenizer using default performance settings.
    pub fn new(assets: &ModelAssets) -> Result<Self> {
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8);
        Self::with_threads(assets, num_threads)
    }

    /// Initializes the ONNX Runtime session and tokenizer with custom thread configuration.
    pub fn with_threads(assets: &ModelAssets, intra_threads: usize) -> Result<Self> {
        if !assets.model_path.exists() {
            return Err(MemexError::ModelLoadError(format!(
                "Model file not found at '{}'",
                assets.model_path.display()
            )));
        }

        if !assets.tokenizer_path.exists() {
            return Err(MemexError::ModelLoadError(format!(
                "Tokenizer file not found at '{}'",
                assets.tokenizer_path.display()
            )));
        }

        let tokenizer = Tokenizer::from_file(&assets.tokenizer_path).map_err(|e| {
            MemexError::TokenizerError(format!(
                "Failed to load tokenizer from '{}': {e}",
                assets.tokenizer_path.display()
            ))
        })?;

        let mut builder = Session::builder()
            .map_err(|e| {
                MemexError::ModelLoadError(format!(
                    "Failed to initialize ONNX session builder: {e}"
                ))
            })?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| {
                MemexError::ModelLoadError(format!("Failed setting ONNX optimization level: {e}"))
            })?
            .with_intra_threads(intra_threads)
            .map_err(|e| {
                MemexError::ModelLoadError(format!("Failed setting ONNX intra threads: {e}"))
            })?;

        let model_bytes = fs::read(&assets.model_path).map_err(MemexError::Io)?;
        let session = builder.commit_from_memory(&model_bytes).map_err(|e| {
            MemexError::ModelLoadError(format!(
                "Failed loading ONNX model from '{}': {e}",
                assets.model_path.display()
            ))
        })?;

        let tokenizer_arc = Arc::new(tokenizer);
        let tokenizer_wrapper = TokenizerWrapper::new(Arc::clone(&tokenizer_arc));

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: tokenizer_arc,
            tokenizer_wrapper,
            assets: assets.clone(),
        })
    }

    /// Returns a reference to the wrapped ONNX Runtime session mutex.
    pub fn session(&self) -> &Arc<Mutex<Session>> {
        &self.session
    }

    /// Returns a reference to the wrapped Tokenizer.
    pub fn tokenizer(&self) -> &Arc<Tokenizer> {
        &self.tokenizer
    }

    /// Returns a reference to the TokenizerWrapper.
    pub fn tokenizer_wrapper(&self) -> &TokenizerWrapper {
        &self.tokenizer_wrapper
    }

    /// Returns the ModelAssets used by this engine.
    pub fn assets(&self) -> &ModelAssets {
        &self.assets
    }

    /// Returns the session inputs information (names).
    pub fn input_names(&self) -> Vec<String> {
        let session = self.session.lock().expect("session lock poisoned");
        session
            .inputs()
            .iter()
            .map(|input| input.name().to_string())
            .collect()
    }

    /// Returns the session outputs information (names).
    pub fn output_names(&self) -> Vec<String> {
        let session = self.session.lock().expect("session lock poisoned");
        session
            .outputs()
            .iter()
            .map(|output| output.name().to_string())
            .collect()
    }

    /// Computes embeddings for a pre-tokenized batch.
    pub fn embed_tokenized_batch(
        &self,
        tokenized: &TokenizedBatch,
    ) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
        if tokenized.is_empty() {
            return Ok(Vec::new());
        }

        let (batch_size, seq_len) = tokenized.shape();
        let input_ids_tensor =
            Tensor::from_array(([batch_size, seq_len], tokenized.input_ids.clone())).map_err(
                |e| MemexError::EmbeddingError {
                    chunk_id: "batch".to_string(),
                    message: format!("Failed to create input_ids tensor: {e}"),
                },
            )?;
        let attention_mask_tensor =
            Tensor::from_array(([batch_size, seq_len], tokenized.attention_mask.clone())).map_err(
                |e| MemexError::EmbeddingError {
                    chunk_id: "batch".to_string(),
                    message: format!("Failed to create attention_mask tensor: {e}"),
                },
            )?;
        let token_type_ids_tensor =
            Tensor::from_array(([batch_size, seq_len], tokenized.token_type_ids.clone())).map_err(
                |e| MemexError::EmbeddingError {
                    chunk_id: "batch".to_string(),
                    message: format!("Failed to create token_type_ids tensor: {e}"),
                },
            )?;

        let inputs = inputs![
            "input_ids" => input_ids_tensor,
            "attention_mask" => attention_mask_tensor,
            "token_type_ids" => token_type_ids_tensor,
        ];

        let mut session = self
            .session
            .lock()
            .map_err(|e| MemexError::EmbeddingError {
                chunk_id: "batch".to_string(),
                message: format!("Session lock poisoned: {e}"),
            })?;

        let outputs = session
            .run(inputs)
            .map_err(|e| MemexError::EmbeddingError {
                chunk_id: "batch".to_string(),
                message: format!("Inference failed: {e}"),
            })?;

        let (shape, data) =
            outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| MemexError::EmbeddingError {
                    chunk_id: "batch".to_string(),
                    message: format!("Failed extracting output tensor: {e}"),
                })?;

        let dims: &[i64] = shape.as_ref();
        if dims.len() < 3 {
            return Err(MemexError::EmbeddingError {
                chunk_id: "batch".to_string(),
                message: format!(
                    "Expected 3D output tensor [batch, seq, dim], got shape {:?}",
                    dims
                ),
            });
        }
        let out_batch = dims[0] as usize;
        let out_seq = dims[1] as usize;
        let hidden_dim = dims[2] as usize;

        mean_pool_and_normalize(
            data,
            &tokenized.attention_mask,
            out_batch,
            out_seq,
            hidden_dim,
        )
    }

    /// Computes normalized embedding vectors for a batch of string slices, automatically
    /// partitioning into sub-batches of up to `DEFAULT_BATCH_SIZE` (64).
    pub fn embed_batch_str(&self, texts: &[&str]) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(DEFAULT_BATCH_SIZE) {
            let tokenized = self.tokenizer_wrapper.encode_batch(chunk)?;
            let chunk_embeddings = self.embed_tokenized_batch(&tokenized)?;
            all_embeddings.extend(chunk_embeddings);
        }

        Ok(all_embeddings)
    }

    /// Computes normalized embedding vectors for a batch of `String`s, automatically
    /// partitioning into sub-batches of up to `DEFAULT_BATCH_SIZE` (64).
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let str_slices: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        self.embed_batch_str(&str_slices)
    }

    /// Computes a normalized embedding vector for a single text string.
    pub fn embed(&self, text: &str) -> Result<[f32; EMBEDDING_DIM]> {
        let embeddings = self.embed_batch_str(&[text])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| MemexError::EmbeddingError {
                chunk_id: "single".to_string(),
                message: "Empty embedding result for single input".to_string(),
            })
    }
}

/// Applies attention-mask-weighted mean pooling over token hidden states of shape `[batch_size, seq_len, hidden_dim]`
/// and normalizes each pooled vector with L2 norm to produce unit vectors (`[f32; 384]`).
pub fn mean_pool_and_normalize(
    hidden_states: &[f32],
    attention_mask: &[i64],
    batch_size: usize,
    seq_len: usize,
    hidden_dim: usize,
) -> Result<Vec<[f32; EMBEDDING_DIM]>> {
    if hidden_dim != EMBEDDING_DIM {
        return Err(MemexError::EmbeddingError {
            chunk_id: "batch".to_string(),
            message: format!(
                "Unexpected embedding hidden dimension {hidden_dim}, expected {EMBEDDING_DIM}"
            ),
        });
    }

    if batch_size == 0 {
        return Ok(Vec::new());
    }

    let expected_len = batch_size * seq_len * hidden_dim;
    if hidden_states.len() != expected_len {
        return Err(MemexError::EmbeddingError {
            chunk_id: "batch".to_string(),
            message: format!(
                "Hidden states length mismatch: expected {expected_len}, got {}",
                hidden_states.len()
            ),
        });
    }

    let mut result = Vec::with_capacity(batch_size);

    for b in 0..batch_size {
        let mut pooled = [0.0f32; EMBEDDING_DIM];
        let mut mask_sum = 0.0f32;

        let mask_row_offset = b * seq_len;
        for s in 0..seq_len {
            let mask_val = attention_mask[mask_row_offset + s] as f32;
            if mask_val > 0.0 {
                mask_sum += mask_val;
                let token_offset = (b * seq_len + s) * hidden_dim;
                for d in 0..EMBEDDING_DIM {
                    pooled[d] += hidden_states[token_offset + d] * mask_val;
                }
            }
        }

        if mask_sum > 0.0 {
            let inv_mask_sum = 1.0 / mask_sum;
            for val in &mut pooled {
                *val *= inv_mask_sum;
            }
        }

        // L2 Normalization
        let dot_product: f32 = pooled.iter().map(|&x| x * x).sum();
        let norm = dot_product.sqrt();

        if norm > 1e-12 {
            let inv_norm = 1.0 / norm;
            for val in &mut pooled {
                *val *= inv_norm;
            }
        }

        result.push(pooled);
    }

    Ok(result)
}

/// Computes the cosine similarity (dot product of L2-normalized vectors) between two vectors.
pub fn cosine_similarity(a: &[f32; EMBEDDING_DIM], b: &[f32; EMBEDDING_DIM]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
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

    #[test]
    fn test_embedding_engine_missing_model_file() {
        let temp = tempdir().expect("temp dir failed");
        let tokenizer_path = temp.path().join("tokenizer.json");
        fs::write(&tokenizer_path, b"{}").expect("write tokenizer failed");

        let assets = ModelAssets {
            model_path: temp.path().join("nonexistent.onnx"),
            tokenizer_path,
        };

        let result = EmbeddingEngine::new(&assets);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::ModelLoadError(msg) => {
                assert!(msg.contains("Model file not found"));
            }
            err => panic!("Unexpected error: {err:?}"),
        }
    }

    #[test]
    fn test_embedding_engine_missing_tokenizer_file() {
        let temp = tempdir().expect("temp dir failed");
        let model_path = temp.path().join("model.onnx");
        fs::write(&model_path, b"dummy onnx").expect("write model failed");

        let assets = ModelAssets {
            model_path,
            tokenizer_path: temp.path().join("nonexistent_tokenizer.json"),
        };

        let result = EmbeddingEngine::new(&assets);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::ModelLoadError(msg) => {
                assert!(msg.contains("Tokenizer file not found"));
            }
            err => panic!("Unexpected error: {err:?}"),
        }
    }

    #[test]
    fn test_embedding_engine_invalid_tokenizer_json() {
        let temp = tempdir().expect("temp dir failed");
        let model_path = temp.path().join("model.onnx");
        let tokenizer_path = temp.path().join("tokenizer.json");
        fs::write(&model_path, b"dummy onnx").expect("write model failed");
        fs::write(&tokenizer_path, b"not valid json").expect("write tokenizer failed");

        let assets = ModelAssets {
            model_path,
            tokenizer_path,
        };

        let result = EmbeddingEngine::new(&assets);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::TokenizerError(msg) => {
                assert!(msg.contains("Failed to load tokenizer"));
            }
            err => panic!("Unexpected error: {err:?}"),
        }
    }

    #[test]
    fn test_embedding_engine_invalid_model_bytes() {
        let temp = tempdir().expect("temp dir failed");
        let model_path = temp.path().join("model.onnx");
        let tokenizer_path = temp.path().join("tokenizer.json");

        let tokenizer = tokenizers::Tokenizer::new(tokenizers::models::bpe::BPE::default());
        tokenizer
            .save(&tokenizer_path, true)
            .expect("save tokenizer failed");

        fs::write(&model_path, b"not a valid onnx protobuf").expect("write model failed");

        let assets = ModelAssets {
            model_path,
            tokenizer_path,
        };

        let result = EmbeddingEngine::with_threads(&assets, 2);
        assert!(result.is_err());
        match result.unwrap_err() {
            MemexError::ModelLoadError(msg) => {
                assert!(msg.contains("Failed loading ONNX model"));
            }
            err => panic!("Unexpected error: {err:?}"),
        }
    }

    #[test]
    fn test_tokenizer_wrapper_empty_batch() {
        let tokenizer = tokenizers::Tokenizer::new(tokenizers::models::bpe::BPE::default());
        let wrapper = TokenizerWrapper::new(Arc::new(tokenizer));

        let batch = wrapper.encode_batch(&[]).expect("empty batch encoding");
        assert_eq!(batch.batch_size, 0);
        assert_eq!(batch.seq_len, 0);
        assert!(batch.is_empty());
        assert_eq!(batch.shape(), (0, 0));
        assert!(batch.input_ids.is_empty());
        assert!(batch.attention_mask.is_empty());
        assert!(batch.token_type_ids.is_empty());
    }

    #[test]
    fn test_tokenizer_wrapper_batch_padding_and_shapes() {
        use tokenizers::models::wordpiece::WordPiece;
        use tokenizers::Tokenizer;

        let vocab = [
            ("[PAD]".to_string(), 0),
            ("[UNK]".to_string(), 1),
            ("[CLS]".to_string(), 2),
            ("[SEP]".to_string(), 3),
            ("hello".to_string(), 4),
            ("world".to_string(), 5),
            ("memex".to_string(), 6),
            ("rust".to_string(), 7),
        ];

        let wp = WordPiece::builder()
            .vocab(vocab)
            .unk_token("[UNK]".to_string())
            .build()
            .expect("wp build failed");

        let mut tokenizer = Tokenizer::new(wp);
        tokenizer.with_pre_tokenizer(Some(tokenizers::pre_tokenizers::whitespace::Whitespace));

        let wrapper = TokenizerWrapper::new(Arc::new(tokenizer));

        let texts = &["hello world", "memex", "hello world memex rust"];

        let batch = wrapper.encode_batch(texts).expect("encode batch failed");
        assert_eq!(batch.batch_size, 3);
        // "hello world memex rust" has 4 tokens
        assert_eq!(batch.seq_len, 4);
        assert_eq!(batch.shape(), (3, 4));
        assert_eq!(batch.input_ids.len(), 3 * 4);
        assert_eq!(batch.attention_mask.len(), 3 * 4);
        assert_eq!(batch.token_type_ids.len(), 3 * 4);

        // Row 0: "hello world" -> 2 tokens, 2 padded
        assert_eq!(&batch.input_ids[0..4], &[4, 5, 0, 0]);
        assert_eq!(&batch.attention_mask[0..4], &[1, 1, 0, 0]);

        // Row 1: "memex" -> 1 token, 3 padded
        assert_eq!(&batch.input_ids[4..8], &[6, 0, 0, 0]);
        assert_eq!(&batch.attention_mask[4..8], &[1, 0, 0, 0]);

        // Row 2: "hello world memex rust" -> 4 tokens, 0 padded
        assert_eq!(&batch.input_ids[8..12], &[4, 5, 6, 7]);
        assert_eq!(&batch.attention_mask[8..12], &[1, 1, 1, 1]);
    }

    #[test]
    fn test_tokenizer_wrapper_max_seq_len_truncation() {
        use tokenizers::models::wordpiece::WordPiece;
        use tokenizers::Tokenizer;

        let vocab = [
            ("[PAD]".to_string(), 0),
            ("[UNK]".to_string(), 1),
            ("a".to_string(), 2),
            ("b".to_string(), 3),
            ("c".to_string(), 4),
            ("d".to_string(), 5),
        ];

        let wp = WordPiece::builder()
            .vocab(vocab)
            .unk_token("[UNK]".to_string())
            .build()
            .expect("wp build failed");

        let mut tokenizer = Tokenizer::new(wp);
        tokenizer.with_pre_tokenizer(Some(tokenizers::pre_tokenizers::whitespace::Whitespace));

        // Set max sequence length to 2
        let wrapper = TokenizerWrapper::with_max_seq_len(Arc::new(tokenizer), 2);

        let texts = &["a b c d"];
        let batch = wrapper.encode_batch(texts).expect("encode failed");

        assert_eq!(batch.batch_size, 1);
        assert_eq!(batch.seq_len, 2);
        assert_eq!(batch.input_ids, vec![2, 3]);
        assert_eq!(batch.attention_mask, vec![1, 1]);
    }

    #[test]
    fn test_tokenizer_wrapper_single_encode() {
        use tokenizers::models::wordpiece::WordPiece;
        use tokenizers::Tokenizer;

        let vocab = [
            ("[PAD]".to_string(), 0),
            ("[UNK]".to_string(), 1),
            ("test".to_string(), 2),
        ];

        let wp = WordPiece::builder()
            .vocab(vocab)
            .unk_token("[UNK]".to_string())
            .build()
            .expect("wp build failed");

        let mut tokenizer = Tokenizer::new(wp);
        tokenizer.with_pre_tokenizer(Some(tokenizers::pre_tokenizers::whitespace::Whitespace));

        let wrapper = TokenizerWrapper::new(Arc::new(tokenizer));
        let batch = wrapper.encode("test").expect("encode failed");

        assert_eq!(batch.batch_size, 1);
        assert_eq!(batch.seq_len, 1);
        assert_eq!(batch.input_ids, vec![2]);
        assert_eq!(batch.attention_mask, vec![1]);
        assert_eq!(batch.token_type_ids, vec![0]);
    }

    #[test]
    fn test_mean_pool_and_normalize_math() {
        let batch_size = 2;
        let seq_len = 3;
        let hidden_dim = EMBEDDING_DIM;

        let mut hidden_states = vec![0.0f32; batch_size * seq_len * hidden_dim];
        // Batch 0: token 0 (value 2.0 everywhere), token 1 (value 4.0 everywhere), token 2 (value 100.0 everywhere, but masked out)
        for d in 0..hidden_dim {
            hidden_states[d] = 2.0;
            hidden_states[hidden_dim + d] = 4.0;
            hidden_states[2 * hidden_dim + d] = 100.0;
        }

        // Batch 1: token 0 (value 1.0 for d=0, 0 otherwise), token 1 (value 1.0 for d=1, 0 otherwise), token 2 (padding)
        let b1_offset = seq_len * hidden_dim;
        hidden_states[b1_offset] = 1.0;
        hidden_states[b1_offset + hidden_dim + 1] = 1.0;

        let attention_mask = vec![
            1, 1, 0, // Batch 0: first 2 tokens active, 3rd is padding
            1, 1, 0, // Batch 1: first 2 tokens active, 3rd is padding
        ];

        let result = mean_pool_and_normalize(
            &hidden_states,
            &attention_mask,
            batch_size,
            seq_len,
            hidden_dim,
        )
        .expect("mean_pool_and_normalize failed");

        assert_eq!(result.len(), 2);

        // For Batch 0: mean before normalization is (2.0 + 4.0) / 2 = 3.0 across all 384 dimensions.
        // L2 norm of [3.0; 384] = sqrt(384 * 9.0) = 3 * sqrt(384).
        // Normalized vector components are 3.0 / (3 * sqrt(384)) = 1 / sqrt(384).
        let expected_val = 1.0f32 / (384.0f32).sqrt();
        for &val in &result[0] {
            assert!((val - expected_val).abs() < 1e-5);
        }

        // Check norm of batch 0 is ~1.0
        let norm0: f32 = result[0].iter().map(|&x| x * x).sum::<f32>().sqrt();
        assert!((norm0 - 1.0).abs() < 1e-5);

        // For Batch 1: mean before normalization is [0.5, 0.5, 0, 0, ...]
        // L2 norm is sqrt(0.25 + 0.25) = sqrt(0.5) = 1 / sqrt(2).
        // Normalized components: [1/sqrt(2), 1/sqrt(2), 0, 0, ...]
        let expected_val1 = 1.0f32 / (2.0f32).sqrt();
        assert!((result[1][0] - expected_val1).abs() < 1e-5);
        assert!((result[1][1] - expected_val1).abs() < 1e-5);
        for &val in &result[1][2..] {
            assert_eq!(val, 0.0);
        }

        let norm1: f32 = result[1].iter().map(|&x| x * x).sum::<f32>().sqrt();
        assert!((norm1 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_mean_pool_and_normalize_empty_and_zero_mask() {
        // Empty batch
        let empty = mean_pool_and_normalize(&[], &[], 0, 0, EMBEDDING_DIM).expect("empty batch");
        assert!(empty.is_empty());

        // Zero mask batch
        let hidden = vec![1.0f32; 2 * EMBEDDING_DIM];
        let mask = vec![0i64, 0i64];
        let zero_res = mean_pool_and_normalize(&hidden, &mask, 1, 2, EMBEDDING_DIM)
            .expect("zero mask handling");
        assert_eq!(zero_res.len(), 1);
        assert_eq!(zero_res[0], [0.0f32; EMBEDDING_DIM]);
    }

    #[test]
    fn test_mean_pool_and_normalize_validation_errors() {
        // Wrong hidden dimension
        let err = mean_pool_and_normalize(&[0.0; 100], &[1], 1, 1, 100);
        assert!(err.is_err());

        // Buffer size mismatch
        let err2 = mean_pool_and_normalize(&[0.0; 10], &[1], 1, 1, EMBEDDING_DIM);
        assert!(err2.is_err());
    }

    #[test]
    fn test_cosine_similarity() {
        let mut v1 = [0.0f32; EMBEDDING_DIM];
        let mut v2 = [0.0f32; EMBEDDING_DIM];
        let mut v3 = [0.0f32; EMBEDDING_DIM];

        v1[0] = 1.0;
        v2[0] = 1.0;
        v3[1] = 1.0;

        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 1e-6);

        let mut v_neg = [0.0f32; EMBEDDING_DIM];
        v_neg[0] = -1.0;
        assert!((cosine_similarity(&v1, &v_neg) - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_embed_batch_empty_slice() {
        let empty_batch: Vec<String> = Vec::new();
        assert!(empty_batch.is_empty());
    }

    #[test]
    fn test_live_model_embedding_and_similarity() {
        // If model assets cannot be resolved/downloaded (e.g. in an offline CI sandbox),
        // we skip gracefully without failing the build.
        let assets = match ModelManager::ensure_model_assets() {
            Ok(assets) => assets,
            Err(e) => {
                println!("Skipping live model inference test (assets unavailable: {e})");
                return;
            }
        };

        let engine = EmbeddingEngine::new(&assets).expect("Failed to initialize EmbeddingEngine");

        let text1 = "How do I configure database connection settings?";
        let text2 = "Configuring the database connection parameters";
        let text3 = "A quick recipe for baking chocolate chip cookies in the oven";

        let embeddings = engine
            .embed_batch(&[text1.to_string(), text2.to_string(), text3.to_string()])
            .expect("embed_batch failed");

        assert_eq!(embeddings.len(), 3);

        // Verify all output vectors are unit vectors (L2 norm ~ 1.0)
        for (i, emb) in embeddings.iter().enumerate() {
            let norm: f32 = emb.iter().map(|&x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-4,
                "Vector {i} norm is {norm}, expected 1.0 +/- 1e-4"
            );
        }

        // Semantic similarity check: text1 and text2 are paraphrases, text3 is completely unrelated
        let sim_1_2 = cosine_similarity(&embeddings[0], &embeddings[1]);
        let sim_1_3 = cosine_similarity(&embeddings[0], &embeddings[2]);

        println!("Similarity between semantically related phrases: {sim_1_2:.4}");
        println!("Similarity between semantically unrelated phrases: {sim_1_3:.4}");

        assert!(
            sim_1_2 > 0.80,
            "Expected high similarity between paraphrases, got {sim_1_2}"
        );
        assert!(
            sim_1_3 < 0.40,
            "Expected low similarity between unrelated texts, got {sim_1_3}"
        );
        assert!(
            sim_1_2 > sim_1_3 + 0.40,
            "Expected paraphrase similarity ({sim_1_2}) to be much greater than unrelated ({sim_1_3})"
        );

        // Single embedding convenience method
        let single_emb = engine.embed(text1).expect("embed single failed");
        assert_eq!(single_emb, embeddings[0]);

        // Large batch chunking test (> 64 items)
        let large_batch: Vec<String> = (0..70)
            .map(|i| format!("Documentation sentence number {i} for batching verification"))
            .collect();

        let large_embeddings = engine
            .embed_batch(&large_batch)
            .expect("embed large batch failed");
        assert_eq!(large_embeddings.len(), 70);

        for (i, emb) in large_embeddings.iter().enumerate() {
            let norm: f32 = emb.iter().map(|&x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-4,
                "Vector {i} norm in large batch is {norm}, expected 1.0 +/- 1e-4"
            );
        }
    }
}
