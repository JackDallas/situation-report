-- Position history for flight/vessel trail tracking
CREATE TABLE IF NOT EXISTS position_history (
    entity_id   TEXT            NOT NULL,
    source_type TEXT            NOT NULL,
    latitude    DOUBLE PRECISION NOT NULL,
    longitude   DOUBLE PRECISION NOT NULL,
    heading     REAL,
    speed       REAL,
    altitude    REAL,
    recorded_at TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    metadata    JSONB           NOT NULL DEFAULT '{}'::jsonb
);

-- Convert to hypertable (1-hour chunks for high-frequency position data)
SELECT create_hypertable('position_history', 'recorded_at',
    chunk_time_interval => INTERVAL '1 hour',
    if_not_exists => TRUE);

-- Index for querying trails per entity
CREATE INDEX idx_pos_history_entity_time
    ON position_history (entity_id, recorded_at DESC);

-- Retention policy: keep 24 hours of position data
SELECT add_retention_policy('position_history', INTERVAL '24 hours', if_not_exists => TRUE);

-- Compression after 2 hours
ALTER TABLE position_history SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'entity_id',
    timescaledb.compress_orderby = 'recorded_at DESC'
);
SELECT add_compression_policy('position_history', INTERVAL '2 hours', if_not_exists => TRUE);
