-- SQLite migration 0013: Create alert_history table.
-- DECIMAL → TEXT; BOOLEAN → INTEGER; TIMESTAMPTZ → TEXT; UUID → TEXT.

CREATE TABLE alert_history (
    id           TEXT    NOT NULL PRIMARY KEY,
    alert_id     TEXT    NOT NULL REFERENCES alerts(id) ON DELETE CASCADE,
    triggered_at TEXT    NOT NULL,
    value        TEXT,
    message      TEXT,
    delivered    INTEGER NOT NULL DEFAULT 0,
    error        TEXT
);

CREATE INDEX idx_alert_history_alert ON alert_history(alert_id, triggered_at);
