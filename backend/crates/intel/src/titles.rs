use std::sync::Arc;

use tracing::{debug, info};

use crate::budget::BudgetManager;
use crate::client::ClaudeClient;
use crate::gemini::GeminiClient;
use crate::ollama::OllamaClient;
use crate::prompts;

/// Multi-word geographic phrases to strip first (longest first to avoid partial matches).
const GEO_PHRASES: &[&str] = &[
    "southeast asia", "southern africa", "central africa", "central asia",
    "latin america", "middle east", "south asia", "east asia", "west africa",
    "east africa", "north africa",
];

/// Single-word geographic/org terms for region-concatenation detection.
const GEO_WORDS: &[&str] = &[
    "africa", "asia", "europe", "americas", "oceania", "pacific", "arctic",
    "antarctic", "mediterranean", "balkans", "caucasus", "sahel", "caribbean",
    "un", "eu", "nato", "usa", "uk",
];

/// Detect garbage titles produced by LLM refusals or vague generation.
fn is_garbage_title(title: &str) -> bool {
    // Empty or excessively long titles are garbage (LLM ran on too long)
    if title.is_empty() || title.len() > 80 {
        return true;
    }

    let lower = title.to_lowercase();
    // "Global" prefix produces vague mega-cluster titles
    if lower.starts_with("global ") {
        return true;
    }
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
    // Region-concatenation garbage: titles that are just geographic/org names strung together
    // with no action word. E.g. "UN South Asia Middle East East Asia"
    if is_region_concatenation(&lower) {
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

/// Detect titles that are just geographic names / org acronyms concatenated with no action.
/// E.g. "UN South Asia Middle East East Asia" or "Iran Israel Syria"
fn is_region_concatenation(lower: &str) -> bool {
    // First, check if the title contains any action/event words that make it meaningful.
    // If it has an action word, it's not just a concatenation.
    let action_words = [
        "war", "conflict", "fighting", "strikes", "attack", "attacks", "strike",
        "bombing", "shelling", "siege", "invasion", "offensive", "battle",
        "crisis", "disaster", "earthquake", "flood", "floods", "wildfire", "wildfires",
        "fire", "fires", "drought", "famine", "hurricane", "typhoon", "cyclone",
        "tsunami", "eruption", "collapse",
        "surge", "spike", "outbreak", "pandemic", "epidemic",
        "protests", "protest", "uprising", "revolution", "coup", "riots", "unrest",
        "election", "elections", "vote", "referendum", "summit", "talks", "negotiations",
        "ban", "sanctions", "embargo", "blockade", "ceasefire", "truce",
        "piracy", "hijacking", "kidnapping", "assassination",
        "flights", "patrols", "deployment", "buildup", "exercises",
        "hack", "breach", "outage", "disruption", "cyberattack",
        "migration", "refugees", "displacement", "evacuation",
        "shooting", "massacre", "genocide", "atrocity",
        "nuclear", "chemical", "biological",
        "sweeps", "raids", "crackdown", "arrests",
        "aid", "relief", "rescue", "humanitarian",
    ];
    for action in &action_words {
        if lower.contains(action) {
            return false;
        }
    }

    // No action word found — check if the title is mostly geographic/org names.
    // Strip out known multi-word geographic phrases first (longest first), then
    // check remaining individual words against single-word geo terms.
    let mut remaining = lower.to_string();
    for phrase in GEO_PHRASES {
        remaining = remaining.replace(phrase, " ");
    }

    // Check remaining words: filter out single-word geo terms and short artifacts
    let remaining_words: Vec<&str> = remaining.split_whitespace()
        .filter(|w| w.len() >= 2)
        .filter(|w| !GEO_WORDS.contains(w))
        .collect();

    // If after removing all geographic terms there's 0-1 words left, it's concatenation garbage.
    // E.g. "UN South Asia Middle East East Asia" → "" after removal
    // But "Horn of Africa Piracy Surge" → ["horn", "of", "piracy", "surge"] → has content
    remaining_words.len() <= 1
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

// Title generation is Ollama-only. Heuristic fallback in caller.
// If Ollama fails or produces garbage, returns None so the caller
// falls back to generate_title() in scoring.rs.
#[allow(clippy::too_many_arguments)]
pub async fn generate_situation_title(
    _claude_client: Option<&ClaudeClient>,
    _gemini_client: Option<&GeminiClient>,
    ollama_client: Option<&OllamaClient>,
    _budget: &Arc<BudgetManager>,
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

    if let Some(ollama) = ollama_client {
        match ollama.complete_text(prompts::TITLE_SYSTEM, &user_prompt, 2048).await {
            Ok(text) => {
                let title = text.trim().trim_matches('"').to_string();
                if is_garbage_title(&title) {
                    info!(title = %title, backend = "ollama", "Rejected garbage title from Ollama, using heuristic");
                    return None;
                }
                info!(title = %title, backend = "ollama", "Generated situation title");
                return Some(title);
            }
            Err(e) => {
                debug!(error = %e, "Ollama title generation failed, using heuristic");
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
    fn test_garbage_global_prefix() {
        assert!(is_garbage_title("Global Wildfire Activity Spreads"));
        assert!(is_garbage_title("Global EU Policy Shifts"));
        assert!(is_garbage_title("Global Central Bank Policy Shifts"));
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

    // Region-concatenation garbage tests
    #[test]
    fn test_garbage_region_concatenation() {
        assert!(is_garbage_title("UN South Asia Middle East East Asia"));
        assert!(is_garbage_title("South Asia Middle East"));
        assert!(is_garbage_title("Africa Asia Europe"));
        assert!(is_garbage_title("EU NATO Pacific"));
    }
    #[test]
    fn test_good_title_with_action_not_region_garbage() {
        // These have action words so should NOT be flagged as region concatenation
        assert!(!is_garbage_title("South Asia Flooding Crisis"));
        assert!(!is_garbage_title("Middle East Ceasefire Talks"));
        assert!(!is_garbage_title("Africa Drought Emergency"));
        assert!(!is_garbage_title("EU Sanctions Vote"));
        assert!(!is_garbage_title("Pacific Typhoon Surge"));
    }
    #[test]
    fn test_good_title_country_with_action() {
        // Country names + action = good title
        assert!(!is_garbage_title("Sudan Civil War"));
        assert!(!is_garbage_title("Yemen Houthi Strikes"));
        assert!(!is_garbage_title("Iran Nuclear Talks"));
        assert!(!is_garbage_title("Syria Refugee Crisis"));
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
