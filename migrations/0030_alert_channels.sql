-- Migration 0030: Add Slack and email delivery channels to alerts.
-- The existing `webhook_url` column remains for generic HTTP webhooks.
-- This migration promotes two first-class channels used by the first 10 customers.

ALTER TABLE alerts
    ADD COLUMN slack_webhook_url TEXT,
    ADD COLUMN email_to          TEXT[];
