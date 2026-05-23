-- SQLite migration 0002: Create workspaces table

CREATE TABLE workspaces (
    id         TEXT NOT NULL PRIMARY KEY,
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_workspaces_slug ON workspaces(slug);
