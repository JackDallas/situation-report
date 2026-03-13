use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tracing::debug;

use crate::budget::BudgetManager;
use crate::client::ClaudeClient;
use crate::gemini::{GeminiClient, GeminiModel};
use crate::prompts;
use crate::types::{ArticleInput, EnrichedArticleV2, Entity, ExtractedRelationship, ExtractedStateChange, InferredLocation};

/// Default enrichment model — fast + cheap.
fn enrichment_model() -> String {
    std::env::var("INTEL_ENRICHMENT_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string())
}

/// Enrich a news article with a single Haiku call:
/// translate + summarize + extract entities + topic tag + relevance score + sentiment.
///
/// Returns None if budget is exhausted.
pub async fn enrich_article(
    client: &ClaudeClient,
    budget: &Arc<BudgetManager>,
    article: &ArticleInput,
) -> Result<EnrichedArticleV2> {
    // Budget gate
    if !budget.can_afford_haiku().await {
        bail!("Daily AI budget exhausted — skipping enrichment");
    }

    let model = enrichment_model();
    let user_msg = prompts::enrichment_user(
        &article.title,
        &article.description,
        article.source_country.as_deref(),
        article.language_hint.as_deref(),
        article.source_type.as_deref(),
    );

    let response = client
        .complete(&model, prompts::ENRICHMENT_SYSTEM, &user_msg, 1024)
        .await
        .context("Haiku enrichment call failed")?;

    // Record token usage
    budget.record_haiku(
        response.usage.input_tokens,
        response.usage.output_tokens,
        response.usage.cache_read_input_tokens,
    );

    let text = ClaudeClient::extract_text(&response)
        .context("No text in enrichment response")?;

    // Parse JSON response — strip markdown code fences if present
    let json_str = text
        .trim()
        .strip_prefix("```json")
        .or_else(|| text.trim().strip_prefix("```"))
        .unwrap_or(text.trim());
    let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

    let parsed: serde_json::Value =
        serde_json::from_str(json_str).context("Failed to parse enrichment JSON")?;

    let inferred_location = parse_inferred_location(&parsed["inferred_location"]);

    let enriched = EnrichedArticleV2 {
        translated_title: parsed["translated_title"]
            .as_str()
            .unwrap_or(&article.title)
            .to_string(),
        summary: parsed["summary"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        entities: parse_entities(&parsed["entities"]),
        topics: parsed["topics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        relationships: parse_relationships(&parsed["relationships"]),
        state_changes: parse_state_changes(&parsed["state_changes"]),
        inferred_location,
        relevance_score: parsed["relevance_score"]
            .as_f64()
            .unwrap_or(0.5) as f32,
        sentiment: parsed["sentiment"]
            .as_f64()
            .unwrap_or(0.0) as f32,
        original_language: parsed["original_language"]
            .as_str()
            .unwrap_or("en")
            .to_string(),
        model: model.clone(),
        tokens_used: response.usage.total_tokens(),
    };

    debug!(
        title = enriched.translated_title,
        lang = enriched.original_language,
        relevance = enriched.relevance_score,
        entities = enriched.entities.len(),
        relationships = enriched.relationships.len(),
        state_changes = enriched.state_changes.len(),
        tokens = enriched.tokens_used,
        "Article enriched"
    );

    Ok(enriched)
}

/// JSON Schema for Gemini responseSchema enforcement — matches EnrichedArticleV2.
fn gemini_enrichment_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "OBJECT",
        "properties": {
            "translated_title": {"type": "STRING"},
            "summary": {"type": "STRING"},
            "original_language": {"type": "STRING"},
            "entities": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "name": {"type": "STRING"},
                        "entity_type": {"type": "STRING"},
                        "role": {"type": "STRING"},
                        "wikidata_qid": {"type": "STRING"}
                    },
                    "required": ["name", "entity_type"]
                }
            },
            "relationships": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "source": {"type": "STRING"},
                        "target": {"type": "STRING"},
                        "type": {"type": "STRING"},
                        "confidence": {"type": "NUMBER"}
                    },
                    "required": ["source", "target", "type"]
                }
            },
            "state_changes": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "entity": {"type": "STRING"},
                        "attribute": {"type": "STRING"},
                        "from": {"type": "STRING"},
                        "to": {"type": "STRING"},
                        "certainty": {"type": "STRING"}
                    },
                    "required": ["entity", "attribute", "to"]
                }
            },
            "topics": {
                "type": "ARRAY",
                "items": {"type": "STRING"}
            },
            "relevance_score": {"type": "NUMBER"},
            "sentiment": {"type": "NUMBER"},
            "inferred_location": {
                "type": "OBJECT",
                "properties": {
                    "name": {"type": "STRING"},
                    "lat": {"type": "NUMBER"},
                    "lon": {"type": "NUMBER"}
                },
                "required": ["name", "lat", "lon"]
            }
        },
        "required": ["translated_title", "summary", "original_language", "entities", "topics", "relevance_score", "sentiment"]
    })
}

/// Enrich a news article via Gemini (Flash-Lite) with responseSchema enforcement.
///
/// Returns None if budget is exhausted.
pub async fn enrich_article_gemini(
    gemini: &GeminiClient,
    budget: &Arc<BudgetManager>,
    article: &ArticleInput,
) -> Result<EnrichedArticleV2> {
    if !budget.can_afford_gemini().await {
        bail!("Gemini monthly budget exhausted — skipping enrichment");
    }

    let model = GeminiModel::FlashLite;
    let user_msg = prompts::enrichment_user(
        &article.title,
        &article.description,
        article.source_country.as_deref(),
        article.language_hint.as_deref(),
        article.source_type.as_deref(),
    );

    let response = gemini
        .generate_json(model, prompts::ENRICHMENT_SYSTEM, &user_msg, 1024, gemini_enrichment_schema())
        .await
        .context("Gemini enrichment call failed")?;

    budget.record_gemini(model, &response);

    let parsed: serde_json::Value =
        serde_json::from_str(&response.text).context("Failed to parse Gemini enrichment JSON")?;

    let inferred_location = parse_inferred_location(&parsed["inferred_location"]);

    let tokens_used = response.usage.prompt_token_count + response.usage.candidates_token_count;

    let enriched = EnrichedArticleV2 {
        translated_title: parsed["translated_title"]
            .as_str()
            .unwrap_or(&article.title)
            .to_string(),
        summary: parsed["summary"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        entities: parse_entities(&parsed["entities"]),
        topics: parsed["topics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        relationships: parse_relationships(&parsed["relationships"]),
        state_changes: parse_state_changes(&parsed["state_changes"]),
        inferred_location,
        relevance_score: parsed["relevance_score"]
            .as_f64()
            .unwrap_or(0.5) as f32,
        sentiment: parsed["sentiment"]
            .as_f64()
            .unwrap_or(0.0) as f32,
        original_language: parsed["original_language"]
            .as_str()
            .unwrap_or("en")
            .to_string(),
        model: model.display_name().to_string(),
        tokens_used,
    };

    debug!(
        title = enriched.translated_title,
        lang = enriched.original_language,
        relevance = enriched.relevance_score,
        entities = enriched.entities.len(),
        tokens = enriched.tokens_used,
        "Article enriched via Gemini Flash-Lite"
    );

    Ok(enriched)
}

// Enrichment is local LLM-only. Cloud fallback removed to prevent budget blowout.
// If LLM fails, the event goes unenriched — the caller handles this gracefully.
pub async fn enrich_article_tiered(
    _claude_client: Option<&ClaudeClient>,
    _gemini_client: Option<&GeminiClient>,
    llm_client: Option<&crate::llm::LlmClient>,
    _budget: &Arc<BudgetManager>,
    article: &ArticleInput,
) -> Result<EnrichedArticleV2> {
    if let Some(llm) = llm_client {
        return llm.enrich_article(article).await;
    }

    bail!("LLM not available for enrichment");
}

fn parse_entities(value: &serde_json::Value) -> Vec<Entity> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(Entity {
                name: v["name"].as_str()?.to_string(),
                entity_type: v["entity_type"].as_str().unwrap_or("unknown").to_string(),
                role: v["role"].as_str().map(String::from),
                wikidata_qid: v["wikidata_qid"].as_str().map(String::from),
            })
        })
        .collect()
}

fn parse_relationships(value: &serde_json::Value) -> Vec<ExtractedRelationship> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(ExtractedRelationship {
                source: v["source"].as_str()?.to_string(),
                target: v["target"].as_str()?.to_string(),
                rel_type: v["type"].as_str().unwrap_or("alliance").to_string(),
                confidence: v["confidence"].as_f64().unwrap_or(0.5) as f32,
            })
        })
        .collect()
}

fn parse_state_changes(value: &serde_json::Value) -> Vec<ExtractedStateChange> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(ExtractedStateChange {
                entity: v["entity"].as_str()?.to_string(),
                attribute: v["attribute"].as_str().unwrap_or("status").to_string(),
                from: v["from"].as_str().map(String::from),
                to: v["to"].as_str()?.to_string(),
                certainty: v["certainty"].as_str().unwrap_or("alleged").to_string(),
            })
        })
        .collect()
}

fn parse_inferred_location(value: &serde_json::Value) -> Option<InferredLocation> {
    if value.is_null() {
        return None;
    }
    let name = value["name"].as_str()?.to_string();
    let lat = value["lat"].as_f64()?;
    let lon = value["lon"].as_f64()?;
    // Sanity check: valid lat/lon range
    if lat < -90.0 || lat > 90.0 || lon < -180.0 || lon > 180.0 {
        return None;
    }
    Some(InferredLocation { name, lat, lon })
}

/// Extract an ArticleInput from an InsertableEvent's fields.
/// Returns None if there's no title (not a useful article).
pub fn article_from_event(event: &sr_sources::InsertableEvent) -> Option<ArticleInput> {
    let title = event.title.as_ref()?;
    if title.is_empty() {
        return None;
    }

    Some(ArticleInput {
        title: title.clone(),
        description: event.description.clone().unwrap_or_default(),
        source_url: event.payload.get("url").and_then(|v| v.as_str()).map(String::from),
        source_country: event.payload.get("sourcecountry")
            .or_else(|| event.payload.get("source_country"))
            .or_else(|| event.payload.get("source_name"))
            .and_then(|v| v.as_str()).map(String::from),
        language_hint: event.payload.get("language").and_then(|v| v.as_str()).map(String::from),
        source_type: Some(event.source_type.as_str().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_entities_valid() {
        let value = json!([
            {"name": "Iran", "entity_type": "location", "role": "target"},
            {"name": "IRGC", "entity_type": "organization", "role": "actor", "wikidata_qid": "Q12345"}
        ]);
        let entities = parse_entities(&value);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "Iran");
        assert_eq!(entities[0].entity_type, "location");
        assert_eq!(entities[0].role, Some("target".to_string()));
        assert_eq!(entities[1].wikidata_qid, Some("Q12345".to_string()));
    }

    #[test]
    fn test_parse_entities_missing_name_skipped() {
        let value = json!([
            {"entity_type": "location"},  // no name -> should be skipped
            {"name": "Iran", "entity_type": "location"}
        ]);
        let entities = parse_entities(&value);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn test_parse_entities_null() {
        let entities = parse_entities(&serde_json::Value::Null);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_parse_entities_not_array() {
        let entities = parse_entities(&json!("not an array"));
        assert!(entities.is_empty());
    }

    #[test]
    fn test_parse_entities_defaults() {
        let value = json!([{"name": "Unknown Entity"}]);
        let entities = parse_entities(&value);
        assert_eq!(entities[0].entity_type, "unknown");
        assert_eq!(entities[0].role, None);
        assert_eq!(entities[0].wikidata_qid, None);
    }

    #[test]
    fn test_parse_relationships_valid() {
        let value = json!([
            {"source": "Iran", "target": "Israel", "type": "rivalry", "confidence": 0.9}
        ]);
        let rels = parse_relationships(&value);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].source, "Iran");
        assert_eq!(rels[0].target, "Israel");
        assert_eq!(rels[0].rel_type, "rivalry");
        assert!((rels[0].confidence - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_parse_relationships_missing_required() {
        let value = json!([
            {"source": "Iran"},  // missing target -> skipped
            {"target": "Israel"},  // missing source -> skipped
            {"source": "Iran", "target": "Israel"}  // valid, defaults
        ]);
        let rels = parse_relationships(&value);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].rel_type, "alliance"); // default
        assert!((rels[0].confidence - 0.5).abs() < 0.01); // default
    }

    #[test]
    fn test_parse_relationships_null() {
        assert!(parse_relationships(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn test_parse_state_changes_valid() {
        let value = json!([
            {"entity": "Assad", "attribute": "status", "from": "in power", "to": "overthrown", "certainty": "confirmed"}
        ]);
        let changes = parse_state_changes(&value);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity, "Assad");
        assert_eq!(changes[0].from, Some("in power".to_string()));
        assert_eq!(changes[0].to, "overthrown");
        assert_eq!(changes[0].certainty, "confirmed");
    }

    #[test]
    fn test_parse_state_changes_defaults() {
        let value = json!([{"entity": "X", "to": "active"}]);
        let changes = parse_state_changes(&value);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].attribute, "status");
        assert_eq!(changes[0].from, None);
        assert_eq!(changes[0].certainty, "alleged");
    }

    #[test]
    fn test_parse_state_changes_missing_required() {
        let value = json!([
            {"entity": "X"},  // missing "to" -> skipped
            {"to": "dead"}   // missing "entity" -> skipped
        ]);
        assert!(parse_state_changes(&value).is_empty());
    }

    #[test]
    fn test_parse_inferred_location_valid() {
        let value = json!({"name": "Damascus, Syria", "lat": 33.51, "lon": 36.29});
        let loc = parse_inferred_location(&value);
        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert_eq!(loc.name, "Damascus, Syria");
        assert!((loc.lat - 33.51).abs() < 0.01);
        assert!((loc.lon - 36.29).abs() < 0.01);
    }

    #[test]
    fn test_parse_inferred_location_null() {
        assert!(parse_inferred_location(&serde_json::Value::Null).is_none());
    }

    #[test]
    fn test_parse_inferred_location_invalid_coords() {
        let value = json!({"name": "Invalid", "lat": 200.0, "lon": 36.0});
        assert!(parse_inferred_location(&value).is_none());
    }

    #[test]
    fn test_parse_inferred_location_missing_lat() {
        let value = json!({"name": "Place", "lon": 36.0});
        assert!(parse_inferred_location(&value).is_none());
    }

    #[test]
    fn test_parse_inferred_location_missing_name() {
        let value = json!({"lat": 33.0, "lon": 36.0});
        assert!(parse_inferred_location(&value).is_none());
    }
}
