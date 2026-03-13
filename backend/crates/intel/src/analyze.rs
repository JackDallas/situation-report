use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::budget::BudgetManager;
use crate::client::ClaudeClient;
use crate::ollama::OllamaClient;
use crate::prompts;
use crate::types::{
    AnalysisReport, EntityConnection, EventSummary, SituationSummary, SuggestedMerge,
    TopicCluster,
};

/// Default analysis model — more capable.
fn analysis_model() -> String {
    std::env::var("INTEL_ANALYSIS_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-6".to_string())
}

/// Input bundle for periodic analysis.
pub struct AnalysisInput {
    pub situations: Vec<SituationSummary>,
    pub recent_events: Vec<EventSummary>,
    pub tempo: String,
}

/// Run periodic situation analysis using Sonnet.
///
/// Returns None if budget doesn't allow Sonnet calls.
pub async fn analyze_current_state(
    client: &ClaudeClient,
    budget: &Arc<BudgetManager>,
    input: &AnalysisInput,
) -> Result<AnalysisReport> {
    if !budget.can_afford_sonnet().await {
        bail!("Budget too low for Sonnet analysis — skipping");
    }

    let model = analysis_model();
    let user_msg = prompts::analysis_user(&input.situations, &input.recent_events, &input.tempo);

    info!(
        situations = input.situations.len(),
        events = input.recent_events.len(),
        tempo = input.tempo,
        "Running periodic intelligence analysis"
    );

    let response = client
        .complete(&model, prompts::ANALYSIS_SYSTEM, &user_msg, 8192)
        .await
        .context("Sonnet analysis call failed")?;

    budget.record_sonnet(
        response.usage.input_tokens,
        response.usage.output_tokens,
        response.usage.cache_read_input_tokens,
    );

    let text = ClaudeClient::extract_text(&response)
        .context("No text in analysis response")?;

    // Strip markdown code fences if present
    let json_str = text
        .trim()
        .strip_prefix("```json")
        .or_else(|| text.trim().strip_prefix("```"))
        .unwrap_or(text.trim());
    let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

    let parsed: serde_json::Value =
        serde_json::from_str(json_str).context("Failed to parse analysis JSON")?;

    let report = AnalysisReport {
        id: Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        narrative: parsed["narrative"]
            .as_str()
            .unwrap_or("Analysis unavailable.")
            .to_string(),
        suggested_merges: parse_merges(&parsed["suggested_merges"]),
        topic_clusters: parse_clusters(&parsed["topic_clusters"]),
        escalation_assessment: parsed["escalation_assessment"]
            .as_str()
            .unwrap_or("STABLE")
            .to_string(),
        key_entities: parse_entity_connections(&parsed["key_entities"]),
        model: model.clone(),
        tokens_used: response.usage.total_tokens(),
        tempo: input.tempo.clone(),
    };

    debug!(
        merges = report.suggested_merges.len(),
        clusters = report.topic_clusters.len(),
        entities = report.key_entities.len(),
        tokens = report.tokens_used,
        "Analysis complete"
    );

    Ok(report)
}

/// Tiered analysis: Qwen for NORMAL/ELEVATED, Sonnet for HIGH tempo.
///
/// If Qwen sets `escalate: true`, returns `Ok((report, true))` to signal
/// the caller should re-run with Sonnet.
pub async fn analyze_tiered(
    claude: Option<&ClaudeClient>,
    ollama: Option<&OllamaClient>,
    budget: &Arc<BudgetManager>,
    input: &AnalysisInput,
) -> Result<(AnalysisReport, bool)> {
    let use_sonnet = input.tempo == "HIGH";

    // HIGH tempo → Sonnet directly
    if use_sonnet {
        if let Some(client) = claude {
            if budget.can_afford_sonnet().await {
                info!(tempo = %input.tempo, "HIGH tempo — running Sonnet analysis");
                let report = analyze_current_state(client, budget, input).await?;
                return Ok((report, false));
            }
        }
    }

    // NORMAL/ELEVATED → Qwen
    if let Some(oc) = ollama {
        let user_msg = prompts::analysis_user(&input.situations, &input.recent_events, &input.tempo);

        info!(
            situations = input.situations.len(),
            events = input.recent_events.len(),
            tempo = %input.tempo,
            "Running periodic analysis via Qwen (routine)"
        );

        let (json_str, tokens) = oc.analyze(prompts::ANALYSIS_SYSTEM, &user_msg).await?;

        // Strip markdown code fences if present
        let clean = json_str
            .trim()
            .strip_prefix("```json")
            .or_else(|| json_str.trim().strip_prefix("```"))
            .unwrap_or(json_str.trim());
        let clean = clean.strip_suffix("```").unwrap_or(clean).trim();

        let parsed: serde_json::Value =
            serde_json::from_str(clean).context("Failed to parse Qwen analysis JSON")?;

        let escalate = parsed["escalate"].as_bool().unwrap_or(false);

        let report = AnalysisReport {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            narrative: parsed["narrative"]
                .as_str()
                .unwrap_or("Analysis unavailable.")
                .to_string(),
            suggested_merges: parse_merges(&parsed["suggested_merges"]),
            topic_clusters: parse_clusters(&parsed["topic_clusters"]),
            escalation_assessment: parsed["escalation_assessment"]
                .as_str()
                .unwrap_or("STABLE")
                .to_string(),
            key_entities: parse_entity_connections(&parsed["key_entities"]),
            model: oc.model().to_string(),
            tokens_used: tokens,
            tempo: input.tempo.clone(),
        };

        debug!(
            merges = report.suggested_merges.len(),
            clusters = report.topic_clusters.len(),
            escalate,
            tokens,
            "Qwen analysis complete"
        );

        if escalate {
            warn!("Qwen flagged escalation — Sonnet re-analysis recommended");
        }

        return Ok((report, escalate));
    }

    // Fallback: Sonnet
    if let Some(client) = claude {
        let report = analyze_current_state(client, budget, input).await?;
        return Ok((report, false));
    }

    bail!("No LLM backend available for analysis");
}

fn parse_merges(value: &serde_json::Value) -> Vec<SuggestedMerge> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(SuggestedMerge {
                incident_a_id: v["incident_a_id"].as_str()?.to_string(),
                incident_b_id: v["incident_b_id"].as_str()?.to_string(),
                confidence: v["confidence"].as_f64().unwrap_or(0.5) as f32,
                reason: v["reason"].as_str().unwrap_or("").to_string(),
                suggested_title: v["suggested_title"].as_str().map(String::from),
            })
        })
        .collect()
}

fn parse_clusters(value: &serde_json::Value) -> Vec<TopicCluster> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(TopicCluster {
                label: v["label"].as_str()?.to_string(),
                topics: v["topics"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                event_count: v["event_count"].as_u64().unwrap_or(0) as u32,
                regions: v["regions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn parse_entity_connections(value: &serde_json::Value) -> Vec<EntityConnection> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            Some(EntityConnection {
                entity_name: v["entity_name"].as_str()?.to_string(),
                entity_type: v["entity_type"].as_str().unwrap_or("unknown").to_string(),
                source_count: v["source_count"].as_u64().unwrap_or(1) as u32,
                context: v["context"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Determine tempo label from events-per-minute rate.
pub fn tempo_label(events_per_min: f64) -> &'static str {
    if events_per_min > 20.0 {
        "HIGH"
    } else if events_per_min > 5.0 {
        "ELEVATED"
    } else {
        "NORMAL"
    }
}

/// Get analysis interval based on tempo.
pub fn analysis_interval_secs(tempo: &str) -> u64 {
    match tempo {
        "HIGH" => 900,      // 15 min
        "ELEVATED" => 3600, // 60 min
        _ => 7200,          // 120 min
    }
}
