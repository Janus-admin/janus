-- Migration 0007: Create cache_entries table
-- Stores both exact-match and semantic cache entries.
-- The HNSW vector index lives in memory — this table is for persistence/recovery.

CREATE TABLE cache_entries (
    id                  UUID            PRIMARY KEY,
    prompt_hash         VARCHAR(64)     NOT NULL UNIQUE,    -- SHA-256 hex of normalized prompt
    embedding           BYTEA,                               -- serialized f32[] vector (NULL until Phase 5)
    provider            VARCHAR(50)     NOT NULL,
    model               VARCHAR(150)    NOT NULL,
    request_body        TEXT            NOT NULL,
    response_body       TEXT            NOT NULL,

    -- Token/cost tracking for savings calculation
    prompt_tokens       INTEGER,
    completion_tokens   INTEGER,
    cost_usd            DECIMAL(12,8),

    -- Usage statistics
    hit_count           INTEGER         NOT NULL DEFAULT 0,
    tokens_saved        BIGINT          NOT NULL DEFAULT 0,
    cost_saved          DECIMAL(12,8)   NOT NULL DEFAULT 0,

    -- Lifecycle
    created_at          TIMESTAMPTZ     NOT NULL,
    last_hit_at         TIMESTAMPTZ,
    expires_at          TIMESTAMPTZ                          -- NULL = no expiry
);

CREATE INDEX idx_cache_hash    ON cache_entries(prompt_hash);
CREATE INDEX idx_cache_expiry  ON cache_entries(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_cache_model   ON cache_entries(provider, model);
