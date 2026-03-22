//! In-process LLM inference backend.
//!
//! When compiled with the `llm-inprocess` feature, this module provides direct
//! GGUF model loading and inference via `llama-cpp-2`. Without the feature,
//! stub types are provided so the rest of the crate compiles.

/// Chat message for in-process inference.
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

/// Parameters for a single inference request.
pub struct InferenceParams {
    pub messages: Vec<ChatMsg>,
    pub temperature: f32,
    pub max_tokens: u32,
    /// Optional GBNF grammar string for constrained decoding.
    pub grammar: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature-gated implementation
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "llm-inprocess")]
mod inner {
    use std::num::NonZeroU32;
    use std::path::PathBuf;

    use anyhow::{Context, Result};
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
    use llama_cpp_2::sampling::LlamaSampler;
    use tokio::sync::Mutex;
    use tracing::{debug, info};

    use super::InferenceParams;

    /// Loaded model + backend held behind a mutex.
    struct LoadedModel {
        backend: LlamaBackend,
        model: LlamaModel,
    }

    /// In-process LLM inference engine using llama-cpp-2.
    ///
    /// The model can be loaded/unloaded to free VRAM (GPU toggle).
    /// When unloaded, all inference calls return an error.
    pub struct InProcessLlm {
        inner: Mutex<Option<LoadedModel>>,
        model_path: PathBuf,
        n_gpu_layers: u32,
        ctx_size: u32,
    }

    impl InProcessLlm {
        /// Create a new in-process LLM engine (does NOT load the model).
        pub fn new(model_path: PathBuf, n_gpu_layers: u32, ctx_size: u32) -> Self {
            Self {
                inner: Mutex::new(None),
                model_path,
                n_gpu_layers,
                ctx_size,
            }
        }

        /// Load the model into memory (and optionally GPU VRAM).
        /// Call from a blocking context (e.g. spawn_blocking or std::thread).
        pub fn load_sync(&self) -> Result<()> {
            let mut guard = self.inner.blocking_lock();
            if guard.is_some() {
                info!("Model already loaded, skipping");
                return Ok(());
            }

            info!(
                path = %self.model_path.display(),
                gpu_layers = self.n_gpu_layers,
                "Loading GGUF model in-process"
            );

            let backend = LlamaBackend::init()
                .map_err(|e| anyhow::anyhow!("Failed to initialize llama backend: {e}"))?;

            let model_params = LlamaModelParams::default()
                .with_n_gpu_layers(self.n_gpu_layers);

            let model = LlamaModel::load_from_file(&backend, &self.model_path, &model_params)
                .map_err(|e| anyhow::anyhow!("Failed to load model: {e}"))?;

            let n_params = model.n_params();
            let size_mb = model.size() / (1024 * 1024);
            info!(params = n_params, size_mb = size_mb, "Model loaded successfully");

            *guard = Some(LoadedModel { backend, model });
            Ok(())
        }

        /// Unload the model from memory, freeing VRAM.
        pub async fn unload(&self) {
            let mut guard = self.inner.lock().await;
            if guard.is_some() {
                info!("Unloading model from memory");
                *guard = None;
                info!("Model unloaded — VRAM freed");
            } else {
                info!("Model already unloaded");
            }
        }

        /// Check if the model is currently loaded.
        pub async fn is_loaded(&self) -> bool {
            self.inner.lock().await.is_some()
        }

        /// Run inference synchronously (call from spawn_blocking).
        pub fn infer_sync(&self, params: &InferenceParams) -> Result<(String, u32)> {
            let guard = self.inner.blocking_lock();
            let loaded = guard.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Model not loaded (GPU paused?)"))?;

            let model = &loaded.model;
            let backend = &loaded.backend;

            // Build chat messages for the template
            let chat_messages: Vec<LlamaChatMessage> = params.messages.iter()
                .map(|m| LlamaChatMessage::new(m.role.clone(), m.content.clone())
                    .map_err(|e| anyhow::anyhow!("Invalid chat message: {e}")))
                .collect::<Result<Vec<_>>>()?;

            // Apply the model's chat template
            let chat_template = model.chat_template(None)
                .map_err(|e| anyhow::anyhow!("Failed to get chat template: {e}"))?;

            let prompt = model.apply_chat_template(&chat_template, &chat_messages, true)
                .map_err(|e| anyhow::anyhow!("Failed to apply chat template: {e}"))?;

            debug!(prompt_len = prompt.len(), "Chat template applied");

            // Tokenize the prompt
            let tokens = model.str_to_token(&prompt, AddBos::Never)
                .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;

            let n_prompt_tokens = tokens.len();
            debug!(n_tokens = n_prompt_tokens, "Prompt tokenized");

            // Context size: prompt + max completion tokens, capped at configured max
            let total_ctx = (n_prompt_tokens as u32 + params.max_tokens).min(self.ctx_size);

            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(total_ctx))
                .with_n_batch(512);

            let mut ctx = model.new_context(backend, ctx_params)
                .map_err(|e| anyhow::anyhow!("Failed to create context: {e}"))?;

            // Build the sampler chain
            let mut sampler = Self::build_sampler(model, params)?;

            // Create batch and add prompt tokens
            let mut batch = LlamaBatch::new(512, 1);
            let last_idx = (n_prompt_tokens - 1) as i32;
            for (i, token) in tokens.iter().enumerate() {
                let is_last = i as i32 == last_idx;
                batch.add(*token, i as i32, &[0], is_last)
                    .map_err(|e| anyhow::anyhow!("Batch add failed: {e}"))?;
            }

            // Decode the prompt
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("Prompt decode failed: {e}"))?;

            // Generate tokens
            let mut output = String::new();
            let mut n_decoded: u32 = 0;
            let mut n_cur = n_prompt_tokens as i32;
            let mut decoder = encoding_rs::UTF_8.new_decoder();

            loop {
                if n_decoded >= params.max_tokens {
                    break;
                }

                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);

                if model.is_eog_token(token) {
                    break;
                }

                let piece = model.token_to_piece(token, &mut decoder, true, None)
                    .map_err(|e| anyhow::anyhow!("Token to piece failed: {e}"))?;
                output.push_str(&piece);

                n_decoded += 1;

                batch.clear();
                batch.add(token, n_cur, &[0], true)
                    .map_err(|e| anyhow::anyhow!("Batch add failed: {e}"))?;

                ctx.decode(&mut batch)
                    .map_err(|e| anyhow::anyhow!("Decode failed: {e}"))?;

                n_cur += 1;
            }

            debug!(completion_tokens = n_decoded, output_len = output.len(), "Inference complete");
            Ok((output, n_decoded))
        }

        /// Build a sampler chain with temperature and optional grammar.
        fn build_sampler(model: &LlamaModel, params: &InferenceParams) -> Result<LlamaSampler> {
            let mut samplers: Vec<LlamaSampler> = Vec::new();

            // Grammar constraint (if specified)
            if let Some(ref grammar_str) = params.grammar {
                let grammar_sampler = LlamaSampler::grammar(model, grammar_str, "root")
                    .map_err(|e| anyhow::anyhow!("Grammar init failed: {e}"))?;
                samplers.push(grammar_sampler);
            }

            // Temperature-based sampling
            if params.temperature < 0.01 {
                // Greedy decoding for temperature ~0
                samplers.push(LlamaSampler::greedy());
            } else {
                samplers.push(LlamaSampler::top_k(40));
                samplers.push(LlamaSampler::top_p(0.95, 1));
                samplers.push(LlamaSampler::temp(params.temperature));
                samplers.push(LlamaSampler::dist(1234));
            }

            Ok(LlamaSampler::chain_simple(samplers))
        }
    }

    impl std::fmt::Debug for InProcessLlm {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("InProcessLlm")
                .field("model_path", &self.model_path)
                .field("n_gpu_layers", &self.n_gpu_layers)
                .field("ctx_size", &self.ctx_size)
                .finish()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Stub implementation when llm-inprocess feature is not enabled
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(not(feature = "llm-inprocess"))]
mod inner {
    use std::path::PathBuf;
    use anyhow::Result;

    use super::InferenceParams;

    /// Stub: in-process LLM is not available without the `llm-inprocess` feature.
    pub struct InProcessLlm {
        _private: (),
    }

    impl InProcessLlm {
        pub fn new(_model_path: PathBuf, _n_gpu_layers: u32, _ctx_size: u32) -> Self {
            Self { _private: () }
        }

        pub fn load_sync(&self) -> Result<()> {
            anyhow::bail!("In-process LLM not available (compiled without llm-inprocess feature)")
        }

        pub async fn unload(&self) {}

        pub async fn is_loaded(&self) -> bool {
            false
        }

        pub fn infer_sync(&self, _params: &InferenceParams) -> Result<(String, u32)> {
            anyhow::bail!("In-process LLM not available (compiled without llm-inprocess feature)")
        }
    }

    impl std::fmt::Debug for InProcessLlm {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("InProcessLlm").field("available", &false).finish()
        }
    }
}

pub use inner::InProcessLlm;
