use crate::{db, metrics, state::AppState};
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;

/// GET /admin/cache/stats
pub async fn get_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::cache::get_stats(&state.pool).await {
        Ok(stats) => Json(json!({ "data": stats })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// DELETE /admin/cache
///
/// Clears the in-memory DashMap hot layer and deletes all rows from cache_entries.
pub async fn flush_cache(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.cache.clear();
    metrics::set_exact_cache_size(0);
    metrics::set_semantic_cache_size(0);

    match db::cache::flush_all(&state.pool).await {
        Ok(deleted) => Json(json!({ "data": { "deleted": deleted } })).into_response(),
        Err(e) => e.into_response(),
    }
}
