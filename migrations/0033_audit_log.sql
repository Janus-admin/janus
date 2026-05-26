-- Migration 0033: SOC2 audit log for the enterprise edition.
--
-- The table is always created (community + enterprise builds share the same
-- schema), but only the enterprise binary ever writes rows to it.
-- This avoids conditional migrations and makes upgrades seamless.

CREATE TABLE IF NOT EXISTS audit_events (
    id              UUID        PRIMARY KEY,
    workspace_id    UUID        REFERENCES workspaces(id) ON DELETE SET NULL,
    actor_user_id   UUID        REFERENCES users(id) ON DELETE SET NULL,
    actor_email     TEXT,
    action          TEXT        NOT NULL,       -- e.g. "key.create", "member.remove"
    resource_type   TEXT        NOT NULL,       -- e.g. "api_key", "workspace_member"
    resource_id     TEXT,                       -- UUID or slug of affected resource
    metadata        JSONB       NOT NULL DEFAULT '{}',
    ip_address      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_events_created_at   ON audit_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_actor        ON audit_events(actor_user_id);
CREATE INDEX IF NOT EXISTS idx_audit_events_workspace    ON audit_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_audit_events_action       ON audit_events(action);
