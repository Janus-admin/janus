-- Migration 0019: V4-0 — Custom provider base_url (SQLite variant)
-- providers.base_url already present (NOT NULL) from migration 0004.
-- No schema change required for SQLite (COMMENT ON not supported).
-- An empty string ('') means "use the adapter's compiled-in default".
SELECT 1;
