-- SQLite migration 0033: SOC2 audit log (enterprise edition).
CREATE TABLE IF NOT EXISTS audit_events (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
    actor_user_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    actor_email     TEXT,
    action          TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_id     TEXT,
    metadata        TEXT NOT NULL DEFAULT '{}',
    ip_address      TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_audit_events_created_at ON audit_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_actor      ON audit_events(actor_user_id);
CREATE INDEX IF NOT EXISTS idx_audit_events_workspace  ON audit_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_audit_events_action     ON audit_events(action);
