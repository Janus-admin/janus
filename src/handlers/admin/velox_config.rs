use crate::{errors::AppResult, state::AppState};
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

/// GET /admin/config — return the current runtime configuration (safe fields only).
///
/// Secrets (jwt_secret, encryption_key, provider API keys) are never included.
/// This is read-only in Phase 6. Runtime mutation will be added in Phase 7.
pub async fn get_config(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let c = &state.config;
    Ok(Json(json!({
        "data": {
            // Server
            "host":                    c.host,
            "port":                    c.port,
            "request_timeout_ms":      c.request_timeout_ms,

            // Database
            "db_pool_max_connections": c.db_pool_max_connections,

            // Auth
            "jwt_expiration_hours":    c.jwt_expiration_hours,

            // Logging
            "log_level":               c.log_level,
            "log_request_bodies":      c.log_request_bodies,
            "log_response_bodies":     c.log_response_bodies,

            // Cache
            "cache_enabled":               c.cache_enabled,
            "cache_ttl_seconds":           c.cache_ttl_seconds,
            "cache_max_entries":           c.cache_max_entries,
            "semantic_cache_threshold":    c.semantic_cache_threshold,
            "embedding_model_path":        c.embedding_model_path,
            "embedding_tokenizer_path":    c.embedding_tokenizer_path,

            // Rate limiting
            "rate_limit_window_secs": c.rate_limit_window_secs,
            "max_retries":            c.max_retries,

            // Metrics
            "prometheus_enabled": c.prometheus_enabled,

            // Derived capabilities
            "semantic_cache_available": state.cache.model.is_some(),
        }
    })))
}
