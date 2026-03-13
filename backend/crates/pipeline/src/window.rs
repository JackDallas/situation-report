use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use chrono::{DateTime, Utc};
use sr_sources::InsertableEvent;
use sr_types::EventType;

pub struct TimestampedEvent {
    pub event: InsertableEvent,
    pub ingested: DateTime<Utc>,
}

/// In-memory sliding window with multi-index lookups for correlation.
pub struct CorrelationWindow {
    max_age: Duration,
    events: VecDeque<TimestampedEvent>,
    // Indices map key → list of positions in `events`
    by_region: HashMap<String, Vec<usize>>,
    by_entity: HashMap<String, Vec<usize>>,
    by_event_type: HashMap<EventType, Vec<usize>>,
}

impl CorrelationWindow {
    pub fn new(max_age: Duration) -> Self {
        Self {
            max_age,
            events: VecDeque::new(),
            by_region: HashMap::new(),
            by_entity: HashMap::new(),
            by_event_type: HashMap::new(),
        }
    }

    pub fn push(&mut self, event: InsertableEvent) {
        let idx = self.events.len();
        let now = Utc::now();

        // Index by region
        if let Some(ref region) = event.region_code {
            self.by_region
                .entry(region.clone())
                .or_default()
                .push(idx);
        }

        // Index by entity
        if let Some(ref entity) = event.entity_id {
            self.by_entity
                .entry(entity.clone())
                .or_default()
                .push(idx);
        }

        // Index by event type
        self.by_event_type
            .entry(event.event_type)
            .or_default()
            .push(idx);

        self.events.push_back(TimestampedEvent {
            event,
            ingested: now,
        });
    }

    /// Remove events older than max_age and rebuild indices.
    pub fn prune(&mut self) {
        let cutoff = Utc::now() - self.max_age;
        let before = self.events.len();

        // Remove from front while too old
        while let Some(front) = self.events.front() {
            if front.ingested < cutoff {
                self.events.pop_front();
            } else {
                break;
            }
        }

        // Only rebuild if we actually removed anything
        if self.events.len() != before {
            self.rebuild_indices();
        }
    }

    fn rebuild_indices(&mut self) {
        self.by_region.clear();
        self.by_entity.clear();
        self.by_event_type.clear();

        for (idx, ts_event) in self.events.iter().enumerate() {
            let event = &ts_event.event;
            if let Some(ref region) = event.region_code {
                self.by_region
                    .entry(region.clone())
                    .or_default()
                    .push(idx);
            }
            if let Some(ref entity) = event.entity_id {
                self.by_entity
                    .entry(entity.clone())
                    .or_default()
                    .push(idx);
            }
            self.by_event_type
                .entry(event.event_type)
                .or_default()
                .push(idx);
        }
    }

    /// Get events of a given type within a time window.
    pub fn by_type(&self, event_type: EventType, within: Duration) -> Vec<&InsertableEvent> {
        let cutoff = Utc::now() - within;
        self.by_event_type
            .get(&event_type)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| {
                        let ts = &self.events[idx];
                        if ts.ingested >= cutoff {
                            Some(&ts.event)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get events of a given type in a specific region within a time window.
    pub fn by_type_and_region(
        &self,
        event_type: EventType,
        region: &str,
        within: Duration,
    ) -> Vec<&InsertableEvent> {
        let cutoff = Utc::now() - within;
        self.by_event_type
            .get(&event_type)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| {
                        let ts = &self.events[idx];
                        if ts.ingested >= cutoff {
                            let ev = &ts.event;
                            if ev.region_code.as_deref() == Some(region) {
                                return Some(ev);
                            }
                        }
                        None
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get events of a given type near a lat/lon within radius_km, using bounding box approximation.
    pub fn near(
        &self,
        event_type: EventType,
        lat: f64,
        lon: f64,
        radius_km: f64,
        within: Duration,
    ) -> Vec<&InsertableEvent> {
        let cutoff = Utc::now() - within;
        // ~111km per degree of latitude
        let lat_delta = radius_km / 111.0;
        // Longitude degrees vary by cos(latitude)
        let lon_delta = radius_km / (111.0 * lat.to_radians().cos().abs().max(0.01));

        let lat_min = lat - lat_delta;
        let lat_max = lat + lat_delta;
        let lon_min = lon - lon_delta;
        let lon_max = lon + lon_delta;

        self.by_event_type
            .get(&event_type)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| {
                        let ts = &self.events[idx];
                        if ts.ingested < cutoff {
                            return None;
                        }
                        let ev = &ts.event;
                        if let (Some(elat), Some(elon)) = (ev.latitude, ev.longitude)
                            && elat >= lat_min
                            && elat <= lat_max
                            && elon >= lon_min
                            && elon <= lon_max
                        {
                            return Some(ev);
                        }
                        None
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get events for a specific entity within a time window.
    pub fn by_entity(&self, entity_id: &str, within: Duration) -> Vec<&InsertableEvent> {
        let cutoff = Utc::now() - within;
        self.by_entity
            .get(entity_id)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| {
                        let ts = &self.events[idx];
                        if ts.ingested >= cutoff {
                            Some(&ts.event)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all events of a type near any geo point, collecting unique regions.
    pub fn near_any_of_type(
        &self,
        event_type: EventType,
        lat: f64,
        lon: f64,
        radius_km: f64,
        within: Duration,
    ) -> (Vec<&InsertableEvent>, HashSet<String>) {
        let events = self.near(event_type, lat, lon, radius_km, within);
        let regions: HashSet<String> = events
            .iter()
            .filter_map(|e| e.region_code.clone())
            .collect();
        (events, regions)
    }

    /// Get the N most recent events (from the back of the deque).
    pub fn recent(&self, n: usize) -> Vec<&InsertableEvent> {
        self.events
            .iter()
            .rev()
            .take(n)
            .map(|ts| &ts.event)
            .collect()
    }

    /// Count of events currently in the window.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sr_types::{Severity, SourceType};

    fn make_event(event_type: EventType, region: Option<&str>, lat: Option<f64>, lon: Option<f64>) -> InsertableEvent {
        InsertableEvent {
            event_type,
            source_type: SourceType::Acled,
            latitude: lat,
            longitude: lon,
            region_code: region.map(|s| s.to_string()),
            payload: serde_json::Value::Null,
            ..Default::default()
        }
    }

    #[test]
    fn test_push_and_query_by_type() {
        let mut w = CorrelationWindow::new(Duration::from_secs(3600));
        w.push(make_event(EventType::BgpAnomaly, Some("US"), None, None));
        w.push(make_event(EventType::BgpAnomaly, Some("DE"), None, None));
        w.push(make_event(EventType::ConflictEvent, Some("UA"), None, None));

        let bgp = w.by_type(EventType::BgpAnomaly, Duration::from_secs(60));
        assert_eq!(bgp.len(), 2);

        let conflict = w.by_type(EventType::ConflictEvent, Duration::from_secs(60));
        assert_eq!(conflict.len(), 1);
    }

    #[test]
    fn test_by_type_and_region() {
        let mut w = CorrelationWindow::new(Duration::from_secs(3600));
        w.push(make_event(EventType::BgpAnomaly, Some("US"), None, None));
        w.push(make_event(EventType::BgpAnomaly, Some("DE"), None, None));

        let us = w.by_type_and_region(EventType::BgpAnomaly, "US", Duration::from_secs(60));
        assert_eq!(us.len(), 1);
        assert_eq!(us[0].region_code.as_deref(), Some("US"));
    }

    #[test]
    fn test_near_query() {
        let mut w = CorrelationWindow::new(Duration::from_secs(3600));
        // Kyiv area
        w.push(make_event(EventType::ThermalAnomaly, None, Some(50.45), Some(30.52)));
        // Far away
        w.push(make_event(EventType::ThermalAnomaly, None, Some(35.0), Some(-90.0)));

        let near_kyiv = w.near(EventType::ThermalAnomaly, 50.4, 30.5, 50.0, Duration::from_secs(60));
        assert_eq!(near_kyiv.len(), 1);
    }

    #[test]
    fn test_prune_removes_old() {
        let mut w = CorrelationWindow::new(Duration::from_secs(1));
        w.push(make_event(EventType::ConflictEvent, None, None, None));
        assert_eq!(w.len(), 1);

        // Force the event to appear old by sleeping briefly
        std::thread::sleep(std::time::Duration::from_millis(1100));
        w.prune();
        assert_eq!(w.len(), 0);
    }
}
