-- 005_entity_graph.sql
-- Entity knowledge graph, situation lifecycle, and alert rules

-- Requires: pg_trgm for fuzzy matching, ltree for hierarchy
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS ltree;

-- ---------------------------------------------------------------------------
-- Entity knowledge graph
-- ---------------------------------------------------------------------------

CREATE TABLE entities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type     TEXT NOT NULL,  -- person, organization, location, weapon_system, military_unit, facility
    canonical_name  TEXT NOT NULL,
    aliases         TEXT[] NOT NULL DEFAULT '{}',
    wikidata_id     TEXT,
    properties      JSONB NOT NULL DEFAULT '{}',
    location        GEOGRAPHY(Point, 4326),
    embedding       vector(512),
    status          TEXT DEFAULT 'active',
    confidence      REAL NOT NULL DEFAULT 0.5,
    mention_count   INTEGER NOT NULL DEFAULT 0,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_enriched_at TIMESTAMPTZ
);

-- Fuzzy name matching via trigrams
CREATE INDEX idx_entities_name_trgm ON entities USING gin (canonical_name gin_trgm_ops);
-- Vector similarity search on entity embeddings
CREATE INDEX idx_entities_embedding ON entities USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
-- Array search on aliases
CREATE INDEX idx_entities_aliases ON entities USING gin (aliases);
-- Lookup by type and status
CREATE INDEX idx_entities_type_status ON entities (entity_type, status);
-- Lookup by wikidata ID
CREATE INDEX idx_entities_wikidata ON entities (wikidata_id) WHERE wikidata_id IS NOT NULL;

CREATE TABLE entity_relationships (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_entity   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    target_entity   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relationship    TEXT NOT NULL,  -- leadership, membership, alliance, rivalry, geographic_association, supply_chain, family, sponsorship
    properties      JSONB NOT NULL DEFAULT '{}',
    confidence      REAL NOT NULL DEFAULT 0.5,
    evidence_count  INTEGER NOT NULL DEFAULT 1,
    first_seen_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_active       BOOLEAN DEFAULT true,
    UNIQUE(source_entity, target_entity, relationship)
);

CREATE INDEX idx_entity_rel_source ON entity_relationships (source_entity) WHERE is_active;
CREATE INDEX idx_entity_rel_target ON entity_relationships (target_entity) WHERE is_active;
CREATE INDEX idx_entity_rel_type ON entity_relationships (relationship);

CREATE TABLE entity_state_changes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id       UUID NOT NULL REFERENCES entities(id),
    change_type     TEXT NOT NULL,  -- killed, arrested, promoted, resigned, sanctioned, relocated
    previous_state  JSONB,
    new_state       JSONB NOT NULL,
    certainty       TEXT NOT NULL DEFAULT 'alleged',  -- confirmed, alleged, denied, rumored
    source_reliability CHAR(1) DEFAULT 'F',  -- Admiralty A-F
    info_credibility   CHAR(1) DEFAULT '6',  -- Admiralty 1-6
    triggering_event_id UUID,
    detected_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_entity_state_entity ON entity_state_changes (entity_id, detected_at DESC);
CREATE INDEX idx_entity_state_type ON entity_state_changes (change_type, detected_at DESC);

-- Link events to entities (many-to-many)
CREATE TABLE event_entities (
    event_id        UUID NOT NULL,
    entity_id       UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    role            TEXT NOT NULL DEFAULT 'mentioned',  -- actor, target, location, mentioned
    confidence      REAL NOT NULL DEFAULT 0.5,
    PRIMARY KEY (event_id, entity_id)
);

CREATE INDEX idx_event_entities_entity ON event_entities (entity_id);

-- ---------------------------------------------------------------------------
-- Situation lifecycle
-- ---------------------------------------------------------------------------

CREATE TYPE situation_phase AS ENUM ('emerging','developing','active','declining','resolved','historical');

CREATE TABLE situations (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title               TEXT NOT NULL,
    phase               situation_phase NOT NULL DEFAULT 'emerging',
    phase_changed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    hierarchy_path      ltree,
    location            GEOGRAPHY(Geometry, 4326),
    embedding           vector(512),
    event_count_5m      INT DEFAULT 0,
    event_count_30m     INT DEFAULT 0,
    peak_event_rate     FLOAT DEFAULT 0,
    current_event_rate  FLOAT DEFAULT 0,
    source_diversity    INT DEFAULT 0,
    max_severity        INT DEFAULT 0,
    narrative_text      TEXT,
    narrative_version   INT NOT NULL DEFAULT 0,
    properties          JSONB NOT NULL DEFAULT '{}',
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at         TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_situations_phase ON situations (phase) WHERE phase != 'historical';
CREATE INDEX idx_situations_updated ON situations (updated_at DESC);
CREATE INDEX idx_situations_hierarchy ON situations USING gist (hierarchy_path);
CREATE INDEX idx_situations_location ON situations USING gist (location);
CREATE INDEX idx_situations_embedding ON situations USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);

CREATE TABLE situation_phase_transitions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    situation_id    UUID NOT NULL REFERENCES situations(id),
    from_phase      situation_phase NOT NULL,
    to_phase        situation_phase NOT NULL,
    trigger_reason  TEXT NOT NULL,
    metrics_snapshot JSONB NOT NULL,
    transitioned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sit_transitions_sit ON situation_phase_transitions (situation_id, transitioned_at DESC);

-- Link situations to events
CREATE TABLE situation_events (
    situation_id    UUID NOT NULL REFERENCES situations(id) ON DELETE CASCADE,
    event_id        UUID NOT NULL,
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (situation_id, event_id)
);

CREATE INDEX idx_situation_events_event ON situation_events (event_id);

-- Link situations to entities
CREATE TABLE situation_entities (
    situation_id    UUID NOT NULL REFERENCES situations(id) ON DELETE CASCADE,
    entity_id       UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    role            TEXT NOT NULL DEFAULT 'involved',
    PRIMARY KEY (situation_id, entity_id)
);

CREATE INDEX idx_situation_entities_entity ON situation_entities (entity_id);

-- Versioned narratives for diff-based catch-up
CREATE TABLE situation_narratives (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    situation_id    UUID NOT NULL REFERENCES situations(id),
    version         INT NOT NULL,
    narrative_text  TEXT NOT NULL,
    model           TEXT NOT NULL,
    tokens_used     INT NOT NULL DEFAULT 0,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(situation_id, version)
);

CREATE INDEX idx_sit_narratives ON situation_narratives (situation_id, version DESC);

-- ---------------------------------------------------------------------------
-- Alert rules
-- ---------------------------------------------------------------------------

CREATE TABLE alert_rules (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    rule_type       TEXT NOT NULL,  -- keyword, entity, geo_fence, semantic, anomaly
    conditions      JSONB NOT NULL,
    delivery        JSONB DEFAULT '["sse"]',
    enabled         BOOLEAN DEFAULT true,
    cooldown        INTERVAL NOT NULL DEFAULT '30 minutes',
    max_per_hour    INTEGER DEFAULT 10,
    min_severity    TEXT DEFAULT 'medium',
    last_fired_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX idx_alert_rules_enabled ON alert_rules (rule_type) WHERE enabled;

-- Alert history for fatigue tracking
CREATE TABLE alert_history (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id         UUID REFERENCES alert_rules(id),
    situation_id    UUID REFERENCES situations(id),
    event_id        UUID,
    severity        TEXT NOT NULL,
    title           TEXT NOT NULL,
    body            TEXT,
    delivered_via   TEXT[] NOT NULL DEFAULT '{}',
    fired_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_alert_history_rule ON alert_history (rule_id, fired_at DESC);
CREATE INDEX idx_alert_history_time ON alert_history (fired_at DESC);

-- ---------------------------------------------------------------------------
-- Analytics: additional continuous aggregates
-- ---------------------------------------------------------------------------

-- Full-text search on events (for hybrid vector+lexical search)
ALTER TABLE events ADD COLUMN IF NOT EXISTS content_tsv tsvector;

-- Populate tsvector from title + description
CREATE OR REPLACE FUNCTION events_tsv_trigger() RETURNS trigger AS $$
BEGIN
    NEW.content_tsv := to_tsvector('english', coalesce(NEW.title, '') || ' ' || coalesce(NEW.description, ''));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Note: triggers on hypertables need to be created on the parent table
-- This applies to new inserts only; existing rows can be backfilled with an UPDATE
CREATE TRIGGER events_tsv_update
    BEFORE INSERT OR UPDATE OF title, description ON events
    FOR EACH ROW EXECUTE FUNCTION events_tsv_trigger();

CREATE INDEX idx_events_tsv ON events USING gin (content_tsv);

-- Anomaly baselines table for z-score detection
CREATE TABLE IF NOT EXISTS anomaly_baselines (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    metric_name     TEXT NOT NULL,
    region_code     TEXT,
    source_type     TEXT,
    baseline_mean   DOUBLE PRECISION NOT NULL,
    baseline_stddev DOUBLE PRECISION NOT NULL,
    sample_count    INTEGER NOT NULL DEFAULT 0,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(metric_name, region_code, source_type)
);

-- Intelligence reports
CREATE TABLE intel_reports (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    report_type     TEXT NOT NULL,  -- flash_alert, intrep, intsum, entity_profile, campaign_assessment
    title           TEXT NOT NULL,
    content_json    JSONB NOT NULL,
    content_html    TEXT,
    situation_id    UUID REFERENCES situations(id),
    entity_id       UUID REFERENCES entities(id),
    model           TEXT NOT NULL,
    tokens_used     INT NOT NULL DEFAULT 0,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_intel_reports_type ON intel_reports (report_type, generated_at DESC);
CREATE INDEX idx_intel_reports_situation ON intel_reports (situation_id) WHERE situation_id IS NOT NULL;
CREATE INDEX idx_intel_reports_entity ON intel_reports (entity_id) WHERE entity_id IS NOT NULL;
