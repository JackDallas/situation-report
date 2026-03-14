use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{debug, info};

use crate::prompts;
use crate::types::{ArticleInput, EnrichedArticleV2};

/// Client for local LLM inference via OpenAI-compatible API (llama-server).
/// Uses a semaphore to serialize requests — a single GPU can only run one
/// inference at a time efficiently.
pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    /// Limits concurrent GPU inference requests. Default: 1 (serial).
    gpu_semaphore: Semaphore,
}

#[derive(Serialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    /// Controls Qwen 3.5 thinking mode. Set to false for direct output
    /// (titles, enrichment, analysis). True for narratives that benefit
    /// from chain-of-thought reasoning.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<bool>,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    type_: String, // "json_schema"
    json_schema: JsonSchemaWrapper,
}

#[derive(Serialize)]
struct JsonSchemaWrapper {
    name: String,
    strict: bool,
    schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    completion_tokens: Option<u32>,
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

impl LlmClient {
    /// Create from environment variables.
    /// Reads LLM_URL (required), falls back to OLLAMA_URL for backwards compat.
    /// Returns None if neither is set.
    pub fn from_env() -> Option<Arc<Self>> {
        let base_url = std::env::var("LLM_URL")
            .or_else(|_| std::env::var("OLLAMA_URL"))
            .ok()?;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        info!(url = %base_url, "LLM client initialized (serial GPU queue)");

        Some(Arc::new(Self { http, base_url, gpu_semaphore: Semaphore::new(1) }))
    }

    /// Check if llama-server is ready. GET {base_url}/health returns 200 when ready.
    pub async fn is_ready(&self) -> bool {
        self.health_check().await
    }

    /// Lightweight health check: GET /health with 15s timeout.
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            self.http.get(&url).send(),
        )
        .await;

        match result {
            Ok(Ok(resp)) => resp.status().is_success(),
            Ok(Err(_)) => false,
            Err(_) => false,
        }
    }

    /// Enrich a news article using local LLM inference.
    /// Acquires GPU semaphore to serialize requests.
    pub async fn enrich_article(&self, article: &ArticleInput) -> Result<EnrichedArticleV2> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let user_msg = format!("{}\n/no_think", prompts::enrichment_user(
            &article.title,
            &article.description,
            article.source_country.as_deref(),
            article.language_hint.as_deref(),
            article.source_type.as_deref(),
        ));

        let request = ChatRequest {
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
            temperature: Some(0.0),
            max_tokens: Some(2048),
            response_format: Some(ResponseFormat {
                type_: "json_schema".to_string(),
                json_schema: JsonSchemaWrapper {
                    name: "enrichment".to_string(),
                    strict: true,
                    schema: enrichment_schema(),
                },
            }),
            thinking: Some(false),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("LLM API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("");

        let tokens_used = chat_resp.usage
            .as_ref()
            .and_then(|u| u.completion_tokens)
            .unwrap_or(0);

        debug!(tokens = tokens_used, "LLM enrichment complete");

        let mut enriched: EnrichedArticleV2 = serde_json::from_str(content)
            .context("Failed to parse enrichment JSON from LLM")?;

        enriched.model = "llama-server".to_string();
        enriched.tokens_used = tokens_used;

        Ok(enriched)
    }

    /// Simple text completion (no structured output). Used for title generation, etc.
    pub async fn complete_text(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        // Append /no_think to suppress Qwen 3.5 chain-of-thought reasoning,
        // which consumes all output tokens leaving no room for actual content.
        let user_content = format!("{user}\n/no_think");

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_content,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(max_tokens),
            response_format: None,
            thinking: Some(false),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("LLM API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("");

        Ok(strip_think_tags(content))
    }

    /// Generate a situation narrative using local LLM.
    /// Returns (content, tokens_used).
    pub async fn generate_narrative(&self, system: &str, user: &str) -> Result<(String, u32)> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let request = ChatRequest {
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
            temperature: Some(0.1),
            max_tokens: None,
            response_format: None,
            thinking: Some(true),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM narrative request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("LLM API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM narrative response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("");

        let tokens = chat_resp.usage
            .as_ref()
            .and_then(|u| u.completion_tokens)
            .unwrap_or(0);

        debug!(tokens, "LLM narrative complete");
        Ok((strip_think_tags(content), tokens))
    }

    /// Run periodic analysis using local LLM with structured JSON output.
    /// Returns (json_string, tokens_used).
    pub async fn analyze(&self, system: &str, user: &str) -> Result<(String, u32)> {
        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let user_content = format!("{user}\n/no_think");

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_content,
                },
            ],
            temperature: Some(0.1),
            max_tokens: Some(4096),
            response_format: Some(ResponseFormat {
                type_: "json_schema".to_string(),
                json_schema: JsonSchemaWrapper {
                    name: "analysis".to_string(),
                    strict: true,
                    schema: analysis_schema(),
                },
            }),
            thinking: Some(false),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM analysis request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("LLM API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM analysis response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let tokens = chat_resp.usage
            .as_ref()
            .and_then(|u| u.completion_tokens)
            .unwrap_or(0);

        debug!(tokens, "LLM analysis complete");
        Ok((content, tokens))
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
             Answer ONLY 'yes' or 'no'.\n/no_think",
            parent_title,
            parent_topics.join(", "),
            child_title,
            child_topics.join(", "),
        );

        let request = ChatRequest {
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
            temperature: Some(0.0),
            max_tokens: Some(100),
            response_format: None,
            thinking: Some(false),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM merge audit request failed")?;

        if !resp.status().is_success() {
            // On error, assume merge is valid (don't undo)
            return Ok(true);
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM merge audit response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("");

        let answer = strip_think_tags(content).trim().to_lowercase();
        Ok(answer.starts_with("yes"))
    }
}

/// Strip any leaked `<think>...</think>` tags from model output.
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
