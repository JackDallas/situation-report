-- Unified event log (all sources write here)
CREATE TABLE events (
    id BIGSERIAL PRIMARY KEY,
    source_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    occurred_at TIMESTAMPTZ NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    data JSONB NOT NULL,
    region TEXT
);
CREATE INDEX idx_events_source ON events(source_id, ingested_at DESC);
CREATE INDEX idx_events_region ON events(region, ingested_at DESC);
CREATE INDEX idx_events_type ON events(event_type, ingested_at DESC);

-- Source configuration (managed via settings UI)
CREATE TABLE source_config (
    source_id TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT true,
    poll_interval_secs INTEGER,
    api_key_encrypted TEXT,
    extra_config JSONB DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Source health tracking
CREATE TABLE source_health (
    source_id TEXT PRIMARY KEY,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    last_error TEXT,
    consecutive_failures INTEGER DEFAULT 0,
    total_events_24h INTEGER DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'unknown'
);
