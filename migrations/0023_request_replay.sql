-- Migration 0023: Request replay + admin playground (V4-6)
-- Adds two columns to the requests table to track replays and playground sessions.

ALTER TABLE requests
    ADD COLUMN replay_of_request_id UUID REFERENCES requests(id),
    ADD COLUMN is_playground        BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX idx_requests_replay_of ON requests(replay_of_request_id)
    WHERE replay_of_request_id IS NOT NULL;
