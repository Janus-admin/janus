-- Migration 0014: Add request_type column to requests for embeddings support.
ALTER TABLE requests ADD COLUMN request_type VARCHAR(20) NOT NULL DEFAULT 'chat';
CREATE INDEX idx_requests_type ON requests(request_type);
