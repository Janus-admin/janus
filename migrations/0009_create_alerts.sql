-- Migration 0009: Create alerts table
-- Threshold-based alerts (spend limits, error rate, latency spikes).
-- Alert firing logic runs in a background task.

CREATE TABLE alerts (
    id              UUID            PRIMARY KEY,
    workspace_id    UUID            REFERENCES workspaces(id) ON DELETE CASCADE,
    name            VARCHAR(255)    NOT NULL,
    type            VARCHAR(50)     NOT NULL,   -- spend_threshold | error_rate | latency_spike
    threshold       DECIMAL(12,4)   NOT NULL,   -- meaning depends on type
    window_minutes  INTEGER         NOT NULL DEFAULT 60,
    is_active       BOOLEAN         NOT NULL DEFAULT TRUE,
    last_triggered  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ     NOT NULL
);

CREATE INDEX idx_alerts_workspace ON alerts(workspace_id);
CREATE INDEX idx_alerts_active    ON alerts(is_active) WHERE is_active = TRUE;
