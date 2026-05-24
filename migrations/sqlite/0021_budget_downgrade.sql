-- Migration 0021: Budget-aware auto-downgrade (SQLite)

ALTER TABLE api_keys
    ADD COLUMN downgrade_at_percent INTEGER;
ALTER TABLE api_keys
    ADD COLUMN downgrade_strategy   TEXT;
ALTER TABLE api_keys
    ADD COLUMN downgrade_to_model   TEXT;

-- Track whether a given request triggered a budget downgrade.
ALTER TABLE requests
    ADD COLUMN downgrade_triggered INTEGER NOT NULL DEFAULT 0;
