-- Phase V2-6: Distributed rate limiting table (SQLite variant).
-- NOTE: cluster mode is only meaningful on PostgreSQL deployments;
-- this migration exists so SQLite schema stays in sync.

CREATE TABLE rate_limit_windows (
    api_key_id   TEXT    NOT NULL,
    request_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    tokens       INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_rate_limit_windows_key
    ON rate_limit_windows(api_key_id, request_at DESC);
