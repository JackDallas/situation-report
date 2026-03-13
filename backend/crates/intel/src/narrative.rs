//! Narrative generation for situations using Claude Sonnet.
//! Follows the Dataminr ReGenAI pattern: regenerate entire narrative
//! on significant events, not append.

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::budget::BudgetManager;
use crate::client::ClaudeClient;
use crate::gemini::{GeminiClient, GeminiModel};
use crate::llm::LlmClient;

/// Generated narrative for a situation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationNarrative {
    pub situation_id: Uuid,
    pub version: i32,
    pub narrative_text: String,
    pub model: String,
    pub tokens_used: u32,
    pub generated_at: DateTime<Utc>,
}

/// Context assembled for narrative generation.
#[derive(Debug, Clone)]
pub struct NarrativeContext {
    pub situation_title: String,
    pub situation_id: Uuid,
    pub phase: String,
    pub severity: String,
    pub event_count: usize,
    pub source_types: Vec<String>,
    pub regions: Vec<String>,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    /// Recent event summaries (title + time + type).
    pub recent_events: Vec<EventBrief>,
    /// Entity relationships from the graph (serialized).
    pub entity_context: Option<String>,
    /// Previous narrative version for continuity.
    pub previous_narrative: Option<String>,
    pub current_version: i32,
    /// Whether this regen was triggered by an entity state change.
    #[allow(dead_code)]
    pub has_state_change: bool,
    /// Recent phase transitions (e.g. ["emerging", "developing", "active"]).
    pub phase_history: Vec<String>,
    /// Event rate trend: "accelerating", "steady", or "decelerating".
    pub event_rate_trend: String,
    /// Hours elapsed since the most recent event in this situation.
    pub hours_since_last_event: f64,
    /// Brief description of a similar historical situation, if any.
    pub similar_historical: Option<String>,
    /// Impact propagation summary from the entity graph.
    pub impact_summary: Option<String>,
    /// Cumulative summary from previous narrative cycles (situation memory).
    /// Provides long-term continuity across narrative regenerations.
    pub previous_summary: Option<String>,
}

/// Brief event description for narrative context.
#[derive(Debug, Clone)]
pub struct EventBrief {
    pub title: String,
    pub event_type: String,
    pub event_time: DateTime<Utc>,
    pub severity: String,
    pub source_type: String,
}

/// System prompt for narrative generation (CACHED).
const NARRATIVE_SYSTEM: &str = r#"You are a senior intelligence analyst writing situation narratives for a monitoring dashboard. Be concise and focus on MEANING, not just facts.

Structure (use markdown headings):

Start with 2-3 sentences: what happened, why it matters globally, and what it could signal. This is the most important part — connect this situation to broader geopolitical context. A reader should understand the significance immediately.

## What We Know
3-5 bullet points of confirmed facts, most important first. Include specific numbers, names, locations.

## Why It Matters
2-3 sentences connecting this to the bigger picture. How does this relate to ongoing conflicts, alliances, or strategic interests? What precedent does it set? Who benefits?

## Watch For
2-3 specific observable signals that would indicate escalation or resolution.

## Trajectory
1-2 sentences: Based on the event rate trend and phase history, what is the most likely
near-term trajectory? Is this situation escalating, stabilizing, or de-escalating?
State the confidence level (high/medium/low) based on source diversity and event count.

## Escalation Probability
One line: LOW / MEDIUM / HIGH / CRITICAL — with a brief justification.

Rules:
- Lead with SIGNIFICANCE, not a list of events
- Entity facts MUST come from the provided context only
- Write in plain English — avoid military jargon, ICAO codes, acronyms
- Never mention source names, data feeds, or telemetry types
- Keep under 300 words total
- Do NOT use "BLUF", acronym headers, or bureaucratic language
- For airspace/NOTAM situations: explain what the restrictions mean practically (military exercises? VIP movement? conflict preparation?)
- For network/cyber: explain impact on population and information flow"#;

/// Generate a narrative for a situation.
pub async fn generate_narrative(
    client: &ClaudeClient,
    budget: &Arc<BudgetManager>,
    ctx: &NarrativeContext,
) -> Result<Option<SituationNarrative>> {
    if !budget.can_afford_sonnet().await {
        tracing::debug!("Skipping narrative generation: budget exhausted");
        return Ok(None);
    }

    let model = std::env::var("INTEL_ANALYSIS_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let user_prompt = build_narrative_prompt(ctx);

    let response = client.complete(&model, NARRATIVE_SYSTEM, &user_prompt, 1500).await?;
    let text = ClaudeClient::extract_text(&response).unwrap_or_default();

    let tokens = response.usage.input_tokens + response.usage.output_tokens;
    let cache_read = response.usage.cache_read_input_tokens;
    budget.record_sonnet(response.usage.input_tokens, response.usage.output_tokens, cache_read);

    Ok(Some(SituationNarrative {
        situation_id: ctx.situation_id,
        version: ctx.current_version + 1,
        narrative_text: text.to_string(),
        model,
        tokens_used: tokens,
        generated_at: Utc::now(),
    }))
}

/// Determine whether this narrative warrants Sonnet (true) or can use Qwen (false).
fn needs_sonnet(ctx: &NarrativeContext) -> bool {
    // State changes (kills, arrests, etc.) → Sonnet
    if ctx.has_state_change {
        return true;
    }

    // High/Critical severity with multi-source corroboration → Sonnet
    let severity_high = ctx.severity == "high" || ctx.severity == "critical";
    if severity_high && ctx.source_types.len() >= 3 {
        return true;
    }

    false
}

/// Tiered narrative generation: deliberate model selection, not automatic fallback.
///
/// - High-severity (needs_advanced) → Gemini Flash directly (not as fallback)
/// - All other situations → Ollama. If Ollama fails → skip (return Ok(None))
/// - Claude removed from the chain entirely.
pub async fn generate_narrative_tiered(
    _claude: Option<&ClaudeClient>,
    gemini: Option<&GeminiClient>,
    llm: Option<&LlmClient>,
    budget: &Arc<BudgetManager>,
    ctx: &NarrativeContext,
) -> Result<Option<SituationNarrative>> {
    let user_prompt = build_narrative_prompt(ctx);
    let needs_advanced = needs_sonnet(ctx);

    // High-priority situations: use Gemini Flash directly (deliberate, not fallback)
    if needs_advanced {
        if let Some(gc) = gemini {
            if budget.can_afford_gemini().await {
                tracing::info!(
                    situation = %ctx.situation_title,
                    severity = %ctx.severity,
                    sources = ctx.source_types.len(),
                    "High-severity narrative → Gemini Flash"
                );
                match gc.generate_text(GeminiModel::Flash, NARRATIVE_SYSTEM, &user_prompt, 1500).await {
                    Ok(response) => {
                        budget.record_gemini(GeminiModel::Flash, &response);
                        let tokens = response.usage.prompt_token_count + response.usage.candidates_token_count;
                        return Ok(Some(SituationNarrative {
                            situation_id: ctx.situation_id,
                            version: ctx.current_version + 1,
                            narrative_text: response.text,
                            model: GeminiModel::Flash.display_name().to_string(),
                            tokens_used: tokens,
                            generated_at: Utc::now(),
                        }));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Gemini Flash narrative failed for high-severity situation, skipping");
                        return Ok(None);
                    }
                }
            }
            tracing::debug!("Gemini budget exhausted for high-severity narrative, falling through to Ollama");
        }
        // If Gemini not available, fall through to Ollama below
    }

    // Routine situations (or high-severity when Gemini unavailable): local LLM
    if let Some(lc) = llm {
        tracing::debug!(
            situation = %ctx.situation_title,
            "Generating narrative via local LLM (routine)"
        );
        match lc.generate_narrative(NARRATIVE_SYSTEM, &user_prompt).await {
            Ok((text, tokens)) => {
                return Ok(Some(SituationNarrative {
                    situation_id: ctx.situation_id,
                    version: ctx.current_version + 1,
                    narrative_text: text,
                    model: "llama-server".to_string(),
                    tokens_used: tokens,
                    generated_at: Utc::now(),
                }));
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM narrative failed, skipping");
                return Ok(None);
            }
        }
    }

    Ok(None)
}

/// Determine if a cumulative summary should be regenerated.
/// Triggers every 50 events or 6 hours, whichever comes first.
pub fn should_regenerate_summary(
    event_count: usize,
    event_count_at_last_summary: usize,
    last_summary_generated: Option<DateTime<Utc>>,
) -> bool {
    // Need at least some events before generating first summary
    if event_count < 10 {
        return false;
    }

    // Never generated — time for the first one
    if last_summary_generated.is_none() {
        return true;
    }

    // 50+ new events since last summary
    if event_count.saturating_sub(event_count_at_last_summary) >= 50 {
        return true;
    }

    // 6 hours elapsed
    if let Some(last) = last_summary_generated {
        let elapsed = Utc::now() - last;
        if elapsed >= chrono::Duration::hours(6) {
            return true;
        }
    }

    false
}

/// System prompt for cumulative summary generation (cheap model).
const SUMMARY_SYSTEM: &str = r#"You are an intelligence analyst creating a concise cumulative summary of a long-running situation. This summary will be used as memory context for future narrative generations.

Produce a JSON object with exactly these fields:
- "summary": A 3-5 sentence summary of the situation's full history and current state. Focus on key developments, turning points, and the current trajectory.
- "key_entities": An array of the most important entity names (people, organizations, locations) involved.
- "key_dates": An array of objects with "date" (ISO 8601) and "event" (brief description) for the most significant milestones.

Rules:
- Be factual and concise — this is reference material, not a narrative
- Prioritize the most significant developments
- Cap key_entities at 10 items
- Cap key_dates at 8 items
- Output valid JSON only, no markdown fencing"#;

/// Generate a cumulative summary for a situation using the cheap model tier.
/// Returns (summary_text, key_entities_json, key_dates_json) on success.
pub async fn generate_summary(
    gemini: Option<&GeminiClient>,
    llm: Option<&LlmClient>,
    budget: &Arc<BudgetManager>,
    narrative_text: &str,
    previous_summary: Option<&str>,
    situation_title: &str,
    event_count: usize,
    entities: &[String],
) -> Result<Option<(String, serde_json::Value, serde_json::Value)>> {
    let mut prompt = format!(
        "Situation: {situation_title}\nTotal events: {event_count}\nEntities: {}\n\n",
        entities.join(", ")
    );

    if let Some(prev) = previous_summary {
        prompt.push_str("## Previous Summary\n");
        prompt.push_str(prev);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Latest Narrative\n");
    prompt.push_str(narrative_text);
    prompt.push_str("\n\nGenerate the updated cumulative summary JSON.");

    // Try local LLM first (cheap), then Gemini Flash-Lite
    if let Some(lc) = llm {
        match lc.generate_narrative(SUMMARY_SYSTEM, &prompt).await {
            Ok((text, _tokens)) => {
                return parse_summary_response(&text);
            }
            Err(e) => {
                tracing::debug!("LLM summary generation failed: {e}");
            }
        }
    }

    if let Some(gc) = gemini {
        if budget.can_afford_gemini().await {
            match gc.generate_text(GeminiModel::FlashLite, SUMMARY_SYSTEM, &prompt, 800).await {
                Ok(response) => {
                    budget.record_gemini(GeminiModel::FlashLite, &response);
                    return parse_summary_response(&response.text);
                }
                Err(e) => {
                    tracing::debug!("Gemini summary generation failed: {e}");
                }
            }
        }
    }

    Ok(None)
}

/// Parse the LLM's JSON response into (summary_text, key_entities, key_dates).
fn parse_summary_response(text: &str) -> Result<Option<(String, serde_json::Value, serde_json::Value)>> {
    // Strip markdown code fences if present
    let cleaned = text
        .trim()
        .strip_prefix("```json")
        .or_else(|| text.trim().strip_prefix("```"))
        .unwrap_or(text.trim())
        .strip_suffix("```")
        .unwrap_or(text.trim())
        .trim();

    let parsed: serde_json::Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let summary = parsed["summary"].as_str().unwrap_or("").to_string();
    if summary.is_empty() {
        return Ok(None);
    }

    let key_entities = parsed.get("key_entities")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let key_dates = parsed.get("key_dates")
        .cloned()
        .unwrap_or(serde_json::json!([]));

    Ok(Some((summary, key_entities, key_dates)))
}

/// Determine if a narrative should be regenerated based on significance.
pub fn should_regenerate(
    current_version: i32,
    last_generated: Option<DateTime<Utc>>,
    event_count_since: usize,
    has_state_change: bool,
    severity_escalated: bool,
) -> bool {
    // Always generate first narrative
    if current_version == 0 {
        return true;
    }

    // Entity state change (kill, arrest, etc.) → immediate regen
    if has_state_change {
        return true;
    }

    // Severity escalation → immediate regen
    if severity_escalated {
        return true;
    }

    // Significant new events (30+) since last narrative
    if event_count_since >= 30 {
        return true;
    }

    // Time-based: regen every 120 minutes if significant new events
    if let Some(last) = last_generated {
        let elapsed = Utc::now() - last;
        if elapsed >= chrono::Duration::minutes(120) && event_count_since >= 10 {
            return true;
        }
    }

    false
}

fn build_narrative_prompt(ctx: &NarrativeContext) -> String {
    let mut prompt = format!(
        "Generate a situation narrative for: {}\n\n",
        ctx.situation_title
    );

    prompt.push_str(&format!("Situation ID: {}\n", ctx.situation_id));
    prompt.push_str(&format!("Phase: {}\n", ctx.phase));
    prompt.push_str(&format!("Severity: {}\n", ctx.severity));
    prompt.push_str(&format!("Event count: {}\n", ctx.event_count));
    prompt.push_str(&format!("Sources: {}\n", ctx.source_types.join(", ")));
    prompt.push_str(&format!("Regions: {}\n", ctx.regions.join(", ")));

    if !ctx.entities.is_empty() {
        prompt.push_str(&format!("\nKey entities: {}\n", ctx.entities.join(", ")));
    }
    if !ctx.topics.is_empty() {
        prompt.push_str(&format!("Topics: {}\n", ctx.topics.join(", ")));
    }

    if !ctx.recent_events.is_empty() {
        prompt.push_str("\n## Recent Events\n");
        for event in ctx.recent_events.iter().take(15) {
            prompt.push_str(&format!(
                "- [{}] {} | {} | sev={} | src={}\n",
                event.event_time.format("%H:%M UTC"),
                event.title,
                event.event_type,
                event.severity,
                event.source_type,
            ));
        }
    }

    if let Some(ref entity_ctx) = ctx.entity_context {
        prompt.push_str(&format!(
            "\n## Entity Context (from knowledge graph)\n{entity_ctx}\n"
        ));
    }

    // Temporal dynamics for predictive narrative
    prompt.push_str("\n## Temporal Dynamics\n");
    prompt.push_str(&format!("Event rate trend: {}\n", ctx.event_rate_trend));
    prompt.push_str(&format!("Hours since last event: {:.1}\n", ctx.hours_since_last_event));
    if !ctx.phase_history.is_empty() {
        prompt.push_str(&format!("Phase history: {}\n", ctx.phase_history.join(" → ")));
    }
    if let Some(ref historical) = ctx.similar_historical {
        prompt.push_str(&format!("Historical parallel: {historical}\n"));
    }

    if let Some(ref impact) = ctx.impact_summary {
        prompt.push_str("\n## Potential Impact Chain\n");
        prompt.push_str(impact);
        prompt.push_str("\n");
    }

    if let Some(ref summary) = ctx.previous_summary {
        prompt.push_str("\n## Cumulative Summary (situation memory)\n");
        prompt.push_str("This summary captures the full history of this long-running situation. ");
        prompt.push_str("Use it to maintain continuity and avoid repeating old information.\n");
        prompt.push_str(summary);
        prompt.push_str("\n");
    }

    if let Some(ref prev) = ctx.previous_narrative {
        prompt.push_str(&format!(
            "\n## Previous Narrative (version {})\n{prev}\n\nUpdate the narrative to reflect new developments. Maintain continuity.\n",
            ctx.current_version
        ));
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_regenerate_first_version() {
        assert!(should_regenerate(0, None, 0, false, false));
    }

    #[test]
    fn test_should_regenerate_on_state_change() {
        assert!(should_regenerate(1, Some(Utc::now()), 0, true, false));
    }

    #[test]
    fn test_should_not_regenerate_too_soon() {
        assert!(!should_regenerate(1, Some(Utc::now()), 2, false, false));
    }

    #[test]
    fn test_should_regenerate_after_timeout() {
        // 120+ min elapsed with 10+ events → should regenerate
        let old = Utc::now() - chrono::Duration::minutes(125);
        assert!(should_regenerate(1, Some(old), 10, false, false));
        // 120+ min but only 5 events → not enough
        assert!(!should_regenerate(1, Some(old), 5, false, false));
        // 90 min with 10 events → not enough time yet
        let recent = Utc::now() - chrono::Duration::minutes(90);
        assert!(!should_regenerate(1, Some(recent), 10, false, false));
    }

    #[test]
    fn test_build_prompt_includes_context() {
        let ctx = NarrativeContext {
            situation_title: "Test Situation".to_string(),
            situation_id: Uuid::new_v4(),
            phase: "active".to_string(),
            severity: "high".to_string(),
            event_count: 10,
            source_types: vec!["acled".to_string(), "gdelt".to_string()],
            regions: vec!["UA".to_string()],
            entities: vec!["Ukraine".to_string()],
            topics: vec!["conflict".to_string()],
            recent_events: vec![],
            entity_context: Some("Ukraine is a country in Eastern Europe".to_string()),
            previous_narrative: None,
            current_version: 0,
            has_state_change: false,
            phase_history: vec!["emerging".to_string(), "developing".to_string(), "active".to_string()],
            event_rate_trend: "accelerating".to_string(),
            hours_since_last_event: 0.5,
            similar_historical: Some("2014 Crimea annexation pattern".to_string()),
            impact_summary: Some("Directly affects: Northern Division (organization). Secondary impact: Base Omega (facility).".to_string()),
            previous_summary: None,
        };
        let prompt = build_narrative_prompt(&ctx);
        assert!(prompt.contains("Test Situation"));
        assert!(prompt.contains("Entity Context"));
        assert!(prompt.contains("Temporal Dynamics"));
        assert!(prompt.contains("accelerating"));
        assert!(prompt.contains("emerging → developing → active"));
        assert!(prompt.contains("Historical parallel: 2014 Crimea annexation pattern"));
        assert!(prompt.contains("Potential Impact Chain"));
        assert!(prompt.contains("Directly affects: Northern Division"));
    }

    fn make_ctx(severity: &str, sources: usize, state_change: bool) -> NarrativeContext {
        NarrativeContext {
            situation_title: "Test".to_string(),
            situation_id: Uuid::new_v4(),
            phase: "active".to_string(),
            severity: severity.to_string(),
            event_count: 10,
            source_types: (0..sources).map(|i| format!("src{i}")).collect(),
            regions: vec![],
            entities: vec![],
            topics: vec![],
            recent_events: vec![],
            entity_context: None,
            previous_narrative: None,
            current_version: 1,
            has_state_change: state_change,
            phase_history: vec![],
            event_rate_trend: "steady".to_string(),
            hours_since_last_event: 0.0,
            similar_historical: None,
            impact_summary: None,
            previous_summary: None,
        }
    }

    #[test]
    fn test_needs_sonnet_state_change() {
        let ctx = make_ctx("low", 1, true);
        assert!(needs_sonnet(&ctx));
    }

    #[test]
    fn test_needs_sonnet_high_severity_multi_source() {
        let ctx = make_ctx("high", 3, false);
        assert!(needs_sonnet(&ctx));
        let ctx = make_ctx("critical", 4, false);
        assert!(needs_sonnet(&ctx));
    }

    #[test]
    fn test_qwen_for_routine() {
        // Low severity, single source → Qwen
        let ctx = make_ctx("low", 1, false);
        assert!(!needs_sonnet(&ctx));
        // High severity but single source → Qwen
        let ctx = make_ctx("high", 1, false);
        assert!(!needs_sonnet(&ctx));
        // Medium severity, multi-source → Qwen
        let ctx = make_ctx("medium", 3, false);
        assert!(!needs_sonnet(&ctx));
    }
}
