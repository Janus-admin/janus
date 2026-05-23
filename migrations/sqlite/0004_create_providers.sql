-- SQLite migration 0004: Create providers table + seed initial configs

CREATE TABLE providers (
    id                VARCHAR(50) NOT NULL PRIMARY KEY,
    display_name      TEXT        NOT NULL,
    is_enabled        INTEGER     NOT NULL DEFAULT 1,
    priority          INTEGER     NOT NULL DEFAULT 1,
    api_key_encrypted TEXT,
    base_url          TEXT        NOT NULL,
    timeout_ms        INTEGER     NOT NULL DEFAULT 30000,
    max_retries       INTEGER     NOT NULL DEFAULT 3,
    retry_delay_ms    INTEGER     NOT NULL DEFAULT 1000,
    health_status     TEXT        NOT NULL DEFAULT 'unknown',
    last_health_check TEXT,
    updated_at        TEXT        NOT NULL
);

INSERT INTO providers (id, display_name, is_enabled, priority, base_url, timeout_ms, max_retries, updated_at) VALUES
('openai',    'OpenAI',     1, 1, 'https://api.openai.com/v1',                              30000, 3, datetime('now')),
('anthropic', 'Anthropic',  1, 2, 'https://api.anthropic.com',                              30000, 3, datetime('now')),
('bedrock',   'AWS Bedrock',1, 3, 'https://bedrock-runtime.{region}.amazonaws.com',         30000, 2, datetime('now'));
