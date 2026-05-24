-- Phase V2-6: Distributed rate limiting table for multi-node deployments.
-- One row per request, used to enforce sliding-window rate limits globally.
-- Cleaned up by a background task (rows older than 2× window are deleted).

CREATE TABLE rate_limit_windows (
    api_key_id   UUID        NOT NULL,
    request_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tokens       INTEGER     NOT NULL DEFAULT 0
);

CREATE INDEX idx_rate_limit_windows_key
    ON rate_limit_windows(api_key_id, request_at DESC);
