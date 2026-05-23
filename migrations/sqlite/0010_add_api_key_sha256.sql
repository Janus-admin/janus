-- SQLite migration 0010: Add SHA-256 hash column to api_keys.
-- SQLite ALTER TABLE can only add columns, not constraints, so the UNIQUE index
-- is created as a separate statement.

ALTER TABLE api_keys ADD COLUMN key_sha256 TEXT;

CREATE UNIQUE INDEX idx_api_keys_sha256
    ON api_keys(key_sha256)
    WHERE key_sha256 IS NOT NULL;
