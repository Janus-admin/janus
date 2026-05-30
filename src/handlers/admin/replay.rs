// src/handlers/admin/replay.rs — V4-6: Request Replay & Admin Playground
//
// POST /admin/requests/:id/replay
//   Load an existing request record, apply optional overrides, re-run it through
//   the full pipeline, and return the new response with extended metadata headers.
//   The original record is never modified. A new record is created with
//   `replay_of_request_id` pointing back to the original.
//
// POST /admin/playground
//   Same pipeline as /v1/chat/completions but authenticated with an admin JWT.
//   No budget or rate-limit checks are applied. The resulting request record is
//   flagged `is_playground = true`.

use crate::{
    db,
    errors::{AppError, AppResult},
    gateway::{pipeline, strategies::RoutingStrategy},
    models::api_key::ApiKey,
    pricing,
    providers::ChatCompletionRequest,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::HeaderValue,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};
use uuid::Uuid;

// ── Synthetic key for admin operations ───────────────────────────────────────

/// An ApiKey with no limits used for replay and playground requests.
/// The pipeline requires an ApiKey for plugin hooks, but budget/rate-limit
/// checks are skipped at the handler level before pipeline::run is called.
fn admin_key() -> ApiKey {
    ApiKey {
        id: Uuid::nil(),
        name: "admin-internal".to_string(),
        key_hash: String::new(),
        key_sha256: None,
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "admin".to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    }
}

// ── Replay ────────────────────────────────────────────────────────────────────

/// Optional override fields for a replay request.
/// All fields are optional; omitting them re-uses the original request settings.
#[derive(Debug, Deserialize, Default)]
pub struct ReplayOptions {
    /// Force a specific provider by its string ID (e.g. "openai").
    pub provider: Option<String>,
    /// Skip cache lookup and write for this replay (default false).
    pub skip_cache: Option<bool>,
    /// Override whether the replay uses SSE streaming (default: original setting).
    pub stream: Option<bool>,
    /// Override the model used for the replay.
    pub model: Option<String>,
}

/// POST /admin/requests/:id/replay
#[utoipa::path(
    post,
    path = "/admin/requests/{id}/replay",
    tag = "Requests",
    params(("id" = uuid::Uuid, Path, description = "Original request UUID")),
    request_body(
        content = serde_json::Value,
        description = "Optional overrides: provider, skip_cache, stream, model",
    ),
    responses(
        (status = 200, description = "Replayed response (JSON or SSE stream)"),
        (status = 404, description = "Original request not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn replay_request(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    body: Option<Json<ReplayOptions>>,
) -> AppResult<Response> {
    let opts = body.map(|Json(o)| o).unwrap_or_default();

    // Load original request record.
    let original = db::requests::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Request {id}")))?;

    // Ensure the original request body was stored — replaying is only possible when
    // request body logging was enabled when the original request was made.
    let raw_body = original.request_body.as_deref().ok_or_else(|| {
        AppError::BadRequest(
            "request_body is not available for this record (enable log_request_bodies \
             in the configuration to capture future request bodies)"
                .to_string(),
        )
    })?;

    // Deserialize back into a ChatCompletionRequest.
    let mut chat_req: ChatCompletionRequest = serde_json::from_str(raw_body)
        .map_err(|e| AppError::BadRequest(format!("Could not parse stored request body: {e}")))?;

    // Apply caller overrides.
    if let Some(ref model) = opts.model {
        chat_req.model = model.clone();
    }
    if let Some(stream) = opts.stream {
        chat_req.stream = Some(stream);
    }

    let bypass_cache = opts.skip_cache.unwrap_or(false);
    let key = admin_key();
    let rc = state.runtime_config.load();
    let max_retries = rc.max_retries;
    drop(rc);

    let cache_ttl_secs = state
        .config
        .cache_ttl_overrides
        .get(&chat_req.model)
        .copied()
        .unwrap_or(state.config.cache_ttl_secs);

    let strategy = RoutingStrategy::Priority;

    let start = Instant::now();
    let (resp, cache_hit) = pipeline::run(
        &state.pool,
        &state.providers,
        &chat_req,
        &key,
        max_retries,
        &state.cache,
        bypass_cache,
        bypass_cache,
        &strategy,
        &[],
        None,
        &state.plugins,
        &state.dedup,
        cache_ttl_secs,
        false,
        opts.provider.as_deref(),
        &serde_json::Value::Object(serde_json::Map::new()),
        "/admin/replay",
        &state.audit,
    )
    .await?;
    let latency_ms = start.elapsed().as_millis() as i64;

    let provider_name = resp.model.split('/').next().unwrap_or(&original.provider);
    let provider_used = original.provider.clone();

    // Calculate cost for metadata header.
    let cost = db::requests::find_pricing(&state.pool, &provider_used, &resp.model)
        .await
        .ok()
        .flatten()
        .map(|(input, output)| {
            pricing::calculate_cost(
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens,
                input,
                output,
            )
        });

    let cache_hit_str = match cache_hit {
        crate::cache::CacheHit::None => "none",
        crate::cache::CacheHit::Exact => "exact",
        crate::cache::CacheHit::Semantic(_) => "semantic",
    };
    let cache_type_opt = if cache_hit_str == "none" {
        None
    } else {
        Some(cache_hit_str)
    };

    // Store the replay record and get the new request ID.
    let request_body_str = serde_json::to_string(&chat_req).ok();
    let new_id = db::requests::insert_request_for_replay(
        &state.pool,
        &provider_used,
        &resp.model,
        Some(resp.usage.prompt_tokens as i32),
        Some(resp.usage.completion_tokens as i32),
        Some(resp.usage.total_tokens as i32),
        cost,
        latency_ms,
        "success",
        chat_req.stream.unwrap_or(false),
        request_body_str.as_deref(),
        Some(id),
        false,
        cache_type_opt,
    )
    .await?;

    let _ = provider_name; // suppress warning — used below

    // Build JSON response body.
    let body_val = json!({
        "data": {
            "request_id": new_id,
            "replay_of_request_id": id,
            "provider": provider_used,
            "model": resp.model,
            "latency_ms": latency_ms,
            "prompt_tokens": resp.usage.prompt_tokens,
            "completion_tokens": resp.usage.completion_tokens,
            "total_tokens": resp.usage.total_tokens,
            "cost_usd": cost,
            "cache_hit": cache_hit_str,
            "response": resp,
        }
    });

    let mut response = Json(body_val).into_response();
    let headers = response.headers_mut();
    headers.insert(
        "x-janus-request-id",
        new_id
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "x-janus-replay-of",
        id.to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "x-janus-latency-ms",
        latency_ms
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert("x-janus-cache-hit", HeaderValue::from_static(cache_hit_str));
    Ok(response)
}

// ── Playground ────────────────────────────────────────────────────────────────

/// Request body for `POST /admin/playground`.
///
/// Same shape as a regular chat completions request, with an extra optional
/// `skip_cache` field that bypasses the cache for this interactive session.
#[derive(Debug, Deserialize)]
pub struct PlaygroundRequest {
    #[serde(flatten)]
    pub request: ChatCompletionRequest,
    /// When true, bypass both exact and semantic cache (default false).
    pub skip_cache: Option<bool>,
}

/// POST /admin/playground
///
/// Interactive test console — same pipeline as `POST /v1/chat/completions` but:
/// - Authenticated with an admin JWT (not a gateway API key).
/// - No budget or rate-limit checks.
/// - Request records are flagged `is_playground = true`.
/// - Response includes extended metadata headers.
#[utoipa::path(
    post,
    path = "/admin/playground",
    tag = "Requests",
    request_body(
        content = serde_json::Value,
        description = "OpenAI-format ChatCompletion request",
    ),
    responses(
        (status = 200, description = "Pipeline response (JSON or SSE stream)"),
        (status = 400, description = "Invalid request body"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn playground(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    // Parse the skip_cache flag before deserializing the rest.
    let skip_cache = body
        .get("skip_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Deserialize the ChatCompletionRequest from the same body.
    let chat_req: ChatCompletionRequest =
        serde_json::from_value(body).map_err(|e| AppError::BadRequest(e.to_string()))?;

    let key = admin_key();
    let rc = state.runtime_config.load();
    let max_retries = rc.max_retries;
    drop(rc);

    let cache_ttl_secs = state
        .config
        .cache_ttl_overrides
        .get(&chat_req.model)
        .copied()
        .unwrap_or(state.config.cache_ttl_secs);

    let strategy = RoutingStrategy::Priority;

    let start = Instant::now();
    let (resp, cache_hit) = pipeline::run(
        &state.pool,
        &state.providers,
        &chat_req,
        &key,
        max_retries,
        &state.cache,
        skip_cache,
        skip_cache,
        &strategy,
        &[],
        None,
        &state.plugins,
        &state.dedup,
        cache_ttl_secs,
        false,
        None,
        &serde_json::Value::Object(serde_json::Map::new()),
        "/admin/playground",
        &state.audit,
    )
    .await?;
    let latency_ms = start.elapsed().as_millis() as i64;

    // Determine which provider served this response.
    // The response model may be "provider/model" or just "model".
    let provider_used = state
        .providers
        .providers()
        .iter()
        .find(|p| p.is_enabled())
        .map(|p| p.name().to_string())
        .unwrap_or_default();

    // Only charge cost when the provider was actually called.
    // Cache hits (exact or semantic) cost $0 — the response came from cache.
    let cost = if matches!(cache_hit, crate::cache::CacheHit::None) {
        db::requests::find_pricing(&state.pool, &provider_used, &resp.model)
            .await
            .ok()
            .flatten()
            .map(|(input, output)| {
                pricing::calculate_cost(
                    resp.usage.prompt_tokens,
                    resp.usage.completion_tokens,
                    input,
                    output,
                )
            })
    } else {
        Some(rust_decimal::Decimal::ZERO)
    };

    let cache_hit_str = match cache_hit {
        crate::cache::CacheHit::None => "none",
        crate::cache::CacheHit::Exact => "exact",
        crate::cache::CacheHit::Semantic(_) => "semantic",
    };
    let cache_type_opt = if cache_hit_str == "none" {
        None
    } else {
        Some(cache_hit_str)
    };

    let request_body_str = serde_json::to_string(&chat_req).ok();
    let new_id = db::requests::insert_request_for_replay(
        &state.pool,
        &provider_used,
        &resp.model,
        Some(resp.usage.prompt_tokens as i32),
        Some(resp.usage.completion_tokens as i32),
        Some(resp.usage.total_tokens as i32),
        cost,
        latency_ms,
        "success",
        chat_req.stream.unwrap_or(false),
        request_body_str.as_deref(),
        None,
        true,
        cache_type_opt,
    )
    .await?;

    let cost_str = cost.map(|c| c.to_string());

    let mut response = Json(serde_json::to_value(&resp).unwrap_or(json!({}))).into_response();
    let headers = response.headers_mut();

    headers.insert(
        "x-janus-request-id",
        new_id
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    if let Ok(v) = provider_used.parse() {
        headers.insert("x-janus-provider", v);
    }
    if let Ok(v) = resp.model.parse() {
        headers.insert("x-janus-model", v);
    }
    headers.insert(
        "x-janus-latency-ms",
        latency_ms
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "x-janus-prompt-tokens",
        resp.usage
            .prompt_tokens
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "x-janus-completion-tokens",
        resp.usage
            .completion_tokens
            .to_string()
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    if let Some(ref c) = cost_str {
        if let Ok(v) = c.parse() {
            headers.insert("x-janus-cost-usd", v);
        }
    }
    headers.insert("x-janus-cache-hit", HeaderValue::from_static(cache_hit_str));
    headers.insert("x-janus-playground", HeaderValue::from_static("true"));

    Ok(response)
}

// ── Playground Multi ──────────────────────────────────────────────────────────

/// POST /admin/playground/multi
///
/// Multi-model interactive test console. Sends the same prompt to every model
/// listed in `models` in parallel and returns all results in a single response.
///
/// Same pipeline as `POST /admin/playground` but fan-out to N models:
/// - Authenticated with an admin JWT (no gateway key required).
/// - No budget or rate-limit checks applied.
/// - Each model call is logged independently to the audit trail.
/// - Partial failures: a failed model returns `"error"` without cancelling others.
#[utoipa::path(
    post,
    path = "/admin/playground/multi",
    tag = "Requests",
    request_body(
        content = serde_json::Value,
        description = r#"Multi-model playground request. Same as playground but replace `model` with `models: [...]` array and add optional `skip_cache: bool`. Example: `{"models":["gpt-4o","claude-opus-4-5"],"messages":[...]}`"#,
    ),
    responses(
        (status = 200, description = "Array of per-model results", body = serde_json::Value),
        (status = 400, description = "models array is empty or request is malformed"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn playground_multi(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let skip_cache = body
        .get("skip_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let models: Vec<String> = match body
        .get("models")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(m) => m,
        None => {
            return AppError::BadRequest(
                "'models' must be a non-empty array of model identifiers".to_string(),
            )
            .into_response()
        }
    };

    if models.is_empty() {
        return AppError::BadRequest(
            "'models' must be a non-empty array of model identifiers".to_string(),
        )
        .into_response();
    }

    // Strip meta fields so the remainder is a valid ChatCompletionRequest base.
    let mut base_body = body.clone();
    if let Some(obj) = base_body.as_object_mut() {
        obj.remove("models");
        obj.remove("skip_cache");
    }

    let key = admin_key();

    let rc = state.runtime_config.load();
    let max_retries = rc.max_retries;
    drop(rc);

    let handles: Vec<_> = models
        .iter()
        .map(|model| {
            let mut model_body = base_body.clone();
            if let Some(obj) = model_body.as_object_mut() {
                obj.insert(
                    "model".to_string(),
                    serde_json::Value::String(model.clone()),
                );
                obj.remove("stream"); // playground/multi never streams
            }

            let chat_req: ChatCompletionRequest = match serde_json::from_value(model_body) {
                Ok(r) => r,
                Err(e) => {
                    let model = model.clone();
                    let msg = e.to_string();
                    return tokio::spawn(
                        async move { (model, Err(AppError::BadRequest(msg)), 0i64) },
                    );
                }
            };

            let pool = state.pool.clone();
            let registry = state.providers.clone();
            let cache = state.cache.clone();
            let plugins = state.plugins.clone();
            let dedup = state.dedup.clone();
            let audit = state.audit.clone();
            let key = key.clone();
            let model = model.clone();
            let cache_ttl_secs = state
                .config
                .cache_ttl_overrides
                .get(&model)
                .copied()
                .unwrap_or(state.config.cache_ttl_secs);
            let task_strategy = RoutingStrategy::Priority;

            tokio::spawn(async move {
                let start = Instant::now();
                let result = pipeline::run(
                    &pool,
                    &registry,
                    &chat_req,
                    &key,
                    max_retries,
                    &cache,
                    skip_cache,
                    skip_cache,
                    &task_strategy,
                    &[],
                    None,
                    &plugins,
                    &dedup,
                    cache_ttl_secs,
                    false,
                    None,
                    &serde_json::Value::Object(serde_json::Map::new()),
                    "/admin/playground/multi",
                    &audit,
                )
                .await;
                let latency_ms = start.elapsed().as_millis() as i64;
                (model, result, latency_ms)
            })
        })
        .collect();

    let join_results = futures_util::future::join_all(handles).await;

    let provider_used = state
        .providers
        .providers()
        .iter()
        .find(|p| p.is_enabled())
        .map(|p| p.name().to_string())
        .unwrap_or_default();

    let mut results = Vec::new();

    for jr in join_results {
        let (model, result, latency_ms) = match jr {
            Ok(inner) => inner,
            Err(_) => (
                String::new(),
                Err(AppError::ProviderUnavailable(
                    "internal task error".to_string(),
                )),
                0i64,
            ),
        };

        match result {
            Ok((resp, cache_hit)) => {
                let prompt_tokens = resp.usage.prompt_tokens;
                let completion_tokens = resp.usage.completion_tokens;

                let cache_hit_str = match cache_hit {
                    crate::cache::CacheHit::None => "none",
                    crate::cache::CacheHit::Exact => "exact",
                    crate::cache::CacheHit::Semantic(_) => "semantic",
                };

                let cost = if matches!(cache_hit, crate::cache::CacheHit::None) {
                    db::requests::find_pricing(&state.pool, &provider_used, &resp.model)
                        .await
                        .ok()
                        .flatten()
                        .map(|(inp, out)| {
                            pricing::calculate_cost(prompt_tokens, completion_tokens, inp, out)
                        })
                } else {
                    Some(rust_decimal::Decimal::ZERO)
                };

                let text = resp
                    .choices
                    .first()
                    .and_then(|c| c.message.content.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                results.push(json!({
                    "model": model,
                    "text": text,
                    "latency_ms": latency_ms,
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "cost_usd": cost.map(|c| c.to_string()),
                    "cache_hit": cache_hit_str,
                    "error": null,
                }));
            }
            Err(e) => {
                results.push(json!({
                    "model": model,
                    "text": null,
                    "latency_ms": latency_ms,
                    "prompt_tokens": 0,
                    "completion_tokens": 0,
                    "cost_usd": null,
                    "cache_hit": "none",
                    "error": e.to_string(),
                }));
            }
        }
    }

    Json(json!({ "results": results })).into_response()
}
