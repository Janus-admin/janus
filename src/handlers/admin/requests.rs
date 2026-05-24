use crate::{
    db::requests as db_requests,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListRequestsQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub api_key_id: Option<Uuid>,
    /// RFC3339 lower bound (inclusive) on created_at (V3-5 audit filter).
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// RFC3339 upper bound (inclusive) on created_at (V3-5 audit filter).
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    /// When true, return only cached responses; false = only live responses (V3-5).
    pub has_cache_hit: Option<bool>,
}

fn default_page() -> i64 {
    1
}
fn default_per_page() -> i64 {
    50
}

/// GET /admin/requests — list requests with optional filters.
///
/// V3-5: adds start_time, end_time, has_cache_hit filters and returns
/// `X-Velox-Audit-Hash: <sha256-of-response-body>` for tamper detection.
#[utoipa::path(
    get,
    path = "/admin/requests",
    tag = "Requests",
    params(ListRequestsQuery),
    responses(
        (status = 200, description = "Paginated requests with X-Velox-Audit-Hash header", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires BillingViewer role or higher"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_requests(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<ListRequestsQuery>,
) -> AppResult<Response> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
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
        params.start_time,
        params.end_time,
        params.has_cache_hit,
    )
    .await?;

    let body = json!({
        "data": rows,
        "meta": {
            "page": page,
            "per_page": per_page,
            "total": total,
        }
    });

    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    let audit_hash = {
        let mut h = Sha256::new();
        h.update(&body_bytes);
        h.finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    };

    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert("X-Velox-Audit-Hash", audit_hash.parse().unwrap());

    Ok(response)
}

/// GET /admin/requests/export — download all matching requests as CSV.
///
/// Accepts the same query filters as `list_requests` (provider, model, status,
/// api_key_id). Returns up to 10 000 rows. Response is `text/csv` with a
/// `Content-Disposition: attachment` header so browsers trigger a download.
#[utoipa::path(
    get,
    path = "/admin/requests/export",
    tag = "Requests",
    params(ListRequestsQuery),
    responses(
        (status = 200, description = "CSV download of up to 10,000 matching requests", content_type = "text/csv"),
        (status = 403, description = "Forbidden — requires BillingViewer role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn export_requests(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<ListRequestsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_role(Role::BillingViewer, &auth.0, &state).await {
        return e.into_response();
    }
    let (rows, _) = match db_requests::list_requests(
        &state.pool,
        1,
        10_000,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.status.as_deref(),
        params.api_key_id,
        params.start_time,
        params.end_time,
        params.has_cache_hit,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    let mut csv = String::from(
        "id,provider,model,prompt_tokens,completion_tokens,total_tokens,\
         cost_usd,latency_ms,ttfb_ms,status,cache_type,stream,created_at\n",
    );

    for r in &rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.id,
            r.provider,
            r.model,
            r.prompt_tokens.map(|v| v.to_string()).unwrap_or_default(),
            r.completion_tokens
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.total_tokens.map(|v| v.to_string()).unwrap_or_default(),
            r.cost_usd.map(|v| v.to_string()).unwrap_or_default(),
            r.latency_ms.map(|v| v.to_string()).unwrap_or_default(),
            r.ttfb_ms.map(|v| v.to_string()).unwrap_or_default(),
            r.status,
            r.cache_type.as_deref().unwrap_or(""),
            r.stream,
            r.created_at.to_rfc3339(),
        ));
    }

    (
        [
            (header::CONTENT_TYPE, "text/csv"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"velox_requests.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

/// GET /admin/requests/:id — get a single request by ID.
#[utoipa::path(
    get,
    path = "/admin/requests/{id}",
    tag = "Requests",
    params(("id" = uuid::Uuid, Path, description = "Request UUID")),
    responses(
        (status = 200, description = "Request row", body = serde_json::Value),
        (status = 404, description = "Request not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn get_request(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let row = db_requests::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Request {id}")))?;

    Ok(Json(json!({ "data": row })))
}
