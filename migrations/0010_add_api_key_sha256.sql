-- Migration 0010: Add SHA-256 hash column to api_keys for fast in-memory lookup.
--
-- See DECISIONS.md D-004: bcrypt is too slow for per-request validation (~300ms).
-- SHA-256(key) is stored here so the dashmap can be rehydrated on restart
-- without needing the plaintext key (which is never stored).
--
-- Nullable because keys created before this migration have no sha256.
-- All keys created in Phase 1+ always set this column.

ALTER TABLE api_keys
    ADD COLUMN key_sha256 VARCHAR(64) UNIQUE;

CREATE INDEX idx_api_keys_sha256
    ON api_keys(key_sha256)
    WHERE key_sha256 IS NOT NULL;
