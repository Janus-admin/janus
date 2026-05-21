-- Migration 0008: Create daily_costs aggregation table
-- Pre-computed daily cost summaries for fast dashboard queries.
-- Updated by a background task after each request — never queried in the hot path.

CREATE TABLE daily_costs (
    date                DATE            NOT NULL,
    workspace_id        UUID            REFERENCES workspaces(id) ON DELETE CASCADE,
    api_key_id          UUID            REFERENCES api_keys(id) ON DELETE SET NULL,
    provider            VARCHAR(50)     NOT NULL,
    model               VARCHAR(150)    NOT NULL,
    request_count       INTEGER         NOT NULL DEFAULT 0,
    error_count         INTEGER         NOT NULL DEFAULT 0,
    cache_hits          INTEGER         NOT NULL DEFAULT 0,
    prompt_tokens       BIGINT          NOT NULL DEFAULT 0,
    completion_tokens   BIGINT          NOT NULL DEFAULT 0,
    total_cost_usd      DECIMAL(12,8)   NOT NULL DEFAULT 0,
    avg_latency_ms      INTEGER,
    p95_latency_ms      INTEGER
);

CREATE UNIQUE INDEX idx_daily_costs_pk ON daily_costs(
    date, provider, model,
    COALESCE(api_key_id, '00000000-0000-0000-0000-000000000000'::UUID),
    COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::UUID)
);

CREATE INDEX idx_daily_costs_date       ON daily_costs(date DESC);
CREATE INDEX idx_daily_costs_workspace  ON daily_costs(workspace_id, date DESC);
CREATE INDEX idx_daily_costs_key        ON daily_costs(api_key_id, date DESC);
