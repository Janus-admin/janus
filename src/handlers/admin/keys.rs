use crate::{
    db::api_keys as db_api_keys,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
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

// ── Key rotation ──────────────────────────────────────────────────────────────

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
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
/// The full `jn-sk-...` key is returned ONCE here and never again.
/// The dashboard should instruct users to copy it immediately.
#[utoipa::path(
    post,
    path = "/admin/keys",
    tag = "Keys",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "Key created — full secret returned ONCE", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden — requires ApiManager role or higher"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn create_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CreateApiKeyRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    require_role(Role::ApiManager, &auth.0, &state).await?;
    let raw_key = db_api_keys::generate_key();

    // bcrypt hash stored as the canonical credential (verified at creation; not for auth)
    let key_hash = hash(&raw_key, DEFAULT_COST)
        .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;

    // SHA-256 hex for fast dashmap lookup
    let key_sha256 = db_api_keys::sha256_hex(&raw_key);

    // Display prefix (first 12 chars after the "jn-sk-" prefix = "jn-sk-" + 6 chars)
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
        &body.routing_strategy,
        body.downgrade_at_percent,
        body.downgrade_strategy.as_deref(),
        body.downgrade_to_model.as_deref(),
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
        routing_strategy: key.routing_strategy.clone(),
        created_at: key.created_at,
    };

    Ok((StatusCode::CREATED, Json(json!({ "data": response }))))
}

/// GET /admin/keys — list all API keys (JWT-protected admin route).
#[utoipa::path(
    get,
    path = "/admin/keys",
    tag = "Keys",
    params(ListKeysQuery),
    responses(
        (status = 200, description = "Paginated list of API keys", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_jwt" = [])),
)]
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

/// GET /admin/keys/:id — get a single key by ID.
#[utoipa::path(
    get,
    path = "/admin/keys/{id}",
    tag = "Keys",
    params(("id" = uuid::Uuid, Path, description = "API key UUID")),
    responses(
        (status = 200, description = "Key details", body = serde_json::Value),
        (status = 404, description = "Key not found"),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn get_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> AppResult<Json<Value>> {
    let key = db_api_keys::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("API key {id}")))?;

    Ok(Json(json!({ "data": ApiKeyView::from(key) })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateKeyRequest {
    pub name: Option<String>,
    /// Pass `null` explicitly to clear the budget limit.
    pub budget_limit: Option<serde_json::Value>,
    pub rate_limit_rpm: Option<serde_json::Value>,
    pub rate_limit_tpm: Option<serde_json::Value>,
    pub allowed_models: Option<serde_json::Value>,
    pub expires_at: Option<serde_json::Value>,
    pub is_active: Option<bool>,
    pub routing_strategy: Option<String>,
    /// Pass `null` to clear downgrade threshold.
    pub downgrade_at_percent: Option<serde_json::Value>,
    pub downgrade_strategy: Option<serde_json::Value>,
    pub downgrade_to_model: Option<serde_json::Value>,
}

/// PATCH /admin/keys/:id — update mutable key fields.
#[utoipa::path(
    patch,
    path = "/admin/keys/{id}",
    tag = "Keys",
    params(("id" = uuid::Uuid, Path, description = "API key UUID")),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Updated key view", body = serde_json::Value),
        (status = 404, description = "Key not found"),
        (status = 403, description = "Forbidden — requires ApiManager role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn update_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<UpdateKeyRequest>,
) -> AppResult<Json<Value>> {
    require_role(Role::ApiManager, &auth.0, &state).await?;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // Helper: turn a JSON Value into Option<Option<T>> where outer None = "not provided"
    // and inner None = "caller wants to clear the field".
    fn parse_opt_decimal(v: Option<serde_json::Value>) -> Option<Option<Decimal>> {
        match v {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(ref s) if s.is_string() => s
                .as_str()
                .and_then(|s| Decimal::from_str(s).ok())
                .map(|d| Some(Some(d)))
                .unwrap_or(None),
            Some(ref n) if n.is_number() => n
                .as_f64()
                .and_then(|f| Decimal::from_str(&f.to_string()).ok())
                .map(|d| Some(Some(d)))
                .unwrap_or(None),
            _ => None,
        }
    }

    fn parse_opt_i32(v: Option<serde_json::Value>) -> Option<Option<i32>> {
        match v {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(ref n) => n.as_i64().map(|i| Some(Some(i as i32))).unwrap_or(None),
        }
    }

    fn parse_opt_strings(v: Option<serde_json::Value>) -> Option<Option<Vec<String>>> {
        match v {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(serde_json::Value::Array(arr)) => {
                let strs: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect();
                Some(Some(strs))
            }
            _ => None,
        }
    }

    fn parse_opt_datetime(
        v: Option<serde_json::Value>,
    ) -> Option<Option<chrono::DateTime<chrono::Utc>>> {
        match v {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(ref s) if s.is_string() => s
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| Some(Some(dt.with_timezone(&chrono::Utc))))
                .unwrap_or(None),
            _ => None,
        }
    }

    fn parse_opt_string(v: Option<serde_json::Value>) -> Option<Option<String>> {
        match v {
            None => None,
            Some(serde_json::Value::Null) => Some(None),
            Some(ref s) if s.is_string() => Some(s.as_str().map(str::to_string)),
            _ => None,
        }
    }

    let params = db_api_keys::UpdateKeyParams {
        name: body.name,
        budget_limit: parse_opt_decimal(body.budget_limit),
        rate_limit_rpm: parse_opt_i32(body.rate_limit_rpm),
        rate_limit_tpm: parse_opt_i32(body.rate_limit_tpm),
        allowed_models: parse_opt_strings(body.allowed_models),
        expires_at: parse_opt_datetime(body.expires_at),
        is_active: body.is_active,
        routing_strategy: body.routing_strategy,
        downgrade_at_percent: parse_opt_i32(body.downgrade_at_percent),
        downgrade_strategy: parse_opt_string(body.downgrade_strategy),
        downgrade_to_model: parse_opt_string(body.downgrade_to_model),
    };

    let key = db_api_keys::update_key(&state.pool, id, params)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("API key {id}")))?;

    Ok(Json(json!({ "data": ApiKeyView::from(key) })))
}

/// POST /admin/keys/:id/rotate — generate a new secret for an existing key.
///
/// The old secret remains valid for `config.rotation_grace_period_secs` (default 300 s)
/// so callers have time to swap to the new key without a hard cutover. After the grace
/// period expires the old key is rejected by the auth middleware.
///
/// The new full key is returned ONCE here. Copy it immediately — it is never stored.
#[utoipa::path(
    post,
    path = "/admin/keys/{id}/rotate",
    tag = "Keys",
    params(("id" = uuid::Uuid, Path, description = "API key UUID")),
    responses(
        (status = 200, description = "Rotated — new secret returned ONCE; old secret valid during grace window", body = serde_json::Value),
        (status = 404, description = "Key not found"),
        (status = 403, description = "Forbidden — requires ApiManager role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn rotate_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> AppResult<(StatusCode, Json<Value>)> {
    require_role(Role::ApiManager, &auth.0, &state).await?;
    let new_raw_key = db_api_keys::generate_key();
    let new_key_hash = hash(&new_raw_key, DEFAULT_COST)
        .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;
    let new_key_sha256 = db_api_keys::sha256_hex(&new_raw_key);
    let new_key_prefix: String = new_raw_key.chars().take(12).collect();

    let grace = state.config.rotation_grace_period_secs;

    let updated = db_api_keys::rotate_key(
        &state.pool,
        id,
        &new_key_sha256,
        &new_key_hash,
        &new_key_prefix,
        grace,
    )
    .await?
    .ok_or_else(|| crate::errors::AppError::NotFound(format!("API key {id}")))?;

    // Register the new hash in the dashmap immediately.
    let new_hash_bytes = db_api_keys::sha256_bytes(&new_raw_key);
    state.key_cache.insert(new_hash_bytes, updated.clone());

    // Also register the old (previous) hash so it remains valid during the grace period.
    if let Some(ref prev_hex) = updated.previous_key_sha256 {
        if let Some(prev_bytes) = db_api_keys::parse_sha256_hex(prev_hex) {
            state.key_cache.insert(prev_bytes, updated.clone());
        }
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "id": updated.id,
                "key": new_raw_key,
                "key_prefix": updated.key_prefix,
                "rotation_expires_at": updated.rotation_expires_at,
            }
        })),
    ))
}

/// DELETE /admin/keys/:id — revoke (deactivate) a key.
///
/// Soft-delete: sets `is_active = false` without removing the record.
/// The key is removed from the in-memory dashmap so it stops working immediately.
/// In cluster mode a `pg_notify` is issued so other nodes also evict the key.
#[utoipa::path(
    delete,
    path = "/admin/keys/{id}",
    tag = "Keys",
    params(("id" = uuid::Uuid, Path, description = "API key UUID")),
    responses(
        (status = 200, description = "Key revoked", body = serde_json::Value),
        (status = 404, description = "Key not found"),
        (status = 403, description = "Forbidden — requires ApiManager role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn revoke_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> AppResult<(axum::http::StatusCode, Json<Value>)> {
    require_role(Role::ApiManager, &auth.0, &state).await?;
    // Revoke in DB.
    let deleted = db_api_keys::revoke_key(&state.pool, id).await?;
    if !deleted {
        return Err(crate::errors::AppError::NotFound(format!("API key {id}")));
    }

    // Evict from local dashmap so the key stops working immediately.
    let mut sha256_hex_opt: Option<String> = None;
    if let Some(key) = db_api_keys::get_by_id(&state.pool, id).await? {
        if let Some(ref sha256_hex) = key.key_sha256 {
            if let Some(hash) = db_api_keys::parse_sha256_hex(sha256_hex) {
                state.key_cache.remove(&hash);
            }
            sha256_hex_opt = Some(sha256_hex.clone());
        }
    }

    // In cluster mode: broadcast the revocation to other nodes via pg_notify.
    #[cfg(not(feature = "sqlite"))]
    if state.config.cluster.enabled {
        if let Some(ref sha256_hex) = sha256_hex_opt {
            let _ = sqlx::query("SELECT pg_notify('api_key_invalidated', $1)")
                .bind(sha256_hex.as_str())
                .execute(&state.pool)
                .await;
        }
    }

    Ok((
        axum::http::StatusCode::OK,
        Json(json!({ "data": { "revoked": true } })),
    ))
}
