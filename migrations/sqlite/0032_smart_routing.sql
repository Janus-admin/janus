-- Migration 0032 (SQLite): Smart Routing Engine
-- SQLite mirror of the PostgreSQL migration.

ALTER TABLE model_pricing ADD COLUMN supports_vision    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE model_pricing ADD COLUMN supports_json_mode INTEGER NOT NULL DEFAULT 1;
ALTER TABLE model_pricing ADD COLUMN complexity_tier    TEXT    NOT NULL DEFAULT 'standard';
ALTER TABLE model_pricing ADD COLUMN quality_score      INTEGER NOT NULL DEFAULT 5;

CREATE TABLE smart_routing_config (
    id                          TEXT    PRIMARY KEY,
    workspace_id                TEXT    UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
    enabled                     INTEGER NOT NULL DEFAULT 0,
    default_model               TEXT    NOT NULL DEFAULT '',
    meta_classifier_enabled     INTEGER NOT NULL DEFAULT 0,
    meta_classifier_provider    TEXT    NOT NULL DEFAULT 'groq',
    meta_classifier_model       TEXT    NOT NULL DEFAULT 'llama-3.1-8b-instant',
    meta_classifier_timeout_ms  INTEGER NOT NULL DEFAULT 300,
    max_cost_per_request        TEXT,
    created_at                  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at                  TEXT    NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO smart_routing_config (id, workspace_id, enabled, default_model)
VALUES (lower(hex(randomblob(16))), NULL, 0, '');

CREATE TABLE routing_rules (
    id              TEXT    PRIMARY KEY,
    workspace_id    TEXT    NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    rule_order      INTEGER NOT NULL DEFAULT 100,
    name            TEXT    NOT NULL,
    is_enabled      INTEGER NOT NULL DEFAULT 1,
    tag_key         TEXT,
    tag_value       TEXT,
    min_token_estimate  INTEGER,
    max_token_estimate  INTEGER,
    requires_tools      INTEGER,
    requires_vision     INTEGER,
    target_model    TEXT    NOT NULL,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_routing_rules_workspace ON routing_rules(workspace_id, is_enabled, rule_order);

-- Seed vision support
UPDATE model_pricing SET supports_vision = 1 WHERE model_id IN (
    'gpt-4o','gpt-4o-mini','gpt-4-turbo','gpt-4.1','gpt-4.1-mini','gpt-4.1-nano',
    'claude-3-5-sonnet-20241022','claude-3-5-haiku-20241022','claude-3-opus-20240229',
    'claude-3-haiku-20240307','claude-opus-4-7','claude-sonnet-4-6','claude-haiku-4-5-20251001',
    'claude-opus-4-5','claude-sonnet-4-5',
    'llama-3.2-90b-vision-preview','llama-3.2-11b-vision-preview'
);
UPDATE model_pricing SET supports_vision = 1 WHERE provider = 'gemini';

UPDATE model_pricing SET supports_json_mode = 0 WHERE model_id IN (
    'o1','o1-mini','o3-mini','o3','deepseek-reasoner','deepseek-r1'
);

-- Micro tier
UPDATE model_pricing SET complexity_tier = 'micro' WHERE model_id IN (
    'gpt-4o-mini','gpt-4.1-nano','gpt-4.1-mini',
    'claude-3-5-haiku-20241022','claude-3-haiku-20240307','claude-haiku-4-5-20251001',
    'gemini-2.0-flash-lite','gemini-1.5-flash','gemini-2.0-flash',
    'llama-3.1-8b-instant','llama-3.2-3b-preview','llama-3.2-1b-preview',
    'llama3-8b-8192','gemma2-9b-it','deepseek-chat','gpt-3.5-turbo','amazon.titan-text-express-v1'
);

-- Standard tier
UPDATE model_pricing SET complexity_tier = 'standard' WHERE model_id IN (
    'gpt-4o','gpt-4.1','claude-3-5-sonnet-20241022','claude-sonnet-4-6','claude-sonnet-4-5',
    'anthropic.claude-3-5-sonnet-20241022-v2:0','anthropic.claude-sonnet-4-5',
    'gemini-2.5-flash','gemini-1.5-pro','llama-3.3-70b-versatile','llama-3.1-70b-versatile',
    'llama3-70b-8192','llama-3.2-90b-vision-preview','mixtral-8x7b-32768',
    'qwen-2.5-coder-32b','qwen-qwq-32b','meta.llama3-1-70b-instruct-v1:0','meta.llama3-70b-instruct-v1:0'
);

-- Premium tier
UPDATE model_pricing SET complexity_tier = 'premium' WHERE model_id IN (
    'gpt-4-turbo','o1','o1-mini','o3-mini','o3','o4-mini',
    'claude-3-opus-20240229','claude-opus-4-7','claude-opus-4-5',
    'gemini-2.5-pro','deepseek-reasoner','deepseek-r1',
    'anthropic.claude-3-haiku-20240307-v1:0','meta.llama3-2-90b-instruct-v1:0'
);

-- Quality scores (micro)
UPDATE model_pricing SET quality_score = 7 WHERE model_id IN ('gpt-4o-mini','gpt-4.1-mini','claude-3-5-haiku-20241022','claude-haiku-4-5-20251001');
UPDATE model_pricing SET quality_score = 6 WHERE model_id IN ('gemini-2.0-flash','gemini-1.5-flash','deepseek-chat','llama-3.1-8b-instant');
UPDATE model_pricing SET quality_score = 5 WHERE model_id IN ('gemini-2.0-flash-lite','gpt-4.1-nano','llama-3.2-3b-preview','gemma2-9b-it');
UPDATE model_pricing SET quality_score = 4 WHERE model_id IN ('gpt-3.5-turbo','claude-3-haiku-20240307','llama-3.2-1b-preview','llama3-8b-8192','amazon.titan-text-express-v1');
-- Quality scores (standard)
UPDATE model_pricing SET quality_score = 9 WHERE model_id IN ('gpt-4o','claude-3-5-sonnet-20241022','claude-sonnet-4-6','anthropic.claude-3-5-sonnet-20241022-v2:0');
UPDATE model_pricing SET quality_score = 8 WHERE model_id IN ('gpt-4.1','claude-sonnet-4-5','gemini-2.5-flash','llama-3.3-70b-versatile','anthropic.claude-sonnet-4-5');
UPDATE model_pricing SET quality_score = 7 WHERE model_id IN ('gemini-1.5-pro','llama-3.1-70b-versatile','llama3-70b-8192','qwen-2.5-coder-32b');
UPDATE model_pricing SET quality_score = 6 WHERE model_id IN ('mixtral-8x7b-32768','qwen-qwq-32b','llama-3.2-90b-vision-preview','meta.llama3-1-70b-instruct-v1:0');
-- Quality scores (premium)
UPDATE model_pricing SET quality_score = 10 WHERE model_id IN ('o3','claude-opus-4-7','gemini-2.5-pro');
UPDATE model_pricing SET quality_score = 9  WHERE model_id IN ('o1','claude-3-opus-20240229','claude-opus-4-5','deepseek-reasoner','deepseek-r1');
UPDATE model_pricing SET quality_score = 8  WHERE model_id IN ('gpt-4-turbo','o4-mini','o3-mini','o1-mini');
UPDATE model_pricing SET quality_score = 7  WHERE model_id IN ('meta.llama3-2-90b-instruct-v1:0','anthropic.claude-3-haiku-20240307-v1:0');
