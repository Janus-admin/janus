use crate::{
    db::providers as db_providers,
    enterprise::AuditEvent,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    models::provider::{ProviderView, UpdateProviderRequest},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /admin/providers — list all providers with health status.
#[utoipa::path(
    get,
    path = "/admin/providers",
    tag = "Providers",
    responses(
        (status = 200, description = "Provider list with health status", body = serde_json::Value),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_providers(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let providers = db_providers::list_providers(&state.pool).await?;
    let views: Vec<ProviderView> = providers.into_iter().map(ProviderView::from).collect();
    Ok(Json(json!({ "data": views })))
}

/// PATCH /admin/providers/:id — update a provider's config.
///
/// Persists to DB immediately. Changes to API keys take effect on next restart.
/// Changes to is_enabled / priority affect routing immediately via the DB record
/// (the running ProviderRegistry is seeded at startup, restart required for those too).
#[utoipa::path(
    patch,
    path = "/admin/providers/{id}",
    tag = "Providers",
    params(("id" = String, Path, description = "Provider ID, e.g. \"openai\"")),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Updated provider view", body = serde_json::Value),
        (status = 404, description = "Provider not found"),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn update_provider(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateProviderRequest>,
) -> AppResult<Json<Value>> {
    require_role(Role::Admin, &auth.0, &state).await?;

    // Encrypt the API key if one was supplied.
    let api_key_encrypted = if let Some(ref plaintext_key) = body.api_key {
        if plaintext_key.is_empty() {
            Some(None) // caller wants to clear the key
        } else if state.config.encryption_key.is_empty() {
            return Err(crate::errors::AppError::BadRequest(
                "encryption_key is not configured — cannot store provider API key".to_string(),
            ));
        } else {
            let aes_key = crate::crypto::parse_key(&state.config.encryption_key)
                .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;
            let encrypted = crate::crypto::encrypt(plaintext_key, &aes_key)
                .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;
            Some(Some(encrypted))
        }
    } else {
        None // no key field in request → leave unchanged
    };

    let params = db_providers::UpdateProviderParams {
        is_enabled: body.is_enabled,
        priority: body.priority,
        api_key_encrypted,
        base_url: body.base_url,
        timeout_ms: body.timeout_ms,
        max_retries: body.max_retries,
        health_status: None,
    };

    let provider = db_providers::update_provider(&state.pool, &id, params)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Provider {id}")))?;

    state.enterprise.audit(
        AuditEvent::new(
            "provider.update",
            "provider",
            Some(id.clone()),
            Some(auth.0.sub),
            Some(auth.0.email.clone()),
        )
        .with_metadata(serde_json::json!({
            "provider_id": id,
            "is_enabled": body.is_enabled,
            "priority": body.priority,
        })),
    );

    Ok(Json(json!({ "data": ProviderView::from(provider) })))
}

/// POST /admin/providers/:id/test — probe the provider and update health status.
///
/// Makes a lightweight HTTP request to verify the provider is reachable and that
/// the stored API key is valid. Updates health_status in DB and returns the result.
#[utoipa::path(
    post,
    path = "/admin/providers/{id}/test",
    tag = "Providers",
    params(("id" = String, Path, description = "Provider ID, e.g. \"openai\"")),
    responses(
        (status = 200, description = "Provider reachable; result body has health_status + http_status", body = serde_json::Value),
        (status = 503, description = "Provider unreachable", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn test_provider(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    require_role(Role::Admin, &auth.0, &state).await?;
    let provider = db_providers::get_provider(&state.pool, &id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Provider {id}")))?;

    // Decrypt the API key for the test request, falling back to config-level
    // env vars when the DB row has no encrypted key stored.
    let api_key: Option<String> = if let Some(encrypted) = &provider.api_key_encrypted {
        if state.config.encryption_key.is_empty() {
            None
        } else {
            crate::crypto::parse_key(&state.config.encryption_key)
                .ok()
                .and_then(|k| crate::crypto::decrypt(encrypted, &k).ok())
        }
    } else {
        match id.as_str() {
            "openai" if !state.config.openai_api_key.is_empty() => {
                Some(state.config.openai_api_key.clone())
            }
            "anthropic" if !state.config.anthropic_api_key.is_empty() => {
                Some(state.config.anthropic_api_key.clone())
            }
            "gemini" if !state.config.gemini_api_key.is_empty() => {
                Some(state.config.gemini_api_key.clone())
            }
            "groq" if !state.config.groq_api_key.is_empty() => {
                Some(state.config.groq_api_key.clone())
            }
            "deepseek" if !state.config.deepseek_api_key.is_empty() => {
                Some(state.config.deepseek_api_key.clone())
            }
            _ => None,
        }
    };

    // Resolve effective base_url (empty string means "use adapter default").
    let effective_base = if provider.base_url.is_empty() {
        match id.as_str() {
            "openai" | "groq" | "deepseek" => "https://api.openai.com/v1".to_string(),
            "anthropic" => "https://api.anthropic.com".to_string(),
            "gemini" => "https://generativelanguage.googleapis.com".to_string(),
            _ => provider.base_url.clone(),
        }
    } else {
        provider.base_url.clone()
    };

    // Build the per-provider check URL. Gemini takes its key as a query param;
    // all others authenticate via header and hit a `/models` listing endpoint.
    let check_url = match id.as_str() {
        "openai" => format!("{}/models", effective_base),
        "anthropic" => format!("{}/v1/models", effective_base),
        "gemini" => {
            let key = api_key.as_deref().unwrap_or("");
            format!("{}/v1beta/models?key={}", effective_base, key)
        }
        "groq" | "deepseek" => format!("{}/models", effective_base),
        _ => effective_base.clone(),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| crate::errors::AppError::Anyhow(anyhow::anyhow!(e)))?;

    let mut req = client.get(&check_url);
    if let Some(ref key) = api_key {
        req = match id.as_str() {
            "anthropic" => req
                .header("x-api-key", key.as_str())
                .header("anthropic-version", "2023-06-01"),
            "gemini" => req, // key already in query string
            _ => req.bearer_auth(key),
        };
    }

    let (health_status, reachable, http_status, error_msg) = match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            // 200 or 401 (key invalid but endpoint reachable) both mean "healthy endpoint".
            let healthy = status < 500;
            let hs = if healthy { "healthy" } else { "degraded" };
            (hs.to_string(), true, Some(status as i32), None)
        }
        Err(e) => {
            let msg = e.to_string();
            ("down".to_string(), false, None, Some(msg))
        }
    };

    db_providers::set_health_status(&state.pool, &id, &health_status).await?;

    let status_code = if reachable {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((
        status_code,
        Json(json!({
            "data": {
                "provider": id,
                "health_status": health_status,
                "reachable": reachable,
                "http_status": http_status,
                "error": error_msg,
            }
        })),
    ))
}
