-- SQLite migration 0030: Add Slack and email delivery channels to alerts.
-- SQLite ALTER TABLE only supports one ADD COLUMN per statement.
-- email_to is stored as JSON text (e.g. '["a@b.com","c@d.com"]').

ALTER TABLE alerts ADD COLUMN slack_webhook_url TEXT;
ALTER TABLE alerts ADD COLUMN email_to          TEXT;
