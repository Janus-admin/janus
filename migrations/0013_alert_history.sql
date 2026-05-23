-- Migration 0013: Create alert_history table.
-- Tracks every time an alert fired: the measured value, the delivery outcome,
-- and any error message from the webhook POST.

CREATE TABLE alert_history (
    id           UUID        PRIMARY KEY,
    alert_id     UUID        NOT NULL REFERENCES alerts(id) ON DELETE CASCADE,
    triggered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    value        DECIMAL(12,8),
    message      TEXT,
    delivered    BOOLEAN     NOT NULL DEFAULT FALSE,
    error        TEXT
);

CREATE INDEX idx_alert_history_alert ON alert_history(alert_id, triggered_at DESC);
