-- Correlated incidents from pipeline rules
CREATE TABLE IF NOT EXISTS incidents (
    id UUID PRIMARY KEY,
    rule_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    severity INT NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.0,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    region_code TEXT,
    location GEOGRAPHY(POINT, 4326),
    tags TEXT[] NOT NULL DEFAULT '{}',
    evidence JSONB NOT NULL DEFAULT '[]',
    parent_id UUID REFERENCES incidents(id) ON DELETE SET NULL,
    display_title TEXT
);

CREATE INDEX idx_incidents_first_seen ON incidents (first_seen DESC);
CREATE INDEX idx_incidents_rule_id ON incidents (rule_id);
CREATE INDEX idx_incidents_severity ON incidents (severity DESC);
