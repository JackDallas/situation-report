-- Materialized timeline buckets for situation activity over time.
-- Hourly buckets recording event count, source diversity, and peak severity.
CREATE TABLE IF NOT EXISTS situation_timeline (
    situation_id UUID NOT NULL REFERENCES situations(id),
    bucket TIMESTAMPTZ NOT NULL,
    event_count INT NOT NULL DEFAULT 0,
    source_count INT NOT NULL DEFAULT 0,
    max_severity TEXT NOT NULL DEFAULT 'low',
    PRIMARY KEY (situation_id, bucket)
);
