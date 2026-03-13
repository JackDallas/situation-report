# Position Pattern Detection System

Design document for detecting behavioral patterns from ADS-B and AIS position
streams in the Situation Report OSINT platform.

**Status:** Design
**Date:** March 2026
**Crate:** `sr-position-patterns` (new crate at `backend/crates/position-patterns/`)

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Position History Storage](#2-position-history-storage)
3. [Pattern Detection Architecture](#3-pattern-detection-architecture)
4. [Pattern Catalog](#4-pattern-catalog)
5. [Active Pattern Lifecycle](#5-active-pattern-lifecycle)
6. [Pipeline Integration](#6-pipeline-integration)
7. [Dynamic Configuration](#7-dynamic-configuration)
8. [Frontend Visualization](#8-frontend-visualization)
9. [Implementation Phases](#9-implementation-phases)

---

## 1. Problem Statement

Currently, the Situation Report stores only the **latest position** per tracked
entity (`latest_positions` table, upsert-on-update). Historical positions exist
only as events in the `events` hypertable, but those are mixed with millions of
other events and not indexed for trail queries. The frontend keeps a 10-point
in-memory trail per entity, which is lost on page refresh.

This means:

- We cannot detect that an RQ-4 Global Hawk has been orbiting the same 20km
  circle off the coast of Cyprus for 45 minutes.
- We cannot detect that a vessel turned off its AIS transponder (dark running)
  and reappeared 200nm away.
- We cannot detect that three military aircraft are flying in formation.
- We cannot correlate a loitering ISR platform with a simultaneous ground
  conflict event.

The position pattern detection system will close these gaps by:

1. Retaining dense position history (2h hot, 24h warm, 7d archive)
2. Running a trait-based pattern detector over each entity's trail
3. Emitting detected patterns as events that feed into the existing pipeline
4. Generating and maintaining "tracking situations" in the SituationGraph

---

## 2. Position History Storage

### 2.1 `position_history` Hypertable

**Migration:** `backend/migrations/008_position_history.sql`

```sql
-- 008_position_history.sql
-- Position history for trail reconstruction and pattern detection

-- =========================================================================
-- 1. position_history table
-- =========================================================================
CREATE TABLE position_history (
    time            TIMESTAMPTZ     NOT NULL,
    entity_id       TEXT            NOT NULL,
    source_type     TEXT            NOT NULL,
    location        GEOGRAPHY(POINT, 4326) NOT NULL,
    heading         REAL,
    speed           REAL,           -- knots for aircraft, SOG for vessels
    altitude        REAL,           -- feet for aircraft, NULL for vessels
    vertical_rate   REAL,           -- ft/min for aircraft
    squawk          TEXT,           -- transponder code (aircraft only)
    nav_status      SMALLINT,       -- AIS navigational status (vessels only)
    on_ground       BOOLEAN         DEFAULT FALSE,
    payload_hash    BIGINT          -- xxhash64 of full payload, for dedup
);

-- Convert to TimescaleDB hypertable with 1-hour chunks (dense data)
SELECT create_hypertable('position_history', 'time',
    chunk_time_interval => INTERVAL '1 hour'
);

-- =========================================================================
-- 2. Indexes
-- =========================================================================

-- Primary query: get trail for a specific entity in a time range
CREATE INDEX idx_poshist_entity_time
    ON position_history (entity_id, time DESC);

-- Spatial query: find all entities near a point in a time window
CREATE INDEX idx_poshist_location
    ON position_history USING GIST (location);

-- Source-type filtering (e.g., only AIS or only ADS-B)
CREATE INDEX idx_poshist_source_time
    ON position_history (source_type, time DESC);

-- =========================================================================
-- 3. Compression and retention
-- =========================================================================

-- Compress after 2 hours (hot data stays uncompressed for fast pattern queries)
ALTER TABLE position_history SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'entity_id, source_type',
    timescaledb.compress_orderby = 'time DESC'
);

SELECT add_compression_policy('position_history',
    INTERVAL '2 hours',
    if_not_exists => true
);

-- Retain 7 days of position history
SELECT add_retention_policy('position_history',
    INTERVAL '7 days',
    if_not_exists => true
);

-- =========================================================================
-- 4. Continuous aggregate: entity activity summary
-- =========================================================================
CREATE MATERIALIZED VIEW entity_activity_5min
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('5 minutes', time)      AS bucket,
    entity_id,
    source_type,
    COUNT(*)                            AS position_count,
    AVG(speed)                          AS avg_speed,
    MAX(speed)                          AS max_speed,
    MIN(altitude)                       AS min_altitude,
    MAX(altitude)                       AS max_altitude
FROM position_history
GROUP BY bucket, entity_id, source_type
WITH NO DATA;

SELECT add_continuous_aggregate_policy('entity_activity_5min',
    start_offset    => INTERVAL '1 day',
    end_offset      => INTERVAL '5 minutes',
    schedule_interval => INTERVAL '5 minutes',
    if_not_exists   => true
);

-- =========================================================================
-- 5. Active patterns tracking table
-- =========================================================================
CREATE TABLE active_patterns (
    id              UUID            PRIMARY KEY DEFAULT gen_random_uuid(),
    pattern_type    TEXT            NOT NULL,
    entity_id       TEXT            NOT NULL,
    -- Multi-entity patterns list all participants
    participant_ids TEXT[]          DEFAULT '{}',
    started_at      TIMESTAMPTZ     NOT NULL,
    last_updated    TIMESTAMPTZ     NOT NULL,
    ended_at        TIMESTAMPTZ,
    -- Center of the pattern (e.g., orbit center, loiter point)
    center_location GEOGRAPHY(POINT, 4326),
    -- Bounding geometry of the pattern
    bounds          GEOGRAPHY,
    -- Pattern-specific parameters (radius, altitude band, etc.)
    parameters      JSONB           NOT NULL DEFAULT '{}',
    -- Current pattern state
    status          TEXT            NOT NULL DEFAULT 'active',
    -- Severity (escalates based on duration, context, entity type)
    severity        TEXT            NOT NULL DEFAULT 'low',
    -- Human-readable summary
    title           TEXT,
    description     TEXT,
    -- Link to situation cluster
    situation_id    UUID,
    -- Metadata
    source_type     TEXT            NOT NULL,
    region_code     TEXT,
    tags            TEXT[]          DEFAULT '{}'
);

CREATE INDEX idx_active_patterns_entity ON active_patterns (entity_id);
CREATE INDEX idx_active_patterns_status ON active_patterns (status, last_updated DESC);
CREATE INDEX idx_active_patterns_location ON active_patterns USING GIST (center_location);
CREATE INDEX idx_active_patterns_type ON active_patterns (pattern_type, status);
```

### 2.2 Dual-Write Strategy

The existing `registry.rs` already calls `upsert_position_if_needed()` for
every `FlightPosition` and `VesselPosition` event. We add a second write:

```rust
// In registry.rs, after upsert_position_if_needed():
if matches!(event.event_type, EventType::FlightPosition | EventType::VesselPosition) {
    if let Err(e) = append_position_history(&pool, &event).await {
        warn!(error = %e, "Failed to append position history");
    }
}
```

The `append_position_history` function inserts into `position_history`:

```rust
pub async fn append_position_history(
    pool: &PgPool,
    event: &InsertableEvent,
) -> anyhow::Result<()> {
    let (lat, lon, entity_id) = match (event.latitude, event.longitude, &event.entity_id) {
        (Some(lat), Some(lon), Some(eid)) => (lat, lon, eid),
        _ => return Ok(()),
    };

    let squawk = event.payload.get("squawk")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let vertical_rate = event.payload.get("vertical_rate")
        .or_else(|| event.payload.get("baro_rate"))
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    let nav_status = event.payload.get("nav_status_code")
        .and_then(|v| v.as_i64())
        .map(|v| v as i16);
    let on_ground = event.payload.get("on_ground")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    sqlx::query(
        r#"
        INSERT INTO position_history (
            time, entity_id, source_type, location,
            heading, speed, altitude, vertical_rate,
            squawk, nav_status, on_ground
        )
        VALUES (
            $1, $2, $3,
            ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography,
            $6, $7, $8, $9, $10, $11, $12
        )
        "#,
    )
    .bind(event.event_time)
    .bind(entity_id)
    .bind(event.source_type.as_str())
    .bind(lon)
    .bind(lat)
    .bind(event.heading)
    .bind(event.speed)
    .bind(event.altitude)
    .bind(vertical_rate)
    .bind(squawk)
    .bind(nav_status)
    .bind(on_ground)
    .execute(pool)
    .await?;

    Ok(())
}
```

### 2.3 Trail Retrieval API

**Endpoint:** `GET /api/positions/:entity_id/trail`

Query parameters:
- `since` -- ISO 8601 timestamp (default: 2 hours ago)
- `until` -- ISO 8601 timestamp (default: now)
- `max_points` -- maximum points to return (default: 500, max: 2000)
- `simplify` -- tolerance for Ramer-Douglas-Peucker simplification in meters
  (default: 0 = no simplification)

```rust
pub async fn get_entity_trail(
    pool: &PgPool,
    entity_id: &str,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    max_points: i64,
) -> anyhow::Result<Vec<TrailPoint>> {
    let points = sqlx::query_as::<_, TrailPoint>(
        r#"
        SELECT
            time,
            ST_Y(location::geometry) as latitude,
            ST_X(location::geometry) as longitude,
            heading, speed, altitude, squawk, on_ground
        FROM position_history
        WHERE entity_id = $1
          AND time >= $2
          AND time <= $3
        ORDER BY time ASC
        LIMIT $4
        "#,
    )
    .bind(entity_id)
    .bind(since)
    .bind(until)
    .bind(max_points)
    .fetch_all(pool)
    .await?;

    Ok(points)
}
```

**Batch trail endpoint:** `GET /api/positions/trails`

For loading trails for all visible entities in one request (used by MapPanel):

```rust
// POST /api/positions/trails
// Body: { "entity_ids": ["ae1234", "338123456"], "since": "2h" }
pub async fn get_batch_trails(
    pool: &PgPool,
    entity_ids: &[String],
    since: DateTime<Utc>,
    max_points_per_entity: i64,
) -> anyhow::Result<HashMap<String, Vec<TrailPoint>>> {
    let rows = sqlx::query_as::<_, TrailPointWithEntity>(
        r#"
        SELECT
            entity_id,
            time,
            ST_Y(location::geometry) as latitude,
            ST_X(location::geometry) as longitude,
            heading, speed, altitude
        FROM (
            SELECT *,
                   ROW_NUMBER() OVER (
                       PARTITION BY entity_id ORDER BY time DESC
                   ) as rn
            FROM position_history
            WHERE entity_id = ANY($1)
              AND time >= $2
        ) sub
        WHERE rn <= $3
        ORDER BY entity_id, time ASC
        "#,
    )
    .bind(entity_ids)
    .bind(since)
    .bind(max_points_per_entity)
    .fetch_all(pool)
    .await?;

    let mut result: HashMap<String, Vec<TrailPoint>> = HashMap::new();
    for row in rows {
        result.entry(row.entity_id.clone())
            .or_default()
            .push(row.into());
    }
    Ok(result)
}
```

### 2.4 Data Volume Estimates

| Source | Entities/poll | Poll interval | Positions/hour | Positions/day |
|--------|--------------|---------------|----------------|---------------|
| AirplanesLive | ~200 mil | 120s | 6,000 | 144,000 |
| adsb.lol | ~150 mil | 120s | 4,500 | 108,000 |
| adsb.fi | ~150 mil | 120s | 4,500 | 108,000 |
| OpenSky | ~100 | 90s | 4,000 | 96,000 |
| AIS (streaming) | ~500/min | continuous | 30,000 | 720,000 |
| **Total** | | | **~49,000** | **~1,176,000** |

At ~1.2M rows/day with ~120 bytes per row (after compression), this is
approximately:

- **Hot (2h, uncompressed):** ~100k rows, ~12 MB
- **Warm (24h, compressed):** ~1.2M rows, ~30 MB (10:1 compression)
- **Archive (7d, compressed):** ~8.4M rows, ~210 MB

This is well within TimescaleDB's comfort zone.

### 2.5 Deduplication

Multiple ADS-B sources often report the same aircraft. The `entity_id` (ICAO
hex code) is shared across sources, so the same aircraft can generate 3-4
position reports per poll cycle. The pattern detector operates on entity trails
regardless of source, so duplicates are acceptable -- they just increase
position density. However, to save storage:

- The `payload_hash` column enables optional dedup: skip insert if the same
  entity already has a position within the last 5 seconds with the same hash.
- Alternatively, use a unique partial index:
  `CREATE UNIQUE INDEX ON position_history (entity_id, time) WHERE ...` -- but
  this adds write overhead on a hypertable. Better to accept minor duplication
  and let compression handle it.

---

## 3. Pattern Detection Architecture

### 3.1 Core Traits

The pattern detection system mirrors the `CorrelationRule` trait pattern from
`pipeline/src/rules/mod.rs` but is purpose-built for position stream analysis.

```rust
// backend/crates/position-patterns/src/lib.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single position fix for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub time: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub heading: Option<f32>,
    pub speed: Option<f32>,       // knots
    pub altitude: Option<f32>,    // feet
    pub vertical_rate: Option<f32>,
    pub squawk: Option<String>,
    pub on_ground: bool,
    pub source_type: String,
}

/// Ordered sequence of positions for one entity.
#[derive(Debug, Clone)]
pub struct EntityTrail {
    pub entity_id: String,
    pub entity_name: Option<String>,
    pub entity_type: EntityType,
    pub positions: Vec<Position>,   // chronological order (oldest first)
    pub is_military: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    MilitaryAircraft,
    CivilianAircraft,
    MilitaryVessel,
    CivilianVessel,
    Unknown,
}

/// Result of pattern detection on a trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPattern {
    pub pattern_type: PatternType,
    pub entity_id: String,
    pub participant_ids: Vec<String>,    // for multi-entity patterns
    pub started_at: DateTime<Utc>,
    pub confidence: f32,                 // 0.0 - 1.0
    pub severity: Severity,
    pub center: Option<(f64, f64)>,      // (lat, lon)
    pub radius_km: Option<f64>,
    pub parameters: serde_json::Value,   // pattern-specific data
    pub title: String,
    pub description: String,
}

/// Status returned when updating an active pattern with new data.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternStatus {
    /// Pattern is still active and was updated.
    Active {
        updated_title: Option<String>,
        updated_severity: Option<Severity>,
    },
    /// Pattern has ended (entity left area, changed behavior, etc.)
    Ended { reason: String },
    /// Pattern has escalated (duration exceeded threshold, entered sensitive area, etc.)
    Escalated { new_severity: Severity, reason: String },
}

/// Enumeration of all supported pattern types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    // --- Single-entity movement patterns ---
    Loitering,
    RacetrackOrbit,
    HoldingPattern,
    StationKeeping,
    SpeedAnomaly,
    AltitudeAnomaly,
    RouteDeviation,

    // --- Disappearance patterns ---
    SignalLost,
    DarkRunning,
    SquawkChange,

    // --- Multi-entity patterns ---
    FormationFlight,
    Convergence,
    EscortPattern,
    Rendezvous,
    AreaDenial,

    // --- Geospatial patterns ---
    BoundaryCrossing,
    BaseApproach,
    NotamViolation,
    ConflictZoneEntry,

    // --- Maritime-specific patterns ---
    ShipToShipTransfer,
    DarkVoyage,
    PortLoitering,
    RestrictedFishing,
    ChokepointAnomaly,
}

/// The trait every pattern detector must implement.
pub trait PositionPattern: Send + Sync {
    /// Unique identifier for this pattern type.
    fn pattern_type(&self) -> PatternType;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Which entity types this pattern applies to.
    fn applicable_entity_types(&self) -> &[EntityType];

    /// Minimum number of positions in the trail required before this
    /// pattern can be detected.
    fn min_trail_length(&self) -> usize;

    /// Analyze an entity's trail and return any newly detected patterns.
    /// This is called periodically for every tracked entity.
    fn detect(&self, trail: &EntityTrail, config: &PatternConfig) -> Vec<DetectedPattern>;

    /// Update an existing active pattern with a new position.
    /// Returns the pattern's current status (active, ended, escalated).
    fn update(
        &self,
        pattern: &ActivePattern,
        trail: &EntityTrail,
        config: &PatternConfig,
    ) -> PatternStatus;
}
```

### 3.2 Pattern Registry

```rust
// backend/crates/position-patterns/src/registry.rs

use std::collections::HashMap;

pub struct PatternRegistry {
    detectors: Vec<Box<dyn PositionPattern>>,
    by_entity_type: HashMap<EntityType, Vec<usize>>,
}

impl PatternRegistry {
    pub fn new() -> Self {
        Self {
            detectors: Vec::new(),
            by_entity_type: HashMap::new(),
        }
    }

    pub fn register(&mut self, detector: Box<dyn PositionPattern>) {
        let idx = self.detectors.len();
        for &entity_type in detector.applicable_entity_types() {
            self.by_entity_type
                .entry(entity_type)
                .or_default()
                .push(idx);
        }
        self.detectors.push(detector);
    }

    /// Get all pattern detectors applicable to a given entity type.
    pub fn detectors_for(&self, entity_type: EntityType) -> Vec<&dyn PositionPattern> {
        self.by_entity_type
            .get(&entity_type)
            .map(|indices| {
                indices.iter()
                    .map(|&idx| self.detectors[idx].as_ref())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all registered detectors.
    pub fn all_detectors(&self) -> Vec<&dyn PositionPattern> {
        self.detectors.iter().map(|d| d.as_ref()).collect()
    }
}

/// Create a PatternRegistry with all built-in pattern detectors.
pub fn default_patterns() -> PatternRegistry {
    let mut reg = PatternRegistry::new();
    // Single-entity movement
    reg.register(Box::new(loitering::LoiteringDetector));
    reg.register(Box::new(racetrack::RacetrackDetector));
    reg.register(Box::new(holding::HoldingPatternDetector));
    reg.register(Box::new(station_keeping::StationKeepingDetector));
    reg.register(Box::new(speed_anomaly::SpeedAnomalyDetector));
    reg.register(Box::new(altitude_anomaly::AltitudeAnomalyDetector));
    // Disappearance
    reg.register(Box::new(signal_lost::SignalLostDetector));
    reg.register(Box::new(dark_running::DarkRunningDetector));
    reg.register(Box::new(squawk_change::SquawkChangeDetector));
    // Multi-entity
    reg.register(Box::new(formation::FormationFlightDetector));
    reg.register(Box::new(convergence::ConvergenceDetector));
    reg.register(Box::new(rendezvous::RendezvousDetector));
    // Geospatial
    reg.register(Box::new(boundary::BoundaryCrossingDetector));
    reg.register(Box::new(base_approach::BaseApproachDetector));
    // Maritime
    reg.register(Box::new(sts_transfer::ShipToShipTransferDetector));
    reg.register(Box::new(dark_voyage::DarkVoyageDetector));
    reg.register(Box::new(port_loitering::PortLoiteringDetector));
    reg
}
```

### 3.3 Pattern Engine

The pattern engine is a background tokio task that:

1. Maintains an in-memory trail buffer per entity (last 2 hours)
2. Receives position updates via the broadcast channel
3. Periodically runs pattern detection on updated entities
4. Manages the lifecycle of active patterns

```rust
// backend/crates/position-patterns/src/engine.rs

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// In-memory trail buffer for all tracked entities.
pub struct TrailBuffer {
    trails: HashMap<String, EntityTrail>,
    max_age: Duration,
    max_positions_per_entity: usize,
}

impl TrailBuffer {
    pub fn new(max_age: Duration, max_positions: usize) -> Self {
        Self {
            trails: HashMap::new(),
            max_age,
            max_positions_per_entity: max_positions,
        }
    }

    /// Append a new position for an entity. Creates the trail if new.
    pub fn append(&mut self, entity_id: &str, position: Position, metadata: EntityMetadata) {
        let trail = self.trails.entry(entity_id.to_string()).or_insert_with(|| {
            EntityTrail {
                entity_id: entity_id.to_string(),
                entity_name: metadata.entity_name,
                entity_type: metadata.entity_type,
                positions: Vec::new(),
                is_military: metadata.is_military,
                tags: metadata.tags,
            }
        });

        trail.positions.push(position);

        // Enforce max positions (drop oldest)
        if trail.positions.len() > self.max_positions_per_entity {
            let excess = trail.positions.len() - self.max_positions_per_entity;
            trail.positions.drain(..excess);
        }
    }

    /// Prune positions older than max_age from all trails.
    /// Remove trails that have no remaining positions.
    pub fn prune(&mut self) {
        let cutoff = Utc::now() - self.max_age;
        self.trails.retain(|_, trail| {
            trail.positions.retain(|p| p.time >= cutoff);
            !trail.positions.is_empty()
        });
    }

    /// Get a reference to an entity's trail.
    pub fn get(&self, entity_id: &str) -> Option<&EntityTrail> {
        self.trails.get(entity_id)
    }

    /// Get all entity IDs that received new positions since the given time.
    pub fn entities_updated_since(&self, since: DateTime<Utc>) -> Vec<String> {
        self.trails
            .iter()
            .filter(|(_, trail)| {
                trail.positions.last()
                    .map(|p| p.time >= since)
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Number of tracked entities.
    pub fn entity_count(&self) -> usize {
        self.trails.len()
    }
}

pub struct PatternEngine {
    registry: PatternRegistry,
    trail_buffer: TrailBuffer,
    active_patterns: HashMap<Uuid, ActivePattern>,
    config: Arc<RwLock<PatternConfig>>,
}

/// Spawn the pattern detection engine as a background task.
pub fn spawn_pattern_engine(
    event_rx: broadcast::Receiver<InsertableEvent>,
    publish_tx: broadcast::Sender<PublishEvent>,
    pool: PgPool,
    config: Arc<RwLock<PatternConfig>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let registry = default_patterns();
        let mut trail_buffer = TrailBuffer::new(
            Duration::from_secs(7200),  // 2h in-memory
            2400,                        // ~1 position per 3s for 2h
        );
        let mut active_patterns: HashMap<Uuid, ActivePattern> = HashMap::new();
        let mut event_rx = event_rx;
        let mut detect_interval = tokio::time::interval(Duration::from_secs(30));
        let mut prune_interval = tokio::time::interval(Duration::from_secs(300));
        let mut last_detect = Utc::now();

        loop {
            tokio::select! {
                // Receive new position events
                result = event_rx.recv() => {
                    match result {
                        Ok(event) => {
                            if !matches!(event.event_type,
                                EventType::FlightPosition | EventType::VesselPosition
                            ) {
                                continue;
                            }
                            // Convert to Position and append to trail buffer
                            if let Some(pos) = event_to_position(&event) {
                                let metadata = extract_metadata(&event);
                                trail_buffer.append(
                                    event.entity_id.as_deref().unwrap_or(""),
                                    pos,
                                    metadata,
                                );
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(skipped = n, "Pattern engine lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }

                // Periodic pattern detection
                _ = detect_interval.tick() => {
                    let cfg = config.read().unwrap().clone();
                    let updated_entities = trail_buffer.entities_updated_since(last_detect);
                    last_detect = Utc::now();

                    for entity_id in &updated_entities {
                        if let Some(trail) = trail_buffer.get(entity_id) {
                            // Run applicable detectors
                            for detector in registry.detectors_for(trail.entity_type) {
                                if trail.positions.len() < detector.min_trail_length() {
                                    continue;
                                }

                                // Check for new patterns
                                let detected = detector.detect(trail, &cfg);
                                for pattern in detected {
                                    let id = Uuid::new_v4();
                                    // Persist to DB
                                    // Emit as pipeline event
                                    // Store in active_patterns
                                    info!(
                                        pattern_type = ?pattern.pattern_type,
                                        entity = %entity_id,
                                        "Pattern detected: {}",
                                        pattern.title,
                                    );
                                    active_patterns.insert(id, ActivePattern {
                                        id,
                                        detected: pattern,
                                        last_updated: Utc::now(),
                                    });
                                }

                                // Update existing active patterns for this entity
                                let entity_patterns: Vec<Uuid> = active_patterns.iter()
                                    .filter(|(_, p)| {
                                        p.detected.entity_id == *entity_id
                                        && p.detected.pattern_type == detector.pattern_type()
                                    })
                                    .map(|(id, _)| *id)
                                    .collect();

                                for pattern_id in entity_patterns {
                                    if let Some(active) = active_patterns.get(&pattern_id) {
                                        let status = detector.update(active, trail, &cfg);
                                        match status {
                                            PatternStatus::Active { .. } => {
                                                // Update timestamp, maybe title/severity
                                            }
                                            PatternStatus::Ended { reason } => {
                                                info!(
                                                    pattern = %pattern_id,
                                                    reason = %reason,
                                                    "Pattern ended"
                                                );
                                                active_patterns.remove(&pattern_id);
                                            }
                                            PatternStatus::Escalated { new_severity, reason } => {
                                                info!(
                                                    pattern = %pattern_id,
                                                    severity = ?new_severity,
                                                    reason = %reason,
                                                    "Pattern escalated"
                                                );
                                                // Update severity, emit alert
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Periodic pruning
                _ = prune_interval.tick() => {
                    trail_buffer.prune();
                    // Remove stale active patterns (no update in 30 minutes)
                    let stale_cutoff = Utc::now() - chrono::Duration::minutes(30);
                    active_patterns.retain(|_, p| p.last_updated >= stale_cutoff);
                }
            }
        }
    })
}
```

---

## 4. Pattern Catalog

### 4.1 Single-Entity Movement Patterns

#### 4.1.1 Loitering / Orbiting

**OSINT value:** ISR aircraft orbiting an area indicates active surveillance
(often precedes or accompanies strikes). Drone loitering near borders suggests
pre-strike positioning or reconnaissance.

**Detection algorithm:**
1. Compute the centroid of the last N positions (N >= 10, ~5 min of data)
2. Calculate the standard deviation of distances from the centroid
3. If stddev < threshold AND elapsed time > min_duration, classify as loitering
4. Sub-classify: if positions show consistent angular progression (heading
   changes monotonically), it is an orbit/racetrack rather than simple loitering

**Parameters:**

| Parameter | Military Aircraft | Civilian Aircraft | Vessel |
|-----------|------------------|-------------------|--------|
| `min_positions` | 10 | 20 | 10 |
| `max_radius_km` | 30 | 15 | 5 |
| `min_duration_min` | 10 | 20 | 30 |
| `max_speed_kts` | 350 | 300 | 3 |

**Severity escalation:**
- Base: Low (civilian), Medium (military)
- +1 if near conflict zone (50km)
- +1 if duration > 30 min
- +1 if entity is high-value platform (RQ-4, MQ-9, P-8, E-3)
- +1 if near military base (25km)

```rust
pub struct LoiteringDetector;

impl PositionPattern for LoiteringDetector {
    fn pattern_type(&self) -> PatternType { PatternType::Loitering }
    fn name(&self) -> &str { "Loitering / Orbiting" }

    fn applicable_entity_types(&self) -> &[EntityType] {
        &[EntityType::MilitaryAircraft, EntityType::CivilianAircraft,
          EntityType::MilitaryVessel, EntityType::CivilianVessel]
    }

    fn min_trail_length(&self) -> usize { 10 }

    fn detect(&self, trail: &EntityTrail, config: &PatternConfig) -> Vec<DetectedPattern> {
        let params = config.loitering_params(trail.entity_type);
        let positions = &trail.positions;
        if positions.len() < params.min_positions { return vec![]; }

        // Use a sliding window over the trail
        let window_size = params.min_positions;
        let mut patterns = vec![];

        for window in positions.windows(window_size) {
            let centroid = compute_centroid(window);
            let distances: Vec<f64> = window.iter()
                .map(|p| haversine_km(p.latitude, p.longitude, centroid.0, centroid.1))
                .collect();
            let max_dist = distances.iter().cloned().fold(0.0f64, f64::max);
            let elapsed = (window.last().unwrap().time - window.first().unwrap().time)
                .num_minutes();

            if max_dist <= params.max_radius_km
                && elapsed >= params.min_duration_min as i64
            {
                // Check it's not just stationary (must have some movement)
                let avg_speed = window.iter()
                    .filter_map(|p| p.speed)
                    .sum::<f32>() / window.len() as f32;

                if avg_speed > 2.0 || trail.entity_type == EntityType::CivilianVessel {
                    let severity = compute_loiter_severity(trail, &centroid, elapsed, config);
                    patterns.push(DetectedPattern {
                        pattern_type: PatternType::Loitering,
                        entity_id: trail.entity_id.clone(),
                        participant_ids: vec![],
                        started_at: window.first().unwrap().time,
                        confidence: 0.8,
                        severity,
                        center: Some(centroid),
                        radius_km: Some(max_dist),
                        parameters: serde_json::json!({
                            "orbit_radius_km": max_dist,
                            "duration_minutes": elapsed,
                            "avg_speed_kts": avg_speed,
                            "positions_in_pattern": window.len(),
                        }),
                        title: format!(
                            "{} loitering {:.0}km orbit for {}min",
                            trail.entity_name.as_deref()
                                .unwrap_or(&trail.entity_id),
                            max_dist,
                            elapsed,
                        ),
                        description: String::new(),
                    });
                    break; // One pattern per detection cycle
                }
            }
        }

        patterns
    }

    fn update(&self, pattern: &ActivePattern, trail: &EntityTrail, config: &PatternConfig) -> PatternStatus {
        let params = config.loitering_params(trail.entity_type);
        let center = pattern.detected.center.unwrap_or((0.0, 0.0));

        // Check if the entity is still within the loiter radius
        if let Some(latest) = trail.positions.last() {
            let dist = haversine_km(latest.latitude, latest.longitude, center.0, center.1);
            if dist > params.max_radius_km * 1.5 {
                return PatternStatus::Ended {
                    reason: format!("Entity departed loiter area ({}km from center)", dist as u32),
                };
            }

            // Check for severity escalation
            let duration_min = (latest.time - pattern.detected.started_at).num_minutes();
            if duration_min > 60 && pattern.detected.severity < Severity::High {
                return PatternStatus::Escalated {
                    new_severity: Severity::High,
                    reason: format!("Extended loitering: {}min", duration_min),
                };
            }
        }

        PatternStatus::Active {
            updated_title: None,
            updated_severity: None,
        }
    }
}
```

#### 4.1.2 Racetrack Orbit

**OSINT value:** ISR aircraft (FORTE, JAKE, DUKE) fly standardized racetrack
patterns for surveillance. Detecting this pattern is more specific than generic
loitering and indicates deliberate intelligence collection.

**Detection algorithm:**
1. Compute heading changes between consecutive positions
2. Identify 180-degree turns (heading reversal within 10 positions)
3. If two reversals are detected with parallel legs, classify as racetrack
4. Compute racetrack dimensions: leg length, track width, orientation

**Parameters:**
- `min_leg_length_km`: 15
- `max_track_width_km`: 10
- `heading_reversal_threshold_deg`: 150
- `min_reversals`: 2

#### 4.1.3 Holding Pattern

**OSINT value:** Aviation holding patterns differ from ISR racetracks --
they are shorter, often near airports, and use standard turn directions.
Detection distinguishes normal ATC holds from abnormal ones (military aircraft
holding near conflict zones suggests pre-strike staging).

**Detection algorithm:**
1. Same as racetrack but with tighter geometry constraints
2. Leg length 5-15 km, right-hand turns standard (left-hand is non-standard)
3. Usually at specific altitudes (flight levels)

**Parameters:**
- `min_leg_length_km`: 5
- `max_leg_length_km`: 15
- `standard_turn_right`: true (non-standard is notable)
- `altitude_band_ft`: 1000 (positions within 1000ft of each other)

#### 4.1.4 Station Keeping (Vessels)

**OSINT value:** A vessel maintaining position at sea (not at port) for an
extended period suggests: surveillance, fisheries enforcement, naval blockade,
or waiting for a rendezvous.

**Detection algorithm:**
1. Speed consistently below threshold (< 3 knots) for minimum duration
2. Position drift < max_drift_km from initial position
3. AIS nav_status NOT moored (5) or at anchor (1) -- those are expected

**Parameters:**
- `max_speed_kts`: 3
- `max_drift_km`: 2
- `min_duration_min`: 60
- `exclude_nav_status`: [1, 5] (at anchor, moored)

#### 4.1.5 Speed Anomaly

**OSINT value:** Sudden acceleration or deceleration can indicate evasive
maneuvering, distress, or military activity. A tanker suddenly slowing in open
water may be performing an underway replenishment. A fighter suddenly
accelerating may be scrambling.

**Detection algorithm:**
1. Compute speed delta between consecutive positions
2. If |delta| exceeds threshold (entity-type-specific), flag anomaly
3. Use z-score against the entity's own speed history (mean + stddev)

**Parameters:**

| Parameter | Military Aircraft | Civilian Aircraft | Vessel |
|-----------|------------------|-------------------|--------|
| `speed_change_threshold_kts` | 100 | 80 | 5 |
| `z_score_threshold` | 3.0 | 3.0 | 2.5 |
| `min_samples_for_baseline` | 10 | 10 | 10 |

#### 4.1.6 Altitude Anomaly

**OSINT value:** Rapid descent or unusual altitude changes can indicate combat
maneuvering, emergencies, or terrain-following flight (military low-level). A
military transport dropping from FL350 to FL100 near a conflict zone suggests
an airdrop or tactical approach.

**Detection algorithm:**
1. Compute altitude rate from consecutive position fixes (or use reported
   `vertical_rate` / `baro_rate`)
2. Flag if rate exceeds thresholds:
   - Rapid descent: > 3000 ft/min sustained for 30+ seconds
   - Terrain following: altitude < 2000 ft with speed > 250 kts (military)
   - Pop-up: altitude increase > 5000 ft in < 2 min (missile launch profile)

**Parameters:**
- `rapid_descent_rate_ftmin`: 3000
- `terrain_following_alt_ft`: 2000
- `terrain_following_speed_kts`: 250
- `popup_altitude_change_ft`: 5000
- `popup_time_window_sec`: 120

### 4.2 Disappearance Patterns

#### 4.2.1 Signal Lost (ADS-B / AIS Drop)

**OSINT value:** An entity that was transmitting regularly and then stops is
one of the most important OSINT signals. Causes include: intentional
transponder shutdown (military operations), being shot down, equipment failure,
or moving out of receiver coverage. The context determines significance.

**Detection algorithm:**
1. Track the expected reporting interval per entity (adaptive: based on
   observed cadence)
2. If no position received for > 3x the expected interval, flag as signal lost
3. Record last known position, heading, speed for extrapolation
4. If the entity later reappears, compute the gap and distance traveled

**Parameters:**
- `expected_interval_factor`: 3.0 (flag at 3x normal interval)
- `min_previous_reports`: 5 (must have seen entity 5+ times)
- `max_gap_before_stale_min`: 60 (stop tracking after 60 min gap)

**Severity:**
- Military entity: Medium (+ escalation if near conflict zone)
- Civilian entity: Low (unless squawk was emergency)
- Entity was squawking 7700/7600/7500 before loss: Critical

```rust
pub struct SignalLostDetector;

impl PositionPattern for SignalLostDetector {
    fn pattern_type(&self) -> PatternType { PatternType::SignalLost }
    fn name(&self) -> &str { "Signal Lost" }

    fn applicable_entity_types(&self) -> &[EntityType] {
        &[EntityType::MilitaryAircraft, EntityType::CivilianAircraft,
          EntityType::MilitaryVessel, EntityType::CivilianVessel]
    }

    fn min_trail_length(&self) -> usize { 5 }

    fn detect(&self, trail: &EntityTrail, config: &PatternConfig) -> Vec<DetectedPattern> {
        let positions = &trail.positions;
        if positions.is_empty() { return vec![]; }

        let last = positions.last().unwrap();
        let age = Utc::now() - last.time;

        // Compute the entity's average reporting interval
        let intervals: Vec<i64> = positions.windows(2)
            .map(|w| (w[1].time - w[0].time).num_seconds())
            .collect();
        if intervals.is_empty() { return vec![]; }

        let avg_interval = intervals.iter().sum::<i64>() / intervals.len() as i64;
        let threshold = avg_interval * config.signal_lost.expected_interval_factor as i64;

        if age.num_seconds() > threshold && age.num_seconds() < 3600 {
            let severity = if trail.is_military {
                Severity::Medium
            } else if last.squawk.as_deref() == Some("7700")
                || last.squawk.as_deref() == Some("7600")
                || last.squawk.as_deref() == Some("7500")
            {
                Severity::Critical
            } else {
                Severity::Low
            };

            return vec![DetectedPattern {
                pattern_type: PatternType::SignalLost,
                entity_id: trail.entity_id.clone(),
                participant_ids: vec![],
                started_at: last.time,
                confidence: 0.7,
                severity,
                center: Some((last.latitude, last.longitude)),
                radius_km: None,
                parameters: serde_json::json!({
                    "last_heading": last.heading,
                    "last_speed": last.speed,
                    "last_altitude": last.altitude,
                    "last_squawk": last.squawk,
                    "gap_seconds": age.num_seconds(),
                    "avg_interval_seconds": avg_interval,
                    "expected_threshold_seconds": threshold,
                }),
                title: format!(
                    "{} signal lost ({}s gap, {}x normal interval)",
                    trail.entity_name.as_deref().unwrap_or(&trail.entity_id),
                    age.num_seconds(),
                    age.num_seconds() / avg_interval.max(1),
                ),
                description: format!(
                    "Last seen at {:.4}, {:.4} heading {:.0} at {}kts/{}ft",
                    last.latitude, last.longitude,
                    last.heading.unwrap_or(0.0),
                    last.speed.unwrap_or(0.0) as u32,
                    last.altitude.unwrap_or(0.0) as u32,
                ),
            }];
        }

        vec![]
    }

    fn update(&self, pattern: &ActivePattern, trail: &EntityTrail, _config: &PatternConfig) -> PatternStatus {
        // If we received new positions after the gap, the entity has reappeared
        if let Some(latest) = trail.positions.last() {
            if latest.time > pattern.detected.started_at {
                let gap_duration = (latest.time - pattern.detected.started_at).num_seconds();
                let center = pattern.detected.center.unwrap_or((0.0, 0.0));
                let distance = haversine_km(latest.latitude, latest.longitude, center.0, center.1);
                return PatternStatus::Ended {
                    reason: format!(
                        "Signal restored after {}s gap, {}km from last known position",
                        gap_duration, distance as u32,
                    ),
                };
            }
        }

        PatternStatus::Active {
            updated_title: None,
            updated_severity: None,
        }
    }
}
```

#### 4.2.2 Dark Running (AIS Off)

**OSINT value:** Vessels intentionally disabling AIS to hide their movements.
Common in sanctions evasion, illegal fishing, and naval operations. Distinguished
from simple signal loss by the intentional nature (vessel reappears far from
expected position).

**Detection algorithm:**
1. Detect AIS gap (same as signal lost but for vessels)
2. When vessel reappears, compute:
   - Time gap
   - Distance from expected position (dead reckoning from last known heading/speed)
   - If distance_actual >> distance_expected, classify as intentional dark running
3. Flag if vessel reappeared near sensitive areas (ports under sanctions,
   restricted waters)

**Parameters:**
- `min_gap_minutes`: 30
- `distance_ratio_threshold`: 2.0 (actual/expected > 2x = suspicious)
- `max_gap_hours`: 48

#### 4.2.3 Squawk Code Changes

**OSINT value:** ADS-B squawk (transponder) codes carry meaning:
- **7700** -- General emergency
- **7600** -- Communications failure
- **7500** -- Hijack
- **0000** -- Cleared from previous squawk (sometimes indicates military)
- **1200** -- VFR (unexpected for IFR flight = possible diversion)
- **Military discrete codes** (in the 01xx-06xx range)

**Detection algorithm:**
1. Track previous squawk per entity
2. Flag any transition to/from emergency codes (7700, 7600, 7500)
3. Flag transitions to 0000 from a valid code (possible military switch)

### 4.3 Multi-Entity Patterns

#### 4.3.1 Formation Flying

**OSINT value:** Multiple military aircraft maintaining fixed relative positions
indicates organized military operations (deployment, patrol, or strike package).

**Detection algorithm:**
1. For every pair of entities in the same spatial neighborhood:
   - Compute inter-aircraft distance
   - Compute heading difference
   - Compute speed difference
2. If distance < threshold AND heading_diff < threshold AND speed_diff <
   threshold, AND this persists for min_duration, classify as formation
3. Track formation membership (can be > 2 aircraft)

**Parameters:**
- `max_inter_distance_km`: 10
- `max_heading_diff_deg`: 15
- `max_speed_diff_kts`: 30
- `min_duration_min`: 5
- `min_formation_size`: 2

**Optimization:** Only check pairs within the same spatial grid cell (geo hash
at resolution ~50km).

#### 4.3.2 Convergence

**OSINT value:** Multiple entities from different starting points converging on
the same location suggests a coordinated operation: airstrike assembly, naval
task force formation, or emergency response.

**Detection algorithm:**
1. For each entity with a consistent heading (stddev < 10 deg over last 5 positions):
   - Project forward position at current heading/speed for 30/60/120 minutes
2. Cluster projected positions
3. If >= 3 entities project to within convergence_radius, flag convergence

**Parameters:**
- `min_entities`: 3
- `convergence_radius_km`: 50
- `projection_horizon_min`: [30, 60, 120]
- `min_heading_consistency_positions`: 5

#### 4.3.3 Escort Pattern

**OSINT value:** Fighter aircraft flying alongside a larger aircraft (tanker,
transport, VIP) indicates force protection. Number and type of escorts
indicates the perceived threat level.

**Detection algorithm:**
1. Identify "principal" aircraft (tanker, transport, VIP types)
2. Check for military aircraft within escort_radius maintaining relative
   position for min_duration
3. Compute relative positioning (ahead, flanking, trailing)

**Parameters:**
- `escort_radius_km`: 20
- `min_duration_min`: 10
- `principal_types`: ["C17", "C5", "KC10", "KC46", "E3", "E6"]

#### 4.3.4 Rendezvous

**OSINT value:** Two entities approaching each other and meeting at a point,
especially at sea, can indicate: aerial refueling, ship-to-ship transfer,
prisoner exchange, or resupply.

**Detection algorithm:**
1. Track closing speed between pairs of entities
2. If distance is decreasing consistently and approaches < rendezvous_radius:
   - Flag as rendezvous
3. If both entities then maintain proximity for min_duration, confirm rendezvous

**Parameters:**
- `rendezvous_radius_km`: 5 (air), 2 (sea)
- `min_closing_speed_kts`: 10
- `min_proximity_duration_min`: 5

#### 4.3.5 Area Denial

**OSINT value:** Multiple military aircraft or vessels establishing a perimeter
around an area suggests an exclusion zone or force protection operation.

**Detection algorithm:**
1. Identify clusters of military entities within the same region
2. Compute convex hull of entity positions
3. If hull area < max_area AND entities are on the perimeter (not clustered
   at center), classify as area denial

**Parameters:**
- `min_entities`: 4
- `max_area_km2`: 10000
- `perimeter_ratio`: 0.6 (60% of entities on hull)

### 4.4 Geospatial Patterns

#### 4.4.1 Boundary / Border Crossing

**OSINT value:** Aircraft or vessels crossing into contested or restricted
airspace/waters is a major escalation indicator. Examples: aircraft entering
another nation's ADIZ, vessels crossing into territorial waters.

**Detection algorithm:**
1. Load boundary polygons from the `zones` table (type = 'boundary',
   'adiz', 'territorial_waters', 'fir')
2. For each position update, test if the entity crossed from outside to inside
   a boundary (ST_Crosses or ST_Intersects state change)
3. Flag the crossing with the boundary name and direction

**Parameters:**
- `zone_types`: ["boundary", "adiz", "territorial_waters", "fir", "restricted"]
- `min_speed_kts`: 5 (filter out stationary boundary noise)

#### 4.4.2 Proximity to Military Bases

**OSINT value:** Non-military aircraft near military bases may indicate
reconnaissance. Military aircraft approaching foreign bases indicates deployment
or posturing.

**Detection algorithm:**
1. Load military base positions from `static/data/military-bases.geojson`
2. For each military entity, compute distance to nearest foreign base
3. If distance < threshold and closing, flag as base approach

**Parameters:**
- `approach_radius_km`: 50
- `warning_radius_km`: 25
- `must_be_closing`: true (ignore entities moving away)

#### 4.4.3 NOTAM Restricted Area Entry

**OSINT value:** Aircraft entering an area covered by a restricted NOTAM
(temporary flight restriction) may indicate that the TFR is being enforced or
violated. Correlation with NOTAM source data adds context.

**Detection algorithm:**
1. Cross-reference entity positions with active NOTAMs from the `notam`
   source (stored as events with bounding polygons in payload)
2. If entity enters NOTAM area, flag with NOTAM details

#### 4.4.4 Conflict Zone Entry

**OSINT value:** Military or civilian entities entering an active conflict zone
(as defined by recent conflict events from ACLED/GeoConfirmed) indicates
escalation or humanitarian risk.

**Detection algorithm:**
1. Query recent conflict events within the pipeline's SituationGraph
2. Compute a dynamic "conflict zone" as a convex hull of recent conflict events
   (buffered by 50km)
3. Flag entities entering this dynamic zone

### 4.5 Maritime-Specific Patterns

#### 4.5.1 Ship-to-Ship Transfer

**OSINT value:** Two vessels approaching and maintaining close proximity at sea
(not at port) suggests cargo transfer, which is a common sanctions evasion
technique (oil transfers between tankers, etc.).

**Detection algorithm:**
1. Identify two vessels that approach within 0.5 km at sea
2. Both vessels slow to < 3 knots
3. Proximity maintained for > 30 minutes
4. Flag with vessel identities, duration, and location

**Parameters:**
- `proximity_km`: 0.5
- `max_speed_kts`: 3
- `min_duration_min`: 30
- `exclude_ports`: true (ignore if within 5km of known port)

#### 4.5.2 Dark Voyage

**OSINT value:** A vessel that turns off AIS, travels a significant distance,
and turns it back on. Distinguished from simple signal loss by the distance
traveled. Strong indicator of sanctions evasion or illegal activity.

**Detection algorithm:**
1. This is a post-hoc pattern: detected when AIS reappears
2. Compute: expected position (dead reckoning) vs actual position
3. If gap > 2 hours AND actual_distance / expected_distance > 2.0, classify
   as dark voyage
4. Compute whether the dark segment crossed into sensitive waters

**Parameters:**
- `min_dark_hours`: 2
- `distance_anomaly_ratio`: 2.0
- `max_dark_hours`: 168 (7 days -- beyond this, it is a new track)

#### 4.5.3 Port Loitering

**OSINT value:** Vessels waiting outside a port for extended periods may
indicate: congestion, waiting for a dark transfer, or avoiding port state
control inspection.

**Detection algorithm:**
1. Identify vessels within 10-50km of known ports
2. Speed < 3 knots for > 4 hours
3. Not in an established anchorage area (if anchorage polygons available)

**Parameters:**
- `port_approach_radius_km`: 50
- `port_exclusion_radius_km`: 10
- `max_speed_kts`: 3
- `min_duration_hours`: 4

#### 4.5.4 Restricted Zone Fishing

**OSINT value:** Fishing vessels operating in restricted zones or foreign EEZs.
Indicator of IUU (illegal, unreported, unregulated) fishing.

**Detection algorithm:**
1. Identify vessels with AIS nav_status = 7 (engaged in fishing)
2. Check if position falls within restricted fishing zones (from `zones` table)
3. Also detect "fishing-like" patterns from non-fishing-flagged vessels:
   slow speed (1-5 kts) with frequent heading changes

#### 4.5.5 Chokepoint Speed Anomaly

**OSINT value:** Vessels slowing unexpectedly in maritime chokepoints (Strait
of Hormuz, Bab-el-Mandeb, Suez Canal, Strait of Malacca, Taiwan Strait)
may indicate: interdiction, mechanical failure, or hostile action.

**Detection algorithm:**
1. Define chokepoint polygons (reuse AIS source `NAMED_REGIONS`)
2. For vessels within a chokepoint, track speed relative to the typical
   transit speed for that chokepoint
3. Flag significant deviations (z-score > 2.0 below mean)

---

## 5. Active Pattern Lifecycle

### 5.1 State Machine

```
            +-----------+
            |           |
  detect()  | DETECTED  |
 ---------> |           |
            +-----+-----+
                  |
                  | persist to DB + emit event
                  v
            +-----------+           +-----------+
            |           |  update() |           |
            |  ACTIVE   +---------> | ESCALATED |
            |           |           |           |
            +-----+-----+           +-----+-----+
                  |                        |
                  | end condition           | end condition
                  v                        v
            +-----------+
            |           |
            |   ENDED   |
            |           |
            +-----------+
                  |
                  | generate summary
                  v
            +-----------+
            |           |
            | ARCHIVED  |
            |           |
            +-----------+
```

### 5.2 Lifecycle Events

1. **Detection:** Pattern detector identifies a new pattern. An event is
   emitted as `EventType::PositionPattern` (new event type to add to
   `sr-types`). The event is inserted into the events table AND creates an
   `active_patterns` row.

2. **Update:** Every 30 seconds, the engine checks active patterns against
   new position data. The pattern's `last_updated` timestamp, duration, and
   parameters are refreshed. No new events are emitted for routine updates.

3. **Escalation:** If the pattern crosses a severity threshold (duration,
   proximity to conflict, entity type), a new event is emitted with updated
   severity. The existing situation is updated rather than creating a new one.

4. **End:** When the entity leaves the pattern area, changes behavior, or
   the gap threshold is exceeded, the pattern transitions to `ended`. A
   final summary event is emitted.

5. **Archive:** After 30 minutes in `ended` state, the pattern row is
   marked `archived` and excluded from active queries.

### 5.3 Situation Generation

Detected patterns generate "tracking situations" in the SituationGraph:

```rust
// When a pattern is detected, inject a synthetic InsertableEvent
fn pattern_to_event(pattern: &DetectedPattern, trail: &EntityTrail) -> InsertableEvent {
    let (lat, lon) = pattern.center.unwrap_or((0.0, 0.0));
    InsertableEvent {
        event_time: pattern.started_at,
        source_type: SourceType::Pipeline,   // or a new SourceType::PatternEngine
        source_id: Some(format!("pattern:{}", pattern.pattern_type.as_str())),
        longitude: Some(lon),
        latitude: Some(lat),
        region_code: determine_region(lat, lon),
        entity_id: Some(trail.entity_id.clone()),
        entity_name: trail.entity_name.clone(),
        event_type: EventType::PositionPattern,
        severity: pattern.severity,
        confidence: Some(pattern.confidence),
        tags: vec![
            format!("pattern:{}", pattern.pattern_type.as_str()),
            if trail.is_military { "military".to_string() } else { "civilian".to_string() },
        ],
        title: Some(pattern.title.clone()),
        description: Some(pattern.description.clone()),
        payload: serde_json::json!({
            "pattern_type": pattern.pattern_type,
            "center_lat": lat,
            "center_lon": lon,
            "radius_km": pattern.radius_km,
            "parameters": pattern.parameters,
            "participant_ids": pattern.participant_ids,
        }),
        heading: None,
        speed: None,
        altitude: None,
    }
}
```

This event flows through the normal pipeline and into the SituationGraph.
Because it has lat/lon and entity_id, it will automatically be correlated with
nearby conflict events, other patterns, and entity graph entries.

**Example auto-generated situation:**

> "RAF Reaper drone (ZZ402) orbiting 20km southeast of Deir ez-Zor, Syria
> for 47 minutes. Pattern detected at 14:23 UTC, currently active. 3 FIRMS
> thermal anomalies detected within the orbit radius in the past 2 hours."

---

## 6. Pipeline Integration

### 6.1 Architecture Overview

```
                     broadcast::Sender<InsertableEvent>
                                  |
                    +-------------+-------------+
                    |             |             |
              CorrelationWindow  SituationGraph  PatternEngine
              (existing)         (existing)      (NEW)
                    |             |             |
                    |             |    pattern events emitted to:
                    |             |     - broadcast channel (for SSE)
                    |             |     - SituationGraph (for clustering)
                    |             |     - CorrelationWindow (for rules)
                    +------+------+-------------+
                           |
                    publish_tx broadcast
                           |
                        SSE /api/events/stream
```

### 6.2 Spawning

In `server/src/main.rs`, after `spawn_pipeline()`:

```rust
// Spawn position pattern engine
let pattern_config = Arc::new(RwLock::new(PatternConfig::default()));
let pattern_handle = sr_position_patterns::spawn_pattern_engine(
    event_tx.subscribe(),   // receives all events
    publish_tx.clone(),     // emits pattern events
    pool.clone(),
    pattern_config.clone(),
);
```

### 6.3 New EventType

Add to `sr-types/src/event_type.rs`:

```rust
/// A behavioral pattern detected from position analysis.
PositionPattern,
```

This type should:
- NOT be in `HIGH_VOLUME_TYPES` (patterns are infrequent, each is significant)
- Pass the `is_important()` filter
- Have its own SSE event name for frontend subscription

### 6.4 Cross-Pattern Correlation

Pattern events feed back into the existing CorrelationWindow and can trigger
existing correlation rules. Example chains:

- **Loitering + Conflict:** An MQ-9 loitering near an area where ACLED reports
  conflict triggers the `military_strike` rule.
- **Signal Lost + Conflict:** ADS-B signal loss of a civilian aircraft over a
  conflict zone triggers a potential shoot-down alert.
- **Formation + NOTAM:** Multiple military aircraft in formation entering a
  NOTAM-restricted area triggers `military_strike`.
- **Dark Running + Sanctions:** Vessel dark running near Iran triggers a
  sanctions evasion alert (custom rule).

### 6.5 Entity Graph Integration

Patterns generate entity graph entries:

- The aircraft/vessel entity already exists from position data
- The pattern creates a new "activity" or "behavior" node
- A relationship connects the entity to the pattern:
  `entity --PERFORMED--> pattern`
- Multi-entity patterns create relationships between participants:
  `entity_A --FORMATION_WITH--> entity_B`

---

## 7. Dynamic Configuration

### 7.1 PatternConfig Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternConfig {
    /// Global enable/disable
    pub enabled: bool,

    /// Per-pattern-type enable/disable and parameter overrides
    pub patterns: HashMap<PatternType, PatternTypeConfig>,

    /// Entity-type-specific thresholds
    pub entity_thresholds: EntityThresholds,

    /// Watchlist: entity IDs that get more sensitive detection
    pub watchlist: Watchlist,

    /// Geospatial sensitivity zones
    pub sensitivity_zones: Vec<SensitivityZone>,

    /// Detection interval (how often to run pattern detection)
    pub detect_interval_secs: u64,

    /// Trail buffer settings
    pub max_trail_age_secs: u64,
    pub max_positions_per_entity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternTypeConfig {
    pub enabled: bool,
    pub parameters: serde_json::Value,
    pub severity_overrides: Option<SeverityOverrides>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityThresholds {
    pub military_aircraft: ThresholdSet,
    pub civilian_aircraft: ThresholdSet,
    pub military_vessel: ThresholdSet,
    pub civilian_vessel: ThresholdSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSet {
    pub loiter_min_duration_min: u32,
    pub loiter_max_radius_km: f64,
    pub speed_anomaly_threshold_kts: f64,
    pub signal_lost_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watchlist {
    /// ICAO hex codes or MMSI numbers to track with heightened sensitivity
    pub entity_ids: Vec<WatchlistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistEntry {
    pub entity_id: String,
    pub reason: String,
    pub sensitivity_multiplier: f64,  // 0.5 = twice as sensitive thresholds
    pub notify_on_any_pattern: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityZone {
    pub name: String,
    pub center: (f64, f64),  // (lat, lon)
    pub radius_km: f64,
    pub severity_boost: i32,  // +1, +2 severity levels for patterns in this zone
    pub reason: String,
}
```

### 7.2 Default Sensitivity Zones

Based on the existing regions and areas of interest in the codebase:

```rust
impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            patterns: HashMap::new(), // all enabled by default
            entity_thresholds: EntityThresholds::default(),
            watchlist: Watchlist { entity_ids: vec![] },
            sensitivity_zones: vec![
                SensitivityZone {
                    name: "Ukraine conflict zone".into(),
                    center: (48.5, 36.0),
                    radius_km: 300.0,
                    severity_boost: 1,
                    reason: "Active conflict zone".into(),
                },
                SensitivityZone {
                    name: "Strait of Hormuz".into(),
                    center: (26.5, 56.0),
                    radius_km: 100.0,
                    severity_boost: 1,
                    reason: "Critical maritime chokepoint".into(),
                },
                SensitivityZone {
                    name: "Taiwan Strait".into(),
                    center: (24.0, 119.0),
                    radius_km: 200.0,
                    severity_boost: 1,
                    reason: "Geopolitical flashpoint".into(),
                },
                SensitivityZone {
                    name: "Red Sea / Bab-el-Mandeb".into(),
                    center: (13.0, 43.5),
                    radius_km: 200.0,
                    severity_boost: 1,
                    reason: "Active threat zone (Houthi attacks)".into(),
                },
                SensitivityZone {
                    name: "Syria / Iraq border".into(),
                    center: (34.5, 41.0),
                    radius_km: 200.0,
                    severity_boost: 1,
                    reason: "Active conflict zone".into(),
                },
            ],
            detect_interval_secs: 30,
            max_trail_age_secs: 7200,
            max_positions_per_entity: 2400,
        }
    }
}
```

### 7.3 API Endpoint

`PATCH /api/config/patterns` -- update pattern configuration at runtime.

```rust
// routes/config.rs
pub async fn update_pattern_config(
    State(state): State<AppState>,
    Json(update): Json<PatternConfigUpdate>,
) -> Result<Json<PatternConfig>, AppError> {
    let mut config = state.pattern_config.write().unwrap();
    if let Some(enabled) = update.enabled {
        config.enabled = enabled;
    }
    if let Some(watchlist) = update.watchlist {
        config.watchlist = watchlist;
    }
    // ... apply other updates
    Ok(Json(config.clone()))
}
```

---

## 8. Frontend Visualization

### 8.1 Trail Display Enhancement

Currently `maxTrailPoints = 10` in `map.svelte.ts`. With the position history
API, we can load full 2-hour trails on demand.

**Changes to `map.svelte.ts`:**

```typescript
class MapStore {
    // Increase from 10 to support full server-side trails
    private maxTrailPoints = 500;

    // New: load trail from server when user clicks an entity
    async loadFullTrail(entityId: string, since?: string): Promise<void> {
        const params = new URLSearchParams();
        if (since) params.set('since', since);
        params.set('max_points', '500');

        const resp = await fetch(`/api/positions/${entityId}/trail?${params}`);
        if (!resp.ok) return;

        const points: TrailPoint[] = await resp.json();
        const nextHistory = new Map(this.positionHistory);
        nextHistory.set(entityId, points.map(p => ({
            lng: p.longitude,
            lat: p.latitude,
            time: new Date(p.time).getTime(),
        })));
        this.positionHistory = nextHistory;
    }
}
```

**Trail layer styling update in `MapPanel.svelte`:**

```javascript
// Enhanced trail rendering with age-based opacity gradient
map.addLayer({
    id: 'position-trails',
    type: 'line',
    source: 'trails',
    paint: {
        'line-color': [
            'match',
            ['get', 'type'],
            'flight-mil', '#f472b6',  // pink for military
            'flight', '#64748b',       // slate for civilian
            '#06b6d4'                  // cyan for vessels
        ],
        'line-width': [
            'case',
            ['==', ['get', 'type'], 'flight-mil'], 2.0,
            1.5
        ],
        'line-opacity': 0.5,
        'line-gradient': [
            'interpolate', ['linear'], ['line-progress'],
            0, 0.1,    // oldest point: 10% opacity
            1, 0.8     // newest point: 80% opacity
        ]
    }
});
```

### 8.2 Pattern Visualization Layers

Add new map layers for active patterns:

```javascript
// Pattern center markers
map.addSource('patterns', {
    type: 'geojson',
    data: { type: 'FeatureCollection', features: [] }
});

// Orbit/loiter circles
map.addLayer({
    id: 'pattern-circles',
    type: 'circle',
    source: 'patterns',
    filter: ['has', 'radius_km'],
    paint: {
        'circle-radius': [
            'interpolate', ['linear'], ['zoom'],
            3, 5,
            10, ['*', ['get', 'radius_km'], 50]  // approximate px
        ],
        'circle-color': 'transparent',
        'circle-stroke-color': [
            'match',
            ['get', 'severity'],
            'critical', '#ef4444',
            'high', '#f97316',
            'medium', '#eab308',
            '#64748b'
        ],
        'circle-stroke-width': 2,
        'circle-stroke-dasharray': [4, 2],
        'circle-opacity': 0.3
    }
});

// Pattern labels
map.addLayer({
    id: 'pattern-labels',
    type: 'symbol',
    source: 'patterns',
    layout: {
        'text-field': ['get', 'title'],
        'text-size': 11,
        'text-offset': [0, 1.5],
        'text-anchor': 'top',
        'text-max-width': 15,
    },
    paint: {
        'text-color': '#e2e8f0',
        'text-halo-color': '#0f172a',
        'text-halo-width': 1,
    }
});
```

### 8.3 Pattern Panel in Situation Drawer

When a pattern-based situation is opened in the `SituationDrawer`, display:

```svelte
<!-- PatternDetail.svelte -->
<script lang="ts">
    let { pattern } = $props<{ pattern: ActivePattern }>();
</script>

<div class="pattern-detail">
    <div class="pattern-header">
        <span class="pattern-type badge">{pattern.pattern_type}</span>
        <span class="severity badge-{pattern.severity}">{pattern.severity}</span>
    </div>

    <div class="pattern-entity">
        <span class="entity-name">{pattern.entity_name ?? pattern.entity_id}</span>
        {#if pattern.is_military}
            <span class="military-badge">MIL</span>
        {/if}
    </div>

    <div class="pattern-metrics">
        <div>Duration: {formatDuration(pattern.started_at)}</div>
        <div>Radius: {pattern.radius_km?.toFixed(1)} km</div>
        <div>Confidence: {(pattern.confidence * 100).toFixed(0)}%</div>
    </div>

    <!-- Mini-map showing the pattern trail -->
    <div class="pattern-trail-map">
        <!-- Embedded MapLibre showing just this entity's trail -->
    </div>

    <div class="pattern-timeline">
        <!-- Timeline showing pattern start, updates, escalations -->
    </div>
</div>
```

### 8.4 Alerts Panel Integration

Active patterns appear in the AlertsPanel feed alongside situations:

```svelte
{#if item.type === 'pattern'}
    <div class="feed-item pattern-item" class:military={item.is_military}>
        <div class="pattern-icon">
            {#if item.pattern_type === 'loitering'}
                <!-- orbit icon -->
            {:else if item.pattern_type === 'signal_lost'}
                <!-- signal-off icon -->
            {:else if item.pattern_type === 'formation_flight'}
                <!-- formation icon -->
            {/if}
        </div>
        <div class="pattern-info">
            <div class="pattern-title">{item.title}</div>
            <div class="pattern-meta">
                <span class="duration">{item.duration}</span>
                <span class="entity">{item.entity_name}</span>
            </div>
        </div>
        <button onclick={() => flyToPattern(item)}>
            <!-- map pin icon -->
        </button>
    </div>
{/if}
```

---

## 9. Implementation Phases

### Phase 1: Position History + Trail API (1-2 weeks)

**Goal:** Store position history and serve trails to the frontend.

Tasks:
1. Write migration `008_position_history.sql`
2. Add `append_position_history()` to `sr-db/src/queries.rs`
3. Add dual-write in `registry.rs` (after `upsert_position_if_needed`)
4. Add `get_entity_trail()` and `get_batch_trails()` query functions
5. Add `GET /api/positions/:entity_id/trail` route
6. Add `POST /api/positions/trails` batch route
7. Update frontend `map.svelte.ts`:
   - Increase `maxTrailPoints` to 500
   - Add `loadFullTrail()` method
   - Load trails from API when entity is clicked
8. Update MapPanel trail rendering:
   - Line gradient for age-based opacity
   - Different widths for military vs civilian

**Deliverable:** Full 2-hour trails visible on map for any tracked entity.

### Phase 2: Pattern Engine Framework + Core Patterns (2-3 weeks)

**Goal:** Trait-based pattern detection with 3 initial detectors.

Tasks:
1. Create `backend/crates/position-patterns/` crate
2. Implement core types: `Position`, `EntityTrail`, `DetectedPattern`,
   `PatternStatus`, `PatternType`
3. Implement `PositionPattern` trait and `PatternRegistry`
4. Implement `TrailBuffer` with in-memory trail management
5. Implement `PatternEngine` background task
6. Add `EventType::PositionPattern` to `sr-types`
7. Implement 3 initial detectors:
   - **Loitering/Orbiting** (highest value, ISR detection)
   - **Signal Lost** (ADS-B/AIS drop detection)
   - **Speed Anomaly** (sudden speed changes)
8. Spawn pattern engine in `server/src/main.rs`
9. Add `active_patterns` table (from migration)
10. Add `GET /api/patterns` and `GET /api/patterns/:id` routes
11. Pattern events emit on SSE

**Deliverable:** System detects orbiting military aircraft and lost signals,
emits events that appear in the feed and on the map.

### Phase 3: Multi-Entity + Maritime Patterns (2-3 weeks)

**Goal:** Cross-entity pattern detection and maritime-specific patterns.

Tasks:
1. Implement spatial indexing in TrailBuffer (geohash grid for neighbor lookups)
2. Implement multi-entity detectors:
   - **Formation Flying**
   - **Convergence**
   - **Rendezvous**
3. Implement maritime detectors:
   - **Dark Running** (AIS off/on)
   - **Ship-to-Ship Transfer**
   - **Station Keeping**
   - **Port Loitering**
4. Implement geospatial detectors:
   - **Boundary Crossing** (using zones table)
   - **Base Approach** (using military-bases.geojson)
5. Frontend: pattern visualization layers (circles, labels)
6. Frontend: pattern detail in SituationDrawer

**Deliverable:** Multi-aircraft formations detected, vessel dark running
flagged, boundary crossing alerts.

### Phase 4: Dynamic Configuration + Advanced Patterns (2-3 weeks)

**Goal:** Runtime configuration, watchlists, and remaining patterns.

Tasks:
1. Implement `PatternConfig` with API endpoint
2. Implement watchlist functionality (per-entity sensitivity)
3. Implement sensitivity zones (severity boost near conflict areas)
4. Implement remaining detectors:
   - **Racetrack Orbit** (ISR-specific)
   - **Holding Pattern** (aviation-specific)
   - **Altitude Anomaly** (rapid descent, terrain following)
   - **Squawk Change** (transponder code transitions)
   - **Escort Pattern**
   - **Area Denial**
   - **NOTAM Violation**
   - **Conflict Zone Entry**
   - **Chokepoint Anomaly**
   - **Restricted Fishing** (IUU)
5. Pattern-to-situation linking (auto-correlate with nearby conflict events)
6. Entity graph integration (activity relationships)
7. Frontend: settings page for pattern configuration
8. Frontend: pattern-specific map icons and legends

**Deliverable:** Full pattern detection suite with dynamic tuning,
watchlists, and complete frontend integration.

---

## Appendix A: Geo Utilities

Shared haversine and centroid functions used by multiple detectors:

```rust
/// Haversine distance in kilometers between two lat/lon points.
pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R * c
}

/// Compute the centroid (average lat/lon) of a set of positions.
pub fn compute_centroid(positions: &[Position]) -> (f64, f64) {
    let n = positions.len() as f64;
    let lat_sum: f64 = positions.iter().map(|p| p.latitude).sum();
    let lon_sum: f64 = positions.iter().map(|p| p.longitude).sum();
    (lat_sum / n, lon_sum / n)
}

/// Compute the bearing (degrees) from point A to point B.
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let y = dlon.sin() * lat2.to_radians().cos();
    let x = lat1.to_radians().cos() * lat2.to_radians().sin()
        - lat1.to_radians().sin() * lat2.to_radians().cos() * dlon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Standard deviation of distances from a centroid.
pub fn position_spread_km(positions: &[Position], centroid: (f64, f64)) -> f64 {
    let distances: Vec<f64> = positions.iter()
        .map(|p| haversine_km(p.latitude, p.longitude, centroid.0, centroid.1))
        .collect();
    let mean = distances.iter().sum::<f64>() / distances.len() as f64;
    let variance = distances.iter()
        .map(|d| (d - mean).powi(2))
        .sum::<f64>() / distances.len() as f64;
    variance.sqrt()
}
```

## Appendix B: Data Flow Diagram

```
AirplanesLive ──┐
adsb.lol ───────┤
adsb.fi ────────┤  broadcast::Sender<InsertableEvent>
OpenSky ────────┤           |
AIS (stream) ───┘           |
                            ├──> events table (existing)
                            ├──> latest_positions (existing)
                            ├──> position_history (NEW)
                            ├──> CorrelationWindow (existing)
                            ├──> SituationGraph (existing)
                            └──> PatternEngine (NEW)
                                      |
                                      ├──> TrailBuffer (in-memory)
                                      ├──> PatternRegistry.detect()
                                      ├──> active_patterns table (NEW)
                                      └──> emit PositionPattern events
                                                |
                                                ├──> SSE stream
                                                ├──> SituationGraph
                                                └──> AlertEngine
```

## Appendix C: Example Detected Pattern Output

```json
{
    "pattern_type": "loitering",
    "entity_id": "ae1460",
    "participant_ids": [],
    "started_at": "2026-03-04T14:23:00Z",
    "confidence": 0.85,
    "severity": "high",
    "center": [34.82, 35.15],
    "radius_km": 18.3,
    "parameters": {
        "orbit_radius_km": 18.3,
        "duration_minutes": 47,
        "avg_speed_kts": 280,
        "positions_in_pattern": 24,
        "avg_altitude_ft": 42000,
        "orbit_direction": "clockwise",
        "sensitivity_zone": "Syria / Iraq border",
        "severity_boost": 1
    },
    "title": "FORTE12 (RQ-4) loitering 18km orbit for 47min near Deir ez-Zor",
    "description": "RQ-4 Global Hawk ISR aircraft maintaining clockwise orbit at FL420. Pattern detected within Syria/Iraq border sensitivity zone. 3 FIRMS thermal anomalies within orbit radius in past 2 hours."
}
```
