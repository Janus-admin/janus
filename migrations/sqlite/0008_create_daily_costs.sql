-- SQLite migration 0008: Create daily_costs aggregation table
-- api_key_id / workspace_id are stored as NOT NULL TEXT (never SQL NULL).
-- The nil UUID '00000000-0000-0000-0000-000000000000' acts as the sentinel for
-- "no api key" / "no workspace", replacing the PG COALESCE(...::UUID) trick.
-- DECIMAL → TEXT; DATE → TEXT.

CREATE TABLE daily_costs (
    date              TEXT    NOT NULL,
    workspace_id      TEXT    NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    api_key_id        TEXT    NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    provider          TEXT    NOT NULL,
    model             TEXT    NOT NULL,
    request_count     INTEGER NOT NULL DEFAULT 0,
    error_count       INTEGER NOT NULL DEFAULT 0,
    cache_hits        INTEGER NOT NULL DEFAULT 0,
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd    TEXT    NOT NULL DEFAULT '0',
    avg_latency_ms    INTEGER,
    p95_latency_ms    INTEGER,

    UNIQUE(date, provider, model, api_key_id, workspace_id)
);

CREATE INDEX idx_daily_costs_date      ON daily_costs(date DESC);
CREATE INDEX idx_daily_costs_workspace ON daily_costs(workspace_id, date DESC);
CREATE INDEX idx_daily_costs_key       ON daily_costs(api_key_id, date DESC);
