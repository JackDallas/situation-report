use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rand::Rng;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use sr_types::EventType;

use crate::rate_limit::{extract_rate_limit_delay, is_auth_error};
use crate::{DataSource, InsertableEvent, SourceContext};

/// Emitted whenever a source's health status changes.
#[derive(Debug, Clone)]
pub struct SourceHealthEvent {
    pub source_id: String,
    pub status: String,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

/// Registry of all data sources. Manages their lifecycle.
pub struct SourceRegistry {
    sources: Vec<Arc<dyn DataSource>>,
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn register(&mut self, source: Arc<dyn DataSource>) {
        info!(source_id = source.id(), name = source.name(), "Registered data source");
        self.sources.push(source);
    }

    pub fn sources(&self) -> &[Arc<dyn DataSource>] {
        &self.sources
    }

    /// Read the effective poll interval from DB, falling back to code default.
    async fn effective_interval(pool: &PgPool, source_id: &str, code_default: Duration) -> Duration {
        if let Ok(Some(config)) = sr_db::queries::get_source_config(pool, source_id).await {
            if let Some(secs) = config.poll_interval_secs {
                if secs > 0 {
                    return Duration::from_secs(secs as u64);
                }
            }
        }
        code_default
    }

    /// Calculate initial delay so polls resume from their last schedule, not app start.
    /// If a source was last polled 3 minutes ago with a 15-minute interval,
    /// it should wait 12 more minutes before polling — not poll immediately.
    async fn initial_delay(pool: &PgPool, source_id: &str, code_default: Duration) -> Duration {
        let interval = Self::effective_interval(pool, source_id, code_default).await;

        // Look up when this source last polled successfully
        if let Ok(Some(health)) = sr_db::queries::get_source_health(pool, source_id).await {
            if let Some(last_success) = health.last_success {
                let elapsed = (Utc::now() - last_success)
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                if elapsed < interval {
                    let remaining = interval - elapsed;
                    debug!(
                        source_id,
                        last_poll_secs_ago = elapsed.as_secs(),
                        next_poll_in_secs = remaining.as_secs(),
                        "Scheduling from last poll time"
                    );
                    return remaining;
                }
                // Overdue — poll immediately
                return Duration::ZERO;
            }
        }

        // No health record — first run, stagger 0-5s to avoid cold-start thundering herd
        Duration::from_millis(rand::thread_rng().gen_range(0..5000))
    }

    /// Spawn tokio tasks for all registered sources.
    pub fn start_all(
        &self,
        pool: PgPool,
        event_tx: broadcast::Sender<InsertableEvent>,
        health_tx: broadcast::Sender<SourceHealthEvent>,
    ) {
        let http = reqwest::Client::builder()
            .user_agent("SituationReport/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        for source in &self.sources {
            let source = Arc::clone(source);
            let pool = pool.clone();
            let event_tx = event_tx.clone();
            let health_tx = health_tx.clone();
            let http = http.clone();

            if source.is_streaming() {
                // Task 1: run the streaming source, which emits events on event_tx
                let source_id = source.id().to_owned();
                {
                    let pool = pool.clone();
                    let event_tx = event_tx.clone();
                    let health_tx = health_tx.clone();
                    let http = http.clone();
                    let source = Arc::clone(&source);
                    tokio::spawn(async move {
                        let ctx = SourceContext {
                            pool: pool.clone(),
                            http,
                            config: serde_json::Value::Object(Default::default()),
                        };
                        let mut consecutive_failures: u32 = 0;
                        loop {
                            info!(source_id = source.id(), "Starting stream");

                            // Mark as "connecting" — only promoted to "healthy" when data flows
                            update_and_emit_health(
                                &pool, source.id(), "connecting", 0, None, &health_tx,
                            ).await;

                            match source.start_stream(&ctx, event_tx.clone()).await {
                                Ok(()) => {
                                    // Stream ended gracefully
                                    consecutive_failures = 0;
                                    warn!(source_id = source.id(), "Stream ended, reconnecting in 10s");
                                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                                }
                                Err(e) => {
                                    if is_auth_error(&e) {
                                        warn!(
                                            source_id = source.id(),
                                            error = %e,
                                            "Stream auth error — source disabled until restart"
                                        );
                                        let err_msg = format!("auth error (disabled): {}", e);
                                        update_and_emit_health(
                                            &pool, source.id(), "error", 1, Some(&err_msg), &health_tx,
                                        ).await;
                                        // Park this task forever — don't spam the server
                                        return;
                                    }

                                    consecutive_failures = consecutive_failures.saturating_add(1);

                                    if consecutive_failures >= 10 {
                                        warn!(
                                            source_id = source.id(),
                                            consecutive_failures,
                                            "Stream exceeded max failures — giving up until restart"
                                        );
                                        let err_msg = format!("exceeded max failures ({}): {}", consecutive_failures, e);
                                        update_and_emit_health(
                                            &pool, source.id(), "error", consecutive_failures, Some(&err_msg), &health_tx,
                                        ).await;
                                        return;
                                    }

                                    let backoff_ms = ((10u64 * 2u64.pow(consecutive_failures.min(8))).min(1800)) * 1000;
                                    let jitter = rand::thread_rng().gen_range(0..=backoff_ms / 4);
                                    let backoff = std::time::Duration::from_millis(backoff_ms + jitter);
                                    error!(
                                        source_id = source.id(),
                                        error = %e,
                                        consecutive_failures,
                                        backoff_ms = backoff.as_millis() as u64,
                                        "Stream failed, reconnecting after backoff"
                                    );

                                    update_and_emit_health(
                                        &pool, source.id(), "error", consecutive_failures,
                                        Some(&e.to_string()), &health_tx,
                                    ).await;

                                    tokio::time::sleep(backoff).await;
                                }
                            }
                        }
                    });
                }

                // Task 2: subscribe to the broadcast channel and persist matching events to DB
                {
                    let pool = pool.clone();
                    let health_tx = health_tx.clone();
                    let mut event_rx = event_tx.subscribe();
                    tokio::spawn(async move {
                        let mut first_event_received = false;
                        loop {
                            match event_rx.recv().await {
                                Ok(event) => {
                                    if event.source_type.as_str() != source_id {
                                        continue;
                                    }

                                    // Promote from "connecting" to "healthy" on first actual data
                                    if !first_event_received {
                                        first_event_received = true;
                                        info!(source_id = %source_id, "Stream data flowing — marking healthy");
                                        update_and_emit_health(
                                            &pool, &source_id, "healthy", 0, None, &health_tx,
                                        ).await;
                                    }

                                    if let Err(e) = persist_event(&pool, &event).await {
                                        error!(
                                            source_id = %source_id,
                                            error = %e,
                                            "Failed to persist streamed event"
                                        );
                                    }
                                    if matches!(
                                        event.event_type,
                                        EventType::FlightPosition | EventType::VesselPosition
                                    )
                                        && let Err(e) = upsert_position_if_needed(&pool, &event).await {
                                            warn!(
                                                source_id = %source_id,
                                                error = %e,
                                                "Failed to upsert position for streamed event"
                                            );
                                        }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    warn!(
                                        source_id = %source_id,
                                        skipped = n,
                                        "Stream persistence task lagged, skipping messages"
                                    );
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    info!(source_id = %source_id, "Broadcast channel closed, stopping persistence task");
                                    break;
                                }
                            }
                        }
                    });
                }
            } else {
                tokio::spawn(async move {
                    let ctx = SourceContext {
                        pool: pool.clone(),
                        http,
                        config: serde_json::Value::Object(Default::default()),
                    };
                    let code_default = source.default_interval();
                    let mut consecutive_failures: u32 = 0;

                    // Calculate initial delay from DB: resume schedule from last poll
                    let initial_delay = Self::initial_delay(&pool, source.id(), code_default).await;
                    if !initial_delay.is_zero() {
                        info!(
                            source_id = source.id(),
                            delay_secs = initial_delay.as_secs(),
                            "Resuming poll schedule"
                        );
                        tokio::time::sleep(initial_delay).await;
                    }

                    loop {
                        // Read poll interval from DB config (falls back to code default)
                        let poll_interval = Self::effective_interval(&pool, source.id(), code_default).await;

                        // Source toggle: skip this poll iteration if source is disabled
                        if let Ok(Some(config)) = sr_db::queries::get_source_config(&pool, source.id()).await
                            && !config.enabled {
                                tokio::time::sleep(poll_interval).await;
                                continue;
                            }

                        match source.poll(&ctx).await {
                            Ok(events) => {
                                consecutive_failures = 0;

                                // Update health to healthy
                                update_and_emit_health(
                                    &pool, source.id(), "healthy", 0, None, &health_tx,
                                ).await;

                                let count = events.len();
                                for event in events {
                                    // Store in DB
                                    if let Err(e) = persist_event(&pool, &event).await {
                                        error!(error = %e, "Failed to store event");
                                    }
                                    // Upsert position for tracking events
                                    if matches!(event.event_type, EventType::FlightPosition | EventType::VesselPosition)
                                        && let Err(e) = upsert_position_if_needed(&pool, &event).await {
                                            warn!(error = %e, "Failed to upsert position");
                                        }
                                    // Broadcast to SSE subscribers
                                    let _ = event_tx.send(event);
                                }
                                if count > 0 {
                                    info!(source_id = source.id(), count, "Polled events");
                                }
                            }
                            Err(e) => {
                                // Auth errors (401/403) — disable source permanently until restart
                                if is_auth_error(&e) {
                                    warn!(
                                        source_id = source.id(),
                                        error = %e,
                                        "Poll auth error — source disabled until restart"
                                    );
                                    let err_msg = format!("auth error (disabled): {}", e);
                                    update_and_emit_health(
                                        &pool, source.id(), "error", 1, Some(&err_msg), &health_tx,
                                    ).await;
                                    return;
                                }

                                // Check if this is a rate-limit (429) error with a server-specified retry delay
                                let backoff = if let Some(retry_delay) = extract_rate_limit_delay(&e) {
                                    consecutive_failures = consecutive_failures.saturating_add(1);

                                    // Additive backoff: server's retry-after + 30s per consecutive failure.
                                    // Avoids exponential death spiral (10s * 2^4 = 160s) while still
                                    // increasing backoff on repeated failures.
                                    let effective_ms = ((retry_delay.as_secs() + 30 * (consecutive_failures as u64)).min(600)) * 1000;
                                    let jitter = rand::thread_rng().gen_range(0..=effective_ms / 4);
                                    let effective = std::time::Duration::from_millis(effective_ms + jitter);

                                    warn!(
                                        source_id = source.id(),
                                        retry_after_secs = retry_delay.as_secs(),
                                        effective_backoff_secs = effective.as_secs(),
                                        consecutive_429s = consecutive_failures,
                                        "Poll rate-limited, backing off"
                                    );

                                    // Update health to rate_limited
                                    let err_msg = format!("429 x{} — backing off {}s", consecutive_failures, effective.as_secs());
                                    update_and_emit_health(
                                        &pool, source.id(), "rate_limited", consecutive_failures,
                                        Some(&err_msg), &health_tx,
                                    ).await;

                                    effective
                                } else {
                                    error!(source_id = source.id(), error = %e, "Poll failed");

                                    // Exponential backoff: 30s * 2^min(failures, 4), capped at 300s
                                    consecutive_failures = consecutive_failures.saturating_add(1);

                                    // Update health to error
                                    update_and_emit_health(
                                        &pool, source.id(), "error", consecutive_failures,
                                        Some(&e.to_string()), &health_tx,
                                    ).await;
                                    let backoff_ms = ((30u64 * 2u64.pow(consecutive_failures.min(4))).min(300)) * 1000;
                                    let jitter = rand::thread_rng().gen_range(0..=backoff_ms / 4);
                                    std::time::Duration::from_millis(backoff_ms + jitter)
                                };

                                if consecutive_failures >= 8 {
                                    warn!(
                                        source_id = source.id(),
                                        consecutive_failures,
                                        "Poll source exceeded max failures — giving up until restart"
                                    );
                                    let err_msg = format!("exceeded max failures ({})", consecutive_failures);
                                    update_and_emit_health(
                                        &pool, source.id(), "error", consecutive_failures,
                                        Some(&err_msg), &health_tx,
                                    ).await;
                                    return;
                                }

                                tokio::time::sleep(backoff).await;
                                // Skip normal poll_interval — backoff already waited long enough.
                                // Without this, a 429 causes backoff + full poll_interval (double wait).
                                continue;
                            }
                        }

                        // Sleep until next poll
                        tokio::time::sleep(poll_interval).await;
                    }
                });
            }
        }
    }
}

/// Update source health in DB and emit a health event on the broadcast channel.
async fn update_and_emit_health(
    pool: &PgPool,
    source_id: &str,
    status: &str,
    consecutive_failures: u32,
    last_error: Option<&str>,
    health_tx: &broadcast::Sender<SourceHealthEvent>,
) {
    if let Err(e) = sr_db::queries::update_source_health(pool, source_id, status, last_error).await {
        warn!(source_id, error = %e, "Failed to update source health");
    }
    let _ = health_tx.send(SourceHealthEvent {
        source_id: source_id.to_owned(),
        status: status.to_owned(),
        consecutive_failures,
        last_error: last_error.map(|s| s.to_owned()),
    });
}

/// Persist an InsertableEvent to the database.
pub async fn persist_event(pool: &PgPool, event: &InsertableEvent) -> anyhow::Result<()> {
    let source_id = event.source_id.as_deref().map(sanitize_string);
    let entity_id = event.entity_id.as_deref().map(sanitize_string);
    let entity_name = event.entity_name.as_deref().map(sanitize_string);
    let title = event.title.as_deref().map(sanitize_string);
    let description = event.description.as_deref().map(sanitize_string);
    let payload = if contains_null_bytes(&event.payload) {
        sanitize_json(&event.payload)
    } else {
        event.payload.clone()
    };
    let tags: Vec<String> = event.tags.iter().map(|t| sanitize_string(t)).collect();
    let tags_ref = if tags.is_empty() { None } else { Some(tags.as_slice()) };
    sr_db::queries::insert_event(
        pool,
        event.event_time,
        event.source_type.as_str(),
        source_id.as_deref(),
        event.longitude,
        event.latitude,
        event.region_code.as_deref(),
        entity_id.as_deref(),
        entity_name.as_deref(),
        Some(event.event_type.as_str()),
        Some(event.severity.as_str()),
        event.confidence,
        tags_ref,
        title.as_deref(),
        description.as_deref(),
        &payload,
    ).await?;
    Ok(())
}

/// Upsert the latest position for tracking entities (flight/vessel).
pub async fn upsert_position_if_needed(pool: &PgPool, event: &InsertableEvent) -> anyhow::Result<()> {
    if let (Some(lat), Some(lon), Some(entity_id)) = (event.latitude, event.longitude, &event.entity_id) {
        let entity_id = sanitize_string(entity_id);
        let entity_name = event.entity_name.as_deref().map(sanitize_string);
        let payload = if contains_null_bytes(&event.payload) {
            sanitize_json(&event.payload)
        } else {
            event.payload.clone()
        };
        sr_db::queries::upsert_latest_position(
            pool,
            &entity_id,
            event.source_type.as_str(),
            entity_name.as_deref(),
            lon,
            lat,
            event.heading,
            event.speed,
            event.altitude,
            event.event_time,
            &payload,
        ).await?;
    }
    Ok(())
}

/// Strip null bytes (`\0`) from a string. PostgreSQL TEXT/JSONB columns reject this character.
fn sanitize_string(s: &str) -> String {
    s.replace('\0', "")
}

/// Recursively strip null bytes from all string values in a [`serde_json::Value`].
fn sanitize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(sanitize_string(s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sanitize_json).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (sanitize_string(k), sanitize_json(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Fast check whether a JSON value contains any null bytes, to avoid cloning when unnecessary.
fn contains_null_bytes(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains('\0'),
        serde_json::Value::Array(arr) => arr.iter().any(contains_null_bytes),
        serde_json::Value::Object(map) => {
            map.iter()
                .any(|(k, v)| k.contains('\0') || contains_null_bytes(v))
        }
        _ => false,
    }
}
