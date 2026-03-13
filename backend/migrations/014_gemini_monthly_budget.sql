-- Persist monthly Gemini spend so container restarts don't reset the $30/month cap.
CREATE TABLE IF NOT EXISTS gemini_monthly_spend (
    month_start DATE PRIMARY KEY,
    spend_micro_usd BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
