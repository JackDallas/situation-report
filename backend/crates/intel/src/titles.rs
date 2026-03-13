use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::budget::BudgetManager;
use crate::client::ClaudeClient;
use crate::ollama::OllamaClient;
use crate::prompts;

/// Detect garbage titles produced by LLM refusals or vague generation.
fn is_garbage_title(title: &str) -> bool {
    // Empty or excessively long titles are garbage (LLM ran on too long)
    if title.is_empty() || title.len() > 80 {
        return true;
    }

    let lower = title.to_lowercase();
    // LLM refusal patterns
    if lower.contains("no relevant")
        || lower.contains("no location")
        || lower.contains("no information")
        || lower.contains("not identified")
        || lower.contains("no core situation")
        || lower.contains("no context provided")
        || lower.contains("unspecified")
        || lower.contains("i need")
        || lower.contains("please provide")
        || lower.contains("please give")
        || lower.contains("more context")
        || lower.contains("more information")
        || lower.contains("insufficient")
        || lower.contains("cannot determine")
        || lower.contains("unable to generate")
        || lower.contains("based on the provided")
        || lower.contains("based on the information")
        || lower.contains("you've provided")
        || lower.contains("you have provided")
        || lower.contains("analyze the")
        || lower.starts_with("i ")
    {
        return true;
    }
    // Compound "and" titles joining unrelated concepts (sign of a magnet cluster)
    // Short "and" titles (<=5 words like "Israel and Lebanon Border Conflict") are OK
    if lower.contains(" and ") && title.split_whitespace().count() >= 6 {
        return true;
    }
    // Titles with banned vague words that sneak through
    let vague_patterns = [
        "economic security concerns",
        "regional security concerns",
        "security tensions",
        "security concerns",
        "unspecified challenges",
        "face unspecified",
        "i cannot generate",
        "cannot generate a meaningful",
        "no logical connection",
    ];
    vague_patterns.iter().any(|p| lower.contains(p))
}

/// Filter entities to only those relevant to the cluster's core situation.
///
/// Problem: clusters accumulate entities from every absorbed event, so high-frequency
/// but irrelevant entities (e.g. names from a single absorbed article) contaminate
/// title generation. This function scores entities by how well they connect to the
/// cluster's other signals (event titles, topics, regions) and caps the output.
fn filter_relevant_entities(
    entities: &[String],
    event_titles: &[String],
    topics: &[String],
    regions: &[String],
) -> Vec<String> {
    if entities.len() <= 5 {
        return entities.to_vec();
    }

    // Build a lowercase corpus from event titles, topics, and regions for matching
    let title_corpus: String = event_titles
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let topic_corpus: String = topics
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let region_corpus: String = regions
        .iter()
        .map(|r| r.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    // Score each entity by how many cluster signals reference it
    let mut scored: Vec<(String, u32)> = entities
        .iter()
        .map(|entity| {
            let lower = entity.to_lowercase();
            let mut score: u32 = 0;

            // +2 for each event title that mentions this entity (strong signal)
            let title_mentions = event_titles
                .iter()
                .filter(|t| t.to_lowercase().contains(&lower))
                .count() as u32;
            score += title_mentions * 2;

            // +1 if any topic references this entity
            if topic_corpus.contains(&lower) {
                score += 1;
            }

            // +1 if entity name matches a region or location signal
            if region_corpus.contains(&lower) {
                score += 1;
            }

            // For multi-word entities (like person names), also check if individual
            // name parts appear in titles (handles partial mentions)
            let parts: Vec<&str> = lower.split_whitespace().collect();
            if parts.len() >= 2 {
                let part_mentions = parts
                    .iter()
                    .filter(|part| part.len() >= 3 && title_corpus.contains(**part))
                    .count() as u32;
                // Only count partial matches if at least 2 name parts match
                if part_mentions >= 2 {
                    score += 1;
                }
            }

            // Location entities (single words matching regions) get a boost
            // since they're likely relevant to the geographic cluster
            if parts.len() == 1 && regions.iter().any(|r| r.to_lowercase() == lower) {
                score += 2;
            }

            (entity.clone(), score)
        })
        .collect();

    // Sort by score descending, then alphabetically for stability
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // Only keep entities with score >= 1 (mentioned in at least one other signal),
    // capped at 8 entities total
    let filtered: Vec<String> = scored
        .into_iter()
        .filter(|(_, score)| *score >= 1)
        .take(8)
        .map(|(entity, _)| entity)
        .collect();

    // If filtering removed everything, fall back to first 5 entities
    // (better than nothing for the LLM)
    if filtered.is_empty() {
        entities.iter().take(5).cloned().collect()
    } else {
        filtered
    }
}

/// Generate a situation title. Tries Ollama (local GPU) first, falls back to Claude API.
/// Returns None if all backends fail or budget exhausted.
/// Cost: ~$0 with Ollama, ~$0.0001 per title with Claude.
#[allow(clippy::too_many_arguments)]
pub async fn generate_situation_title(
    claude_client: Option<&ClaudeClient>,
    ollama_client: Option<&OllamaClient>,
    budget: &Arc<BudgetManager>,
    entities: &[String],
    topics: &[String],
    regions: &[String],
    event_titles: &[String],
    event_count: usize,
    source_count: usize,
    severity_dist: Option<&str>,
    event_type_breakdown: Option<&str>,
    fatality_count: Option<u32>,
    enrichment_summaries: &[String],
) -> Option<String> {
    // Filter entities to only those relevant to the cluster's core situation.
    // This prevents high-frequency but unrelated entities from contaminating
    // the title (e.g. names absorbed from a single merged event).
    let relevant_entities = filter_relevant_entities(entities, event_titles, topics, regions);

    let user_prompt = prompts::title_user(
        &relevant_entities, topics, regions, event_titles, event_count, source_count,
        severity_dist, event_type_breakdown, fatality_count, enrichment_summaries,
    );

    // Try Ollama first (free, local GPU)
    if let Some(ollama) = ollama_client {
        match ollama.complete_text(prompts::TITLE_SYSTEM, &user_prompt, 2048).await {
            Ok(text) => {
                let title = text.trim().trim_matches('"').to_string();
                // Reject garbage titles (LLM refusals) and fall through to Claude
                if is_garbage_title(&title) {
                    info!(title = %title, backend = "ollama", "Rejected garbage title from Ollama, trying Claude");
                } else {
                    info!(title = %title, backend = "ollama", "Generated situation title");
                    return Some(title);
                }
            }
            Err(e) => {
                debug!(error = %e, "Ollama title generation failed, trying Claude");
            }
        }
    }

    // Fall back to Claude API
    if let Some(client) = claude_client {
        if !budget.can_afford_haiku().await {
            return None;
        }

        let model = std::env::var("INTEL_ENRICHMENT_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

        match client
            .complete(&model, prompts::TITLE_SYSTEM, &user_prompt, 40)
            .await
        {
            Ok(response) => {
                budget.record_haiku(
                    response.usage.input_tokens + response.usage.cache_creation_input_tokens,
                    response.usage.output_tokens,
                    response.usage.cache_read_input_tokens,
                );

                if let Some(text) = ClaudeClient::extract_text(&response) {
                    let title = text.trim().trim_matches('"').to_string();
                    if is_garbage_title(&title) {
                        warn!(title = %title, backend = "claude", "Rejected garbage title from Claude");
                        return None;
                    }
                    info!(title = %title, backend = "claude", tokens = response.usage.total_tokens(), "Generated situation title");
                    return Some(title);
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to generate situation title via Claude");
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // is_garbage_title tests
    #[test]
    fn test_garbage_title_no_relevant() {
        assert!(is_garbage_title("No relevant information found"));
    }
    #[test]
    fn test_garbage_title_no_location() {
        assert!(is_garbage_title("No location identified in data"));
    }
    #[test]
    fn test_garbage_title_mixed_case() {
        assert!(is_garbage_title("NO RELEVANT Information"));
    }
    #[test]
    fn test_good_title_not_garbage() {
        assert!(!is_garbage_title("Israel-Hezbollah Cross-Border War"));
    }
    #[test]
    fn test_good_title_short_with_and() {
        // Short "and" titles (<=5 words) are OK
        assert!(!is_garbage_title("Israel and Lebanon Border Conflict"));
    }
    #[test]
    fn test_garbage_six_word_and_title() {
        // 6-word "and" titles are compound garbage
        assert!(is_garbage_title("Yemen Military Activity and Asset Movements"));
        assert!(is_garbage_title("Myanmar Military Conflict and Weapon Deployments"));
        assert!(is_garbage_title("Ukraine Russia Maritime and Energy Conflict"));
    }
    #[test]
    fn test_garbage_no_context() {
        assert!(is_garbage_title("No context provided for analysis"));
    }
    #[test]
    fn test_garbage_compound_and_title() {
        assert!(is_garbage_title("Germany-China Trade Shifts and Economic Security Concerns"));
    }
    #[test]
    fn test_garbage_security_concerns() {
        assert!(is_garbage_title("South Korea Iran Regional Security Concerns"));
    }
    #[test]
    fn test_garbage_security_tensions() {
        assert!(is_garbage_title("Horn of Africa Humanitarian Crisis Security Tensions"));
    }
    #[test]
    fn test_garbage_unspecified() {
        assert!(is_garbage_title("Doctors in Asia Face Unspecified Challenges"));
    }

    // filter_relevant_entities tests
    #[test]
    fn test_filter_entities_few_passthrough() {
        // <= 5 entities should pass through unchanged
        let entities = vec![
            "iran".to_string(),
            "israel".to_string(),
            "hezbollah".to_string(),
        ];
        let result = filter_relevant_entities(&entities, &[], &[], &[]);
        assert_eq!(result, entities);
    }

    #[test]
    fn test_filter_entities_scores_by_title_mention() {
        let entities: Vec<String> = (0..8).map(|i| format!("entity_{i}")).collect();
        let event_titles = vec!["entity_0 attacks entity_1".to_string()];
        let result = filter_relevant_entities(&entities, &event_titles, &[], &[]);
        // entity_0 and entity_1 should be in results (mentioned in titles)
        assert!(result.contains(&"entity_0".to_string()));
        assert!(result.contains(&"entity_1".to_string()));
        // entity_7 should NOT be in results (not mentioned anywhere)
        assert!(!result.contains(&"entity_7".to_string()));
    }

    #[test]
    fn test_filter_entities_scores_by_topic() {
        let entities: Vec<String> = vec![
            "iran".into(),
            "baseball".into(),
            "soccer".into(),
            "sports".into(),
            "israel".into(),
            "hezbollah".into(),
            "random1".into(),
            "random2".into(),
        ];
        let topics = vec![
            "iran-israel-conflict".to_string(),
            "hezbollah-strikes".to_string(),
        ];
        let event_titles = vec!["Iran strikes Israel border".to_string()];
        let result = filter_relevant_entities(&entities, &event_titles, &topics, &[]);
        assert!(result.contains(&"iran".to_string()));
        assert!(result.contains(&"israel".to_string()));
    }

    #[test]
    fn test_filter_entities_fallback_when_no_scores() {
        // When no entities score >= 1, should fall back to first 5
        let entities: Vec<String> = (0..10).map(|i| format!("obscure_{i}")).collect();
        let result = filter_relevant_entities(&entities, &[], &[], &[]);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_filter_entities_caps_at_8() {
        let entities: Vec<String> = (0..20).map(|i| format!("entity_{i}")).collect();
        let event_titles: Vec<String> =
            (0..20).map(|i| format!("Event about entity_{i}")).collect();
        let result = filter_relevant_entities(&entities, &event_titles, &[], &[]);
        assert!(result.len() <= 8);
    }

    #[test]
    fn test_filter_entities_region_boost() {
        let entities: Vec<String> = vec![
            "iran".into(),
            "unrelated1".into(),
            "unrelated2".into(),
            "unrelated3".into(),
            "unrelated4".into(),
            "unrelated5".into(),
        ];
        let regions = vec!["iran".to_string()];
        let result = filter_relevant_entities(&entities, &[], &[], &regions);
        // iran should be boosted by matching region
        assert!(result.contains(&"iran".to_string()));
    }
}
