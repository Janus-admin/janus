-- V5-L2: OIDC Login
-- Adds identity_providers (one per OIDC app) and identities (user ↔ IdP link).

CREATE TABLE identity_providers (
    id              UUID PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind            VARCHAR(20) NOT NULL CHECK (kind = 'oidc'),
    name            VARCHAR(100) NOT NULL,
    -- JSON: { "discovery_url": "...", "client_id": "...", "client_secret": "<encrypted>" }
    config          JSONB NOT NULL,
    -- Maps IdP group names to Velox roles: { "engineering": "ApiManager", ... }
    group_role_map  JSONB NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE identities (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idp_id       UUID NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    external_id  TEXT NOT NULL,
    last_login   TIMESTAMPTZ,
    UNIQUE (idp_id, external_id)
);
