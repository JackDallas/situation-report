//! Golden expectation files for replay regression testing.
//!
//! A golden file defines what the algorithm *should* produce for a given dataset:
//! - Expected situations (title patterns, min events, severity)
//! - Anti-patterns (titles/patterns that should NOT appear)
//! - Quality thresholds (max clusters, min certainty, etc.)
//!
//! After a replay run, the scorer checks every expectation and produces a report
//! card with pass/fail for each assertion.

use serde::{Deserialize, Serialize};

use super::types::ReplayMetrics;
use crate::situation_graph::SituationClusterDTO;

/// A complete golden expectation file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenFile {
    /// Human-readable name for this golden file.
    pub name: String,
    /// Description of what this golden file covers.
    pub description: String,
    /// Expected situations that should be present in replay output.
    #[serde(default)]
    pub expectations: Vec<SituationExpectation>,
    /// Anti-patterns: situations that should NOT appear.
    #[serde(default)]
    pub anti_expectations: Vec<AntiExpectation>,
    /// Global quality thresholds.
    pub thresholds: QualityThresholds,
}

/// An expected situation in the replay output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationExpectation {
    /// Regex pattern to match against situation titles (case-insensitive).
    pub title_pattern: String,
    /// Must have at least this many events.
    #[serde(default)]
    pub min_events: Option<usize>,
    /// Must have at least this many sources.
    #[serde(default)]
    pub min_sources: Option<usize>,
    /// Expected minimum severity (info, low, medium, high, critical).
    #[serde(default)]
    pub min_severity: Option<String>,
    /// Expected minimum certainty (0.0-1.0).
    #[serde(default)]
    pub min_certainty: Option<f32>,
    /// How important this expectation is (affects overall score).
    #[serde(default = "default_weight")]
    pub weight: f64,
    /// Human notes about why this expectation exists.
    #[serde(default)]
    pub notes: String,
}

fn default_weight() -> f64 {
    1.0
}

/// Something that should NOT appear in the output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiExpectation {
    /// Regex pattern that should NOT match any top-level situation title.
    pub title_pattern: String,
    /// Why this is bad.
    pub reason: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

/// Global quality thresholds for the replay output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityThresholds {
    /// Maximum number of top-level situations.
    #[serde(default)]
    pub max_top_level: Option<usize>,
    /// Minimum average certainty across all top-level situations.
    #[serde(default)]
    pub min_avg_certainty: Option<f32>,
    /// Minimum fraction of top-level situations with 2+ source types.
    #[serde(default)]
    pub min_multi_source_ratio: Option<f64>,
    /// Maximum fraction of situations with severity=info (noise).
    #[serde(default)]
    pub max_info_ratio: Option<f64>,
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Result of evaluating one expectation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectationResult {
    pub title_pattern: String,
    pub passed: bool,
    pub weight: f64,
    /// Which situation matched (if any).
    pub matched_title: Option<String>,
    /// Details about pass/fail.
    pub details: Vec<String>,
}

/// Result of evaluating one anti-expectation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiExpectationResult {
    pub title_pattern: String,
    pub passed: bool,
    pub weight: f64,
    /// Titles that violated the anti-expectation.
    pub violations: Vec<String>,
    pub reason: String,
}

/// Result of evaluating quality thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdResult {
    pub name: String,
    pub passed: bool,
    pub actual: f64,
    pub threshold: f64,
}

/// Complete score card from evaluating a golden file against replay output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreCard {
    pub golden_name: String,
    pub expectations: Vec<ExpectationResult>,
    pub anti_expectations: Vec<AntiExpectationResult>,
    pub thresholds: Vec<ThresholdResult>,
    /// Weighted pass rate (0.0-1.0).
    pub score: f64,
    /// Number of expectations that passed.
    pub passed: usize,
    /// Total number of checks.
    pub total: usize,
}

impl ScoreCard {
    /// Evaluate a golden file against replay metrics.
    pub fn evaluate(golden: &GoldenFile, metrics: &ReplayMetrics) -> Self {
        let final_snap = metrics.snapshots.last();
        let clusters: Vec<&SituationClusterDTO> = final_snap
            .map(|s| s.clusters.iter().filter(|c| c.parent_id.is_none()).collect())
            .unwrap_or_default();

        let mut expectation_results = Vec::new();
        let mut anti_results = Vec::new();
        let mut threshold_results = Vec::new();

        // Check positive expectations
        for exp in &golden.expectations {
            let re = regex_lite::Regex::new(&format!("(?i){}", exp.title_pattern));
            let result = match re {
                Ok(re) => evaluate_expectation(exp, &clusters, &re),
                Err(e) => ExpectationResult {
                    title_pattern: exp.title_pattern.clone(),
                    passed: false,
                    weight: exp.weight,
                    matched_title: None,
                    details: vec![format!("Invalid regex: {e}")],
                },
            };
            expectation_results.push(result);
        }

        // Check anti-expectations
        for anti in &golden.anti_expectations {
            let re = regex_lite::Regex::new(&format!("(?i){}", anti.title_pattern));
            let result = match re {
                Ok(re) => {
                    let violations: Vec<String> = clusters
                        .iter()
                        .filter(|c| re.is_match(&c.title))
                        .map(|c| c.title.clone())
                        .collect();
                    AntiExpectationResult {
                        title_pattern: anti.title_pattern.clone(),
                        passed: violations.is_empty(),
                        weight: anti.weight,
                        violations,
                        reason: anti.reason.clone(),
                    }
                }
                Err(e) => AntiExpectationResult {
                    title_pattern: anti.title_pattern.clone(),
                    passed: false,
                    weight: anti.weight,
                    violations: vec![format!("Invalid regex: {e}")],
                    reason: anti.reason.clone(),
                },
            };
            anti_results.push(result);
        }

        // Check quality thresholds
        let top_count = clusters.len();

        if let Some(max) = golden.thresholds.max_top_level {
            threshold_results.push(ThresholdResult {
                name: "max_top_level".to_string(),
                passed: top_count <= max,
                actual: top_count as f64,
                threshold: max as f64,
            });
        }

        if let Some(min_cert) = golden.thresholds.min_avg_certainty {
            let avg = if clusters.is_empty() {
                0.0
            } else {
                clusters.iter().map(|c| c.certainty).sum::<f32>() / clusters.len() as f32
            };
            threshold_results.push(ThresholdResult {
                name: "min_avg_certainty".to_string(),
                passed: avg >= min_cert,
                actual: avg as f64,
                threshold: min_cert as f64,
            });
        }

        if let Some(min_ratio) = golden.thresholds.min_multi_source_ratio {
            let multi = clusters.iter().filter(|c| c.source_count >= 2).count();
            let ratio = if clusters.is_empty() {
                0.0
            } else {
                multi as f64 / clusters.len() as f64
            };
            threshold_results.push(ThresholdResult {
                name: "min_multi_source_ratio".to_string(),
                passed: ratio >= min_ratio,
                actual: ratio,
                threshold: min_ratio,
            });
        }

        if let Some(max_ratio) = golden.thresholds.max_info_ratio {
            let info_count = clusters
                .iter()
                .filter(|c| c.severity == sr_types::Severity::Info)
                .count();
            let ratio = if clusters.is_empty() {
                0.0
            } else {
                info_count as f64 / clusters.len() as f64
            };
            threshold_results.push(ThresholdResult {
                name: "max_info_ratio".to_string(),
                passed: ratio <= max_ratio,
                actual: ratio,
                threshold: max_ratio,
            });
        }

        // Compute weighted score
        let mut total_weight = 0.0;
        let mut passed_weight = 0.0;
        let mut total_checks = 0usize;
        let mut passed_checks = 0usize;

        for r in &expectation_results {
            total_weight += r.weight;
            total_checks += 1;
            if r.passed {
                passed_weight += r.weight;
                passed_checks += 1;
            }
        }
        for r in &anti_results {
            total_weight += r.weight;
            total_checks += 1;
            if r.passed {
                passed_weight += r.weight;
                passed_checks += 1;
            }
        }
        for r in &threshold_results {
            total_weight += 1.0;
            total_checks += 1;
            if r.passed {
                passed_weight += 1.0;
                passed_checks += 1;
            }
        }

        let score = if total_weight > 0.0 {
            passed_weight / total_weight
        } else {
            1.0
        };

        ScoreCard {
            golden_name: golden.name.clone(),
            expectations: expectation_results,
            anti_expectations: anti_results,
            thresholds: threshold_results,
            score,
            passed: passed_checks,
            total: total_checks,
        }
    }
}

fn evaluate_expectation(
    exp: &SituationExpectation,
    clusters: &[&SituationClusterDTO],
    re: &regex_lite::Regex,
) -> ExpectationResult {
    // Find best matching cluster
    let matching: Vec<&SituationClusterDTO> = clusters
        .iter()
        .filter(|c| re.is_match(&c.title))
        .copied()
        .collect();

    if matching.is_empty() {
        return ExpectationResult {
            title_pattern: exp.title_pattern.clone(),
            passed: false,
            weight: exp.weight,
            matched_title: None,
            details: vec!["No situation matched title pattern".to_string()],
        };
    }

    // Use the best match (most events)
    let best = matching.iter().max_by_key(|c| c.event_count).unwrap();
    let mut details = Vec::new();
    let mut passed = true;

    if let Some(min_ev) = exp.min_events {
        if best.event_count < min_ev {
            details.push(format!(
                "events: {} < {} minimum",
                best.event_count, min_ev
            ));
            passed = false;
        }
    }

    if let Some(min_src) = exp.min_sources {
        if best.source_count < min_src {
            details.push(format!(
                "sources: {} < {} minimum",
                best.source_count, min_src
            ));
            passed = false;
        }
    }

    if let Some(ref min_sev) = exp.min_severity {
        let expected_rank = sr_types::Severity::from_str_lossy(min_sev).rank();
        let actual_rank = best.severity.rank();
        if actual_rank < expected_rank {
            details.push(format!(
                "severity: {:?} < {}",
                best.severity, min_sev
            ));
            passed = false;
        }
    }

    if let Some(min_cert) = exp.min_certainty {
        if best.certainty < min_cert {
            details.push(format!(
                "certainty: {:.2} < {:.2}",
                best.certainty, min_cert
            ));
            passed = false;
        }
    }

    if details.is_empty() && passed {
        details.push(format!(
            "matched: {} events, {} sources, {:.0}% certainty",
            best.event_count,
            best.source_count,
            best.certainty * 100.0
        ));
    }

    ExpectationResult {
        title_pattern: exp.title_pattern.clone(),
        passed,
        weight: exp.weight,
        matched_title: Some(best.title.clone()),
        details,
    }
}

/// Auto-generate a golden file from replay metrics.
/// Uses the final snapshot to create expectations for each top-level situation.
/// The generated file is a starting point — human should review and adjust.
pub fn generate_golden(metrics: &ReplayMetrics, name: String, description: String) -> GoldenFile {
    let final_snap = match metrics.snapshots.last() {
        Some(s) => s,
        None => {
            return GoldenFile {
                name,
                description,
                expectations: vec![],
                anti_expectations: vec![],
                thresholds: QualityThresholds {
                    max_top_level: Some(50),
                    min_avg_certainty: Some(0.5),
                    min_multi_source_ratio: Some(0.3),
                    max_info_ratio: Some(0.1),
                },
            };
        }
    };

    let top_level: Vec<&SituationClusterDTO> = final_snap
        .clusters
        .iter()
        .filter(|c| c.parent_id.is_none())
        .collect();

    // Only create expectations for significant clusters (5+ events or 2+ sources).
    // Singletons are noise in replay — we care about the algorithm's ability to
    // correlate, not to create one-off buckets.
    let significant: Vec<&&SituationClusterDTO> = top_level
        .iter()
        .filter(|c| c.event_count >= 5 || c.source_count >= 2)
        .collect();

    // Deduplicate by title pattern — many clusters share the same auto-generated
    // title (e.g., "Fire in SOUTH-ASIA"). Keep the one with the most events.
    let mut best_by_pattern: std::collections::HashMap<String, &SituationClusterDTO> =
        std::collections::HashMap::new();
    for c in &significant {
        let pattern = regex_lite::escape(&c.title);
        best_by_pattern
            .entry(pattern)
            .and_modify(|existing| {
                if c.event_count > existing.event_count {
                    *existing = c;
                }
            })
            .or_insert(c);
    }

    let mut expectations: Vec<SituationExpectation> = best_by_pattern
        .into_iter()
        .map(|(pattern, c)| {
            SituationExpectation {
                title_pattern: pattern,
                // Allow 50% variance, but never require more than actual count
                min_events: Some((c.event_count / 2).max(1)),
                // Don't require min_sources in auto-generated golden — source diversity
                // is unstable in replay (depends on event ordering and merge timing).
                min_sources: None,
                min_severity: Some(c.severity.as_str().to_string()),
                min_certainty: None,
                weight: if c.event_count >= 50 {
                    3.0
                } else if c.event_count >= 20 || c.source_count >= 3 {
                    2.0
                } else if c.source_count >= 2 {
                    1.5
                } else {
                    1.0
                },
                notes: format!(
                    "Auto-generated: {} events, {} sources, {:?} phase",
                    c.event_count, c.source_count, c.phase
                ),
            }
        })
        .collect();
    // Sort by weight descending for readability
    expectations.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

    // Standard anti-patterns for production-quality output.
    // Note: "Word in REGION" titles are expected in raw replay (auto-generated),
    // so we only flag double-dash garbage titles here.
    let anti_expectations = vec![
        AntiExpectation {
            title_pattern: r" — .+ — ".to_string(),
            reason: "Double em-dash titles are garbage".to_string(),
            weight: 2.0,
        },
    ];

    // Thresholds based on current output with slack
    let multi_src_count = top_level.iter().filter(|c| c.source_count >= 2).count();
    let multi_src_ratio = multi_src_count as f64 / top_level.len().max(1) as f64;

    GoldenFile {
        name,
        description,
        expectations,
        anti_expectations,
        thresholds: QualityThresholds {
            max_top_level: Some((top_level.len() as f64 * 1.2).ceil() as usize),
            // No certainty threshold for raw replay (all 0.0 without AI)
            min_avg_certainty: None,
            // Use 60% of actual ratio as floor (replay source diversity is lower than production)
            min_multi_source_ratio: if multi_src_ratio > 0.01 {
                Some(multi_src_ratio * 0.6)
            } else {
                None
            },
            max_info_ratio: None,
        },
    }
}
