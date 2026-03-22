use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::{EvidenceRef, Incident};
use crate::window::CorrelationWindow;

/// Rule 2: Military Strike
/// Triggers on flight_position + notam_event + seismic_event within 10 min.
/// Military aircraft activity near a NOTAM zone with seismic activity suggests a strike.
pub struct MilitaryStrikeRule;

impl CorrelationRule for MilitaryStrikeRule {
    fn id(&self) -> &str {
        "military_strike"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::FlightPosition, EventType::NotamEvent, EventType::SeismicEvent]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let (lat, lon) = (trigger.latitude?, trigger.longitude?);
        let within = Duration::from_secs(600); // 10 min
        let radius = 100.0; // 100km

        // Check for existing active incident nearby
        if active.iter().any(|i| {
            if i.rule_id != "military_strike" {
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

        let flights = window.near(EventType::FlightPosition, lat, lon, radius, within);
        let mut seismic = window.near(EventType::SeismicEvent, lat, lon, radius, within);

        // NOTAMs often lack lat/lon — fall back to region-based matching
        let mut notams = {
            let geo_notams = window.near(EventType::NotamEvent, lat, lon, radius, within);
            if geo_notams.is_empty() {
                if let Some(region) = trigger.region_code.as_deref() {
                    window.by_type_and_region(EventType::NotamEvent, region, within)
                } else {
                    vec![]
                }
            } else {
                geo_notams
            }
        };

        if flights.is_empty() || notams.is_empty() || seismic.is_empty() {
            return None;
        }

        // Filter out natural earthquakes: genuine strike-induced seismic events
        // are typically shallow (< 10km depth) and low magnitude (< 4.0).
        // Natural earthquakes (M4.0+, deeper) in seismically active regions like
        // Greece should not be treated as evidence of military strikes.
        seismic.retain(|s| {
            let mag = s.payload.get("mag")
                .or_else(|| s.payload.get("magnitude"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let depth = s.payload.get("depth")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            // Keep only explosion-like signatures: low magnitude, shallow depth
            mag < 4.0 || depth < 10.0
        });
        if seismic.is_empty() {
            return None;
        }

        // Check for military-related flights
        let mut military_flights: Vec<&InsertableEvent> = flights
            .iter()
            .filter(|f| {
                let tags = &f.tags;
                let name = f.entity_name.as_deref().unwrap_or("");
                tags.iter().any(|t| t.contains("military"))
                    || name.to_uppercase().contains("MIL")
                    || f.source_type.as_str().contains("military")
                    || f.source_type.is_flight_source()
            })
            .copied()
            .collect();

        if military_flights.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut military_flights);
        sort_by_severity(&mut notams);
        sort_by_severity(&mut seismic);

        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        // Flights use entity_name as the display title (callsign/tail number)
        for f in military_flights.iter().take(3) {
            evidence.push(EvidenceRef {
                title: f.entity_name.clone(),
                ..evidence_from(f, EvidenceRole::Corroboration)
            });
        }

        if let Some(n) = notams.first() {
            evidence.push(evidence_from(n, EvidenceRole::Context));
        }

        if let Some(s) = seismic.first() {
            evidence.push(evidence_from(s, EvidenceRole::Corroboration));
        }

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "military_strike".into(),
            title: format!(
                "Possible military strike near {:.1}, {:.1}",
                lat, lon
            ),
            description: format!(
                "{} military aircraft, {} NOTAMs, and {} seismic events detected within {}km in 10 minutes.",
                military_flights.len(),
                notams.len(),
                seismic.len(),
                radius as u32
            ),
            severity: Severity::Critical,
            confidence: 0.7,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(lat),
            longitude: Some(lon),
            tags: vec!["military".into(), "strike".into(), "multi-source".into()],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
