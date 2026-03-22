use std::time::Duration;

use rand::Rng;

/// Compute exponential backoff delay with jitter.
///
/// Used by Claude, Gemini, and other API clients to avoid retry storms.
/// Formula: `base_ms * 2^attempt + random_jitter(0..base_jitter_ms)`.
pub fn backoff_delay(attempt: u32, base_ms: u64, base_jitter_ms: u64) -> Duration {
    let backoff = base_ms * 2u64.pow(attempt.min(8));
    let jitter = rand::rng().random_range(0..base_jitter_ms.max(1));
    Duration::from_millis(backoff + jitter)
}
