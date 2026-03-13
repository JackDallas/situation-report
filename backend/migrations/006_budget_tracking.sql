-- Budget tracking: persist daily AI spend so container restarts don't reset the counter.
CREATE TABLE IF NOT EXISTS budget_daily (
    day DATE PRIMARY KEY DEFAULT CURRENT_DATE,
    haiku_input_tokens BIGINT NOT NULL DEFAULT 0,
    haiku_output_tokens BIGINT NOT NULL DEFAULT 0,
    haiku_cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    sonnet_input_tokens BIGINT NOT NULL DEFAULT 0,
    sonnet_output_tokens BIGINT NOT NULL DEFAULT 0,
    sonnet_cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
