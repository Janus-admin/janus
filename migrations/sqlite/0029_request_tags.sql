-- V5-L3: Cost Tags (SQLite mirror)
-- SQLite stores tags as TEXT (JSON); no GIN index available, use a plain index.
ALTER TABLE requests ADD COLUMN tags TEXT NOT NULL DEFAULT '{}';
CREATE INDEX idx_requests_tags ON requests(tags);
