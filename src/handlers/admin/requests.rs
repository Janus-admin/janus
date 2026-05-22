use crate::{db::requests as db_requests, errors::AppResult, state::AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ListRequestsQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub api_key_id: Option<Uuid>,
}

fn default_page() -> i64 {
    1
}
fn default_per_page() -> i64 {
    50
}

/// GET /admin/requests — list requests with optional filters.
pub async fn list_requests(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListRequestsQuery>,
) -> AppResult<Json<Value>> {
    let page = params.page.max(1);
    let per_page = params.per_page.clamp(1, 100);

    let (rows, total) = db_requests::list_requests(
        &state.pool,
        page,
        per_page,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.status.as_deref(),
        params.api_key_id,
    )
    .await?;

    Ok(Json(json!({
        "data": rows,
        "meta": {
            "page": page,
            "per_page": per_page,
            "total": total,
        }
    })))
}

/// GET /admin/requests/:id — get a single request by ID.
pub async fn get_request(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let row = db_requests::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Request {id}")))?;

    Ok(Json(json!({ "data": row })))
}
