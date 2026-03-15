use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::prompts;
use crate::types::{ArticleInput, EnrichedArticleV2};

/// Client for local LLM inference via OpenAI-compatible API (llama-server).
/// Uses a semaphore to serialize requests — a single GPU can only run one
/// inference at a time efficiently.
///
/// Supports two structured output modes (controlled by `LLM_USE_GBNF` env var):
///   - **JSON Schema** (default): sends `response_format` with `json_schema` type.
///     llama-server internally converts this to a GBNF grammar via its built-in
///     `json_schema_to_grammar` converter. This is the OpenAI-compatible path.
///   - **GBNF Grammar** (`LLM_USE_GBNF=1`): sends hand-written GBNF grammar strings
///     directly via the `grammar` field. Bypasses the server's schema-to-grammar
///     converter for tighter control. llama-server's `/v1/chat/completions` endpoint
///     passes through `/completion`-specific fields including `grammar`.
pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    /// Limits concurrent GPU inference requests. Default: 1 (serial).
    gpu_semaphore: Semaphore,
    /// When true, send GBNF grammar directly instead of response_format json_schema.
    use_gbnf: bool,
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
    /// GBNF grammar string for constrained decoding.
    /// Mutually exclusive with response_format — llama-server rejects requests
    /// that contain both `grammar` and `json_schema`.
    #[serde(skip_serializing_if = "Option::is_none")]
    grammar: Option<String>,
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
    ///
    /// Set `LLM_USE_GBNF=1` to send hand-written GBNF grammars instead of
    /// JSON Schema `response_format`. Both paths produce grammar-constrained
    /// output — the difference is whether the grammar is generated server-side
    /// (from JSON schema) or client-side (pre-written GBNF constants).
    pub fn from_env() -> Option<Arc<Self>> {
        let base_url = std::env::var("LLM_URL")
            .or_else(|_| std::env::var("OLLAMA_URL"))
            .ok()?;

        let use_gbnf = std::env::var("LLM_USE_GBNF")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        let mode = if use_gbnf { "GBNF grammar" } else { "JSON schema" };
        info!(url = %base_url, structured_output = mode, "LLM client initialized (serial GPU queue)");

        Some(Arc::new(Self { http, base_url, gpu_semaphore: Semaphore::new(1), use_gbnf }))
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

        let (response_format, grammar) = if self.use_gbnf {
            (None, Some(ENRICHMENT_GBNF.to_string()))
        } else {
            (Some(ResponseFormat {
                type_: "json_schema".to_string(),
                json_schema: JsonSchemaWrapper {
                    name: "enrichment".to_string(),
                    strict: true,
                    schema: enrichment_schema(),
                },
            }), None)
        };

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
            response_format,
            grammar,
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
            grammar: None,
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
            grammar: None,
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

        let (response_format, grammar) = if self.use_gbnf {
            (None, Some(ANALYSIS_GBNF.to_string()))
        } else {
            (Some(ResponseFormat {
                type_: "json_schema".to_string(),
                json_schema: JsonSchemaWrapper {
                    name: "analysis".to_string(),
                    strict: true,
                    schema: analysis_schema(),
                },
            }), None)
        };

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
            response_format,
            grammar,
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

    /// Batch consolidation: ask the LLM to group related situations.
    ///
    /// Takes numbered situation titles and returns groups of indices that
    /// should be merged together. Each group becomes one parent situation.
    ///
    /// Returns `Vec<Vec<usize>>` where each inner Vec is a group of 1-based
    /// indices that the LLM says should be merged.
    pub async fn consolidate_situations(&self, titles: &[(usize, &str)]) -> Result<Vec<Vec<usize>>> {
        if titles.len() < 2 {
            return Ok(Vec::new());
        }

        let _permit = self.gpu_semaphore.acquire().await
            .map_err(|_| anyhow::anyhow!("GPU semaphore closed"))?;

        let numbered: Vec<String> = titles.iter()
            .map(|(idx, title)| format!("{}. {}", idx, title))
            .collect();
        let titles_block = numbered.join("\n");

        let user_msg = format!(
            "These intelligence situations may overlap or fragment. Group them by number \u{2014} \
             situations in the same group should be merged into one parent situation.\n\
             Return groups as JSON: {{\"groups\": [[1,3,5], [2,4]]}}\n\n\
             Rules:\n\
             - Group situations about the SAME country/region AND the same broad conflict or crisis\n\
             - Group near-duplicate titles (e.g. 'Iran President Raisi Death' and 'Iran President Raisi Dies in Plane Crash')\n\
             - Group different facets of one crisis (e.g. 'Iran Executes Spy' + 'Iranian Judiciary Crackdown' = same Iranian domestic policy)\n\
             - Do NOT group situations from different countries/regions just because they share a keyword\n\
             - Prefer larger groups when situations clearly belong together\n\
             - If no situations should be merged, return {{\"groups\": []}}\n\n\
             {}\n/no_think",
            titles_block
        );

        let grammar = if self.use_gbnf {
            Some(CONSOLIDATION_GBNF.to_string())
        } else {
            None
        };

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "You are an intelligence analyst. You group related situation reports \
                              that describe the same underlying conflict, crisis, or event. \
                              Output ONLY valid JSON.".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_msg,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(1024),
            response_format: None,
            grammar,
            thinking: Some(false),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self.http
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("LLM consolidation request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("LLM API error {status}: {body}");
        }

        let chat_resp: ChatResponse = resp.json().await
            .context("Failed to parse LLM consolidation response")?;

        let content = chat_resp.choices.first()
            .and_then(|c| c.message.content.as_deref())
            .unwrap_or("");

        let content = strip_think_tags(content);

        // Parse JSON — extract the "groups" array
        // Strip markdown code fences if present, then find JSON object
        let stripped = if content.contains("```") {
            content
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
        } else {
            &content
        };
        let json_str = if let Some(start) = stripped.find('{') {
            if let Some(end) = stripped.rfind('}') {
                &stripped[start..=end]
            } else {
                stripped
            }
        } else {
            stripped
        };

        #[derive(Deserialize)]
        struct ConsolidationResponse {
            groups: Vec<Vec<usize>>,
        }

        match serde_json::from_str::<ConsolidationResponse>(json_str) {
            Ok(parsed) => {
                // Filter out single-element groups and validate indices
                let valid_groups: Vec<Vec<usize>> = parsed.groups.into_iter()
                    .filter(|g| g.len() >= 2)
                    .collect();
                debug!(group_count = valid_groups.len(), "LLM consolidation parsed groups");
                Ok(valid_groups)
            }
            Err(e) => {
                warn!(error = %e, raw = %content, "Failed to parse LLM consolidation JSON — merge data lost");
                Ok(Vec::new())
            }
        }
    }

    /// Audit a merge: ask if two situations are about the same conflict area.
    /// Returns true if the merge is valid.
    /// Defaults to ACCEPT — only an explicit "no" rejects.
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
            "Should these two intelligence situations be grouped together?\n\n\
             Situation A: {}\nTopics: {}\n\n\
             Situation B: {}\nTopics: {}\n\n\
             Group them if they cover the same conflict, crisis, or topic area — \
             even if they describe different specific incidents within that area.\n\
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
                    content: "You decide whether two intelligence situations belong together. Answer 'yes' or 'no'. When in doubt, answer 'yes'.".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_msg,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(16),
            response_format: None,
            grammar: None,
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
        // Default to accept — only explicit "no" rejects the merge.
        // This prevents empty responses, thinking artifacts, or ambiguous
        // output from causing fragmentation.
        let rejected = answer.starts_with("no") || answer.starts_with("**no");
        debug!(parent = parent_title, child = child_title, raw = content, accepted = !rejected, "Merge audit result");
        Ok(!rejected)
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

// ---------------------------------------------------------------------------
// GBNF Grammars
// ---------------------------------------------------------------------------
// Hand-written GBNF grammars that mirror enrichment_schema() and
// analysis_schema(). Used when LLM_USE_GBNF=1 to bypass llama-server's
// internal json_schema_to_grammar converter for tighter control.
//
// These grammars enforce the same required/optional field structure as the
// JSON schemas above. Primitives are defined once at the bottom and reused.
// ---------------------------------------------------------------------------

/// GBNF grammar for enrichment structured output.
/// Required fields: translated_title, summary, original_language, entities, topics,
///                  relevance_score, sentiment
/// Optional fields: relationships, state_changes, inferred_location
const ENRICHMENT_GBNF: &str = r#"
root ::= "{" ws enrichment-body ws "}"

enrichment-body ::= (
  "\"translated_title\"" ws ":" ws string ws "," ws
  "\"summary\"" ws ":" ws string ws "," ws
  "\"original_language\"" ws ":" ws string ws "," ws
  "\"entities\"" ws ":" ws entity-array ws "," ws
  enrichment-optional
  "\"topics\"" ws ":" ws string-array ws "," ws
  "\"relevance_score\"" ws ":" ws number ws "," ws
  "\"sentiment\"" ws ":" ws number
)

enrichment-optional ::= (
  ("\"relationships\"" ws ":" ws relationship-array ws "," ws)?
  ("\"state_changes\"" ws ":" ws state-change-array ws "," ws)?
  ("\"inferred_location\"" ws ":" ws inferred-location ws "," ws)?
)

entity-array ::= "[" ws "]" | "[" ws entity (ws "," ws entity)* ws "]"
entity ::= "{" ws "\"name\"" ws ":" ws string ws "," ws "\"entity_type\"" ws ":" ws string (ws "," ws entity-opt-fields)? ws "}"
entity-opt-fields ::= ("\"role\"" ws ":" ws string (ws "," ws "\"wikidata_qid\"" ws ":" ws string)?) | ("\"wikidata_qid\"" ws ":" ws string)

relationship-array ::= "[" ws "]" | "[" ws relationship (ws "," ws relationship)* ws "]"
relationship ::= "{" ws "\"source\"" ws ":" ws string ws "," ws "\"target\"" ws ":" ws string ws "," ws "\"type\"" ws ":" ws string (ws "," ws "\"confidence\"" ws ":" ws number)? ws "}"

state-change-array ::= "[" ws "]" | "[" ws state-change (ws "," ws state-change)* ws "]"
state-change ::= "{" ws "\"entity\"" ws ":" ws string ws "," ws "\"attribute\"" ws ":" ws string ws "," ws state-change-opt ws "\"to\"" ws ":" ws string (ws "," ws "\"certainty\"" ws ":" ws string)? ws "}"
state-change-opt ::= ("\"from\"" ws ":" ws string ws "," ws)?

inferred-location ::= "{" ws "\"name\"" ws ":" ws string ws "," ws "\"lat\"" ws ":" ws number ws "," ws "\"lon\"" ws ":" ws number ws "}"

string-array ::= "[" ws "]" | "[" ws string (ws "," ws string)* ws "]"
string ::= "\"" ([^"\\] | "\\" .)* "\""
number ::= "-"? [0-9]+ ("." [0-9]+)? ([eE] [+-]? [0-9]+)?
ws ::= [ \t\n\r]*
"#;

/// GBNF grammar for analysis structured output.
/// Required fields: narrative, escalation_assessment, escalate
/// Optional fields: suggested_merges, topic_clusters, key_entities
const ANALYSIS_GBNF: &str = r#"
root ::= "{" ws analysis-body ws "}"

analysis-body ::= (
  "\"narrative\"" ws ":" ws string ws "," ws
  analysis-optional
  "\"escalation_assessment\"" ws ":" ws string ws "," ws
  "\"escalate\"" ws ":" ws boolean
)

analysis-optional ::= (
  ("\"suggested_merges\"" ws ":" ws merge-array ws "," ws)?
  ("\"topic_clusters\"" ws ":" ws cluster-array ws "," ws)?
  ("\"key_entities\"" ws ":" ws key-entity-array ws "," ws)?
)

merge-array ::= "[" ws "]" | "[" ws merge-item (ws "," ws merge-item)* ws "]"
merge-item ::= "{" ws "\"incident_a_id\"" ws ":" ws string ws "," ws "\"incident_b_id\"" ws ":" ws string ws "," ws "\"confidence\"" ws ":" ws number ws "," ws "\"reason\"" ws ":" ws string (ws "," ws "\"suggested_title\"" ws ":" ws string)? ws "}"

cluster-array ::= "[" ws "]" | "[" ws cluster-item (ws "," ws cluster-item)* ws "]"
cluster-item ::= "{" ws "\"label\"" ws ":" ws string ws "," ws "\"topics\"" ws ":" ws string-array ws "," ws "\"event_count\"" ws ":" ws number (ws "," ws "\"regions\"" ws ":" ws string-array)? ws "}"

key-entity-array ::= "[" ws "]" | "[" ws key-entity (ws "," ws key-entity)* ws "]"
key-entity ::= "{" ws "\"entity_name\"" ws ":" ws string ws "," ws "\"entity_type\"" ws ":" ws string (ws "," ws key-entity-opt)? ws "}"
key-entity-opt ::= ("\"source_count\"" ws ":" ws number (ws "," ws "\"context\"" ws ":" ws string)?) | ("\"context\"" ws ":" ws string)

boolean ::= "true" | "false"
string-array ::= "[" ws "]" | "[" ws string (ws "," ws string)* ws "]"
string ::= "\"" ([^"\\] | "\\" .)* "\""
number ::= "-"? [0-9]+ ("." [0-9]+)? ([eE] [+-]? [0-9]+)?
ws ::= [ \t\n\r]*
"#;

/// GBNF grammar for consolidation structured output.
/// Produces: {"groups": [[1,3,5], [2,4]]}
const CONSOLIDATION_GBNF: &str = r#"
root ::= "{" ws "\"groups\"" ws ":" ws groups ws "}"
groups ::= "[" ws "]" | "[" ws group (ws "," ws group)* ws "]"
group ::= "[" ws number (ws "," ws number)* ws "]"
number ::= [0-9]+
ws ::= [ \t\n\r]*
"#;

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
