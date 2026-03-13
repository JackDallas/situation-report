-- Cumulative situation summaries for long-running situation memory.
-- Stores a rolling summary of key facts, entities, and dates so that
-- narrative generation has continuity across regenerations.
CREATE TABLE IF NOT EXISTS situation_summaries (
    situation_id UUID PRIMARY KEY REFERENCES situations(id),
    summary_text TEXT NOT NULL,
    key_entities JSONB DEFAULT '[]',
    key_dates JSONB DEFAULT '[]',
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
