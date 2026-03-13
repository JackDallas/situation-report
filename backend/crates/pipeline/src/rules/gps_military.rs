use std::time::Duration;

use chrono::Utc;
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{CorrelationRule, sort_by_severity, evidence_from};
use crate::types::{EvidenceRef, Incident};
use crate::window::CorrelationWindow;

/// Rule 8: GPS/Military Activity
/// Triggers on gps_interference + flight_position + notam_event within 30 min.
/// GPS jamming combined with military aircraft and NOTAMs indicates electronic warfare.
pub struct GpsMilitaryRule;

impl CorrelationRule for GpsMilitaryRule {
    fn id(&self) -> &str {
        "gps_military"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::GpsInterference, EventType::FlightPosition, EventType::NotamEvent]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        let within = Duration::from_secs(1800); // 30 min

        // Try geo-based correlation first, fall back to region
        if let (Some(lat), Some(lon)) = (trigger.latitude, trigger.longitude) {
            return self.evaluate_geo(trigger, window, active, lat, lon, within);
        }

        // Fall back to region-based
        let region = trigger.region_code.as_deref()?;

        if active.iter().any(|i| {
            i.rule_id == "gps_military" && i.region_code.as_deref() == Some(region)
        }) {
            return None;
        }

        let mut gps = window.by_type_and_region(EventType::GpsInterference, region, within);
        let mut flights = window.by_type_and_region(EventType::FlightPosition, region, within);
        let mut notams = window.by_type_and_region(EventType::NotamEvent, region, within);

        if gps.is_empty() || flights.is_empty() || notams.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut gps);
        sort_by_severity(&mut flights);
        sort_by_severity(&mut notams);

        self.build_incident(trigger, &gps, &flights, &notams, Some(region), None, None)
    }
}

impl GpsMilitaryRule {
    fn evaluate_geo(
        &self,
        trigger: &InsertableEvent,
        window: &CorrelationWindow,
        active: &[Incident],
        lat: f64,
        lon: f64,
        within: Duration,
    ) -> Option<Incident> {
        let radius = 150.0; // 150km — GPS jamming has wide effect

        if active.iter().any(|i| {
            if i.rule_id != "gps_military" {
                return false;
            }
            if let (Some(ilat), Some(ilon)) = (i.latitude, i.longitude) {
                (ilat - lat).abs() < 1.5 && (ilon - lon).abs() < 1.5
            } else {
                false
            }
        }) {
            return None;
        }

        let mut gps = window.near(EventType::GpsInterference, lat, lon, radius, within);
        let mut flights = window.near(EventType::FlightPosition, lat, lon, radius, within);

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

        if gps.is_empty() || flights.is_empty() || notams.is_empty() {
            return None;
        }

        // Sort evidence pools by severity descending (highest first)
        sort_by_severity(&mut gps);
        sort_by_severity(&mut flights);
        sort_by_severity(&mut notams);

        self.build_incident(
            trigger,
            &gps,
            &flights,
            &notams,
            trigger.region_code.as_deref(),
            Some(lat),
            Some(lon),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_incident(
        &self,
        trigger: &InsertableEvent,
        gps: &[&InsertableEvent],
        flights: &[&InsertableEvent],
        notams: &[&InsertableEvent],
        region: Option<&str>,
        lat: Option<f64>,
        lon: Option<f64>,
    ) -> Option<Incident> {
        let now = Utc::now();
        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];

        for g in gps.iter().take(2) {
            if g.event_type != trigger.event_type || g.entity_id != trigger.entity_id {
                evidence.push(evidence_from(g, EvidenceRole::Corroboration));
            }
        }

        // Flights use entity_name as the display title (callsign/tail number)
        for f in flights.iter().take(3) {
            evidence.push(EvidenceRef {
                title: f.entity_name.clone(),
                ..evidence_from(f, EvidenceRole::Corroboration)
            });
        }

        for n in notams.iter().take(2) {
            evidence.push(evidence_from(n, EvidenceRole::Context));
        }

        let location = region.unwrap_or("unknown");

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "gps_military".into(),
            title: format!("GPS jamming with military activity near {location}"),
            description: format!(
                "GPS interference detected alongside military aircraft and NOTAMs. \
                 {} jamming events, {} flights, {} NOTAMs in 30 min window. \
                 Indicates electronic warfare activity.",
                gps.len(),
                flights.len(),
                notams.len()
            ),
            severity: Severity::High,
            confidence: 0.7,
            first_seen: now,
            last_updated: now,
            region_code: region.map(|r| r.to_string()),
            latitude: lat.or(trigger.latitude),
            longitude: lon.or(trigger.longitude),
            tags: vec![
                "gps".into(),
                "electronic-warfare".into(),
                "military".into(),
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
