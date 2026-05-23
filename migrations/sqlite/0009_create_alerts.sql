-- SQLite migration 0009: Create alerts table
-- DECIMAL → TEXT; BOOLEAN → INTEGER; TIMESTAMPTZ → TEXT; UUID → TEXT.

CREATE TABLE alerts (
    id             TEXT    NOT NULL PRIMARY KEY,
    workspace_id   TEXT    REFERENCES workspaces(id) ON DELETE CASCADE,
    name           TEXT    NOT NULL,
    type           TEXT    NOT NULL,
    threshold      TEXT    NOT NULL,
    window_minutes INTEGER NOT NULL DEFAULT 60,
    is_active      INTEGER NOT NULL DEFAULT 1,
    last_triggered TEXT,
    created_at     TEXT    NOT NULL
);

CREATE INDEX idx_alerts_workspace ON alerts(workspace_id);
CREATE INDEX idx_alerts_active    ON alerts(is_active) WHERE is_active = 1;
