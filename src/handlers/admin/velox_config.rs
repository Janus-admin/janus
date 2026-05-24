use crate::{
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    state::AppState,
};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Patch payload for PATCH /admin/config.
/// All fields are optional; only provided fields are updated.
#[derive(Debug, Deserialize)]
pub struct PatchConfigRequest {
    pub log_request_bodies: Option<bool>,
    pub log_response_bodies: Option<bool>,
    pub cache_enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub semantic_cache_threshold: Option<f64>,
}

/// PATCH /admin/config — update runtime-safe config fields.
///
/// Secrets and server/db parameters are not patchable; they require a restart.
#[utoipa::path(
    patch,
    path = "/admin/config",
    tag = "Config",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Updated runtime-safe config", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn patch_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<PatchConfigRequest>,
) -> AppResult<Json<Value>> {
    require_role(Role::Admin, &auth.0, &state).await?;
    let mut rc = state.runtime_config.write().await;

    if let Some(v) = body.log_request_bodies {
        rc.log_request_bodies = v;
    }
    if let Some(v) = body.log_response_bodies {
        rc.log_response_bodies = v;
    }
    if let Some(v) = body.cache_enabled {
        rc.cache_enabled = v;
    }
    if let Some(v) = body.max_retries {
        rc.max_retries = v;
    }
    if let Some(v) = body.semantic_cache_threshold {
        rc.semantic_cache_threshold = v;
    }

    Ok(Json(json!({
        "data": {
            "log_request_bodies":      rc.log_request_bodies,
            "log_response_bodies":     rc.log_response_bodies,
            "cache_enabled":           rc.cache_enabled,
            "max_retries":             rc.max_retries,
            "semantic_cache_threshold": rc.semantic_cache_threshold,
        }
    })))
}

/// GET /admin/config — return the current runtime configuration (safe fields only).
///
/// Secrets (jwt_secret, encryption_key, provider API keys) are never included.
/// This is read-only in Phase 6. Runtime mutation will be added in Phase 7.
#[utoipa::path(
    get,
    path = "/admin/config",
    tag = "Config",
    responses(
        (status = 200, description = "Current runtime configuration (no secrets)", body = serde_json::Value),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn get_config(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let c = &state.config;
    let rc = state.runtime_config.read().await;
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

            // Logging (live values from runtime_config)
            "log_level":               c.log_level,
            "log_request_bodies":      rc.log_request_bodies,
            "log_response_bodies":     rc.log_response_bodies,

            // Cache (live values from runtime_config)
            "cache_enabled":               rc.cache_enabled,
            "cache_ttl_seconds":           c.cache_ttl_seconds,
            "cache_max_entries":           c.cache_max_entries,
            "semantic_cache_threshold":    rc.semantic_cache_threshold,
            "embedding_model_path":        c.embedding_model_path,
            "embedding_tokenizer_path":    c.embedding_tokenizer_path,

            // Rate limiting (live value from runtime_config)
            "rate_limit_window_secs": c.rate_limit_window_secs,
            "max_retries":            rc.max_retries,

            // Metrics
            "prometheus_enabled": c.prometheus_enabled,

            // Derived capabilities
            "semantic_cache_available": state.cache.model.is_some(),
        }
    })))
}
