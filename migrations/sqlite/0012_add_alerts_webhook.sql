-- SQLite migration 0012: Add webhook delivery columns to alerts table.
-- SQLite ALTER TABLE only supports one ADD COLUMN per statement.

ALTER TABLE alerts ADD COLUMN webhook_url    TEXT;
ALTER TABLE alerts ADD COLUMN webhook_format TEXT NOT NULL DEFAULT 'generic';
ALTER TABLE alerts ADD COLUMN webhook_secret TEXT;
