-- Migration 0024: RBAC — roles and workspace membership (V4-8, SQLite)

CREATE TABLE roles (
    id   TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

INSERT INTO roles (id, name) VALUES
    (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))), 'admin'),
    (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))), 'api_manager'),
    (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))), 'billing_viewer'),
    (lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))), 'read_only');

CREATE TABLE workspace_members (
    id           TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(id)      ON DELETE CASCADE,
    role_id      TEXT NOT NULL REFERENCES roles(id),
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(workspace_id, user_id)
);
CREATE INDEX idx_workspace_members_user      ON workspace_members(user_id);
CREATE INDEX idx_workspace_members_workspace ON workspace_members(workspace_id);

-- Existing users get admin role on all existing workspaces.
INSERT OR IGNORE INTO workspace_members (id, workspace_id, user_id, role_id, created_at)
SELECT
    lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))),
    w.id,
    u.id,
    r.id,
    datetime('now')
FROM workspaces w
CROSS JOIN users u
CROSS JOIN roles r
WHERE r.name = 'admin';
