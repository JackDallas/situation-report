use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Rule 6: APT Staging
/// Triggers on cert_issued + threat_intel within 60 min.
/// New certificate issuance correlating with OTX threat intelligence pulse for the same domain
/// suggests APT infrastructure staging.
pub struct AptStagingRule;

impl CorrelationRule for AptStagingRule {
    fn id(&self) -> &str {
        "apt_staging"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::CertIssued, EventType::ThreatIntel]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let within = Duration::from_secs(3600); // 60 min

        // Extract domain from the trigger event
        let trigger_domain = extract_domain(trigger)?;

        // Deduplicate by domain
        if active.iter().any(|i| {
            i.rule_id == "apt_staging"
                && i.tags.iter().any(|t| t == &trigger_domain)
        }) {
            return None;
        }

        let certs = window.by_type(EventType::CertIssued, within);
        let intel = window.by_type(EventType::ThreatIntel, within);

        if certs.is_empty() || intel.is_empty() {
            return None;
        }

        // Look for domain overlap between certs and threat intel
        let mut matching_certs = Vec::new();
        let mut matching_intel = Vec::new();

        for cert in certs {
            if let Some(domain) = extract_domain(cert)
                && domain == trigger_domain
            {
                matching_certs.push(cert);
            }
        }

        for ti in intel {
            let ti_domain = extract_domain(ti);

            // Check if domain matches directly or appears in indicators array
            let indicators_match = ti
                .payload
                .get("indicators")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter().any(|ind| {
                        ind.as_str()
                            .or_else(|| ind.get("indicator").and_then(|i| i.as_str()))
                            .is_some_and(|s| s.contains(&trigger_domain))
                    })
                })
                .unwrap_or(false);

            if ti_domain.as_deref() == Some(&trigger_domain) || indicators_match {
                matching_intel.push(ti);
            }
        }

        if matching_certs.is_empty() || matching_intel.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut matching_certs);
        sort_by_severity(&mut matching_intel);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for c in matching_certs.iter().take(2) {
            evidence.push(evidence_from(c, EvidenceRole::Corroboration));
        }

        for i in matching_intel.iter().take(2) {
            evidence.push(evidence_from(i, EvidenceRole::Corroboration));
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "apt_staging".into(),
            title: format!("APT staging infrastructure: {trigger_domain}"),
            description: format!(
                "New TLS certificate for {trigger_domain} coincides with threat intelligence pulse. \
                 {} matching certs, {} matching intel reports in 60 min window.",
                matching_certs.len(),
                matching_intel.len()
            ),
            severity: Severity::High,
            confidence: 0.65,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: trigger.latitude,
            longitude: trigger.longitude,
            tags: vec![
                "apt".into(),
                "cyber".into(),
                "infrastructure".into(),
                trigger_domain,
            ],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}

/// Extract a domain string from an event's payload or entity_id.
fn extract_domain(event: &InsertableEvent) -> Option<String> {
    // Try payload fields
    if let Some(domain) = event.payload.get("domain").and_then(|v| v.as_str()) {
        return Some(domain.to_string());
    }
    if let Some(cn) = event.payload.get("common_name").and_then(|v| v.as_str()) {
        return Some(cn.to_string());
    }
    if let Some(name) = event.payload.get("pulse_name").and_then(|v| v.as_str()) {
        // Try to extract domain-like strings from pulse name
        for part in name.split_whitespace() {
            if part.contains('.') && !part.starts_with('.') && part.len() > 3 {
                return Some(part.to_string());
            }
        }
    }
    // Fall back to entity_id
    event.entity_id.clone()
}
