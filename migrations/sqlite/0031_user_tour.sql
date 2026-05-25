-- SQLite migration 0031: Track when a user dismisses the onboarding tour.
ALTER TABLE users ADD COLUMN tour_completed_at TEXT;
