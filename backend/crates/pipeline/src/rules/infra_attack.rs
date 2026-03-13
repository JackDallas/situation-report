use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 1: Infrastructure Attack
/// Triggers on shodan_banner + bgp_anomaly + internet_outage in the same region within 5 min.
/// Detects potential coordinated attacks on infrastructure (ICS exposure + BGP disruption + outage).
pub struct InfraAttackRule;

impl CorrelationRule for InfraAttackRule {
    fn id(&self) -> &str {
        "infra_attack"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::ShodanBanner, EventType::BgpAnomaly, EventType::InternetOutage]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let region = trigger.region_code.as_deref()?;
        let within = Duration::from_secs(300); // 5 min

        // Check for existing active incident of this type in this region
        if active.iter().any(|i| {
            i.rule_id == "infra_attack" && i.region_code.as_deref() == Some(region)
        }) {
            return None;
        }

        // Need all three types in the same region
        let mut shodan = window.by_type_and_region(EventType::ShodanBanner, region, within);
        let mut bgp = window.by_type_and_region(EventType::BgpAnomaly, region, within);
        let mut outage = window.by_type_and_region(EventType::InternetOutage, region, within);

        // Require meaningful volume — single events of each type is routine
        // co-occurrence, not an attack. Need at least 2 Shodan exposures,
        // 3 BGP anomalies, and 2 outage events to reduce false positives.
        if shodan.len() < 2 || bgp.len() < 3 || outage.len() < 2 {
            return None;
        }

        // Sort each evidence pool by severity descending (highest first)
        sort_by_severity(&mut shodan);
        sort_by_severity(&mut bgp);
        sort_by_severity(&mut outage);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        // Add corroboration from each type (highest severity first)
        if let Some(s) = shodan.first()
            && (s.event_type != trigger.event_type || s.entity_id != trigger.entity_id)
        {
            evidence.push(evidence_from(s, EvidenceRole::Corroboration));
        }
        if let Some(b) = bgp.first()
            && (b.event_type != trigger.event_type || b.entity_id != trigger.entity_id)
        {
            evidence.push(evidence_from(b, EvidenceRole::Corroboration));
        }
        if let Some(o) = outage.first()
            && (o.event_type != trigger.event_type || o.entity_id != trigger.entity_id)
        {
            evidence.push(evidence_from(o, EvidenceRole::Corroboration));
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "infra_attack".into(),
            title: format!("Infrastructure attack detected in {region}"),
            description: format!(
                "ICS/SCADA exposure (Shodan), BGP disruption, and internet outage detected in region {region} within 5 minutes. \
                 {} exposed services, {} BGP anomalies, {} outage events.",
                shodan.len(),
                bgp.len(),
                outage.len()
            ),
            severity: Severity::High,
            confidence: 0.75,
            first_seen: now,
            last_updated: now,
            region_code: Some(region.to_string()),
            latitude: trigger.latitude,
            longitude: trigger.longitude,
            tags: vec!["infrastructure".into(), "cyber".into(), "multi-source".into()],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
