-- V2-5: Prompt Management
-- Creates prompts, prompt_versions tables and links requests to prompt versions.

CREATE TABLE prompts (
    id          UUID PRIMARY KEY,
    name        VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE prompt_versions (
    id              UUID PRIMARY KEY,
    prompt_id       UUID NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    content         TEXT NOT NULL,
    system_prompt   TEXT,
    is_active       BOOLEAN NOT NULL DEFAULT FALSE,
    ab_weight       INTEGER NOT NULL DEFAULT 100,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(prompt_id, version)
);
CREATE INDEX idx_prompt_versions_prompt ON prompt_versions(prompt_id, version DESC);

ALTER TABLE requests ADD COLUMN prompt_version_id UUID REFERENCES prompt_versions(id);
