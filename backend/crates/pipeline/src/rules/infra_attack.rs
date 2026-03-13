use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 1: Infrastructure Attack
/// Triggers on shodan_banner + bgp_anomaly + internet_outage in the **same country** within 10 min.
/// Detects potential coordinated attacks on infrastructure (ICS exposure + BGP disruption + outage).
///
/// Country-level correlation prevents false positives from Shodan's continuous stream
/// of banners across a mega-region being correlated with unrelated BGP/outage events.
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
        // Extract country code — required for country-level correlation
        let country = window.event_country(trigger)?;

        // Cooldown: no duplicate for same country within active incidents
        if active.iter().any(|i| {
            i.rule_id == "infra_attack"
                && i.tags.iter().any(|t| t.starts_with("country:") && &t[8..] == country)
        }) {
            return None;
        }

        let within = Duration::from_secs(300); // 5 min — tight window for genuine attacks

        // Need all three types in the same country
        let mut shodan = window.by_type_and_country(EventType::ShodanBanner, country, within);
        let mut bgp = window.by_type_and_country(EventType::BgpAnomaly, country, within);
        let mut outage = window.by_type_and_country(EventType::InternetOutage, country, within);

        // Shodan streams hundreds of banners/hour, BGP churn is 400+/hour for
        // countries like Iran/Russia. A real infra attack needs extreme co-occurrence:
        // 20+ Shodan exposures, 50+ BGP anomalies, and 5+ outage reports in 5 min.
        if shodan.len() < 20 || bgp.len() < 50 || outage.len() < 5 {
            return None;
        }

        sort_by_severity(&mut shodan);
        sort_by_severity(&mut bgp);
        sort_by_severity(&mut outage);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for s in shodan.iter().take(2) {
            if s.event_type != trigger.event_type || s.entity_id != trigger.entity_id {
                evidence.push(evidence_from(s, EvidenceRole::Corroboration));
            }
        }
        for b in bgp.iter().take(2) {
            if b.event_type != trigger.event_type || b.entity_id != trigger.entity_id {
                evidence.push(evidence_from(b, EvidenceRole::Corroboration));
            }
        }
        for o in outage.iter().take(2) {
            if o.event_type != trigger.event_type || o.entity_id != trigger.entity_id {
                evidence.push(evidence_from(o, EvidenceRole::Corroboration));
            }
        }

        let region = trigger.region_code.clone().unwrap_or_default();

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "infra_attack".into(),
            title: format!("Infrastructure attack detected in {country}"),
            description: format!(
                "ICS/SCADA exposure (Shodan), BGP disruption, and internet outage detected in {country} within 10 minutes. \
                 {} exposed services, {} BGP anomalies, {} outage events.",
                shodan.len(),
                bgp.len(),
                outage.len()
            ),
            severity: Severity::High,
            confidence: 0.75,
            first_seen: now,
            last_updated: now,
            region_code: Some(region),
            latitude: trigger.latitude,
            longitude: trigger.longitude,
            tags: vec![
                "infrastructure".into(),
                "cyber".into(),
                "multi-source".into(),
                format!("country:{country}"),
            ],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
