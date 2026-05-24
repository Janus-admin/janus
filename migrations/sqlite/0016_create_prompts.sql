-- V2-5: Prompt Management (SQLite)
-- SQLite adaptations: UUID→TEXT, TIMESTAMPTZ→TEXT, BOOLEAN→INTEGER

CREATE TABLE prompts (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE prompt_versions (
    id              TEXT PRIMARY KEY,
    prompt_id       TEXT NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    content         TEXT NOT NULL,
    system_prompt   TEXT,
    is_active       INTEGER NOT NULL DEFAULT 0,
    ab_weight       INTEGER NOT NULL DEFAULT 100,
    created_at      TEXT NOT NULL,
    UNIQUE(prompt_id, version)
);
CREATE INDEX idx_prompt_versions_prompt ON prompt_versions(prompt_id, version DESC);

ALTER TABLE requests ADD COLUMN prompt_version_id TEXT REFERENCES prompt_versions(id);
