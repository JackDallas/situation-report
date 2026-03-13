//! Replay harness for deterministic re-execution of the situation clustering algorithm.
//!
//! Replays a captured event stream through `SituationGraph` with clock injection,
//! producing `SituationClusterDTO` snapshots that can be compared across algorithm
//! versions for A/B testing and regression detection.

pub mod types;
pub mod harness;
pub mod golden;

pub use types::{ReplayEvent, ReplayDataset, ReplayMetadata, ReplaySnapshot, ReplayMetrics};
pub use harness::{ReplayHarness, ReplayConfig, ReplayComparison};
pub use golden::{GoldenFile, ScoreCard, generate_golden};
