use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use fastembed::{InitOptions, TextEmbedding};
use tracing::info;

/// Wrapper around fastembed's BGE-M3 model (1024-dim, multilingual).
///
/// The model is wrapped in `Mutex` because `TextEmbedding` is not `Sync`.
/// All embedding calls go through `spawn_blocking` so the mutex is only held
/// briefly on the blocking thread pool.
pub struct EmbeddingModel {
    inner: Mutex<TextEmbedding>,
}

// Safety: we only access `inner` through spawn_blocking, and the Mutex
// ensures exclusive access. TextEmbedding is Send.
unsafe impl Sync for EmbeddingModel {}

impl EmbeddingModel {
    /// Try to initialize BGE-M3. Downloads ~600MB on first run.
    /// Attempts CUDA (GPU) first, falls back to CPU automatically.
    pub fn try_new() -> Result<Self> {
        let mut options = InitOptions::new(fastembed::EmbeddingModel::BGEM3)
            .with_show_download_progress(true);

        // Set cache directory if configured
        if let Ok(dir) = std::env::var("EMBEDDINGS_CACHE_DIR")
            && !dir.is_empty()
        {
            info!(cache_dir = %dir, "Using custom embeddings cache directory");
            options = options.with_cache_dir(PathBuf::from(dir));
        }

        // Request CUDA EP — ort silently falls back to CPU if CUDA unavailable
        options = options.with_execution_providers(vec![
            ort::ep::CUDA::default().build(),
        ]);

        let model = TextEmbedding::try_new(options).context("Failed to initialize BGE-M3 model")?;

        // Log which execution provider is active
        info!("BGE-M3 loaded (CUDA requested with CPU fallback)");

        Ok(Self {
            inner: Mutex::new(model),
        })
    }

    /// Embed a batch of texts. Call from `spawn_blocking` to avoid blocking
    /// the async runtime.
    pub fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut model = self.inner.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {e}"))?;
        let embeddings = model
            .embed(texts, None)
            .context("BGE-M3 embedding failed")?;
        Ok(embeddings)
    }
}
