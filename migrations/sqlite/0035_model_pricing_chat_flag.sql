-- Migration 0035 (SQLite): Add supports_chat flag to model_pricing

ALTER TABLE model_pricing ADD COLUMN supports_chat INTEGER NOT NULL DEFAULT 1;

UPDATE model_pricing SET supports_chat = 0
WHERE model_id IN (
  'whisper-large-v3',
  'whisper-large-v3-turbo',
  'whisper-1',
  'tts-1',
  'tts-1-hd'
);

UPDATE model_pricing SET is_active = 1
WHERE model_id IN (
  'whisper-large-v3',
  'whisper-large-v3-turbo'
);
