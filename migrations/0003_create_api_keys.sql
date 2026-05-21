-- Migration 0003: Create api_keys table
-- Gateway API keys (vx-sk-...) — separate from admin user auth.
-- Full key is NEVER stored. Only hashes are kept.

CREATE TABLE api_keys (
    id              UUID            PRIMARY KEY,
    name            VARCHAR(255)    NOT NULL,
    key_hash        VARCHAR(255)    NOT NULL UNIQUE,  -- bcrypt hash
    key_prefix      VARCHAR(16)     NOT NULL,          -- first 12 chars for display
    workspace_id    UUID            REFERENCES workspaces(id) ON DELETE CASCADE,

    -- Spending limits
    budget_limit    DECIMAL(12,8),                     -- NULL = unlimited
    budget_used     DECIMAL(12,8)   NOT NULL DEFAULT 0,

    -- Rate limits
    rate_limit_rpm  INTEGER,                           -- requests per minute, NULL = unlimited
    rate_limit_tpm  INTEGER,                           -- tokens per minute, NULL = unlimited

    -- Restrictions
    allowed_models  TEXT[],                            -- NULL = all models allowed

    -- Lifecycle
    is_active       BOOLEAN         NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ     NOT NULL,
    expires_at      TIMESTAMPTZ,                       -- NULL = no expiry
    last_used_at    TIMESTAMPTZ
);

CREATE INDEX idx_api_keys_key_hash    ON api_keys(key_hash);
CREATE INDEX idx_api_keys_workspace   ON api_keys(workspace_id);
CREATE INDEX idx_api_keys_is_active   ON api_keys(is_active);
