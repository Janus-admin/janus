use crate::{
    db::api_keys as db_api_keys,
    errors::AppResult,
    models::api_key::{ApiKeyView, CreateApiKeyRequest, CreateApiKeyResponse},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use bcrypt::{hash, DEFAULT_COST};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListKeysQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

fn default_page() -> i64 {
    1
}
fn default_per_page() -> i64 {
    50
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /admin/keys — create a new API key (JWT-protected admin route).
///
/// The full `vx-sk-...` key is returned ONCE here and never again.
/// The dashboard should instruct users to copy it immediately.
pub async fn create_key(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let raw_key = db_api_keys::generate_key();

    // bcrypt hash stored as the canonical credential (verified at creation; not for auth)
    let key_hash = hash(&raw_key, DEFAULT_COST)
        .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;

    // SHA-256 hex for fast dashmap lookup
    let key_sha256 = db_api_keys::sha256_hex(&raw_key);

    // Display prefix (first 12 chars after the "vx-sk-" prefix = "vx-sk-" + 6 chars)
    let key_prefix: String = raw_key.chars().take(12).collect();

    let id = Uuid::new_v4();

    let key = db_api_keys::create(
        &state.pool,
        id,
        &body.name,
        &key_hash,
        &key_sha256,
        &key_prefix,
        body.workspace_id,
        body.budget_limit,
        body.rate_limit_rpm,
        body.rate_limit_tpm,
        body.allowed_models.clone(),
        body.expires_at,
    )
    .await?;

    // Immediately insert into the dashmap so subsequent requests work without restart
    let key_bytes = db_api_keys::sha256_bytes(&raw_key);
    state.key_cache.insert(key_bytes, key.clone());

    let response = CreateApiKeyResponse {
        id: key.id,
        name: key.name,
        key: raw_key,
        key_prefix: key.key_prefix,
        created_at: key.created_at,
    };

    Ok((StatusCode::CREATED, Json(json!({ "data": response }))))
}

/// GET /admin/keys — list all API keys (JWT-protected admin route).
pub async fn list_keys(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListKeysQuery>,
) -> AppResult<Json<Value>> {
    let page = params.page.max(1);
    let per_page = params.per_page.clamp(1, 100);

    let (keys, total) = db_api_keys::list(&state.pool, page, per_page).await?;

    let views: Vec<ApiKeyView> = keys.into_iter().map(ApiKeyView::from).collect();

    Ok(Json(json!({
        "data": views,
        "meta": {
            "page": page,
            "per_page": per_page,
            "total": total
        }
    })))
}
