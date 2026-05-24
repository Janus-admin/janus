-- SQLite migration 0014: Add request_type column to requests.
ALTER TABLE requests ADD COLUMN request_type TEXT NOT NULL DEFAULT 'chat';
CREATE INDEX idx_requests_type ON requests(request_type);
