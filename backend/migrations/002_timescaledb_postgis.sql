-- 002_timescaledb_postgis.sql
-- Breaking migration: PostgreSQL 17 + TimescaleDB + PostGIS
-- Replaces the basic events table with a unified hypertable.

-- ==========================================================================
-- 1. Enable extensions
-- ==========================================================================
CREATE EXTENSION IF NOT EXISTS timescaledb;
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- ==========================================================================
-- 2. Migrate events table
-- ==========================================================================

-- Rename old events table to preserve data
ALTER TABLE IF EXISTS events RENAME TO events_old;

-- Drop old indexes (they reference the old table name)
DROP INDEX IF EXISTS idx_events_source;
DROP INDEX IF EXISTS idx_events_region;
DROP INDEX IF EXISTS idx_events_type;

-- Create new events table with full schema
CREATE TABLE events (
    -- Temporal
    event_time      TIMESTAMPTZ     NOT NULL,
    ingested_at     TIMESTAMPTZ     NOT NULL DEFAULT NOW(),

    -- Source identification
    source_type     TEXT            NOT NULL,
    source_id       TEXT,

    -- Geospatial
    location        GEOGRAPHY(POINT, 4326),
    geometry        GEOGRAPHY,
    region_code     TEXT,

    -- Entity tracking
    entity_id       TEXT,
    entity_name     TEXT,

    -- Classification
    event_type      TEXT,
    severity        TEXT            DEFAULT 'low',
    confidence      REAL            DEFAULT 1.0,
    tags            TEXT[],

    -- Content
    title           TEXT,
    description     TEXT,

    -- Source-specific payload
    payload         JSONB           NOT NULL DEFAULT '{}'::jsonb
);

-- Convert to TimescaleDB hypertable with 1-day chunks
SELECT create_hypertable('events', 'event_time',
    chunk_time_interval => INTERVAL '1 day'
);

-- ==========================================================================
-- 3. Indexes
-- ==========================================================================

-- Primary geospatial index (GIST on geography column)
CREATE INDEX idx_events_location ON events USING GIST (location);

-- Compound time + source filtering (most common dashboard query pattern)
CREATE INDEX idx_events_source_time ON events (source_type, event_time DESC);

-- Entity tracking (find all positions for a specific aircraft/ship)
CREATE INDEX idx_events_entity_time ON events (entity_id, event_time DESC)
    WHERE entity_id IS NOT NULL;

-- Region-level filtering
CREATE INDEX idx_events_region_time ON events (region_code, event_time DESC)
    WHERE region_code IS NOT NULL;

-- Severity filtering for alerts
CREATE INDEX idx_events_severity ON events (severity, event_time DESC)
    WHERE severity IN ('high', 'critical');

-- JSONB payload queries (GIN index)
CREATE INDEX idx_events_payload ON events USING GIN (payload);

-- Tags array queries (GIN index)
CREATE INDEX idx_events_tags ON events USING GIN (tags);

-- Deduplication index
CREATE UNIQUE INDEX idx_events_dedup ON events (source_type, source_id, event_time)
    WHERE source_id IS NOT NULL;

-- ==========================================================================
-- 4. Migrate existing data (best-effort)
-- ==========================================================================
INSERT INTO events (event_time, ingested_at, source_type, source_id, location, region_code, event_type, payload)
SELECT
    occurred_at,
    ingested_at,
    source_id,
    NULL,
    CASE
        WHEN latitude IS NOT NULL AND longitude IS NOT NULL
        THEN ST_SetSRID(ST_MakePoint(longitude, latitude), 4326)::geography
        ELSE NULL
    END,
    region,
    event_type,
    data
FROM events_old
ON CONFLICT DO NOTHING;

-- Drop old table
DROP TABLE IF EXISTS events_old;

-- ==========================================================================
-- 5. Latest positions table
-- ==========================================================================
CREATE TABLE IF NOT EXISTS latest_positions (
    entity_id       TEXT            PRIMARY KEY,
    source_type     TEXT            NOT NULL,
    entity_name     TEXT,
    location        GEOGRAPHY(POINT, 4326) NOT NULL,
    heading         REAL,
    speed           REAL,
    altitude        REAL,
    last_seen       TIMESTAMPTZ     NOT NULL,
    payload         JSONB           NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_latest_pos_location ON latest_positions USING GIST (location);
CREATE INDEX IF NOT EXISTS idx_latest_pos_source ON latest_positions (source_type, last_seen DESC);

-- ==========================================================================
-- 6. Zones table
-- ==========================================================================
CREATE TABLE IF NOT EXISTS zones (
    id              SERIAL PRIMARY KEY,
    name            TEXT            NOT NULL,
    zone_type       TEXT            NOT NULL,
    geometry        GEOGRAPHY       NOT NULL,
    properties      JSONB           DEFAULT '{}'::jsonb,
    valid_from      TIMESTAMPTZ,
    valid_until     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ     DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_zones_geometry ON zones USING GIST (geometry);
CREATE INDEX IF NOT EXISTS idx_zones_type ON zones (zone_type);

-- ==========================================================================
-- 7. Continuous aggregates
-- ==========================================================================

-- Hourly event counts by source and region
CREATE MATERIALIZED VIEW IF NOT EXISTS events_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)   AS bucket,
    source_type,
    region_code,
    severity,
    COUNT(*)                            AS event_count,
    COUNT(DISTINCT entity_id)           AS unique_entities
FROM events
GROUP BY bucket, source_type, region_code, severity
WITH NO DATA;

SELECT add_continuous_aggregate_policy('events_hourly',
    start_offset    => INTERVAL '3 days',
    end_offset      => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists   => true
);

-- Daily summary
CREATE MATERIALIZED VIEW IF NOT EXISTS events_daily
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 day', event_time)    AS bucket,
    source_type,
    region_code,
    COUNT(*)                            AS event_count,
    COUNT(DISTINCT entity_id)           AS unique_entities,
    AVG(confidence)                     AS avg_confidence
FROM events
GROUP BY bucket, source_type, region_code
WITH NO DATA;

SELECT add_continuous_aggregate_policy('events_daily',
    start_offset    => INTERVAL '3 days',
    end_offset      => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour',
    if_not_exists   => true
);

-- Anomaly detection baseline
CREATE MATERIALIZED VIEW IF NOT EXISTS anomaly_baseline
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', event_time)                       AS bucket,
    source_type,
    region_code,
    EXTRACT(DOW FROM event_time)::int                       AS day_of_week,
    COUNT(*)                                                AS event_count
FROM events
GROUP BY bucket, source_type, region_code, EXTRACT(DOW FROM event_time)::int
WITH NO DATA;

SELECT add_continuous_aggregate_policy('anomaly_baseline',
    start_offset    => INTERVAL '7 days',
    end_offset      => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour',
    if_not_exists   => true
);

-- ==========================================================================
-- 8. Compression and retention policies
-- ==========================================================================
ALTER TABLE events SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'source_type, region_code',
    timescaledb.compress_orderby = 'event_time DESC'
);

SELECT add_compression_policy('events', INTERVAL '7 days', if_not_exists => true);

SELECT add_retention_policy('events', INTERVAL '180 days', if_not_exists => true);

-- Keep continuous aggregates longer
SELECT add_retention_policy('events_hourly', INTERVAL '2 years', if_not_exists => true);
SELECT add_retention_policy('events_daily', INTERVAL '5 years', if_not_exists => true);

-- ==========================================================================
-- 9. LISTEN/NOTIFY trigger for high-severity events
-- ==========================================================================
CREATE OR REPLACE FUNCTION notify_new_event()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('new_event', json_build_object(
        'event_time', NEW.event_time,
        'source_type', NEW.source_type,
        'entity_id', NEW.entity_id,
        'severity', NEW.severity,
        'title', NEW.title,
        'lat', CASE WHEN NEW.location IS NOT NULL THEN ST_Y(NEW.location::geometry) ELSE NULL END,
        'lon', CASE WHEN NEW.location IS NOT NULL THEN ST_X(NEW.location::geometry) ELSE NULL END
    )::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_notify_event
    AFTER INSERT ON events
    FOR EACH ROW
    WHEN (NEW.severity IN ('high', 'critical'))
    EXECUTE FUNCTION notify_new_event();
