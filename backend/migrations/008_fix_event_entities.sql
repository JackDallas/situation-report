-- 008_fix_event_entities.sql
-- Fix event_entities join table: the events table has no UUID primary key,
-- it uses (source_type, source_id, event_time) as its composite key.
-- Replace the UUID-based event_id with the composite key columns.

-- Drop the old table (it has 0 rows, so no data loss)
DROP TABLE IF EXISTS event_entities;

-- Recreate with composite key matching the events table
CREATE TABLE event_entities (
    source_type TEXT        NOT NULL,
    source_id   TEXT        NOT NULL,
    event_time  TIMESTAMPTZ NOT NULL,
    entity_id   UUID        NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    role        TEXT        NOT NULL DEFAULT 'mentioned',  -- actor, target, location, mentioned
    confidence  REAL        NOT NULL DEFAULT 0.5,
    PRIMARY KEY (source_type, source_id, event_time, entity_id)
);

-- Index for querying all events for a given entity
CREATE INDEX idx_event_entities_entity ON event_entities (entity_id);

-- Index for querying all entities for a given event
CREATE INDEX idx_event_entities_event ON event_entities (source_type, source_id, event_time);
