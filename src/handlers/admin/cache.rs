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
#[utoipa::path(
    get,
    path = "/admin/cache/stats",
    tag = "Cache",
    responses(
        (status = 200, description = "Aggregate cache stats (entries, hits, tokens saved, cost saved)", body = serde_json::Value),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn get_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::cache::get_stats(&state.pool).await {
        Ok(stats) => Json(json!({ "data": stats })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// DELETE /admin/cache/entries/:id
///
/// Remove a single cache entry from both the in-memory hot layer and PostgreSQL.
#[utoipa::path(
    delete,
    path = "/admin/cache/entries/{id}",
    tag = "Cache",
    params(("id" = uuid::Uuid, Path, description = "Cache entry UUID")),
    responses(
        (status = 200, description = "Cache entry deleted", body = serde_json::Value),
        (status = 404, description = "Entry not found"),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
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
#[utoipa::path(
    delete,
    path = "/admin/cache",
    tag = "Cache",
    responses(
        (status = 200, description = "All cache entries flushed", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn flush_cache(State(state): State<Arc<AppState>>, auth: AuthUser) -> impl IntoResponse {
    if let Err(e) = require_role(Role::Admin, &auth.0, &state).await {
        return e.into_response();
    }
    state.cache.clear().await;
    metrics::set_exact_cache_size(0);
    metrics::set_semantic_cache_size(0);

    match db::cache::flush_all(&state.pool).await {
        Ok(deleted) => Json(json!({ "data": { "deleted": deleted } })).into_response(),
        Err(e) => e.into_response(),
    }
}
