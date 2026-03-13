//! Google Gemini API client (native REST API).
//!
//! Uses the generativelanguage.googleapis.com endpoint with responseSchema
//! enforcement for structured JSON output. Implicit caching provides 90%
//! input cost reduction on repeated system prompts (>1024 tokens, automatic).

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Gemini model tiers.
#[derive(Debug, Clone, Copy)]
pub enum GeminiModel {
    /// Fast + cheap: enrichment, titles. $0.10/M in, $0.40/M out.
    FlashLite,
    /// Better reasoning: narratives, analysis. $0.30/M in, $2.50/M out.
    Flash,
}

impl GeminiModel {
    pub fn api_id(&self) -> &'static str {
        match self {
            GeminiModel::FlashLite => "gemini-2.5-flash-lite",
            GeminiModel::Flash => "gemini-2.5-flash",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            GeminiModel::FlashLite => "Gemini 2.5 Flash-Lite",
            GeminiModel::Flash => "Gemini 2.5 Flash",
        }
    }
}

/// Per-model pricing in USD per million tokens.
pub struct GeminiPricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cached_input_per_m: f64,
}

pub const FLASH_LITE_PRICING: GeminiPricing = GeminiPricing {
    input_per_m: 0.10,
    output_per_m: 0.40,
    cached_input_per_m: 0.01,
};

pub const FLASH_PRICING: GeminiPricing = GeminiPricing {
    input_per_m: 0.30,
    output_per_m: 2.50,
    cached_input_per_m: 0.03,
};

pub fn pricing_for(model: GeminiModel) -> &'static GeminiPricing {
    match model {
        GeminiModel::FlashLite => &FLASH_LITE_PRICING,
        GeminiModel::Flash => &FLASH_PRICING,
    }
}

/// Client for the Gemini REST API.
pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
}

// --- Request types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_schema: Option<serde_json::Value>,
    /// Disable thinking for Flash models (reduces cost).
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    thinking_budget: i32,
}

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateResponse {
    candidates: Vec<Candidate>,
    #[serde(default)]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    #[serde(default)]
    pub prompt_token_count: u32,
    #[serde(default)]
    pub candidates_token_count: u32,
    #[serde(default)]
    pub total_token_count: u32,
    #[serde(default)]
    pub cached_content_token_count: u32,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(default)]
    code: u32,
}

/// Result of a Gemini API call.
pub struct GeminiResponse {
    pub text: String,
    pub usage: UsageMetadata,
    pub model: GeminiModel,
}

impl GeminiResponse {
    /// Compute cost in USD for this call.
    pub fn cost_usd(&self) -> f64 {
        let p = pricing_for(self.model);
        let input = self.usage.prompt_token_count as f64 / 1_000_000.0;
        let output = self.usage.candidates_token_count as f64 / 1_000_000.0;
        let cached = self.usage.cached_content_token_count as f64 / 1_000_000.0;
        // Cached tokens are charged at cached rate instead of full input rate
        let uncached_input = (input - cached).max(0.0);
        uncached_input * p.input_per_m + output * p.output_per_m + cached * p.cached_input_per_m
    }
}

impl GeminiClient {
    /// Create from GEMINI_API_KEY env var.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("GEMINI_API_KEY").ok()?;
        if api_key.is_empty() {
            return None;
        }
        Some(Self::new(api_key))
    }

    pub fn new(api_key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");

        Self { http, api_key }
    }

    /// Generate text with optional JSON schema enforcement.
    ///
    /// When `response_schema` is Some, the model is constrained to produce valid
    /// JSON matching the schema (decode-level enforcement, not just prompting).
    pub async fn generate(
        &self,
        model: GeminiModel,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
        response_schema: Option<serde_json::Value>,
    ) -> Result<GeminiResponse> {
        let (response_mime_type, schema) = if let Some(schema) = response_schema {
            (Some("application/json".to_string()), Some(schema))
        } else {
            (None, None)
        };

        let request = GenerateRequest {
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part { text: Some(system_prompt.to_string()), inline_data: None }],
            }),
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part { text: Some(user_message.to_string()), inline_data: None }],
            }],
            generation_config: GenerationConfig {
                temperature: Some(0.2),
                max_output_tokens: Some(max_tokens),
                response_mime_type,
                response_schema: schema,
                thinking_config: Some(ThinkingConfig {
                    thinking_budget: 0,
                }),
            },
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model.api_id()
        );

        // Retry with exponential backoff on 429/503
        let mut last_err = String::new();
        for attempt in 0..4u32 {
            if attempt > 0 {
                let base = 500 * 2u64.pow(attempt);
                let jitter = rand::thread_rng().gen_range(0..500);
                let delay = Duration::from_millis(base + jitter);
                tokio::time::sleep(delay).await;
            }

            let resp = self
                .http
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await
                .context("Gemini API request failed")?;

            let status = resp.status();
            if status.is_success() {
                let api_resp: GenerateResponse = resp
                    .json()
                    .await
                    .context("Failed to parse Gemini response")?;

                let text = api_resp
                    .candidates
                    .into_iter()
                    .next()
                    .and_then(|c| c.content.parts.into_iter().next())
                    .and_then(|p| p.text)
                    .unwrap_or_default();

                let usage = api_resp.usage_metadata.unwrap_or(UsageMetadata {
                    prompt_token_count: 0,
                    candidates_token_count: 0,
                    total_token_count: 0,
                    cached_content_token_count: 0,
                });

                debug!(
                    model = model.display_name(),
                    input_tokens = usage.prompt_token_count,
                    output_tokens = usage.candidates_token_count,
                    cached_tokens = usage.cached_content_token_count,
                    "Gemini API call complete"
                );

                return Ok(GeminiResponse { text, usage, model });
            }

            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 || status.as_u16() == 503 {
                debug!(attempt, status = %status, "Gemini API rate limited, retrying");
                last_err = format!("rate limited ({status})");
                continue;
            }

            // Try to extract error message
            if let Ok(err_resp) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("Gemini API error {}: {}", err_resp.error.code, err_resp.error.message);
            }
            bail!("Gemini API error {status}: {body}");
        }
        bail!("Gemini API {last_err} after 4 attempts")
    }

    /// Convenience: generate with JSON schema enforcement.
    pub async fn generate_json(
        &self,
        model: GeminiModel,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
        schema: serde_json::Value,
    ) -> Result<GeminiResponse> {
        self.generate(model, system_prompt, user_message, max_tokens, Some(schema)).await
    }

    /// Convenience: generate plain text (no schema).
    pub async fn generate_text(
        &self,
        model: GeminiModel,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
    ) -> Result<GeminiResponse> {
        self.generate(model, system_prompt, user_message, max_tokens, None).await
    }

    /// Maximum image size for OCR (5 MB).
    const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

    /// OCR prompt sent to Gemini Vision.
    const OCR_PROMPT: &'static str =
        "Extract all text from this image. Return only the extracted text, nothing else. If there is no text, return an empty string.";

    /// Download an image and extract text via Gemini Vision (Flash-Lite).
    ///
    /// Returns `Ok(None)` if the image is too large, unreachable, or contains
    /// no text. Only returns `Err` on Gemini API failures.
    pub async fn ocr_image_url(&self, image_url: &str) -> Result<Option<GeminiResponse>> {
        // Download the image
        let resp = self
            .http
            .get(image_url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .context("Failed to download image for OCR")?;

        if !resp.status().is_success() {
            debug!(url = %image_url, status = %resp.status(), "Image download failed, skipping OCR");
            return Ok(None);
        }

        // Determine MIME type from Content-Type header, default to jpeg
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| {
                // Strip parameters like "; charset=utf-8"
                ct.split(';').next().unwrap_or(ct).trim().to_string()
            })
            .unwrap_or_else(|| "image/jpeg".to_string());

        let image_bytes = resp
            .bytes()
            .await
            .context("Failed to read image bytes")?;

        if image_bytes.len() > Self::MAX_IMAGE_BYTES {
            debug!(
                url = %image_url,
                size_bytes = image_bytes.len(),
                "Image too large for OCR, skipping"
            );
            return Ok(None);
        }

        if image_bytes.is_empty() {
            return Ok(None);
        }

        self.ocr_image_bytes(&image_bytes, &mime_type).await
    }

    /// Extract text from raw image bytes via Gemini Vision (Flash-Lite).
    ///
    /// Returns `Ok(None)` if the response is empty (no text in image).
    pub async fn ocr_image_bytes(
        &self,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> Result<Option<GeminiResponse>> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);

        let request = GenerateRequest {
            system_instruction: None,
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![
                    Part {
                        text: None,
                        inline_data: Some(InlineData {
                            mime_type: mime_type.to_string(),
                            data: b64,
                        }),
                    },
                    Part {
                        text: Some(Self::OCR_PROMPT.to_string()),
                        inline_data: None,
                    },
                ],
            }],
            generation_config: GenerationConfig {
                temperature: Some(0.0),
                max_output_tokens: Some(1024),
                response_mime_type: None,
                response_schema: None,
                thinking_config: Some(ThinkingConfig {
                    thinking_budget: 0,
                }),
            },
        };

        let model = GeminiModel::FlashLite;
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model.api_id()
        );

        // Retry with exponential backoff on 429/503
        let mut last_err = String::new();
        for attempt in 0..3u32 {
            if attempt > 0 {
                let base = 500 * 2u64.pow(attempt);
                let jitter = rand::thread_rng().gen_range(0..500);
                let delay = Duration::from_millis(base + jitter);
                tokio::time::sleep(delay).await;
            }

            let resp = self
                .http
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await
                .context("Gemini OCR request failed")?;

            let status = resp.status();
            if status.is_success() {
                let api_resp: GenerateResponse = resp
                    .json()
                    .await
                    .context("Failed to parse Gemini OCR response")?;

                let text = api_resp
                    .candidates
                    .into_iter()
                    .next()
                    .and_then(|c| c.content.parts.into_iter().next())
                    .and_then(|p| p.text)
                    .unwrap_or_default();

                let usage = api_resp.usage_metadata.unwrap_or(UsageMetadata {
                    prompt_token_count: 0,
                    candidates_token_count: 0,
                    total_token_count: 0,
                    cached_content_token_count: 0,
                });

                debug!(
                    input_tokens = usage.prompt_token_count,
                    output_tokens = usage.candidates_token_count,
                    text_len = text.len(),
                    "Gemini OCR complete"
                );

                if text.trim().is_empty() {
                    return Ok(None);
                }

                return Ok(Some(GeminiResponse { text, usage, model }));
            }

            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 || status.as_u16() == 503 {
                debug!(attempt, status = %status, "Gemini OCR rate limited, retrying");
                last_err = format!("rate limited ({status})");
                continue;
            }

            if let Ok(err_resp) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("Gemini OCR error {}: {}", err_resp.error.code, err_resp.error.message);
            }
            bail!("Gemini OCR error {status}: {body}");
        }
        bail!("Gemini OCR {last_err} after 3 attempts")
    }

    /// Health check: verify the API key works by listing models.
    pub async fn health_check(&self) -> bool {
        let url = "https://generativelanguage.googleapis.com/v1beta/models";
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            self.http.get(url)
                .header("x-goog-api-key", &self.api_key)
                .send(),
        )
        .await;

        match result {
            Ok(Ok(resp)) => resp.status().is_success(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_ids() {
        assert_eq!(GeminiModel::FlashLite.api_id(), "gemini-2.5-flash-lite");
        assert_eq!(GeminiModel::Flash.api_id(), "gemini-2.5-flash");
    }

    #[test]
    fn test_cost_calculation() {
        let resp = GeminiResponse {
            text: String::new(),
            usage: UsageMetadata {
                prompt_token_count: 1000,
                candidates_token_count: 500,
                total_token_count: 1500,
                cached_content_token_count: 800,
            },
            model: GeminiModel::FlashLite,
        };
        let cost = resp.cost_usd();
        // 200 uncached input * 0.10/M + 500 output * 0.40/M + 800 cached * 0.01/M
        let expected = 200.0 / 1e6 * 0.10 + 500.0 / 1e6 * 0.40 + 800.0 / 1e6 * 0.01;
        assert!((cost - expected).abs() < 0.0001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn test_pricing_for() {
        let p = pricing_for(GeminiModel::Flash);
        assert!((p.input_per_m - 0.30).abs() < 0.001);
        assert!((p.output_per_m - 2.50).abs() < 0.001);
    }

    #[test]
    fn test_serialize_request() {
        let req = GenerateRequest {
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part { text: Some("You are helpful.".to_string()), inline_data: None }],
            }),
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part { text: Some("Hello".to_string()), inline_data: None }],
            }],
            generation_config: GenerationConfig {
                temperature: Some(0.2),
                max_output_tokens: Some(1024),
                response_mime_type: Some("application/json".to_string()),
                response_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "summary": {"type": "string"}
                    }
                })),
                thinking_config: Some(ThinkingConfig {
                    thinking_budget: 0,
                }),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("systemInstruction"));
        assert!(json.contains("generationConfig"));
        assert!(json.contains("responseMimeType"));
        assert!(json.contains("responseSchema"));
        assert!(json.contains("thinkingConfig"));
    }
}
