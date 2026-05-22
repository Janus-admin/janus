use crate::{
    cache::CacheHit, errors::AppError, gateway::pipeline, middleware::api_key_auth::GatewayAuth,
    middleware::budget::check_budget, pii, providers::ChatCompletionRequest, state::AppState,
};
use axum::{
    extract::{rejection::JsonRejection, FromRequest, State},
    http::{HeaderMap, HeaderValue, Request},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

// ── ValidatedJson extractor ───────────────────────────────────────────────────

/// Wraps `axum::Json` so that deserialization failures become `AppError::BadRequest`
/// (HTTP 400) instead of axum's default `422 Unprocessable Entity`.
pub struct ValidatedJson<T>(pub T);

#[axum::async_trait]
impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = AppError;

    async fn from_request(
        req: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ValidatedJson(value)),
            Err(rejection) => Err(AppError::BadRequest(rejection.body_text())),
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// POST /v1/chat/completions
///
/// Drop-in replacement for the OpenAI Chat Completions endpoint.
/// Supports both non-streaming (default) and SSE streaming (`"stream": true`).
///
/// Request header `X-Velox-Cache: false` bypasses the cache for that request.
/// Response header `X-Velox-Cache-Hit: exact` is present on exact cache hits.
/// Response header `X-Velox-Cache-Hit: semantic` + `X-Velox-Cache-Similarity: 0.97`
/// are present on semantic cache hits.
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    headers: HeaderMap,
    ValidatedJson(request): ValidatedJson<ChatCompletionRequest>,
) -> impl IntoResponse {
    // Budget gate.
    if let Err(e) = check_budget(&api_key) {
        return e.into_response();
    }

    // Rate limit gate.
    if let Some(rpm) = api_key.rate_limit_rpm {
        if let Err(retry_after) = state.rate_limiter.check_and_record(api_key.id, rpm) {
            return AppError::RateLimitExceeded(Some(retry_after)).into_response();
        }
    }

    // Model restriction check.
    if let Some(ref allowed) = api_key.allowed_models {
        if !allowed.is_empty() && !allowed.contains(&request.model) {
            return AppError::Forbidden(format!(
                "Model '{}' is not permitted for this API key",
                request.model
            ))
            .into_response();
        }
    }

    // Bypass cache when disabled globally or when the client sends X-Velox-Cache: false.
    let bypass_cache = !state.config.cache_enabled
        || headers
            .get("x-velox-cache")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("false"))
            .unwrap_or(false);

    let max_retries = state.config.max_retries;

    if state.config.log_request_bodies {
        if let Ok(raw) = serde_json::to_string(&request) {
            tracing::debug!(body = %pii::scrub(&raw), "gateway request body");
        }
    }

    if request.stream == Some(true) {
        let start = Instant::now();
        match pipeline::run_streaming(
            state.pool.clone(),
            state.providers.clone(),
            request.clone(),
            api_key.clone(),
            max_retries,
            state.cache.clone(),
            bypass_cache,
        )
        .await
        {
            Ok((mut response, cache_hit)) => {
                attach_cache_headers(response.headers_mut(), &cache_hit);
                broadcast_event(
                    &state,
                    &request.model,
                    api_key.id,
                    None,
                    None,
                    start.elapsed().as_millis() as i64,
                    "success",
                    &cache_hit,
                    true,
                );
                response
            }
            Err(e) => {
                broadcast_event(
                    &state,
                    &request.model,
                    api_key.id,
                    None,
                    None,
                    start.elapsed().as_millis() as i64,
                    "error",
                    &CacheHit::None,
                    true,
                );
                e.into_response()
            }
        }
    } else {
        let start = Instant::now();
        match pipeline::run(
            &state.pool,
            &state.providers,
            &request,
            &api_key,
            max_retries,
            &state.cache,
            bypass_cache,
        )
        .await
        {
            Ok((resp, cache_hit)) => {
                let latency_ms = start.elapsed().as_millis() as i64;
                broadcast_event(
                    &state,
                    &resp.model,
                    api_key.id,
                    Some(resp.usage.prompt_tokens),
                    Some(resp.usage.total_tokens),
                    latency_ms,
                    "success",
                    &cache_hit,
                    false,
                );
                if state.config.log_response_bodies {
                    if let Ok(raw) = serde_json::to_string(&resp) {
                        tracing::debug!(body = %raw, "gateway response body");
                    }
                }
                match serde_json::to_value(resp) {
                    Ok(v) => {
                        let mut response = Json::<Value>(v).into_response();
                        attach_cache_headers(response.headers_mut(), &cache_hit);
                        response
                    }
                    Err(e) => {
                        AppError::Anyhow(anyhow::anyhow!("Failed to serialize response: {e}"))
                            .into_response()
                    }
                }
            }
            Err(e) => {
                broadcast_event(
                    &state,
                    &request.model,
                    api_key.id,
                    None,
                    None,
                    start.elapsed().as_millis() as i64,
                    "error",
                    &CacheHit::None,
                    false,
                );
                e.into_response()
            }
        }
    }
}

// ── Broadcast helper ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn broadcast_event(
    state: &AppState,
    model: &str,
    api_key_id: uuid::Uuid,
    prompt_tokens: Option<u32>,
    total_tokens: Option<u32>,
    latency_ms: i64,
    status: &str,
    cache_hit: &CacheHit,
    stream: bool,
) {
    let cache_type = match cache_hit {
        CacheHit::None => serde_json::Value::Null,
        CacheHit::Exact => json!("exact"),
        CacheHit::Semantic(_) => json!("semantic"),
    };
    let similarity = match cache_hit {
        CacheHit::Semantic(s) => json!(s),
        _ => serde_json::Value::Null,
    };
    let event = json!({
        "model":        model,
        "api_key_id":   api_key_id,
        "prompt_tokens": prompt_tokens,
        "total_tokens": total_tokens,
        "latency_ms":   latency_ms,
        "status":       status,
        "cache_type":   cache_type,
        "similarity":   similarity,
        "stream":       stream,
        "ts":           Utc::now().to_rfc3339(),
    });
    // send() only fails when there are zero active receivers — safe to ignore.
    let _ = state.event_tx.send(event);
}

// ── Header helpers ────────────────────────────────────────────────────────────

fn attach_cache_headers(headers: &mut axum::http::HeaderMap, hit: &CacheHit) {
    match hit {
        CacheHit::None => {}
        CacheHit::Exact => {
            headers.insert("x-velox-cache-hit", HeaderValue::from_static("exact"));
        }
        CacheHit::Semantic(score) => {
            headers.insert("x-velox-cache-hit", HeaderValue::from_static("semantic"));
            if let Ok(v) = HeaderValue::from_str(&format!("{score:.4}")) {
                headers.insert("x-velox-cache-similarity", v);
            }
        }
    }
}
