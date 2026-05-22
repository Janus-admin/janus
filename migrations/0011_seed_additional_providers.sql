-- Migration 0011: Add rows for Gemini, Groq, and DeepSeek so the admin
-- dashboard's Providers page lists them. The gateway already registers
-- them at startup from env vars; these rows exist only for UI display
-- and per-provider config overrides (priority, timeout, max_retries).

INSERT INTO providers (id, display_name, is_enabled, priority, base_url, timeout_ms, max_retries, updated_at) VALUES
('gemini',   'Google Gemini', TRUE, 4, 'https://generativelanguage.googleapis.com', 30000, 3, NOW()),
('groq',     'Groq',          TRUE, 5, 'https://api.groq.com/openai/v1',            30000, 3, NOW()),
('deepseek', 'DeepSeek',      TRUE, 6, 'https://api.deepseek.com/v1',               30000, 3, NOW())
ON CONFLICT (id) DO NOTHING;
