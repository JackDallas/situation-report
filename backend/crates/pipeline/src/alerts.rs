//! Alert evaluation engine — checks events against alert rules and tracks fatigue.
//!
//! 4 tiers:
//! 1. Rule-based: keyword + severity + region matches on raw events
//! 2. Entity-driven: entity state change alerts (requires entity graph)
//! 3. Anomaly-based: z-score thresholds (driven by analytics)
//! 4. Semantic: cosine similarity against saved reference embeddings
//!
//! Default to situation phase change alerts for 10-100x noise reduction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sr_types::Severity;
use ts_rs::TS;
use uuid::Uuid;

use sr_sources::InsertableEvent;

/// An alert rule loaded from the database.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct AlertRule {
    pub id: Uuid,
    pub name: String,
    pub rule_type: String,
    #[ts(type = "Record<string, unknown>")]
    pub conditions: serde_json::Value,
    pub enabled: bool,
    pub cooldown_minutes: i32,
    pub max_per_hour: i32,
    pub min_severity: Severity,
    pub last_fired_at: Option<DateTime<Utc>>,
}

/// A fired alert ready for delivery.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../frontend/src/lib/types/generated/")]
pub struct FiredAlert {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub situation_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub severity: Severity,
    pub title: String,
    pub body: Option<String>,
    pub fired_at: DateTime<Utc>,
}

/// Tracks alert fatigue state per rule.
pub struct AlertTracker {
    /// Last fire time per rule ID.
    last_fired: HashMap<Uuid, DateTime<Utc>>,
    /// Fire count per rule in current hour.
    hourly_counts: HashMap<Uuid, (DateTime<Utc>, u32)>,
    /// Situation-scoped dedup: (rule_id, situation_id) → last fire time.
    situation_dedup: HashMap<(Uuid, Uuid), DateTime<Utc>>,
}

impl AlertTracker {
    pub fn new() -> Self {
        Self {
            last_fired: HashMap::new(),
            hourly_counts: HashMap::new(),
            situation_dedup: HashMap::new(),
        }
    }

    /// Check if a rule can fire (respects cooldown and hourly limits).
    pub fn can_fire(&self, rule: &AlertRule) -> bool {
        let now = Utc::now();

        // Cooldown check
        if let Some(last) = self.last_fired.get(&rule.id) {
            let elapsed = (now - *last).num_minutes();
            if elapsed < rule.cooldown_minutes as i64 {
                return false;
            }
        }

        // Hourly limit check
        if let Some((hour_start, count)) = self.hourly_counts.get(&rule.id) {
            let elapsed = (now - *hour_start).num_hours();
            if elapsed < 1 && *count >= rule.max_per_hour as u32 {
                return false;
            }
        }

        true
    }

    /// Check situation-scoped dedup (30min default).
    pub fn can_fire_for_situation(&self, rule_id: Uuid, situation_id: Uuid) -> bool {
        if let Some(last) = self.situation_dedup.get(&(rule_id, situation_id)) {
            let elapsed = (Utc::now() - *last).num_minutes();
            elapsed >= 30
        } else {
            true
        }
    }

    /// Record that a rule fired.
    pub fn record_fire(&mut self, rule_id: Uuid, situation_id: Option<Uuid>) {
        let now = Utc::now();
        self.last_fired.insert(rule_id, now);

        // Update hourly counter
        let entry = self.hourly_counts.entry(rule_id).or_insert((now, 0));
        let elapsed = (now - entry.0).num_hours();
        if elapsed >= 1 {
            *entry = (now, 1);
        } else {
            entry.1 += 1;
        }

        // Situation dedup
        if let Some(sit_id) = situation_id {
            self.situation_dedup.insert((rule_id, sit_id), now);
        }
    }

    /// Cleanup old entries (call periodically).
    pub fn cleanup(&mut self) {
        let cutoff = Utc::now() - chrono::Duration::hours(2);
        self.last_fired.retain(|_, v| *v > cutoff);
        self.hourly_counts.retain(|_, (t, _)| *t > cutoff);
        self.situation_dedup.retain(|_, v| *v > cutoff);
    }
}

impl Default for AlertTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluate a keyword-type rule against an event.
pub fn evaluate_keyword_rule(rule: &AlertRule, event: &InsertableEvent) -> Option<FiredAlert> {
    // Check minimum severity
    if event.severity.rank() < rule.min_severity.rank() {
        return None;
    }

    let conditions = &rule.conditions;

    // Keyword matching
    if let Some(keywords) = conditions.get("keywords").and_then(|v| v.as_array()) {
        let text = format!(
            "{} {}",
            event.title.as_deref().unwrap_or(""),
            event.description.as_deref().unwrap_or("")
        )
        .to_lowercase();

        let matched = keywords.iter().any(|kw| {
            kw.as_str()
                .is_some_and(|k| text.contains(&k.to_lowercase()))
        });
        if !matched {
            return None;
        }
    }

    // Region filter
    if let Some(regions) = conditions.get("regions").and_then(|v| v.as_array()) {
        if let Some(ref rc) = event.region_code {
            let region_match = regions.iter().any(|r| r.as_str() == Some(rc.as_str()));
            if !region_match {
                return None;
            }
        } else {
            return None; // No region on event but rule requires one
        }
    }

    // Event type filter
    if let Some(types) = conditions.get("event_types").and_then(|v| v.as_array()) {
        let type_match = types
            .iter()
            .any(|t| t.as_str() == Some(event.event_type.as_str()));
        if !type_match {
            return None;
        }
    }

    Some(FiredAlert {
        id: Uuid::new_v4(),
        rule_id: Some(rule.id),
        situation_id: None,
        event_id: None,
        severity: event.severity,
        title: format!("Alert: {}", rule.name),
        body: event.title.clone(),
        fired_at: Utc::now(),
    })
}

/// Evaluate all enabled rules against an event, respecting fatigue limits.
pub fn evaluate_rules(
    rules: &[AlertRule],
    tracker: &mut AlertTracker,
    event: &InsertableEvent,
) -> Vec<FiredAlert> {
    let mut alerts = Vec::new();

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if !tracker.can_fire(rule) {
            continue;
        }

        let fired = match rule.rule_type.as_str() {
            "keyword" => evaluate_keyword_rule(rule, event),
            // Other rule types (entity, anomaly, semantic) are evaluated
            // by their respective subsystems and call record_fire directly
            _ => None,
        };

        if let Some(alert) = fired {
            tracker.record_fire(rule.id, alert.situation_id);
            alerts.push(alert);
        }
    }

    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use sr_types::{EventType, SourceType};

    fn make_rule(name: &str, conditions: serde_json::Value) -> AlertRule {
        AlertRule {
            id: Uuid::new_v4(),
            name: name.to_string(),
            rule_type: "keyword".to_string(),
            conditions,
            enabled: true,
            cooldown_minutes: 30,
            max_per_hour: 10,
            min_severity: Severity::Medium,
            last_fired_at: None,
        }
    }

    fn make_event(title: &str, severity: Severity, region: &str) -> InsertableEvent {
        InsertableEvent {
            source_type: SourceType::Acled,
            event_type: EventType::ConflictEvent,
            severity,
            title: Some(title.to_string()),
            region_code: Some(region.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_keyword_match() {
        let rule = make_rule(
            "Nuclear Alert",
            json!({"keywords": ["nuclear", "radiation"]}),
        );
        let event = make_event("Nuclear test detected", Severity::High, "KP");
        let result = evaluate_keyword_rule(&rule, &event);
        assert!(result.is_some());
    }

    #[test]
    fn test_keyword_no_match() {
        let rule = make_rule("Nuclear Alert", json!({"keywords": ["nuclear"]}));
        let event = make_event("Weather update", Severity::Medium, "US");
        let result = evaluate_keyword_rule(&rule, &event);
        assert!(result.is_none());
    }

    #[test]
    fn test_severity_filter() {
        let rule = make_rule("High Only", json!({"keywords": ["attack"]}));
        let event = make_event("Cyber attack detected", Severity::Low, "US");
        let result = evaluate_keyword_rule(&rule, &event);
        assert!(result.is_none()); // low < medium (min_severity)
    }

    #[test]
    fn test_region_filter() {
        let rule = make_rule(
            "Ukraine Only",
            json!({"keywords": ["strike"], "regions": ["UA"]}),
        );

        let ua_event = make_event("Air strike reported", Severity::High, "UA");
        assert!(evaluate_keyword_rule(&rule, &ua_event).is_some());

        let ru_event = make_event("Air strike reported", Severity::High, "RU");
        assert!(evaluate_keyword_rule(&rule, &ru_event).is_none());
    }

    #[test]
    fn test_cooldown_enforcement() {
        let rule = make_rule("Test", json!({"keywords": ["test"]}));
        let mut tracker = AlertTracker::new();

        assert!(tracker.can_fire(&rule));
        tracker.record_fire(rule.id, None);
        assert!(!tracker.can_fire(&rule)); // cooldown active
    }

    #[test]
    fn test_evaluate_rules() {
        let rules = vec![
            make_rule("Nuclear", json!({"keywords": ["nuclear"]})),
            make_rule("Cyber", json!({"keywords": ["cyber"]})),
        ];
        let mut tracker = AlertTracker::new();
        let event = make_event("Nuclear test detected", Severity::High, "KP");

        let alerts = evaluate_rules(&rules, &mut tracker, &event);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].title.contains("Nuclear"));
    }
}
