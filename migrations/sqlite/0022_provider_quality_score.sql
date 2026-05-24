-- Migration 0022: Provider quality score (SQLite)

ALTER TABLE providers
    ADD COLUMN quality_score      REAL    NOT NULL DEFAULT 1.0;
ALTER TABLE providers
    ADD COLUMN quality_updated_at TEXT;
