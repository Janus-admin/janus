-- Migration 0005: Create model_pricing table + seed current prices
-- Prices stored here so they can be updated without redeployment.
-- All prices are USD per 1,000,000 tokens (per 1M tokens).

CREATE TABLE model_pricing (
    id                      UUID            PRIMARY KEY,
    provider                VARCHAR(50)     NOT NULL,
    model_id                VARCHAR(150)    NOT NULL,
    model_display_name      VARCHAR(150),
    input_per_1m_tokens     DECIMAL(10,6)   NOT NULL,
    output_per_1m_tokens    DECIMAL(10,6)   NOT NULL,
    context_window          INTEGER,
    supports_streaming      BOOLEAN         NOT NULL DEFAULT TRUE,
    supports_functions      BOOLEAN         NOT NULL DEFAULT FALSE,
    is_active               BOOLEAN         NOT NULL DEFAULT TRUE,
    updated_at              TIMESTAMPTZ     NOT NULL,

    UNIQUE(provider, model_id)
);

CREATE INDEX idx_model_pricing_provider ON model_pricing(provider, is_active);

-- ─── OpenAI Models ────────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
(gen_random_uuid(), 'openai', 'gpt-4o',            'GPT-4o',                5.00,  15.00, 128000, TRUE,  NOW()),
(gen_random_uuid(), 'openai', 'gpt-4o-mini',        'GPT-4o Mini',           0.15,   0.60, 128000, TRUE,  NOW()),
(gen_random_uuid(), 'openai', 'gpt-4-turbo',         'GPT-4 Turbo',          10.00,  30.00, 128000, TRUE,  NOW()),
(gen_random_uuid(), 'openai', 'gpt-3.5-turbo',       'GPT-3.5 Turbo',         0.50,   1.50,  16385, TRUE,  NOW()),
(gen_random_uuid(), 'openai', 'o1',                  'o1',                   15.00,  60.00, 200000, FALSE, NOW()),
(gen_random_uuid(), 'openai', 'o1-mini',             'o1-mini',               3.00,  12.00, 128000, FALSE, NOW()),
(gen_random_uuid(), 'openai', 'o3-mini',             'o3-mini',               1.10,   4.40, 200000, FALSE, NOW());

-- ─── Anthropic Models ─────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
(gen_random_uuid(), 'anthropic', 'claude-3-5-sonnet-20241022', 'Claude 3.5 Sonnet',  3.00,  15.00, 200000, TRUE,  NOW()),
(gen_random_uuid(), 'anthropic', 'claude-3-5-haiku-20241022',  'Claude 3.5 Haiku',   0.80,   4.00, 200000, TRUE,  NOW()),
(gen_random_uuid(), 'anthropic', 'claude-3-opus-20240229',      'Claude 3 Opus',     15.00,  75.00, 200000, TRUE,  NOW()),
(gen_random_uuid(), 'anthropic', 'claude-3-haiku-20240307',     'Claude 3 Haiku',     0.25,   1.25, 200000, TRUE,  NOW());

-- ─── AWS Bedrock Models ───────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
(gen_random_uuid(), 'bedrock', 'anthropic.claude-3-5-sonnet-20241022-v2:0', 'Claude 3.5 Sonnet (Bedrock)',  3.00, 15.00, 200000, TRUE,  NOW()),
(gen_random_uuid(), 'bedrock', 'anthropic.claude-3-haiku-20240307-v1:0',    'Claude 3 Haiku (Bedrock)',     0.25,  1.25, 200000, TRUE,  NOW()),
(gen_random_uuid(), 'bedrock', 'amazon.titan-text-express-v1',              'Titan Text Express',           0.20,  0.60,   8192, FALSE, NOW()),
(gen_random_uuid(), 'bedrock', 'meta.llama3-70b-instruct-v1:0',             'Llama 3 70B (Bedrock)',        0.99,  0.99, 131072, FALSE, NOW());
