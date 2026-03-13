use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 4: Coordinated Shutdown
/// Triggers on internet_outage + bgp_anomaly + censorship_event in the **same country** within 15 min.
/// Multiple infrastructure disruptions + censorship = state-directed internet shutdown.
///
/// Country-level correlation prevents false positives from mega-regions like "middle-east"
/// where routine BGP churn in one country + censorship in another would falsely correlate.
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
        // Extract country code from payload/title — required for country-level correlation
        let country = window.event_country(trigger)?;

        // Cooldown: no duplicate for same country within active incidents
        if active.iter().any(|i| {
            i.rule_id == "coordinated_shutdown"
                && i.tags.iter().any(|t| t.starts_with("country:") && &t[8..] == country)
        }) {
            return None;
        }

        let within = Duration::from_secs(300); // 5 min — tight window to catch genuine surges

        let mut outage = window.by_type_and_country(EventType::InternetOutage, country, within);
        let mut bgp = window.by_type_and_country(EventType::BgpAnomaly, country, within);
        let mut censorship = window.by_type_and_country(EventType::CensorshipEvent, country, within);

        // Countries like Iran produce 400+ BGP anomalies/hour and constant censorship
        // events as baseline. A real coordinated shutdown needs an EXTREME spike:
        // 50+ BGP withdrawals in 5 min (vs ~30 baseline), 5+ outage reports, and
        // 10+ censorship events simultaneously.
        if bgp.len() < 50 || outage.len() < 5 || censorship.len() < 10 {
            return None;
        }

        sort_by_severity(&mut outage);
        sort_by_severity(&mut bgp);
        sort_by_severity(&mut censorship);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for o in outage.iter().take(3) {
            if o.event_type != trigger.event_type || o.entity_id != trigger.entity_id {
                evidence.push(evidence_from(o, EvidenceRole::Corroboration));
            }
        }

        for b in bgp.iter().take(3) {
            if b.event_type != trigger.event_type || b.entity_id != trigger.entity_id {
                evidence.push(evidence_from(b, EvidenceRole::Corroboration));
            }
        }

        for c in censorship.iter().take(3) {
            if c.event_type != trigger.event_type || c.entity_id != trigger.entity_id {
                evidence.push(evidence_from(c, EvidenceRole::Corroboration));
            }
        }

        let region = trigger.region_code.clone().unwrap_or_default();

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "coordinated_shutdown".into(),
            title: format!("Coordinated internet shutdown in {country}"),
            description: format!(
                "Internet outage, BGP disruption, and censorship detected in {country} within 15 minutes. \
                 {} outages, {} BGP anomalies, {} censorship events. Likely state-directed shutdown.",
                outage.len(),
                bgp.len(),
                censorship.len()
            ),
            severity: Severity::High,
            confidence: 0.8,
            first_seen: now,
            last_updated: now,
            region_code: Some(region),
            latitude: trigger.latitude,
            longitude: trigger.longitude,
            tags: vec![
                "shutdown".into(),
                "censorship".into(),
                "infrastructure".into(),
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
