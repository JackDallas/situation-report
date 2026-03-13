-- Disable sources with persistent failures and marginal OSINT value.
-- CertStream: 80+ consecutive failures, internal retry bypasses health tracking.
-- ShodanStream: 80+ failures, requires paid streaming plan we don't have.
-- Both can be re-enabled via source_config UPDATE if needed.

INSERT INTO source_config (source_id, enabled) VALUES ('certstream', false)
    ON CONFLICT (source_id) DO UPDATE SET enabled = false;

INSERT INTO source_config (source_id, enabled) VALUES ('shodan-stream', false)
    ON CONFLICT (source_id) DO UPDATE SET enabled = false;
