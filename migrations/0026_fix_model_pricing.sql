-- Migration 0026: Fix model grouping and add missing models
-- Remove DeepSeek R1 from groq (it belongs under deepseek provider)
DELETE FROM model_pricing WHERE provider = 'groq' AND model_id = 'deepseek-r1-distill-llama-70b';

-- Remove duplicate haiku entries (keep the versioned one)
DELETE FROM model_pricing WHERE provider = 'anthropic' AND model_id = 'claude-haiku-4-5';

-- Add DeepSeek R1 properly under deepseek provider
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES (gen_random_uuid(), 'deepseek', 'deepseek-r1', 'DeepSeek R1', 0.55, 2.19, 65536, FALSE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- Add missing Groq models (llama 3.2 family)
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'groq', 'llama-3.2-90b-vision-preview',  'Llama 3.2 90B Vision',  0.90, 0.90, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama-3.2-11b-vision-preview',  'Llama 3.2 11B Vision',  0.18, 0.18, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama-3.2-3b-preview',          'Llama 3.2 3B',          0.06, 0.06, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama-3.2-1b-preview',          'Llama 3.2 1B',          0.04, 0.04, 131072, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama3-70b-8192',               'Llama 3 70B',           0.59, 0.79,   8192, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'llama3-8b-8192',                'Llama 3 8B',            0.05, 0.08,   8192, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'qwen-qwq-32b',                  'Qwen QwQ 32B',          0.29, 0.39,  32768, TRUE, NOW()),
  (gen_random_uuid(), 'groq', 'qwen-2.5-coder-32b',            'Qwen 2.5 Coder 32B',    0.29, 0.39,  32768, TRUE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;

-- Ensure Gemini 2.5 is present (in case 0025 didn't run)
INSERT INTO model_pricing (id, provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions, updated_at)
VALUES
  (gen_random_uuid(), 'gemini', 'gemini-2.5-pro',         'Gemini 2.5 Pro',          1.25, 10.00, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.5-flash',       'Gemini 2.5 Flash',         0.15,  0.60, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.0-flash',       'Gemini 2.0 Flash',         0.10,  0.40, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-2.0-flash-lite',  'Gemini 2.0 Flash Lite',    0.075, 0.30, 1048576, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-1.5-pro',         'Gemini 1.5 Pro',           1.25,  5.00, 2097152, TRUE, NOW()),
  (gen_random_uuid(), 'gemini', 'gemini-1.5-flash',       'Gemini 1.5 Flash',         0.075, 0.30, 1048576, TRUE, NOW())
ON CONFLICT (provider, model_id) DO NOTHING;
