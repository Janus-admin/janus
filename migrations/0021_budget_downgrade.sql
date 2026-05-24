-- Migration 0021: Budget-aware auto-downgrade
-- When an API key's spend approaches its budget limit, Velox can automatically
-- switch to a cheaper model or routing strategy instead of hard-blocking at 100%.

ALTER TABLE api_keys
    ADD COLUMN downgrade_at_percent INTEGER,
    ADD COLUMN downgrade_strategy   VARCHAR(20),
    ADD COLUMN downgrade_to_model   VARCHAR(100);

-- Track whether a given request triggered a budget downgrade.
ALTER TABLE requests
    ADD COLUMN downgrade_triggered BOOLEAN NOT NULL DEFAULT FALSE;
