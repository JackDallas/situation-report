use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 7: Conflict Thermal Cluster
/// Triggers on thermal_anomaly + conflict_event within 6h.
/// 3+ thermal anomalies near a conflict zone indicate sustained bombardment.
pub struct ConflictThermalClusterRule;

impl CorrelationRule for ConflictThermalClusterRule {
    fn id(&self) -> &str {
        "conflict_thermal_cluster"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::ThermalAnomaly, EventType::ConflictEvent]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let (lat, lon) = (trigger.latitude?, trigger.longitude?);
        let within = Duration::from_secs(6 * 3600); // 6h
        let radius = 50.0; // 50km

        // Deduplicate
        if active.iter().any(|i| {
            if i.rule_id != "conflict_thermal_cluster" {
                return false;
            }
            if let (Some(ilat), Some(ilon)) = (i.latitude, i.longitude) {
                (ilat - lat).abs() < 0.5 && (ilon - lon).abs() < 0.5
            } else {
                false
            }
        }) {
            return None;
        }

        let mut thermal = window.near(EventType::ThermalAnomaly, lat, lon, radius, within);
        let mut conflicts = window.near(EventType::ConflictEvent, lat, lon, radius, within);

        // Need 3+ thermal anomalies AND at least 1 conflict
        if thermal.len() < 3 || conflicts.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut thermal);
        sort_by_severity(&mut conflicts);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for t in thermal.iter().take(5) {
            if t.event_type != trigger.event_type || t.entity_id != trigger.entity_id {
                evidence.push(evidence_from(t, EvidenceRole::Corroboration));
            }
        }

        for c in conflicts.iter().take(3) {
            evidence.push(evidence_from(c, EvidenceRole::Context));
        }

        let region = trigger
            .region_code
            .as_deref()
            .or_else(|| {
                trigger
                    .payload
                    .get("country")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("unknown");

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "conflict_thermal_cluster".into(),
            title: format!("Sustained bombardment cluster near {region}"),
            description: format!(
                "{} thermal anomalies and {} conflict events detected within {}km over 6 hours. \
                 Indicates sustained kinetic activity.",
                thermal.len(),
                conflicts.len(),
                radius as u32
            ),
            severity: Severity::High,
            confidence: 0.8,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(lat),
            longitude: Some(lon),
            tags: vec![
                "conflict".into(),
                "thermal".into(),
                "cluster".into(),
                "sustained".into(),
            ],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
