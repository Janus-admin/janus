ALTER TABLE api_keys
    ADD COLUMN routing_strategy TEXT NOT NULL DEFAULT 'priority';

CREATE TABLE routing_fallbacks (
    id              TEXT PRIMARY KEY,
    model_id        TEXT NOT NULL,
    fallback_model  TEXT NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 1,
    UNIQUE(model_id, fallback_model)
);
