use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 4: Coordinated Shutdown
/// Triggers on internet_outage + bgp_anomaly + censorship_event in the same region within 15 min.
/// Multiple infrastructure disruptions + censorship = state-directed internet shutdown.
pub struct CoordinatedShutdownRule;

impl CorrelationRule for CoordinatedShutdownRule {
    fn id(&self) -> &str {
        "coordinated_shutdown"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::InternetOutage, EventType::BgpAnomaly, EventType::CensorshipEvent]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let region = trigger.region_code.as_deref()?;
        let within = Duration::from_secs(900); // 15 min

        if active.iter().any(|i| {
            i.rule_id == "coordinated_shutdown" && i.region_code.as_deref() == Some(region)
        }) {
            return None;
        }

        let mut outage = window.by_type_and_region(EventType::InternetOutage, region, within);
        let mut bgp = window.by_type_and_region(EventType::BgpAnomaly, region, within);
        let mut censorship = window.by_type_and_region(EventType::CensorshipEvent, region, within);

        // Require meaningful volume — single events of each type is routine
        // co-occurrence in monitored regions. Need at least 3 BGP anomalies,
        // 2 outage events, and 2 censorship events to reduce false positives.
        if bgp.len() < 3 || outage.len() < 2 || censorship.len() < 2 {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut outage);
        sort_by_severity(&mut bgp);
        sort_by_severity(&mut censorship);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for o in outage.iter().take(2) {
            if o.event_type != trigger.event_type || o.entity_id != trigger.entity_id {
                evidence.push(evidence_from(o, EvidenceRole::Corroboration));
            }
        }

        for b in bgp.iter().take(2) {
            if b.event_type != trigger.event_type || b.entity_id != trigger.entity_id {
                evidence.push(evidence_from(b, EvidenceRole::Corroboration));
            }
        }

        for c in censorship.iter().take(2) {
            if c.event_type != trigger.event_type || c.entity_id != trigger.entity_id {
                evidence.push(evidence_from(c, EvidenceRole::Corroboration));
            }
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "coordinated_shutdown".into(),
            title: format!("Coordinated internet shutdown in {region}"),
            description: format!(
                "Internet outage, BGP disruption, and censorship detected in {region} within 15 minutes. \
                 {} outages, {} BGP anomalies, {} censorship events. Likely state-directed shutdown.",
                outage.len(),
                bgp.len(),
                censorship.len()
            ),
            severity: Severity::High,
            confidence: 0.8,
            first_seen: now,
            last_updated: now,
            region_code: Some(region.to_string()),
            latitude: trigger.latitude,
            longitude: trigger.longitude,
            tags: vec![
                "shutdown".into(),
                "censorship".into(),
                "infrastructure".into(),
                "multi-source".into(),
            ],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
