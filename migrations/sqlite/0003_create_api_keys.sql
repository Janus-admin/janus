-- SQLite migration 0003: Create api_keys table
-- TEXT[]   → TEXT  (JSON array string, encoded/decoded in Rust)
-- DECIMAL  → TEXT  (rust_decimal stored as its Display string)
-- BOOLEAN  → INTEGER (0/1)
-- TIMESTAMPTZ → TEXT (ISO 8601 UTC)

CREATE TABLE api_keys (
    id              TEXT    NOT NULL PRIMARY KEY,
    name            TEXT    NOT NULL,
    key_hash        TEXT    NOT NULL UNIQUE,
    key_prefix      TEXT    NOT NULL,
    workspace_id    TEXT    REFERENCES workspaces(id) ON DELETE CASCADE,

    budget_limit    TEXT,                   -- NULL = unlimited
    budget_used     TEXT    NOT NULL DEFAULT '0',

    rate_limit_rpm  INTEGER,
    rate_limit_tpm  INTEGER,

    allowed_models  TEXT,                   -- NULL = all models; or JSON array

    is_active       INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT    NOT NULL,
    expires_at      TEXT,
    last_used_at    TEXT
);

CREATE INDEX idx_api_keys_key_hash  ON api_keys(key_hash);
CREATE INDEX idx_api_keys_workspace ON api_keys(workspace_id);
CREATE INDEX idx_api_keys_is_active ON api_keys(is_active);
