use std::time::Duration;

use reqwest::Response;
use tracing::warn;

/// Error returned when an HTTP response is 429 Too Many Requests.
/// Contains the recommended wait duration parsed from response headers.
#[derive(Debug)]
pub struct RateLimited {
    pub retry_after: Duration,
    pub source: String,
}

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rate limited by {} — retry after {}s",
            self.source,
            self.retry_after.as_secs()
        )
    }
}

impl std::error::Error for RateLimited {}

/// Error returned when a source gets HTTP 401/403 or an auth-failure message
/// in a WebSocket protocol (e.g. aisstream.io "Api Key Is Not Valid").
#[derive(Debug)]
pub struct AuthError {
    pub source: String,
    pub message: String,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: auth error: {}", self.source, self.message)
    }
}

impl std::error::Error for AuthError {}

/// Returns true if the error is an AuthError (401/403 or protocol auth failure).
pub fn is_auth_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<AuthError>().is_some()
}

/// Default backoff when no Retry-After header is present (60 seconds).
const DEFAULT_RETRY_SECS: u64 = 60;

/// Parse the `Retry-After` header value.
/// Supports both delta-seconds ("120") and HTTP-date ("Wed, 21 Oct 2015 07:28:00 GMT") formats.
fn parse_retry_after(value: &str) -> Option<Duration> {
    // Try as integer seconds first
    if let Ok(secs) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // Try as HTTP-date (RFC 2822 / RFC 7231)
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(value.trim()) {
        let now = chrono::Utc::now();
        let target = date.with_timezone(&chrono::Utc);
        let delta = (target - now).num_seconds().max(1) as u64;
        return Some(Duration::from_secs(delta));
    }

    // Also try the IMF-fixdate format used in HTTP headers
    if let Ok(date) = chrono::NaiveDateTime::parse_from_str(value.trim(), "%a, %d %b %Y %H:%M:%S GMT") {
        let target = date.and_utc();
        let now = chrono::Utc::now();
        let delta = (target - now).num_seconds().max(1) as u64;
        return Some(Duration::from_secs(delta));
    }

    None
}

/// Parse `X-RateLimit-Reset` header (typically a Unix epoch timestamp).
fn parse_ratelimit_reset(value: &str) -> Option<Duration> {
    if let Ok(epoch) = value.trim().parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        let delta = (epoch - now).max(1) as u64;
        return Some(Duration::from_secs(delta));
    }
    None
}

/// Check an HTTP response for rate limiting (429 status).
///
/// Returns `Ok(response)` if the status is not 429.
/// Returns `Err(RateLimited { .. })` if the status is 429, with the retry duration
/// parsed from headers in this priority:
/// 1. `Retry-After` header
/// 2. `X-RateLimit-Reset` header
/// 3. Default of 60 seconds
///
/// For non-429 errors, calls `error_for_status()` to convert to a reqwest error.
///
/// # Usage
/// Replace `.error_for_status()?` with `check_rate_limit(resp, "source-name")?`.
pub fn check_rate_limit(response: Response, source_name: &str) -> Result<Response, anyhow::Error> {
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after)
            .or_else(|| {
                response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_ratelimit_reset)
            })
            .unwrap_or(Duration::from_secs(DEFAULT_RETRY_SECS));

        // Also log X-RateLimit-Remaining if present for visibility
        if let Some(remaining) = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
        {
            warn!(
                source = source_name,
                remaining,
                "Rate limit headers indicate remaining quota"
            );
        }

        warn!(
            source = source_name,
            retry_after_secs = retry_after.as_secs(),
            "HTTP 429 — rate limited"
        );

        return Err(RateLimited {
            retry_after,
            source: source_name.to_string(),
        }
        .into());
    }

    // Not rate-limited — proceed with normal error handling
    Ok(response.error_for_status()?)
}

/// Convenience: send a request and check for rate limits in one step.
/// For sources that use the `match ctx.http.get(url).send().await { Ok(r) => r, ... }` pattern,
/// this wraps both the send and the rate-limit check.
pub async fn send_with_rate_limit(
    request: reqwest::RequestBuilder,
    source_name: &str,
) -> Result<Response, anyhow::Error> {
    let response = request.send().await?;
    check_rate_limit(response, source_name)
}

/// Check if an error is a RateLimited error and extract the retry duration.
/// Used by the registry polling loop to apply intelligent backoff.
pub fn extract_rate_limit_delay(err: &anyhow::Error) -> Option<Duration> {
    err.downcast_ref::<RateLimited>()
        .map(|rl| rl.retry_after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after(" 60 "), Some(Duration::from_secs(60)));
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn parse_retry_after_invalid() {
        assert!(parse_retry_after("not-a-number").is_none());
        assert!(parse_retry_after("").is_none());
    }

    #[test]
    fn parse_ratelimit_reset_epoch() {
        // Use a future timestamp
        let future = chrono::Utc::now().timestamp() + 120;
        let result = parse_ratelimit_reset(&future.to_string());
        assert!(result.is_some());
        let secs = result.unwrap().as_secs();
        // Should be roughly 120 seconds (allow ±2 for test execution time)
        assert!((118..=122).contains(&secs), "got {secs}");
    }

    #[test]
    fn extract_rate_limit_from_anyhow() {
        let rl = RateLimited {
            retry_after: Duration::from_secs(90),
            source: "test".to_string(),
        };
        let err: anyhow::Error = rl.into();
        let delay = extract_rate_limit_delay(&err);
        assert_eq!(delay, Some(Duration::from_secs(90)));
    }

    #[test]
    fn extract_rate_limit_from_other_error() {
        let err = anyhow::anyhow!("some other error");
        assert!(extract_rate_limit_delay(&err).is_none());
    }
}
