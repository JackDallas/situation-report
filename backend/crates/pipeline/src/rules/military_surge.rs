use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sr_sources::InsertableEvent;
use sr_types::{EventType, EvidenceRole, Severity};
use uuid::Uuid;

use crate::rules::{sort_by_severity, evidence_from, CorrelationRule};
use crate::types::Incident;
use crate::window::CorrelationWindow;

/// Military aircraft categories of interest (from modes.csv category field).
/// Used for documentation; category breakdown is built dynamically from tags.
#[allow(dead_code)]
const MILITARY_CATEGORIES: &[&str] = &[
    "fighter",
    "tanker",
    "reconnaissance",
    "electronic_warfare",
    "UAV",
    "transport_military",
    "bomber",
    "helicopter_military",
    "trainer",
];

/// Grid cell size in degrees (~110km at equator for 1.0).
const GRID_CELL_DEG: f64 = 2.0;

/// Sliding window for counting current military aircraft (6 hours).
const WINDOW_SECS: u64 = 6 * 3600;

/// Baseline rolling window (7 days of hourly snapshots).
const BASELINE_HOURS: usize = 7 * 24;

/// Standard deviation multiplier for alert threshold.
const SIGMA_MULTIPLIER: f64 = 2.0;

/// Minimum distinct aircraft before we even check the baseline.
/// Prevents alerts in regions that rarely see military traffic.
const MIN_ABSOLUTE_COUNT: usize = 5;

/// Cooldown: suppress duplicate alerts for the same grid cell.
const COOLDOWN_SECS: u64 = 3600; // 1 hour

/// Quantise a lat/lon to a grid cell key.
fn grid_key(lat: f64, lon: f64) -> (i32, i32) {
    let lat_cell = (lat / GRID_CELL_DEG).floor() as i32;
    let lon_cell = (lon / GRID_CELL_DEG).floor() as i32;
    (lat_cell, lon_cell)
}

/// Centroid of a grid cell.
fn grid_centroid(cell: (i32, i32)) -> (f64, f64) {
    let lat = (cell.0 as f64 + 0.5) * GRID_CELL_DEG;
    let lon = (cell.1 as f64 + 0.5) * GRID_CELL_DEG;
    (lat, lon)
}

/// Per-cell rolling baseline: hourly snapshots of distinct military aircraft counts.
#[derive(Debug, Default)]
struct CellBaseline {
    /// Ring buffer of hourly counts (length up to BASELINE_HOURS).
    hourly_counts: Vec<u32>,
    /// Timestamp of the last snapshot.
    last_snapshot: Option<DateTime<Utc>>,
}

impl CellBaseline {
    /// Push a new hourly observation, trimming to BASELINE_HOURS.
    fn push(&mut self, count: u32, now: DateTime<Utc>) {
        self.hourly_counts.push(count);
        if self.hourly_counts.len() > BASELINE_HOURS {
            self.hourly_counts.remove(0);
        }
        self.last_snapshot = Some(now);
    }

    /// Mean and standard deviation of the baseline.
    fn stats(&self) -> Option<(f64, f64)> {
        if self.hourly_counts.len() < 12 {
            // Need at least 12 hours of data before alerting
            return None;
        }
        let n = self.hourly_counts.len() as f64;
        let mean = self.hourly_counts.iter().map(|&c| c as f64).sum::<f64>() / n;
        let variance = self.hourly_counts.iter()
            .map(|&c| {
                let d = c as f64 - mean;
                d * d
            })
            .sum::<f64>() / n;
        let stddev = variance.sqrt();
        Some((mean, stddev))
    }
}

/// Tracks a distinct military aircraft sighting within the window.
#[derive(Debug, Clone)]
struct Sighting {
    hex: String,
    category: String,
    cell: (i32, i32),
    seen_at: DateTime<Utc>,
}

/// Internal state for the military surge detector.
#[derive(Debug, Default)]
struct SurgeState {
    /// Recent sightings within the sliding window.
    sightings: Vec<Sighting>,
    /// Per-cell rolling baseline.
    baselines: HashMap<(i32, i32), CellBaseline>,
    /// Cooldown: cell -> last alert time.
    last_alert: HashMap<(i32, i32), DateTime<Utc>>,
}

/// Rule: Military Aircraft Surge Detection
///
/// Detects unusual concentrations of military aircraft in a geographic region
/// by comparing the current 6h count of distinct ICAO hex codes against a
/// rolling 7-day baseline (mean + 2 sigma).
///
/// Triggered by FlightPosition events tagged as military.
pub struct MilitarySurgeRule {
    state: Mutex<SurgeState>,
}

impl MilitarySurgeRule {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SurgeState::default()),
        }
    }
}

/// Extract the aircraft category from tags (e.g. "aircraft_category:fighter" -> "fighter").
fn extract_category(event: &InsertableEvent) -> String {
    for tag in &event.tags {
        if let Some(cat) = tag.strip_prefix("aircraft_category:") {
            return cat.to_string();
        }
    }
    // Fallback: check payload
    event.payload
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Check if an event represents a military aircraft of a relevant category.
fn is_relevant_military(event: &InsertableEvent) -> bool {
    if !event.tags.iter().any(|t| t == "military") {
        return false;
    }
    let cat = extract_category(event);
    // Accept all military aircraft, but category breakdown is used in the alert
    // MILITARY_CATEGORIES is for reporting, not filtering
    let _ = cat;
    true
}

impl CorrelationRule for MilitarySurgeRule {
    fn id(&self) -> &str {
        "military_surge"
    }

    fn trigger_types(&self) -> &[EventType] {
        &[EventType::FlightPosition]
    }

    fn evaluate(
        &self,
        trigger: &InsertableEvent,
        _window: &CorrelationWindow,
        active: &[Incident],
    ) -> Option<Incident> {
        // Only process military flight positions
        if trigger.event_type != EventType::FlightPosition {
            return None;
        }
        if !is_relevant_military(trigger) {
            return None;
        }

        let (lat, lon) = (trigger.latitude?, trigger.longitude?);
        let hex = trigger.entity_id.as_deref()?;
        let cell = grid_key(lat, lon);
        let now = Utc::now();
        let category = extract_category(trigger);

        let mut state = self.state.lock().ok()?;

        // Record the sighting
        state.sightings.push(Sighting {
            hex: hex.to_string(),
            category: category.clone(),
            cell,
            seen_at: now,
        });

        // Prune old sightings outside the 6h window
        let cutoff = now - chrono::Duration::seconds(WINDOW_SECS as i64);
        state.sightings.retain(|s| s.seen_at >= cutoff);

        // Count distinct hex codes in this cell within the window.
        // Collect as owned data so we release the borrow on state.sightings.
        let mut distinct_hex: HashMap<String, String> = HashMap::new();
        for s in state.sightings.iter().filter(|s| s.cell == cell) {
            distinct_hex.entry(s.hex.clone()).or_insert_with(|| s.category.clone());
        }
        let current_count = distinct_hex.len();

        // Build category breakdown (owned, no borrows into state)
        let mut by_category: HashMap<String, usize> = HashMap::new();
        for cat in distinct_hex.values() {
            *by_category.entry(cat.clone()).or_insert(0) += 1;
        }
        let category_summary: Vec<String> = by_category
            .iter()
            .map(|(cat, count)| format!("{}: {}", cat, count))
            .collect();

        // Update hourly baseline snapshot (at most once per hour)
        let baseline = state.baselines.entry(cell).or_default();
        let should_snapshot = baseline.last_snapshot
            .map(|t| (now - t).num_seconds() >= 3600)
            .unwrap_or(true);
        if should_snapshot {
            baseline.push(current_count as u32, now);
        }

        // Check absolute minimum
        if current_count < MIN_ABSOLUTE_COUNT {
            return None;
        }

        // Check baseline threshold
        let (mean, stddev) = baseline.stats()?;
        let threshold = mean + SIGMA_MULTIPLIER * stddev;

        // Need to actually exceed baseline (and threshold must be meaningful)
        if (current_count as f64) <= threshold {
            return None;
        }

        // Cooldown check
        if let Some(&last) = state.last_alert.get(&cell) {
            if (now - last).num_seconds() < COOLDOWN_SECS as i64 {
                return None;
            }
        }

        // Check for existing active incident in this cell
        let (clat, clon) = grid_centroid(cell);
        if active.iter().any(|i| {
            i.rule_id == "military_surge"
                && i.tags.iter().any(|t| t == &format!("cell:{}:{}", cell.0, cell.1))
        }) {
            return None;
        }

        // Record cooldown
        state.last_alert.insert(cell, now);

        // Build evidence from recent military flights in the window near this cell
        let cell_radius_km = GRID_CELL_DEG * 111.0; // rough conversion
        let window_dur = Duration::from_secs(WINDOW_SECS);
        let mut flights = _window.near(
            EventType::FlightPosition,
            clat,
            clon,
            cell_radius_km,
            window_dur,
        );
        // Filter to military only
        flights.retain(|f| f.tags.iter().any(|t| t == "military"));
        sort_by_severity(&mut flights);

        let mut evidence = vec![evidence_from(trigger, EvidenceRole::Trigger)];
        for f in flights.iter().take(5) {
            if f.entity_id.as_deref() != Some(hex) {
                evidence.push(evidence_from(f, EvidenceRole::Corroboration));
            }
        }

        let sigma_above = if stddev > 0.0 {
            ((current_count as f64) - mean) / stddev
        } else {
            0.0
        };

        // Severity scales with how far above baseline
        let severity = if sigma_above >= 4.0 {
            Severity::Critical
        } else if sigma_above >= 3.0 {
            Severity::High
        } else {
            Severity::Medium
        };

        let confidence = (0.6 + (sigma_above - 2.0).min(2.0) * 0.1).min(0.95) as f32;

        Some(Incident {
            id: Uuid::new_v4(),
            rule_id: "military_surge".into(),
            title: format!(
                "Military aircraft surge near {:.1}, {:.1}",
                clat, clon
            ),
            description: format!(
                "{} distinct military aircraft detected in 6h window (baseline: {:.1} +/- {:.1}, {:.1}\u{03c3} above). \
                 Categories: {}.",
                current_count,
                mean,
                stddev,
                sigma_above,
                category_summary.join(", "),
            ),
            severity,
            confidence,
            first_seen: now,
            last_updated: now,
            region_code: trigger.region_code.clone(),
            latitude: Some(clat),
            longitude: Some(clon),
            tags: vec![
                "military".into(),
                "surge".into(),
                "aviation".into(),
                format!("cell:{}:{}", cell.0, cell.1),
                format!("count:{}", current_count),
            ],
            evidence,
            parent_id: None,
            related_ids: Vec::new(),
            merged_from: Vec::new(),
            display_title: None,
        })
    }
}
