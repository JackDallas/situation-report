use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 3: Confirmed Strike
/// Triggers on conflict_event + thermal_anomaly within 50km in 30 min.
/// ACLED conflict data + FIRMS fire/thermal detection = confirmed kinetic event.
pub struct ConfirmedStrikeRule;

impl CorrelationRule for ConfirmedStrikeRule {
    fn id(&self) -> &str {
        "confirmed_strike"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::ConflictEvent, EventType::ThermalAnomaly]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let (lat, lon) = (trigger.latitude?, trigger.longitude?);
        let within = Duration::from_secs(1800); // 30 min
        let radius = 50.0; // 50km

        // Deduplicate: skip if active incident already covers this area
        if active.iter().any(|i| {
            if i.rule_id != "confirmed_strike" {
                return false;
            }
            if let (Some(ilat), Some(ilon)) = (i.latitude, i.longitude) {
                let dlat = (ilat - lat).abs();
                let dlon = (ilon - lon).abs();
                dlat < 0.5 && dlon < 0.5 // ~55km
            } else {
                false
            }
        }) {
            return None;
        }

        let mut conflicts = window.near(EventType::ConflictEvent, lat, lon, radius, within);
        let mut thermal = window.near(EventType::ThermalAnomaly, lat, lon, radius, within);

        if conflicts.is_empty() || thermal.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut conflicts);
        sort_by_severity(&mut thermal);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        // Add conflict evidence
        for c in conflicts.iter().take(3) {
            if c.event_type != trigger.event_type
                || c.entity_id != trigger.entity_id
            {
                evidence.push(evidence_from(c, EvidenceRole::Corroboration));
            }
        }

        // Add thermal evidence
        for t in thermal.iter().take(3) {
            if t.event_type != trigger.event_type
                || t.entity_id != trigger.entity_id
            {
                evidence.push(evidence_from(t, EvidenceRole::Corroboration));
            }
        }

        let location = trigger
            .payload
            .get("location")
            .and_then(|v| v.as_str())
            .or(trigger.payload.get("country").and_then(|v| v.as_str()))
            .unwrap_or("unknown location");

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "confirmed_strike".into(),
            title: format!("Confirmed strike near {location}"),
            description: format!(
                "Conflict event corroborated by thermal anomaly within {}km. \
                 {} conflict reports, {} thermal detections in 30 min window.",
                radius as u32,
                conflicts.len(),
                thermal.len()
            ),
            severity: Severity::High,
            confidence: 0.85,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(lat),
            longitude: Some(lon),
            tags: vec!["conflict".into(), "thermal".into(), "confirmed".into()],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
