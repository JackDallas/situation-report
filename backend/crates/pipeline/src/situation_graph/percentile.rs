//! Rolling percentile tracker for adaptive severity thresholds.
//!
//! Records per-cluster event_count snapshots each sweep tick and computes
//! percentile-based thresholds. During cold start (fewer than `cold_start`
//! observations), returns `None` so callers fall back to static floors.

use std::collections::VecDeque;

/// A single sweep-tick observation: the event counts of all active clusters.
#[derive(Debug, Clone)]
struct Snapshot {
    /// Sorted event counts from all active clusters at this sweep tick.
    event_counts: Vec<usize>,
}

/// Computes rolling percentile thresholds from recent cluster snapshots.
#[derive(Debug, Clone)]
pub struct PercentileTracker {
    /// Ring buffer of recent snapshots.
    snapshots: VecDeque<Snapshot>,
    /// Maximum number of snapshots to retain.
    window: usize,
}

/// Dynamic thresholds computed from rolling percentiles.
#[derive(Debug, Clone, Copy)]
pub struct DynamicThresholds {
    pub medium_events: usize,
    pub high_events: usize,
    pub critical_events: usize,
}

impl PercentileTracker {
    pub fn new(window: usize) -> Self {
        Self {
            snapshots: VecDeque::with_capacity(window.min(4096)),
            window,
        }
    }

    /// Record a snapshot of event counts from all active clusters.
    /// The input does NOT need to be sorted — we sort internally.
    pub fn record(&mut self, mut event_counts: Vec<usize>) {
        event_counts.sort_unstable();
        if self.snapshots.len() >= self.window {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(Snapshot { event_counts });
    }

    /// Number of snapshots currently stored.
    pub fn observation_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Compute dynamic thresholds from the rolling window.
    /// Returns `None` if fewer than `cold_start` observations exist.
    pub fn thresholds(
        &self,
        cold_start: usize,
        p_medium: f64,
        p_high: f64,
        p_critical: f64,
    ) -> Option<DynamicThresholds> {
        if self.snapshots.len() < cold_start {
            return None;
        }

        // Merge all event counts from recent snapshots into one sorted list.
        // This gives us the distribution of cluster sizes over the rolling window.
        let total_count: usize = self.snapshots.iter().map(|s| s.event_counts.len()).sum();
        if total_count == 0 {
            return None;
        }

        let mut merged: Vec<usize> = Vec::with_capacity(total_count);
        for snap in &self.snapshots {
            merged.extend_from_slice(&snap.event_counts);
        }
        merged.sort_unstable();

        Some(DynamicThresholds {
            medium_events: percentile_value(&merged, p_medium),
            high_events: percentile_value(&merged, p_high),
            critical_events: percentile_value(&merged, p_critical),
        })
    }
}

/// Compute the value at a given percentile (0.0–1.0) from a sorted slice.
/// Uses nearest-rank method.
fn percentile_value(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let p = p.clamp(0.0, 1.0);
    let idx = ((sorted.len() as f64 * p).ceil() as usize).saturating_sub(1);
    let idx = idx.min(sorted.len() - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_start_returns_none() {
        let tracker = PercentileTracker::new(100);
        assert!(tracker.thresholds(50, 0.75, 0.90, 0.99).is_none());
    }

    #[test]
    fn basic_percentiles() {
        let mut tracker = PercentileTracker::new(100);
        // Record 60 identical snapshots (over cold_start=50)
        for _ in 0..60 {
            // 100 clusters with event counts 1..=100
            tracker.record((1..=100).collect());
        }
        let t = tracker.thresholds(50, 0.75, 0.90, 0.99).unwrap();
        assert_eq!(t.medium_events, 75);
        assert_eq!(t.high_events, 90);
        assert_eq!(t.critical_events, 99);
    }

    #[test]
    fn window_eviction() {
        let mut tracker = PercentileTracker::new(5);
        for i in 0..10 {
            tracker.record(vec![i * 10]);
        }
        assert_eq!(tracker.observation_count(), 5);
        // Only last 5 snapshots: [50, 60, 70, 80, 90]
        let t = tracker.thresholds(1, 0.5, 0.9, 1.0).unwrap();
        assert!(t.medium_events >= 50);
    }

    #[test]
    fn percentile_edge_cases() {
        assert_eq!(percentile_value(&[10, 20, 30], 0.0), 10);
        assert_eq!(percentile_value(&[10, 20, 30], 1.0), 30);
        assert_eq!(percentile_value(&[], 0.5), 0);
    }
}
