-- Migration 0023: Request replay + admin playground (V4-6, SQLite)

ALTER TABLE requests
    ADD COLUMN replay_of_request_id TEXT;
ALTER TABLE requests
    ADD COLUMN is_playground INTEGER NOT NULL DEFAULT 0;
