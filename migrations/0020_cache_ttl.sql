-- Migration 0020: Add TTL column to cache_entries
-- expires_at already exists from 0007; ttl_secs records the configured TTL at write time.
-- 0 or NULL = no expiry (backward compatible).

ALTER TABLE cache_entries ADD COLUMN ttl_secs INTEGER;
