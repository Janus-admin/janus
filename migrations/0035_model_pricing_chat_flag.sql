-- Migration 0035: Add supports_chat flag to model_pricing
-- Allows audio/image models to stay in the catalogue for routing
-- while being excluded from chat completions requests.

ALTER TABLE model_pricing
  ADD COLUMN IF NOT EXISTS supports_chat BOOLEAN NOT NULL DEFAULT TRUE;

-- Audio-only models: should never receive chat completion requests
UPDATE model_pricing SET supports_chat = FALSE
WHERE model_id IN (
  'whisper-large-v3',
  'whisper-large-v3-turbo',
  'whisper-1',
  'tts-1',
  'tts-1-hd'
);

-- Re-activate audio models (they were incorrectly deactivated earlier)
UPDATE model_pricing SET is_active = TRUE
WHERE model_id IN (
  'whisper-large-v3',
  'whisper-large-v3-turbo'
);
