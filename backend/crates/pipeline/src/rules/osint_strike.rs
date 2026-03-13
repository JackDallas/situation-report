use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Conflict-related keywords that indicate a strike/attack in Telegram messages.
const STRIKE_KEYWORDS: &[&str] = &[
    "strike",
    "attack",
    "airstrike",
    "missile",
    "explosion",
    "killed",
    "eliminated",
    "shelling",
    "bombardment",
    "drone",
    "intercept",
];

/// Rule 9: OSINT Strike
///
/// Detects strikes reported via OSINT channels (Telegram + news) with optional
/// satellite thermal confirmation.
///
/// - **Primary signal**: `telegram_message` with severity >= Medium and
///   conflict-related tags or keywords in title/description.
/// - **Corroboration**: `news_article` within 100km and 6h.
/// - **Optional confirmation**: `thermal_anomaly` within 50km and 12h
///   (accounting for satellite pass delays).
///
/// Severity:
/// - Telegram + news = High (dual-source OSINT confirmation)
/// - Telegram + news + thermal = Critical (triple-source confirmation)
pub struct OsintStrikeRule;

impl OsintStrikeRule {
    /// Check whether the trigger event contains conflict-related keywords
    /// in its tags, title, or description.
    fn has_strike_indicators(event: &InsertableEvent) -> bool {
        // Check tags
        let tag_match = event.tags.iter().any(|tag| {
            let lower = tag.to_lowercase();
            STRIKE_KEYWORDS.iter().any(|kw| lower.contains(kw))
        });
        if tag_match {
            return true;
        }

        // Check title
        if let Some(ref title) = event.title {
            let lower = title.to_lowercase();
            if STRIKE_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
                return true;
            }
        }

        // Check description
        if let Some(ref desc) = event.description {
            let lower = desc.to_lowercase();
            if STRIKE_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
                return true;
            }
        }

        // Check enrichment state_changes in payload
        if let Some(enrichment) = event.payload.get("enrichment") {
            if let Some(state_changes) = enrichment.get("state_changes") {
                if state_changes.is_array() && !state_changes.as_array().unwrap().is_empty() {
                    return true;
                }
            }
        }

        false
    }
}

impl CorrelationRule for OsintStrikeRule {
    fn id(&self) -> &str {
        "osint_strike"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::TelegramMessage, EventType::NewsArticle]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        // Rule requires geo-located trigger for spatial correlation
        let (lat, lon) = (trigger.latitude?, trigger.longitude?);

        // Only fire on Telegram triggers that have conflict indicators and
        // sufficient severity, or on NewsArticle triggers if a matching
        // Telegram message is already in the window.
        let is_telegram_trigger = trigger.event_type == EventType::TelegramMessage;
        let is_news_trigger = trigger.event_type == EventType::NewsArticle;

        if is_telegram_trigger {
            // Telegram trigger: must have severity >= Medium and strike keywords
            if trigger.severity.rank() < Severity::Medium.rank() {
                return None;
            }
            if !Self::has_strike_indicators(trigger) {
                return None;
            }
        }

        // Deduplicate: skip if active incident already covers this area
        if active.iter().any(|i| {
            if i.rule_id != "osint_strike" {
                return false;
            }
            if let (Some(ilat), Some(ilon)) = (i.latitude, i.longitude) {
                let dlat = (ilat - lat).abs();
                let dlon = (ilon - lon).abs();
                dlat < 1.0 && dlon < 1.0 // ~111km
            } else {
                false
            }
        }) {
            return None;
        }

        let news_within = Duration::from_secs(6 * 3600); // 6 hours
        let news_radius = 100.0; // 100km
        let thermal_within = Duration::from_secs(12 * 3600); // 12 hours
        let thermal_radius = 50.0; // 50km

        // Collect corroborating events depending on trigger type
        let (telegrams, mut news);

        if is_telegram_trigger {
            // Telegram is the trigger — look for news corroboration
            telegrams = vec![trigger];
            news = window.near(EventType::NewsArticle, lat, lon, news_radius, news_within);
            if news.is_empty() {
                return None; // Need at least news corroboration
            }
        } else if is_news_trigger {
            // News is the trigger — look for a qualifying Telegram message in the window
            let telegram_within = Duration::from_secs(6 * 3600);
            let telegram_candidates =
                window.near(EventType::TelegramMessage, lat, lon, news_radius, telegram_within);

            // Filter to only Telegram messages with strike indicators and severity >= Medium
            let qualifying: Vec<&InsertableEvent> = telegram_candidates
                .into_iter()
                .filter(|t| {
                    t.severity.rank() >= Severity::Medium.rank()
                        && Self::has_strike_indicators(t)
                })
                .collect();

            if qualifying.is_empty() {
                return None; // No qualifying Telegram to corroborate
            }
            telegrams = qualifying;
            news = vec![trigger];
        } else {
            return None;
        }

        // Optional: thermal anomaly confirmation (satellite data can lag)
        let thermal = window.near(EventType::ThermalAnomaly, lat, lon, thermal_radius, thermal_within);
        let has_thermal = !thermal.is_empty();

        // Sort evidence pools
        sort_by_severity(&mut news);

        // Build evidence list
        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        // Add Telegram evidence (if trigger was news, add the matching telegrams)
        if is_news_trigger {
            for t in telegrams.iter().take(3) {
                evidence.push(evidence_from(t, EvidenceRole::Corroboration));
            }
        }

        // Add news evidence (if trigger was telegram, add the matching news)
        if is_telegram_trigger {
            for n in news.iter().take(3) {
                if n.entity_id != trigger.entity_id
                    || n.event_type != trigger.event_type
                {
                    evidence.push(evidence_from(n, EvidenceRole::Corroboration));
                }
            }
        }

        // Add thermal evidence as context
        for t in thermal.iter().take(2) {
            evidence.push(evidence_from(t, EvidenceRole::Context));
        }

        let severity = if has_thermal {
            Severity::Critical // triple-source: telegram + news + satellite
        } else {
            Severity::High // dual-source: telegram + news
        };

        let confidence = if has_thermal { 0.90 } else { 0.75 };

        let location = trigger
            .payload
            .get("location")
            .and_then(|v| v.as_str())
            .or(trigger.payload.get("country").and_then(|v| v.as_str()))
            .or(trigger.region_code.as_deref())
            .unwrap_or("unknown location");

        let source_desc = if has_thermal {
            format!(
                "OSINT strike report corroborated by {} Telegram, {} news, and {} thermal sources.",
                telegrams.len(),
                news.len(),
                thermal.len()
            )
        } else {
            format!(
                "OSINT strike report corroborated by {} Telegram and {} news sources within {}km.",
                telegrams.len(),
                news.len(),
                news_radius as u32
            )
        };

        let mut tags = vec![
            "osint".into(),
            "strike".into(),
            "multi-source".into(),
        ];
        if has_thermal {
            tags.push("satellite-confirmed".into());
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "osint_strike".into(),
            title: format!("OSINT-confirmed strike near {location}"),
            description: source_desc,
            severity,
            confidence,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(lat),
            longitude: Some(lon),
            tags,
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
