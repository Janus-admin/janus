-- V5-L3: Cost Tags
-- Adds a JSONB tags column to requests so clients can label spend by team/project/env.
-- Tags are extracted from the OpenAI `metadata` request field and the `X-Velox-Tags` header.
ALTER TABLE requests ADD COLUMN tags JSONB NOT NULL DEFAULT '{}';
CREATE INDEX idx_requests_tags_gin ON requests USING gin(tags);
