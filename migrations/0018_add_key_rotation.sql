-- Migration 0018: Add API key rotation support (V3-5).
--
-- Two new nullable columns on api_keys:
--   previous_key_sha256 — SHA-256 hex of the old key, valid until rotation_expires_at.
--   rotation_expires_at — Grace-period deadline; old key accepted before this timestamp.
--
-- Auth middleware accepts key_sha256 OR previous_key_sha256 when rotation_expires_at
-- is in the future, enabling zero-downtime key rotation.

ALTER TABLE api_keys
    ADD COLUMN previous_key_sha256  VARCHAR(64),
    ADD COLUMN rotation_expires_at  TIMESTAMPTZ;
