-- Migration 0032: Smart Routing Engine
--
-- 1. Add capability + tier columns to model_pricing (the "smart" metadata layer)
-- 2. Create smart_routing_config (per-workspace on/off + meta-classifier settings)
-- 3. Create routing_rules (admin-defined explicit contract rules, Layer 2)
-- 4. Seed capability data for all known models

-- ── model_pricing extensions ──────────────────────────────────────────────────

ALTER TABLE model_pricing
    ADD COLUMN supports_vision    BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN supports_json_mode BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN complexity_tier    VARCHAR(10) NOT NULL DEFAULT 'standard'
        CHECK (complexity_tier IN ('micro', 'standard', 'premium')),
    ADD COLUMN quality_score      INTEGER NOT NULL DEFAULT 5
        CHECK (quality_score BETWEEN 1 AND 10);

-- ── Smart routing per-workspace config ───────────────────────────────────────

CREATE TABLE smart_routing_config (
    id                          UUID            PRIMARY KEY,
    workspace_id                UUID            UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
    enabled                     BOOLEAN         NOT NULL DEFAULT FALSE,
    default_model               VARCHAR(150)    NOT NULL DEFAULT '',
    -- Layer 4b: Meta-Classifier (opt-in)
    meta_classifier_enabled     BOOLEAN         NOT NULL DEFAULT FALSE,
    meta_classifier_provider    VARCHAR(50)     NOT NULL DEFAULT 'groq',
    meta_classifier_model       VARCHAR(150)    NOT NULL DEFAULT 'llama-3.1-8b-instant',
    meta_classifier_timeout_ms  INTEGER         NOT NULL DEFAULT 300,
    -- Cost guardrail: reject models whose estimated per-call cost exceeds this
    max_cost_per_request        DECIMAL(12, 8),
    created_at                  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    updated_at                  TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);

-- NULL workspace_id row = global default (used when workspace has no config)
INSERT INTO smart_routing_config (id, workspace_id, enabled, default_model)
VALUES (gen_random_uuid(), NULL, FALSE, '');

-- ── Routing rules (Layer 2 explicit contract) ─────────────────────────────────

CREATE TABLE routing_rules (
    id              UUID            PRIMARY KEY,
    workspace_id    UUID            NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    rule_order      INTEGER         NOT NULL DEFAULT 100,
    name            VARCHAR(100)    NOT NULL,
    is_enabled      BOOLEAN         NOT NULL DEFAULT TRUE,
    -- Conditions: all non-NULL conditions must match (AND logic)
    tag_key         VARCHAR(50),
    tag_value       VARCHAR(100),
    min_token_estimate  INTEGER,
    max_token_estimate  INTEGER,
    requires_tools      BOOLEAN,
    requires_vision     BOOLEAN,
    -- Action
    target_model    VARCHAR(150)    NOT NULL,
    created_at      TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_routing_rules_workspace ON routing_rules(workspace_id, is_enabled, rule_order);

-- ── Seed: vision capability ───────────────────────────────────────────────────
-- OpenAI vision-capable models
UPDATE model_pricing SET supports_vision = TRUE
WHERE provider = 'openai'
  AND model_id IN ('gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-4.1', 'gpt-4.1-mini', 'gpt-4.1-nano');

-- Anthropic: Claude 3+ family all support vision
UPDATE model_pricing SET supports_vision = TRUE
WHERE provider = 'anthropic'
  AND model_id IN (
    'claude-3-5-sonnet-20241022', 'claude-3-5-haiku-20241022',
    'claude-3-opus-20240229', 'claude-3-haiku-20240307',
    'claude-opus-4-7', 'claude-sonnet-4-6', 'claude-haiku-4-5-20251001',
    'claude-opus-4-5', 'claude-sonnet-4-5'
  );

-- Gemini: all support vision
UPDATE model_pricing SET supports_vision = TRUE
WHERE provider = 'gemini';

-- Groq: vision-preview models
UPDATE model_pricing SET supports_vision = TRUE
WHERE provider = 'groq'
  AND model_id IN ('llama-3.2-90b-vision-preview', 'llama-3.2-11b-vision-preview');

-- ── Seed: JSON mode ───────────────────────────────────────────────────────────
-- o1/o3 family does not support json_object response_format
UPDATE model_pricing SET supports_json_mode = FALSE
WHERE provider = 'openai'
  AND model_id IN ('o1', 'o1-mini', 'o3-mini', 'o3');

UPDATE model_pricing SET supports_json_mode = FALSE
WHERE provider = 'deepseek'
  AND model_id = 'deepseek-reasoner';

UPDATE model_pricing SET supports_json_mode = FALSE
WHERE provider = 'deepseek'
  AND model_id = 'deepseek-r1';

-- ── Seed: complexity_tier ─────────────────────────────────────────────────────

-- Micro tier (cheap, fast, simple queries)
UPDATE model_pricing SET complexity_tier = 'micro' WHERE model_id IN (
    'gpt-4o-mini', 'gpt-4.1-nano', 'gpt-4.1-mini',
    'claude-3-5-haiku-20241022', 'claude-3-haiku-20240307', 'claude-haiku-4-5-20251001',
    'gemini-2.0-flash-lite', 'gemini-1.5-flash', 'gemini-2.0-flash',
    'llama-3.1-8b-instant', 'llama-3.2-3b-preview', 'llama-3.2-1b-preview',
    'llama3-8b-8192', 'gemma2-9b-it',
    'deepseek-chat',
    'gpt-3.5-turbo', 'amazon.titan-text-express-v1'
);

-- Standard tier (balanced)
UPDATE model_pricing SET complexity_tier = 'standard' WHERE model_id IN (
    'gpt-4o', 'gpt-4.1',
    'claude-3-5-sonnet-20241022', 'claude-sonnet-4-6', 'claude-sonnet-4-5',
    'anthropic.claude-3-5-sonnet-20241022-v2:0', 'anthropic.claude-sonnet-4-5',
    'gemini-2.5-flash', 'gemini-1.5-pro',
    'llama-3.3-70b-versatile', 'llama-3.1-70b-versatile',
    'llama3-70b-8192', 'llama-3.2-90b-vision-preview',
    'mixtral-8x7b-32768', 'qwen-2.5-coder-32b', 'qwen-qwq-32b',
    'meta.llama3-1-70b-instruct-v1:0', 'meta.llama3-70b-instruct-v1:0'
);

-- Premium tier (heavy reasoning, complex tasks)
UPDATE model_pricing SET complexity_tier = 'premium' WHERE model_id IN (
    'gpt-4-turbo', 'o1', 'o1-mini', 'o3-mini', 'o3', 'o4-mini',
    'claude-3-opus-20240229', 'claude-opus-4-7', 'claude-opus-4-5',
    'gemini-2.5-pro',
    'deepseek-reasoner', 'deepseek-r1',
    'anthropic.claude-3-haiku-20240307-v1:0',
    'meta.llama3-2-90b-instruct-v1:0'
);

-- ── Seed: quality_score (1-10 within tier) ───────────────────────────────────

-- Micro tier quality scores
UPDATE model_pricing SET quality_score = 7 WHERE model_id IN ('gpt-4o-mini', 'gpt-4.1-mini', 'claude-3-5-haiku-20241022', 'claude-haiku-4-5-20251001');
UPDATE model_pricing SET quality_score = 6 WHERE model_id IN ('gemini-2.0-flash', 'gemini-1.5-flash', 'deepseek-chat', 'llama-3.1-8b-instant');
UPDATE model_pricing SET quality_score = 5 WHERE model_id IN ('gemini-2.0-flash-lite', 'gpt-4.1-nano', 'llama-3.2-3b-preview', 'gemma2-9b-it');
UPDATE model_pricing SET quality_score = 4 WHERE model_id IN ('gpt-3.5-turbo', 'claude-3-haiku-20240307', 'llama-3.2-1b-preview', 'llama3-8b-8192', 'amazon.titan-text-express-v1');

-- Standard tier quality scores
UPDATE model_pricing SET quality_score = 9 WHERE model_id IN ('gpt-4o', 'claude-3-5-sonnet-20241022', 'claude-sonnet-4-6', 'anthropic.claude-3-5-sonnet-20241022-v2:0');
UPDATE model_pricing SET quality_score = 8 WHERE model_id IN ('gpt-4.1', 'claude-sonnet-4-5', 'gemini-2.5-flash', 'llama-3.3-70b-versatile', 'anthropic.claude-sonnet-4-5');
UPDATE model_pricing SET quality_score = 7 WHERE model_id IN ('gemini-1.5-pro', 'llama-3.1-70b-versatile', 'llama3-70b-8192', 'qwen-2.5-coder-32b');
UPDATE model_pricing SET quality_score = 6 WHERE model_id IN ('mixtral-8x7b-32768', 'qwen-qwq-32b', 'llama-3.2-90b-vision-preview', 'meta.llama3-1-70b-instruct-v1:0');

-- Premium tier quality scores
UPDATE model_pricing SET quality_score = 10 WHERE model_id IN ('o3', 'claude-opus-4-7', 'gemini-2.5-pro');
UPDATE model_pricing SET quality_score = 9  WHERE model_id IN ('o1', 'claude-3-opus-20240229', 'claude-opus-4-5', 'deepseek-reasoner', 'deepseek-r1');
UPDATE model_pricing SET quality_score = 8  WHERE model_id IN ('gpt-4-turbo', 'o4-mini', 'o3-mini', 'o1-mini');
UPDATE model_pricing SET quality_score = 7  WHERE model_id IN ('meta.llama3-2-90b-instruct-v1:0', 'anthropic.claude-3-haiku-20240307-v1:0');
