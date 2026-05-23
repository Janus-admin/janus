-- SQLite migration 0005: Create model_pricing table + seed prices
-- DECIMAL → TEXT; gen_random_uuid() → hardcoded deterministic UUIDs for seed rows.
-- BOOLEAN → INTEGER.

CREATE TABLE model_pricing (
    id                   TEXT NOT NULL PRIMARY KEY,
    provider             TEXT NOT NULL,
    model_id             TEXT NOT NULL,
    model_display_name   TEXT,
    input_per_1m_tokens  TEXT NOT NULL,
    output_per_1m_tokens TEXT NOT NULL,
    context_window       INTEGER,
    supports_streaming   INTEGER NOT NULL DEFAULT 1,
    supports_functions   INTEGER NOT NULL DEFAULT 0,
    is_active            INTEGER NOT NULL DEFAULT 1,
    updated_at           TEXT NOT NULL,

    UNIQUE(provider, model_id)
);

CREATE INDEX idx_model_pricing_provider ON model_pricing(provider, is_active);

-- ─── OpenAI Models ────────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
('a0000001-0000-4000-8000-000000000001', 'openai', 'gpt-4o',        'GPT-4o',        '5.000000',  '15.000000', 128000, 1, datetime('now')),
('a0000001-0000-4000-8000-000000000002', 'openai', 'gpt-4o-mini',   'GPT-4o Mini',   '0.150000',   '0.600000', 128000, 1, datetime('now')),
('a0000001-0000-4000-8000-000000000003', 'openai', 'gpt-4-turbo',   'GPT-4 Turbo',  '10.000000',  '30.000000', 128000, 1, datetime('now')),
('a0000001-0000-4000-8000-000000000004', 'openai', 'gpt-3.5-turbo', 'GPT-3.5 Turbo', '0.500000',   '1.500000',  16385, 1, datetime('now')),
('a0000001-0000-4000-8000-000000000005', 'openai', 'o1',            'o1',            '15.000000',  '60.000000', 200000, 0, datetime('now')),
('a0000001-0000-4000-8000-000000000006', 'openai', 'o1-mini',       'o1-mini',        '3.000000',  '12.000000', 128000, 0, datetime('now')),
('a0000001-0000-4000-8000-000000000007', 'openai', 'o3-mini',       'o3-mini',        '1.100000',   '4.400000', 200000, 0, datetime('now'));

-- ─── Anthropic Models ─────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
('a0000002-0000-4000-8000-000000000001', 'anthropic', 'claude-3-5-sonnet-20241022', 'Claude 3.5 Sonnet',  '3.000000', '15.000000', 200000, 1, datetime('now')),
('a0000002-0000-4000-8000-000000000002', 'anthropic', 'claude-3-5-haiku-20241022',  'Claude 3.5 Haiku',   '0.800000',  '4.000000', 200000, 1, datetime('now')),
('a0000002-0000-4000-8000-000000000003', 'anthropic', 'claude-3-opus-20240229',     'Claude 3 Opus',     '15.000000', '75.000000', 200000, 1, datetime('now')),
('a0000002-0000-4000-8000-000000000004', 'anthropic', 'claude-3-haiku-20240307',    'Claude 3 Haiku',     '0.250000',  '1.250000', 200000, 1, datetime('now'));

-- ─── AWS Bedrock Models ───────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at) VALUES
('a0000003-0000-4000-8000-000000000001', 'bedrock', 'anthropic.claude-3-5-sonnet-20241022-v2:0', 'Claude 3.5 Sonnet (Bedrock)',  '3.000000', '15.000000', 200000, 1, datetime('now')),
('a0000003-0000-4000-8000-000000000002', 'bedrock', 'anthropic.claude-3-haiku-20240307-v1:0',    'Claude 3 Haiku (Bedrock)',     '0.250000',  '1.250000', 200000, 1, datetime('now')),
('a0000003-0000-4000-8000-000000000003', 'bedrock', 'amazon.titan-text-express-v1',              'Titan Text Express',           '0.200000',  '0.600000',  8192, 0, datetime('now')),
('a0000003-0000-4000-8000-000000000004', 'bedrock', 'meta.llama3-70b-instruct-v1:0',             'Llama 3 70B (Bedrock)',        '0.990000',  '0.990000', 131072, 0, datetime('now'));
