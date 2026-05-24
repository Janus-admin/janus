-- SQLite migration 0027: V5-0 API Surface Expansion
-- JSONB → TEXT (SQLite stores JSON as TEXT); no GIN index — plain index instead.

ALTER TABLE requests ADD COLUMN tool_calls TEXT;
ALTER TABLE requests ADD COLUMN endpoint   TEXT NOT NULL DEFAULT '/v1/chat/completions';

CREATE INDEX idx_requests_endpoint   ON requests(endpoint);
CREATE INDEX idx_requests_tool_calls ON requests(tool_calls);

ALTER TABLE model_pricing ADD COLUMN price_per_image        TEXT;
ALTER TABLE model_pricing ADD COLUMN price_per_audio_second TEXT;
ALTER TABLE model_pricing ADD COLUMN price_per_character    TEXT;
