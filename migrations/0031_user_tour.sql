-- Migration 0031: Track when a user dismisses the onboarding tour.
-- NULL means the tour has not yet been completed/dismissed.
ALTER TABLE users ADD COLUMN tour_completed_at TIMESTAMPTZ;
