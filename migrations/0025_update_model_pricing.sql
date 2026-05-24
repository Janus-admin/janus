-- Migration 0025: Add current (2025) models to model_pricing
-- Prices in USD per 1,000,000 tokens. ON CONFLICT = skip if already seeded.

-- ─── Anthropic Claude 4.x ─────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'anthropic', 'claude-opus-4-7',           'Claude Opus 4.7',    15.00, 75.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'anthropic', 'claude-sonnet-4-6',         'Claude Sonnet 4.6',   3.00, 15.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'anthropic', 'claude-haiku-4-5-20251001', 'Claude Haiku 4.5',    0.80,  4.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'anthropic', 'claude-opus-4-5',           'Claude Opus 4.5',    15.00, 75.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'anthropic', 'claude-sonnet-4-5',         'Claude Sonnet 4.5',   3.00, 15.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'anthropic', 'claude-haiku-4-5',          'Claude Haiku 4.5',    0.80,  4.00, 200000, TRUE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- ─── OpenAI GPT-4.1 / o-series ────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'openai', 'gpt-4.1',       'GPT-4.1',        2.00,  8.00, 1047576, TRUE, NOW()),
  (gen_random_uuid(), 'openai', 'gpt-4.1-mini',   'GPT-4.1 Mini',   0.40,  1.60, 1047576, TRUE, NOW()),
  (gen_random_uuid(), 'openai', 'gpt-4.1-nano',   'GPT-4.1 Nano',   0.10,  0.40, 1047576, TRUE, NOW()),
  (gen_random_uuid(), 'openai', 'o3',             'o3',            10.00, 40.00,  200000, FALSE, NOW()),
  (gen_random_uuid(), 'openai', 'o4-mini',         'o4-mini',        1.10,  4.40,  200000, FALSE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- ─── Google Gemini ─────────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'gemini', 'gemini-2.5-pro',           'Gemini 2.5 Pro',         1.25,  10.00, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.5-flash',         'Gemini 2.5 Flash',        0.15,   0.60, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.0-flash',         'Gemini 2.0 Flash',        0.10,   0.40, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.0-flash-lite',    'Gemini 2.0 Flash Lite',   0.075,  0.30, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-1.5-pro',           'Gemini 1.5 Pro',          1.25,   5.00, 2097152, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-1.5-flash',         'Gemini 1.5 Flash',        0.075,  0.30, 1048576, TRUE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- ─── Groq ──────────────────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'groq', 'llama-3.3-70b-versatile',    'Llama 3.3 70B',           0.59,  0.79, 128000, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama-3.1-70b-versatile',    'Llama 3.1 70B',           0.59,  0.79, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama-3.1-8b-instant',       'Llama 3.1 8B',            0.05,  0.08, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'mixtral-8x7b-32768',         'Mixtral 8x7B',            0.24,  0.24,  32768, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'gemma2-9b-it',               'Gemma 2 9B',              0.20,  0.20,   8192, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'deepseek-r1-distill-llama-70b', 'DeepSeek R1 Distill',  0.75,  0.99, 131072, FALSE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- ─── DeepSeek ──────────────────────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'deepseek', 'deepseek-chat',      'DeepSeek Chat (V3)',    0.27, 1.10,  65536, TRUE,  NOW()),
  (gen_random_uuid(), 'deepseek', 'deepseek-reasoner',  'DeepSeek Reasoner (R1)', 0.55, 2.19, 65536, FALSE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- ─── AWS Bedrock — new models ─────────────────────────────────────────────────
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'bedrock', 'anthropic.claude-3-5-sonnet-20241022-v2:0', 'Claude 3.5 Sonnet v2 (Bedrock)',  3.00, 15.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'bedrock', 'anthropic.claude-sonnet-4-5',               'Claude Sonnet 4.5 (Bedrock)',      3.00, 15.00, 200000, TRUE, NOW()),
  (gen_random_uuid(), 'bedrock', 'meta.llama3-1-70b-instruct-v1:0',           'Llama 3.1 70B (Bedrock)',          0.99,  0.99, 128000, FALSE, NOW()),
  (gen_random_uuid(), 'bedrock', 'meta.llama3-2-90b-instruct-v1:0',           'Llama 3.2 90B (Bedrock)',          2.00,  2.00, 128000, FALSE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;
