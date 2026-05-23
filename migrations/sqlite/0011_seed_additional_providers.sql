-- SQLite migration 0011: Seed Gemini, Groq, DeepSeek providers.
-- ON CONFLICT DO NOTHING requires SQLite 3.24+ (2018).

INSERT INTO providers (id, display_name, is_enabled, priority, base_url, timeout_ms, max_retries, updated_at) VALUES
('gemini',   'Google Gemini', 1, 4, 'https://generativelanguage.googleapis.com', 30000, 3, datetime('now')),
('groq',     'Groq',          1, 5, 'https://api.groq.com/openai/v1',            30000, 3, datetime('now')),
('deepseek', 'DeepSeek',      1, 6, 'https://api.deepseek.com/v1',               30000, 3, datetime('now'))
ON CONFLICT (id) DO NOTHING;
