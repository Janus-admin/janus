-- Migration 0002: Create workspaces table
-- Workspaces provide multi-tenancy. Every resource belongs to a workspace.
-- For solo deployments, a single default workspace is used.

CREATE TABLE workspaces (
    id          UUID        PRIMARY KEY,
    name        VARCHAR(255) NOT NULL,
    slug        VARCHAR(100) NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_workspaces_slug ON workspaces(slug);
