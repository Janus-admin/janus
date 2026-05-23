-- SQLite migration 0007: Create cache_entries table
-- BYTEA → BLOB; DECIMAL → TEXT; TIMESTAMPTZ → TEXT; UUID → TEXT.

CREATE TABLE cache_entries (
    id                TEXT    NOT NULL PRIMARY KEY,
    prompt_hash       TEXT    NOT NULL UNIQUE,
    embedding         BLOB,

    provider          TEXT    NOT NULL,
    model             TEXT    NOT NULL,
    request_body      TEXT    NOT NULL,
    response_body     TEXT    NOT NULL,

    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    cost_usd          TEXT,

    hit_count         INTEGER NOT NULL DEFAULT 0,
    tokens_saved      INTEGER NOT NULL DEFAULT 0,
    cost_saved        TEXT    NOT NULL DEFAULT '0',

    created_at        TEXT    NOT NULL,
    last_hit_at       TEXT,
    expires_at        TEXT
);

CREATE INDEX idx_cache_hash   ON cache_entries(prompt_hash);
CREATE INDEX idx_cache_expiry ON cache_entries(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_cache_model  ON cache_entries(provider, model);
