-- Migration 0027: V5-0 API Surface Expansion
--
-- Adds audit columns for the new modality endpoints (embeddings, images,
-- audio, legacy completions) and the function-calling capture, plus
-- per-modality pricing columns on model_pricing.

ALTER TABLE requests
    ADD COLUMN tool_calls JSONB,
    ADD COLUMN endpoint   VARCHAR(50) NOT NULL DEFAULT '/v1/chat/completions';

CREATE INDEX idx_requests_endpoint       ON requests(endpoint);
CREATE INDEX idx_requests_tool_calls_gin ON requests USING gin(tool_calls);

ALTER TABLE model_pricing
    ADD COLUMN price_per_image           DECIMAL(12,8),
    ADD COLUMN price_per_audio_second    DECIMAL(12,8),
    ADD COLUMN price_per_character       DECIMAL(12,8);
