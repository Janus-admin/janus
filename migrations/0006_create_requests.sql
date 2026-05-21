-- Migration 0006: Create requests table
-- Every LLM request through the gateway is logged here.
-- This is the core audit log and the source for all analytics.

CREATE TABLE requests (
    id                  UUID            PRIMARY KEY,
    api_key_id          UUID            REFERENCES api_keys(id) ON DELETE SET NULL,
    workspace_id        UUID            REFERENCES workspaces(id) ON DELETE SET NULL,

    -- Provider routing
    provider            VARCHAR(50)     NOT NULL,
    model               VARCHAR(150)    NOT NULL,
    base_url            TEXT,

    -- Token usage
    prompt_tokens       INTEGER,
    completion_tokens   INTEGER,
    total_tokens        INTEGER,

    -- Cost (DECIMAL for precision — see DECISIONS.md D-010)
    cost_usd            DECIMAL(12,8),

    -- Performance
    latency_ms          INTEGER,        -- total round trip time
    ttfb_ms             INTEGER,        -- time to first byte (streaming only)

    -- Status
    status              VARCHAR(20)     NOT NULL,   -- success | error | cached
    cache_type          VARCHAR(20),                -- exact | semantic | NULL
    cache_similarity    DECIMAL(5,4),               -- 0.0000 to 1.0000, NULL if not semantic
    http_status         INTEGER,
    error_code          VARCHAR(100),
    error_message       TEXT,

    -- Payload logging (NULL by default — enabled per config)
    request_body        TEXT,
    response_body       TEXT,

    -- Metadata
    stream              BOOLEAN         NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ     NOT NULL
);

-- Indexes for common dashboard queries
-- Note: index on created_at DESC for time-series queries
CREATE INDEX idx_requests_created_at        ON requests(created_at DESC);
CREATE INDEX idx_requests_api_key_time      ON requests(api_key_id, created_at DESC);
CREATE INDEX idx_requests_workspace_time    ON requests(workspace_id, created_at DESC);
CREATE INDEX idx_requests_provider_model    ON requests(provider, model);
CREATE INDEX idx_requests_status            ON requests(status);
CREATE INDEX idx_requests_cache_type        ON requests(cache_type) WHERE cache_type IS NOT NULL;
