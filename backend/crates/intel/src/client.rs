use anyhow::{Context, Result, bail};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Thin wrapper around the Anthropic Messages API with prompt caching support.
pub struct ClaudeClient {
    http: reqwest::Client,
    api_key: String,
}

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// A message in the Claude API conversation.
#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

/// A system content block with optional cache control.
#[derive(Debug, Serialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    block_type: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: &'static str,
}

/// Request body for the Messages API.
#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: Vec<SystemBlock>,
    messages: Vec<Message>,
}

/// Top-level API response.
#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
}

/// A content block in the response.
#[derive(Debug, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
}

/// Token usage from the API response.
#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    pub output_tokens: u32,
}

impl Usage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens + self.output_tokens
    }
}

impl ClaudeClient {
    /// Create a new client. Reads ANTHROPIC_API_KEY from env if not provided.
    pub fn new(api_key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");

        Self { http, api_key }
    }

    /// Create from environment variable.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY not set")?;
        Ok(Self::new(api_key))
    }

    /// Send a message to Claude with prompt caching on the system prompt.
    ///
    /// The system prompt is marked with `cache_control: ephemeral` so identical
    /// system prompts across calls are cached for ~5 minutes.
    pub async fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
    ) -> Result<ApiResponse> {
        let request = ApiRequest {
            model: model.to_string(),
            max_tokens,
            system: vec![SystemBlock {
                block_type: "text",
                text: system_prompt.to_string(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral",
                }),
            }],
            messages: vec![Message {
                role: "user",
                content: user_message.to_string(),
            }],
        };

        // Retry with exponential backoff on 429/529
        let mut last_err = String::new();
        for attempt in 0..4u32 {
            if attempt > 0 {
                let base = 500 * 2u64.pow(attempt);
                let jitter = rand::thread_rng().gen_range(0..500);
                let delay = std::time::Duration::from_millis(base + jitter);
                tokio::time::sleep(delay).await;
            }

            let resp = self
                .http
                .post(API_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", "prompt-caching-2024-07-31")
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await
                .context("Claude API request failed")?;

            let status = resp.status();
            if status.is_success() {
                let api_resp: ApiResponse = resp.json().await.context("Failed to parse Claude API response")?;

                debug!(
                    model,
                    input_tokens = api_resp.usage.input_tokens,
                    cache_read = api_resp.usage.cache_read_input_tokens,
                    cache_write = api_resp.usage.cache_creation_input_tokens,
                    output_tokens = api_resp.usage.output_tokens,
                    "Claude API call complete"
                );

                return Ok(api_resp);
            }

            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 || status.as_u16() == 529 {
                debug!(attempt, "Claude API rate limited, retrying");
                last_err = format!("rate limited ({status})");
                continue;
            }
            bail!("Claude API error {status}: {body}");
        }
        bail!("Claude API {last_err} after 4 attempts")
    }

    /// Extract text content from the first text block of a response.
    pub fn extract_text(response: &ApiResponse) -> Option<&str> {
        response
            .content
            .iter()
            .find(|b| b.block_type == "text")
            .and_then(|b| b.text.as_deref())
    }
}
