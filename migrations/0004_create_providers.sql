-- Migration 0004: Create providers table + seed initial provider configs
-- Provider API keys are encrypted with AES-256-GCM at rest.

CREATE TABLE providers (
    id                  VARCHAR(50)     PRIMARY KEY,   -- openai | anthropic | bedrock
    display_name        VARCHAR(100)    NOT NULL,
    is_enabled          BOOLEAN         NOT NULL DEFAULT TRUE,
    priority            INTEGER         NOT NULL DEFAULT 1,  -- lower = higher priority
    api_key_encrypted   TEXT,                                -- AES-256-GCM(key) as base64
    base_url            TEXT            NOT NULL,
    timeout_ms          INTEGER         NOT NULL DEFAULT 30000,
    max_retries         INTEGER         NOT NULL DEFAULT 3,
    retry_delay_ms      INTEGER         NOT NULL DEFAULT 1000,
    health_status       VARCHAR(20)     NOT NULL DEFAULT 'unknown',
    last_health_check   TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ     NOT NULL
);

-- Seed: initial provider configurations
-- API keys are intentionally empty — must be configured via velox.toml or env vars
INSERT INTO providers (id, display_name, is_enabled, priority, base_url, timeout_ms, max_retries, updated_at) VALUES
('openai',    'OpenAI',    TRUE, 1, 'https://api.openai.com/v1',      30000, 3, NOW()),
('anthropic', 'Anthropic', TRUE, 2, 'https://api.anthropic.com',      30000, 3, NOW()),
('bedrock',   'AWS Bedrock',TRUE, 3, 'https://bedrock-runtime.{region}.amazonaws.com', 30000, 2, NOW());
