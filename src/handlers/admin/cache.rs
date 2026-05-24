use crate::{
    db,
    errors::AppError,
    metrics,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// GET /admin/cache/stats
pub async fn get_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::cache::get_stats(&state.pool).await {
        Ok(stats) => Json(json!({ "data": stats })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// DELETE /admin/cache/entries/:id
///
/// Remove a single cache entry from both the in-memory hot layer and PostgreSQL.
pub async fn delete_entry(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_role(Role::Admin, &auth.0, &state).await {
        return e.into_response();
    }
    match db::cache::delete_entry(&state.pool, id).await {
        Ok(Some(hash)) => {
            state.cache.remove(&hash);
            metrics::set_exact_cache_size(state.cache.len());
            Json(json!({ "data": { "id": id, "deleted": true } })).into_response()
        }
        Ok(None) => AppError::NotFound(format!("Cache entry {id}")).into_response(),
        Err(e) => e.into_response(),
    }
}

/// DELETE /admin/cache
///
/// Clears the in-memory DashMap hot layer and deletes all rows from cache_entries.
pub async fn flush_cache(State(state): State<Arc<AppState>>, auth: AuthUser) -> impl IntoResponse {
    if let Err(e) = require_role(Role::Admin, &auth.0, &state).await {
        return e.into_response();
    }
    state.cache.clear();
    metrics::set_exact_cache_size(0);
    metrics::set_semantic_cache_size(0);

    match db::cache::flush_all(&state.pool).await {
        Ok(deleted) => Json(json!({ "data": { "deleted": deleted } })).into_response(),
        Err(e) => e.into_response(),
    }
}
