use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::{EvidenceRef, Incident};
use crate::window::CorrelationWindow;

/// Rule 5: Maritime Enforcement
/// Triggers on vessel_position + fishing_event within 30km in 30 min.
/// Vessel near an illegal fishing event suggests enforcement activity or IUU detection.
pub struct MaritimeEnforcementRule;

impl CorrelationRule for MaritimeEnforcementRule {
    fn id(&self) -> &str {
        "maritime_enforcement"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::VesselPosition, EventType::FishingEvent]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let (lat, lon) = (trigger.latitude?, trigger.longitude?);
        let within = Duration::from_secs(1800); // 30 min
        let radius = 30.0; // 30km

        // Deduplicate
        if active.iter().any(|i| {
            if i.rule_id != "maritime_enforcement" {
                return false;
            }
            if let (Some(ilat), Some(ilon)) = (i.latitude, i.longitude) {
                (ilat - lat).abs() < 0.3 && (ilon - lon).abs() < 0.3
            } else {
                false
            }
        }) {
            return None;
        }

        let mut vessels = window.near(EventType::VesselPosition, lat, lon, radius, within);
        let mut fishing = window.near(EventType::FishingEvent, lat, lon, radius, within);

        if vessels.is_empty() || fishing.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut vessels);
        sort_by_severity(&mut fishing);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        // Vessels use entity_name as the display title (vessel name/MMSI)
        for v in vessels.iter().take(3) {
            if v.event_type != trigger.event_type || v.entity_id != trigger.entity_id {
                evidence.push(EvidenceRef {
                    title: v.entity_name.clone(),
                    ..evidence_from(v, EvidenceRole::Corroboration)
                });
            }
        }

        for f in fishing.iter().take(2) {
            if f.event_type != trigger.event_type || f.entity_id != trigger.entity_id {
                evidence.push(evidence_from(f, EvidenceRole::Corroboration));
            }
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "maritime_enforcement".into(),
            title: format!("Maritime enforcement activity near {:.1}, {:.1}", lat, lon),
            description: format!(
                "Vessel activity detected near illegal fishing event within {}km. \
                 {} vessels, {} fishing events in 30 min window.",
                radius as u32,
                vessels.len(),
                fishing.len()
            ),
            severity: Severity::Medium,
            confidence: 0.6,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(lat),
            longitude: Some(lon),
            tags: vec!["maritime".into(), "fishing".into(), "enforcement".into()],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
