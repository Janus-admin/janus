-- Migration 0012: Add webhook delivery columns to alerts table.
-- Adds the fields needed for V2-2 webhook delivery; evaluation engine lands in V2-0.

ALTER TABLE alerts
    ADD COLUMN webhook_url     TEXT,
    ADD COLUMN webhook_format  VARCHAR(20) NOT NULL DEFAULT 'generic',
    ADD COLUMN webhook_secret  TEXT;
