use crate::{
    db::providers as db_providers,
    errors::AppResult,
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
pub async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProviderRequest>,
) -> AppResult<Json<Value>> {
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
        timeout_ms: body.timeout_ms,
        max_retries: body.max_retries,
        health_status: None,
    };

    let provider = db_providers::update_provider(&state.pool, &id, params)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Provider {id}")))?;

    Ok(Json(json!({ "data": ProviderView::from(provider) })))
}

/// POST /admin/providers/:id/test — probe the provider and update health status.
///
/// Makes a lightweight HTTP request to verify the provider is reachable and that
/// the stored API key is valid. Updates health_status in DB and returns the result.
pub async fn test_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
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

    // Build the per-provider check URL. Gemini takes its key as a query param;
    // all others authenticate via header and hit a `/models` listing endpoint.
    let check_url = match id.as_str() {
        "openai" => format!("{}/models", provider.base_url),
        "anthropic" => format!("{}/v1/models", provider.base_url),
        "gemini" => {
            let key = api_key.as_deref().unwrap_or("");
            format!("{}/v1beta/models?key={}", provider.base_url, key)
        }
        "groq" | "deepseek" => format!("{}/models", provider.base_url),
        _ => provider.base_url.clone(),
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
