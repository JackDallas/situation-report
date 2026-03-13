-- 5-minute buckets (real-time tier)
CREATE MATERIALIZED VIEW IF NOT EXISTS events_5min
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('5 minutes', event_time) AS bucket,
    source_type,
    region_code,
    severity,
    COUNT(*) AS event_count,
    COUNT(DISTINCT entity_id) AS unique_entities
FROM events
GROUP BY bucket, source_type, region_code, severity
WITH NO DATA;

SELECT add_continuous_aggregate_policy('events_5min',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute',
    if_not_exists => true);

-- 15-minute buckets (tactical tier)
CREATE MATERIALIZED VIEW IF NOT EXISTS events_15min
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('15 minutes', event_time) AS bucket,
    source_type,
    region_code,
    severity,
    COUNT(*) AS event_count,
    COUNT(DISTINCT entity_id) AS unique_entities,
    AVG(confidence) AS avg_confidence
FROM events
GROUP BY bucket, source_type, region_code, severity
WITH NO DATA;

SELECT add_continuous_aggregate_policy('events_15min',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '5 minutes',
    schedule_interval => INTERVAL '5 minutes',
    if_not_exists => true);
