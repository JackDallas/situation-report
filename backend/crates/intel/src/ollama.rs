use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::prompts;
use crate::types::{ArticleInput, EnrichedArticleV2};

/// Client for local Ollama LLM inference (enrichment on GPU).
/// Uses a semaphore to serialize requests — a single GPU can only run one
/// inference at a time efficiently. Without this, 150 concurrent RSS articles
/// would all hit Ollama simultaneously, causing GPU memory thrashing and
/// fan spikes followed by idle periods.
pub struct OllamaClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    /// Limits concurrent GPU inference requests. Default: 1 (serial).
    gpu_semaphore: Semaphore,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
    options: ChatOptions,
    /// Disable thinking mode (Qwen3.5+ emits <think> tokens by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatOptions {
    temperature: f32,
    num_ctx: u32,
    /// Cap output token count (e.g. 50 for titles). None = model default.
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ResponseMessage,
    #[serde(default)]
    eval_count: u32,
    #[serde(default)]
    prompt_eval_count: u32,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

/// JSON Schema for structured output — matches EnrichedArticleV2.
fn enrichment_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "translated_title": {"type": "string"},
            "summary": {"type": "string"},
            "original_language": {"type": "string"},
            "entities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "entity_type": {"type": "string"},
                        "role": {"type": "string"},
                        "wikidata_qid": {"type": "string"}
                    },
                    "required": ["name", "entity_type"]
                }
            },
            "relationships": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "source": {"type": "string"},
                        "target": {"type": "string"},
                        "type": {"type": "string"},
                        "confidence": {"type": "number"}
                    },
                    "required": ["source", "target", "type"]
                }
            },
            "state_changes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "entity": {"type": "string"},
                        "attribute": {"type": "string"},
                        "from": {"type": "string"},
                        "to": {"type": "string"},
                        "certainty": {"type": "string"}
                    },
                    "required": ["entity", "attribute", "to"]
                }
            },
            "topics": {
                "type": "array",
                "items": {"type": "string"}
            },
            "relevance_score": {"type": "number"},
            "sentiment": {"type": "number"},
            "inferred_location": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "lat": {"type": "number"},
                    "lon": {"type": "number"}
                },
                "required": ["name", "lat", "lon"]
            }
        },
        "required": ["translated_title", "summary", "original_language", "entities", "topics", "relevance_score", "sentiment"]
    })
}

impl OllamaClient {
    /// Create from environment variables (OLLAMA_URL, OLLAMA_MODEL).
    /// Returns None if OLLAMA_URL is not set.
    pub fn from_env() -> Option<Arc<Self>> {
        let base_url = std::env::var("OLLAMA_URL").ok()?;
        let model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "qwen2.5:7b".to_string());

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120)) // local inference can be slow on first call
            .build()
            .expect("Failed to build HTTP client");

        info!(url = %base_url, model = %model, "Ollama client initialized (serial GPU queue)");

        Some(Arc::new(Self { http, base_url, model, gpu_semaphore: Semaphore::new(1) }))
    }

    /// Check if the model is loaded and ready.
    pub async fn is_ready(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        match self.http.get(&url).send().await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(models) = body.get("models").and_then(|m| m.as_array()) {
                        return models.iter().any(|m| {
                            m.get("name")
                                .and_then(|n| n.as_str())
                                .is_some_and(|n| n.starts_with(&self.model))
                        });
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Lightweight health check: queries /api/tags to verify Ollama is responsive
    /// without triggering GPU work (no model inference, no contention with BGE-M3).
    /// Returns true if Ollama responds within 15 seconds.
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            self.http.get(&url).send(),
        )
        .await;

        match result {
            Ok(Ok(resp)) => resp.status().is_success(),
            Ok(Err(_)) => false, // request error
            Err(_) => false,     // timeout
        }
    }

    /// Force-load the model into GPU memory by sending a minimal generation request.
    /// This counteracts Ollama's automatic model unloading.
    pub async fn warm_model(&self) -> Result<()> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": "warmup",
            "stream": false,
            "options": { "num_predict": 1 }
        });

        let url = format!("{}/api/generate", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Ollama warm_model request failed")?;

        if resp.status().is_success() {
            info!(model = %self.model, "Ollama model warmed successfully");
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(model = %self.model, status = %status, "Ollama warm_model failed: {body}");
            bail!("Ollama warm_model failed: {status}")
        }
    }

    /// Enrich a news article using local LLM inference.
    /// Acquires GPU semaphore to serialize requests — prevents thrashing.
    pub async fn enrich_article(&self, article: &ArticleInput) -> Result<EnrichedArticleV2> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let user_msg = prompts::enrichment_user(
            &article.title,
            &article.description,
            article.source_country.as_deref(),
            article.language_hint.as_deref(),
            article.source_type.as_deref(),
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: prompts::ENRICHMENT_SYSTEM.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_msg,
                },
            ],
            stream: false,
            format: Some(enrichment_schema()),
            options: ChatOptions {
                temperature: 0.0,
                num_ctx: 4096,
                num_predict: None,
            },
            think: Some(false),
        };

        let url = format!("{}/api/chat", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Ollama request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse Ollama response")?;

        let tokens_used = chat_resp.prompt_eval_count + chat_resp.eval_count;
        debug!(
            model = %self.model,
            tokens = tokens_used,
            "Ollama enrichment complete"
        );

        let mut enriched: EnrichedArticleV2 = serde_json::from_str(&chat_resp.message.content)
            .context("Failed to parse enrichment JSON from Ollama")?;

        enriched.model = self.model.clone();
        enriched.tokens_used = tokens_used;

        Ok(enriched)
    }

    /// Simple text completion (no structured output). Used for title generation, etc.
    /// Thinking disabled — titles are short and simple, and `num_predict` limits
    /// total output tokens (thinking + content). With `think: true`, Qwen 3.5
    /// spends all tokens on `<think>` reasoning and returns empty content.
    pub async fn complete_text(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            stream: false,
            format: None,
            options: ChatOptions {
                temperature: 0.0,
                num_ctx: max_tokens.max(2048),
                num_predict: Some(50),
            },
            think: Some(false),
        };

        let url = format!("{}/api/chat", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Ollama request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse Ollama response")?;

        Ok(strip_think_tags(&chat_resp.message.content))
    }

    /// Generate a situation narrative using local LLM.
    /// Thinking mode enabled — produces more analytical, coherent narratives.
    pub async fn generate_narrative(&self, system: &str, user: &str) -> Result<(String, u32)> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            stream: false,
            format: None,
            options: ChatOptions {
                temperature: 0.1,
                num_ctx: 8192,
                num_predict: None,
            },
            think: Some(true),
        };

        let url = format!("{}/api/chat", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Ollama narrative request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse Ollama narrative response")?;

        let tokens = chat_resp.prompt_eval_count + chat_resp.eval_count;
        debug!(model = %self.model, tokens, "Ollama narrative complete");
        Ok((strip_think_tags(&chat_resp.message.content), tokens))
    }

    /// Run periodic analysis using local LLM with structured JSON output.
    /// Returns (json_string, tokens_used, escalate_flag).
    pub async fn analyze(&self, system: &str, user: &str) -> Result<(String, u32)> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        // Use the analysis schema for structured output
        let schema = analysis_schema();

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            stream: false,
            format: Some(schema),
            options: ChatOptions {
                temperature: 0.1,
                num_ctx: 8192,
                num_predict: None,
            },
            think: Some(false),
        };

        let url = format!("{}/api/chat", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Ollama analysis request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse Ollama analysis response")?;

        let tokens = chat_resp.prompt_eval_count + chat_resp.eval_count;
        debug!(model = %self.model, tokens, "Ollama analysis complete");
        Ok((chat_resp.message.content, tokens))
    }

    /// Audit a merge: ask if two situations are about the same event.
    /// Returns true if the merge is valid.
    pub async fn audit_merge(
        &self,
        parent_title: &str,
        parent_topics: &[String],
        child_title: &str,
        child_topics: &[String],
    ) -> Result<bool> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let user_msg = format!(
            "Are these two situations about the same underlying event or conflict?\n\n\
             Situation A: {}\nTopics: {}\n\n\
             Situation B: {}\nTopics: {}\n\n\
             Think carefully about whether these are truly the same situation or just \
             superficially similar (e.g. same region but different events). \
             After reasoning, answer ONLY 'yes' or 'no'.",
            parent_title,
            parent_topics.join(", "),
            child_title,
            child_topics.join(", "),
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "You evaluate whether two intelligence situations describe the same underlying event. Think step by step, then answer only 'yes' or 'no'.".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_msg,
                },
            ],
            stream: false,
            format: None,
            options: ChatOptions {
                temperature: 0.0,
                num_ctx: 2048,
                num_predict: Some(50),
            },
            think: Some(false),
        };

        let url = format!("{}/api/chat", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Ollama merge audit request failed")?;

        if !resp.status().is_success() {
            // On error, assume merge is valid (don't undo)
            return Ok(true);
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse Ollama merge audit response")?;

        let answer = strip_think_tags(&chat_resp.message.content).trim().to_lowercase();
        Ok(answer.starts_with("yes"))
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Strip any leaked `<think>...</think>` tags from model output.
/// Ollama normally puts thinking in a separate `thinking` field, but as a safety
/// net we also strip it from content in case of model/version differences.
fn strip_think_tags(content: &str) -> String {
    let s = content.trim();
    if let Some(start) = s.find("<think>") {
        if let Some(end) = s.find("</think>") {
            let before = &s[..start];
            let after = &s[end + 8..];
            return format!("{}{}", before, after).trim().to_string();
        }
    }
    s.to_string()
}

/// JSON Schema for structured analysis output.
fn analysis_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "narrative": {"type": "string"},
            "suggested_merges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "incident_a_id": {"type": "string"},
                        "incident_b_id": {"type": "string"},
                        "confidence": {"type": "number"},
                        "reason": {"type": "string"},
                        "suggested_title": {"type": "string"}
                    },
                    "required": ["incident_a_id", "incident_b_id", "confidence", "reason"]
                }
            },
            "topic_clusters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": {"type": "string"},
                        "topics": {"type": "array", "items": {"type": "string"}},
                        "event_count": {"type": "number"},
                        "regions": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["label", "topics", "event_count"]
                }
            },
            "escalation_assessment": {"type": "string"},
            "key_entities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "entity_name": {"type": "string"},
                        "entity_type": {"type": "string"},
                        "source_count": {"type": "number"},
                        "context": {"type": "string"}
                    },
                    "required": ["entity_name", "entity_type"]
                }
            },
            "escalate": {"type": "boolean"}
        },
        "required": ["narrative", "escalation_assessment", "escalate"]
    })
}
