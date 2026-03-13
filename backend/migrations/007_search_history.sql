CREATE TABLE IF NOT EXISTS situation_search_history (
    situation_id    UUID NOT NULL,
    gap_type        TEXT NOT NULL,
    last_searched_at TIMESTAMPTZ NOT NULL,
    total_searches  INT NOT NULL DEFAULT 1,
    empty_searches  INT NOT NULL DEFAULT 0,
    PRIMARY KEY (situation_id, gap_type)
);
