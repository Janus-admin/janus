-- V5-L2: OIDC Login (SQLite mirror of 0028_oidc.sql)

CREATE TABLE identity_providers (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind = 'oidc'),
    name            TEXT NOT NULL,
    config          TEXT NOT NULL,
    group_role_map  TEXT NOT NULL DEFAULT '{}',
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE identities (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idp_id       TEXT NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    external_id  TEXT NOT NULL,
    last_login   TEXT,
    UNIQUE (idp_id, external_id)
);
