-- Migration 0022: Provider quality score (Postgres)
-- Background task recomputes this every 15 minutes from the requests table.

ALTER TABLE providers
    ADD COLUMN quality_score       DECIMAL(5,4)  NOT NULL DEFAULT 1.0,
    ADD COLUMN quality_updated_at  TIMESTAMPTZ;
