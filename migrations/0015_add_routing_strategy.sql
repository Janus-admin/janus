ALTER TABLE api_keys
    ADD COLUMN routing_strategy VARCHAR(20) NOT NULL DEFAULT 'priority';

CREATE TABLE routing_fallbacks (
    id              UUID PRIMARY KEY,
    model_id        VARCHAR(100) NOT NULL,
    fallback_model  VARCHAR(100) NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 1,
    UNIQUE(model_id, fallback_model)
);
