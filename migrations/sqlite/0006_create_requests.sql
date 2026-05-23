-- SQLite migration 0006: Create requests table
-- DECIMAL → TEXT; UUID → TEXT; BOOLEAN → INTEGER; TIMESTAMPTZ → TEXT.
-- Partial indexes (WHERE ...) supported since SQLite 3.8.0.

CREATE TABLE requests (
    id                TEXT    NOT NULL PRIMARY KEY,
    api_key_id        TEXT    REFERENCES api_keys(id) ON DELETE SET NULL,
    workspace_id      TEXT    REFERENCES workspaces(id) ON DELETE SET NULL,

    provider          TEXT    NOT NULL,
    model             TEXT    NOT NULL,
    base_url          TEXT,

    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    total_tokens      INTEGER,

    cost_usd          TEXT,

    latency_ms        INTEGER,
    ttfb_ms           INTEGER,

    status            TEXT    NOT NULL,
    cache_type        TEXT,
    cache_similarity  TEXT,
    http_status       INTEGER,
    error_code        TEXT,
    error_message     TEXT,

    request_body      TEXT,
    response_body     TEXT,

    stream            INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT    NOT NULL
);

CREATE INDEX idx_requests_created_at     ON requests(created_at DESC);
CREATE INDEX idx_requests_api_key_time   ON requests(api_key_id, created_at DESC);
CREATE INDEX idx_requests_workspace_time ON requests(workspace_id, created_at DESC);
CREATE INDEX idx_requests_provider_model ON requests(provider, model);
CREATE INDEX idx_requests_status         ON requests(status);
CREATE INDEX idx_requests_cache_type     ON requests(cache_type) WHERE cache_type IS NOT NULL;
