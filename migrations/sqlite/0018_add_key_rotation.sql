-- Migration 0018: Add API key rotation support (V3-5) — SQLite variant.
-- SQLite stores TIMESTAMPTZ as TEXT (ISO-8601) and VARCHAR as TEXT.

ALTER TABLE api_keys ADD COLUMN previous_key_sha256 TEXT;
ALTER TABLE api_keys ADD COLUMN rotation_expires_at TEXT;
