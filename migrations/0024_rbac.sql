-- Migration 0024: RBAC — roles and workspace membership (V4-8)

CREATE TABLE roles (
    id   UUID        PRIMARY KEY,
    name VARCHAR(50) NOT NULL UNIQUE
);

-- Four built-in roles (hierarchy: admin > api_manager > billing_viewer > read_only)
INSERT INTO roles (id, name) VALUES
    (gen_random_uuid(), 'admin'),
    (gen_random_uuid(), 'api_manager'),
    (gen_random_uuid(), 'billing_viewer'),
    (gen_random_uuid(), 'read_only');

CREATE TABLE workspace_members (
    id           UUID        PRIMARY KEY,
    workspace_id UUID        NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      UUID        NOT NULL REFERENCES users(id)      ON DELETE CASCADE,
    role_id      UUID        NOT NULL REFERENCES roles(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(workspace_id, user_id)
);
CREATE INDEX idx_workspace_members_user ON workspace_members(user_id);
CREATE INDEX idx_workspace_members_workspace ON workspace_members(workspace_id);

-- Existing users get admin role on all existing workspaces.
-- This ensures continuity: no one loses access after the migration.
INSERT INTO workspace_members (id, workspace_id, user_id, role_id, created_at)
SELECT
    gen_random_uuid(),
    w.id,
    u.id,
    r.id,
    NOW()
FROM workspaces w
CROSS JOIN users u
CROSS JOIN roles r
WHERE r.name = 'admin'
ON CONFLICT DO NOTHING;
