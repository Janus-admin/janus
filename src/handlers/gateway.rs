use crate::{
    cache::{exact, CacheHit},
    db,
    errors::AppError,
    gateway::{pipeline, smart_router::SmartRouter, strategies::RoutingStrategy},
    middleware::api_key_auth::GatewayAuth,
    middleware::budget::{check_budget, DowngradeDecision},
    pii, pricing,
    pricing::calculate_cost,
    prompts::template,
    providers::{
        ChatCompletionRequest, ChatMessage, EmbeddingRequest, ImagesRequest, ModelInfo,
        SpeechRequest, TranscribeRequest,
    },
    state::AppState,
    telemetry,
};
use axum::{
    body::Body,
    extract::{rejection::JsonRejection, FromRequest, Multipart, State},
    http::{HeaderMap, HeaderValue, Request},
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use chrono::Utc;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

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
/// Request header `X-Janus-Cache: false` bypasses the cache for that request.
/// Response header `X-Janus-Cache-Hit: exact` is present on exact cache hits.
/// Response header `X-Janus-Cache-Hit: semantic` + `X-Janus-Cache-Similarity: 0.97`
/// are present on semantic cache hits.
#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    tag = "Gateway",
    request_body(
        content = serde_json::Value,
        description = "OpenAI-format ChatCompletionRequest (model, messages, stream, etc.)",
    ),
    responses(
        (status = 200, description = "OpenAI-format ChatCompletion (JSON) or SSE stream when `stream: true`"),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded for this key"),
        (status = 429, description = "Rate limit exceeded — `Retry-After` header present"),
        (status = 503, description = "All providers unavailable"),
    ),
    security(("api_key" = [])),
)]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    headers: HeaderMap,
    ValidatedJson(request): ValidatedJson<ChatCompletionRequest>,
) -> impl IntoResponse {
    // Root span for this gateway request.
    // W3C `traceparent` from the incoming request becomes the parent context so
    // end-to-end traces are linked when Janus sits behind an instrumented caller.
    let span = tracing::info_span!(
        "janus.request",
        otel.kind = "server",
        janus.model = %request.model,  // may be empty; smart router fills it in inner()
        janus.api_key_id = %api_key.id,
        janus.cache_hit = tracing::field::Empty,
        http.status_code = tracing::field::Empty,
    );
    {
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        span.set_parent(telemetry::extract_context(&headers));
    }
    chat_completions_inner(state, api_key, headers, request)
        .instrument(span)
        .await
}

async fn chat_completions_inner(
    state: Arc<AppState>,
    api_key: crate::models::api_key::ApiKey,
    headers: HeaderMap,
    mut request: ChatCompletionRequest,
) -> impl IntoResponse {
    // ── Layer 0 / Smart Router ────────────────────────────────────────────────
    // When model is absent the Smart Router fills it in before any other gate.
    // When model is set we skip the router entirely for full backward compat.
    let routing_decision = if request.model.is_empty() {
        if !state.config.smart_routing.enabled {
            return AppError::BadRequest(
                "The 'model' field is required. Enable smart routing \
                 (smart_routing.enabled = true) to allow automatic model selection."
                    .to_string(),
            )
            .into_response();
        }
        // Parse X-Janus-Tags header for the router's Layer 2 tag matching.
        let extra_tags = headers
            .get("x-janus-tags")
            .and_then(|v| v.to_str().ok())
            .map(crate::gateway::smart_router::parse_tag_header)
            .unwrap_or_default();

        match SmartRouter::select(
            &state.pool,
            &request,
            api_key.workspace_id,
            &extra_tags,
            &state.config.smart_routing,
            api_key.allowed_models.as_deref(),
        )
        .await
        {
            Ok(decision) => {
                tracing::info!(
                    model = %decision.model,
                    reason = %decision.reason.header_value(),
                    "Smart router selected model"
                );
                request.model = decision.model.clone();
                Some(decision)
            }
            Err(e) => return e.into_response(),
        }
    } else {
        None
    };

    // Budget gate — also returns a DowngradeDecision when spend nears the limit.
    let downgrade = {
        let result = tracing::info_span!("janus.budget.check")
            .in_scope(|| check_budget(&api_key, &state.config.budget_downgrade));
        match result {
            Ok(d) => d,
            Err(e) => return e.into_response(),
        }
    };
    let downgrade_triggered = !matches!(downgrade, DowngradeDecision::None);

    // Rate limit gate (RPM).
    // Cluster mode: use DB-backed sliding window shared across all nodes.
    // Single-node mode: use fast in-memory DashMap.
    let rate_limit_result = async {
        if let Some(rpm) = api_key.rate_limit_rpm {
            if let Some(ref cluster_rl) = state.cluster_rate_limiter {
                if let Err(retry_after) = cluster_rl.check_and_record(api_key.id, rpm).await {
                    return Err(AppError::RateLimitExceeded(Some(retry_after)));
                }
            } else if let Err(retry_after) = state.rate_limiter.check_and_record(api_key.id, rpm) {
                return Err(AppError::RateLimitExceeded(Some(retry_after)));
            }
        }
        Ok::<(), AppError>(())
    }
    .instrument(tracing::info_span!("janus.rate_limit.check"))
    .await;
    if let Err(e) = rate_limit_result {
        return e.into_response();
    }

    // Token-per-minute gate (TPM) — rough pre-flight estimate.
    if let Some(tpm) = api_key.rate_limit_tpm {
        let estimated_tokens = estimate_request_tokens(&request);
        if let Some(ref cluster_rl) = state.cluster_rate_limiter {
            if let Err(retry_after) = cluster_rl
                .check_and_record_tokens(api_key.id, estimated_tokens, tpm)
                .await
            {
                return AppError::RateLimitExceeded(Some(retry_after)).into_response();
            }
        } else if let Err(retry_after) =
            state
                .rate_limiter
                .check_and_record_tokens(api_key.id, estimated_tokens, tpm)
        {
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

    // ── Prompt injection (X-Janus-Prompt header) ─────────────────────────────
    // If the caller supplies a prompt ID we load the active version, render the
    // template with variables from X-Janus-Variables, and prepend the result to
    // the messages array.  Requests without this header are unaffected.
    let mut request = request;
    let prompt_version_id: Option<Uuid> = if let Some(pid_str) =
        headers.get("x-janus-prompt").and_then(|v| v.to_str().ok())
    {
        match Uuid::parse_str(pid_str) {
            Err(_) => {
                return AppError::BadRequest("X-Janus-Prompt must be a valid UUID".to_string())
                    .into_response();
            }
            Ok(prompt_id) => match db::prompts::get_active_versions(&state.pool, prompt_id).await {
                Err(e) => {
                    return AppError::Anyhow(anyhow::anyhow!("Failed to load prompt: {e}"))
                        .into_response();
                }
                Ok(versions) if versions.is_empty() => {
                    return AppError::NotFound(format!("No active version for prompt {prompt_id}"))
                        .into_response();
                }
                Ok(versions) => {
                    let selected = select_version_by_weight(&versions);
                    let variables = parse_variables_header(&headers);
                    let rendered = template::render(&selected.content, &variables);
                    inject_prompt(&mut request, selected.system_prompt.as_deref(), &rendered);
                    Some(selected.id)
                }
            },
        }
    } else {
        None
    };

    // Snapshot mutable config once per request.
    let rc = state.runtime_config.load();

    // Bypass cache when disabled globally or when the client sends X-Janus-Cache: false.
    let explicit_bypass = !rc.cache_enabled
        || headers
            .get("x-janus-cache")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("false"))
            .unwrap_or(false);

    // Time-sensitive detection: skip cache for time-bound prompts.
    let time_sensitive = !explicit_bypass && state.time_guard.is_time_sensitive(&request);
    let bypass_cache = explicit_bypass || time_sensitive;

    // Bypass semantic cache when the policy excludes this model/route/key combination.
    // Exact cache is unaffected by this flag.
    let bypass_semantic = bypass_cache
        || !state
            .semantic_policy
            .allows(&request.model, "/v1/chat/completions", &api_key.name);

    let max_retries = rc.max_retries;

    // Compute effective cache TTL: per-model override takes precedence over global.
    let cache_ttl_secs = state
        .config
        .cache_ttl_overrides
        .get(&request.model)
        .copied()
        .unwrap_or(state.config.cache_ttl_secs);

    if rc.log_request_bodies {
        if let Ok(raw) = serde_json::to_string(&request) {
            tracing::debug!(body = %pii::scrub(&raw), "gateway request body");
        }
    }

    let log_response_bodies = rc.log_response_bodies;
    drop(rc); // release read lock before the potentially-long provider call

    // Parse routing strategy — downgrade may override the key's default strategy.
    let strategy = match &downgrade {
        DowngradeDecision::UseStrategy(s) => RoutingStrategy::from_db_str(s),
        _ => RoutingStrategy::from_db_str(&api_key.routing_strategy),
    };
    // Model override: apply before fallback lookup so fallbacks are model-aware.
    if let DowngradeDecision::UseModel(ref m) = downgrade {
        request.model = m.clone();
    }
    let fallback_models: Vec<String> = state
        .config
        .routing
        .fallbacks
        .get(&request.model)
        .cloned()
        .unwrap_or_default();

    // V5-L3: extract cost tags from metadata field + X-Janus-Tags header.
    let tags = extract_tags(&request, &headers);

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
            bypass_semantic,
            strategy,
            fallback_models,
            prompt_version_id,
            state.plugins.clone(),
            cache_ttl_secs,
            downgrade_triggered,
            tags.clone(),
            "/v1/chat/completions",
            state.audit.clone(),
        )
        .await
        {
            Ok((mut response, cache_hit)) => {
                attach_cache_headers(response.headers_mut(), &cache_hit);
                attach_routing_headers(response.headers_mut(), &routing_decision);
                if time_sensitive {
                    response.headers_mut().insert(
                        "x-janus-cache-skip",
                        HeaderValue::from_static("time_sensitive"),
                    );
                }
                if downgrade_triggered {
                    if let Ok(v) = HeaderValue::from_str(downgrade.header_value()) {
                        response.headers_mut().insert("x-janus-downgraded", v);
                    }
                }
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
            bypass_semantic,
            &strategy,
            &fallback_models,
            prompt_version_id,
            &state.plugins,
            &state.dedup,
            cache_ttl_secs,
            downgrade_triggered,
            None,
            &tags,
            "/v1/chat/completions",
            &state.audit,
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
                if log_response_bodies {
                    if let Ok(raw) = serde_json::to_string(&resp) {
                        tracing::debug!(body = %raw, "gateway response body");
                    }
                }
                match serde_json::to_value(resp) {
                    Ok(v) => {
                        let mut response = Json::<Value>(v).into_response();
                        attach_cache_headers(response.headers_mut(), &cache_hit);
                        attach_routing_headers(response.headers_mut(), &routing_decision);
                        if time_sensitive {
                            response.headers_mut().insert(
                                "x-janus-cache-skip",
                                HeaderValue::from_static("time_sensitive"),
                            );
                        }
                        if downgrade_triggered {
                            if let Ok(v) = HeaderValue::from_str(downgrade.header_value()) {
                                response.headers_mut().insert("x-janus-downgraded", v);
                            }
                        }
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

// ── GET /v1/models ────────────────────────────────────────────────────────────

const MODELS_CACHE_TTL_SECS: u64 = 5;

/// GET /v1/models
///
/// Aggregates models from every enabled provider (those that implement
/// `Provider::list_models`) and unions with the active rows from
/// `model_pricing`. Result is cached in-memory for 5 seconds.
/// No auth required — matches OpenAI behaviour.
#[utoipa::path(
    get,
    path = "/v1/models",
    tag = "Gateway",
    responses(
        (status = 200, description = "OpenAI-format `{object: \"list\", data: [...]}` model list", body = serde_json::Value),
    ),
)]
pub async fn list_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Cache hit (within TTL): return immediately.
    {
        let guard = state.models_cache.lock().unwrap();
        if let Some((ts, ref cached)) = *guard {
            if ts.elapsed().as_secs() < MODELS_CACHE_TTL_SECS {
                return Json(cached.clone()).into_response();
            }
        }
    }

    let mut by_id: std::collections::BTreeMap<String, ModelInfo> =
        std::collections::BTreeMap::new();

    // 1. Live provider aggregation — best effort, errors silently dropped.
    for provider in state.providers.providers() {
        if !provider.is_enabled() {
            continue;
        }
        if let Ok(models) = provider.list_models().await {
            for m in models {
                by_id.entry(m.id.clone()).or_insert(m);
            }
        }
    }

    // 2. Augment with DB catalogue so non-introspecting providers still appear.
    #[derive(sqlx::FromRow)]
    struct ModelRow {
        provider: String,
        model_id: String,
    }

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let active_clause = "WHERE is_active = TRUE";
    #[cfg(feature = "sqlite")]
    let active_clause = "WHERE is_active = 1";

    let sql = format!(
        "SELECT provider, model_id FROM model_pricing {active_clause} ORDER BY provider, model_id"
    );

    if let Ok(rows) = sqlx::query_as::<_, ModelRow>(&sql)
        .fetch_all(&state.pool)
        .await
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        for r in rows {
            by_id.entry(r.model_id.clone()).or_insert(ModelInfo {
                id: r.model_id,
                object: "model".to_string(),
                created: now,
                owned_by: r.provider,
            });
        }
    }

    let aggregated: Vec<ModelInfo> = by_id.into_values().collect();
    let body = serde_json::json!({ "object": "list", "data": aggregated });

    // Refresh cache.
    {
        let mut guard = state.models_cache.lock().unwrap();
        *guard = Some((std::time::Instant::now(), body.clone()));
    }

    Json(body).into_response()
}

// ── POST /v1/images/generations ───────────────────────────────────────────────

/// POST /v1/images/generations
///
/// OpenAI-compatible image generation. No caching (high cost, low repetition).
/// Cost is per-image, looked up from `model_pricing.price_per_image`.
#[utoipa::path(
    post,
    path = "/v1/images/generations",
    tag = "Gateway",
    request_body(content = serde_json::Value, description = "OpenAI-format image generation request"),
    responses(
        (status = 200, description = "OpenAI-format image response", body = serde_json::Value),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded"),
        (status = 503, description = "Provider unavailable"),
    ),
    security(("api_key" = [])),
)]
pub async fn images_generations(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<ImagesRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_budget(&api_key, &state.config.budget_downgrade) {
        return e.into_response();
    }

    let provider = match crate::gateway::router::select_provider(&state.providers, &request.model) {
        Some(p) => p,
        None => {
            return AppError::ProviderUnavailable("No providers available".into()).into_response()
        }
    };

    let start = Instant::now();
    let result = provider.images_generate(&request).await;
    let latency_ms = start.elapsed().as_millis() as i32;

    match result {
        Ok(resp) => {
            let image_count = resp.data.len() as u32;
            let (price_per_image, _, _) =
                db::requests::find_modality_pricing(&state.pool, provider.name(), &request.model)
                    .await
                    .unwrap_or((None, None, None));
            let cost = price_per_image.map(|p| pricing::calculate_image_cost(image_count, p));

            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &request.model,
                cost,
                latency_ms,
                "success",
                "images",
                "/v1/images/generations",
            )
            .await;

            Json(resp).into_response()
        }
        Err(e) => {
            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &request.model,
                None,
                latency_ms,
                "error",
                "images",
                "/v1/images/generations",
            )
            .await;
            crate::gateway::pipeline::map_provider_error(e).into_response()
        }
    }
}

// ── POST /v1/audio/transcriptions ─────────────────────────────────────────────

/// POST /v1/audio/transcriptions
///
/// Multipart upload (OpenAI-compatible). Returns transcribed text.
/// Cost is per-audio-second when the provider reports a duration.
#[utoipa::path(
    post,
    path = "/v1/audio/transcriptions",
    tag = "Gateway",
    request_body(content_type = "multipart/form-data", description = "Fields: `model` (string), `file` (audio file), plus OpenAI transcription options"),
    responses(
        (status = 200, description = "OpenAI-format transcription response", body = serde_json::Value),
        (status = 400, description = "Missing or invalid multipart fields"),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded"),
    ),
    security(("api_key" = [])),
)]
pub async fn audio_transcriptions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if let Err(e) = check_budget(&api_key, &state.config.budget_downgrade) {
        return e.into_response();
    }

    let mut model = String::new();
    let mut file_bytes: Option<Bytes> = None;
    let mut filename = "audio.bin".to_string();
    let mut language: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut response_format: Option<String> = None;
    let mut temperature: Option<f64> = None;

    while let Some(field) = multipart.next_field().await.transpose() {
        let field = match field {
            Ok(f) => f,
            Err(e) => return AppError::BadRequest(format!("multipart error: {e}")).into_response(),
        };
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "model" => model = field.text().await.unwrap_or_default(),
            "language" => language = field.text().await.ok(),
            "prompt" => prompt = field.text().await.ok(),
            "response_format" => response_format = field.text().await.ok(),
            "temperature" => {
                temperature = field.text().await.ok().and_then(|s| s.parse::<f64>().ok())
            }
            "file" => {
                if let Some(fname) = field.file_name() {
                    filename = fname.to_string();
                }
                file_bytes = field.bytes().await.ok();
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let Some(bytes) = file_bytes else {
        return AppError::BadRequest("missing 'file' part".into()).into_response();
    };
    if model.is_empty() {
        return AppError::BadRequest("missing 'model' part".into()).into_response();
    }

    let provider = match crate::gateway::router::select_provider(&state.providers, &model) {
        Some(p) => p,
        None => {
            return AppError::ProviderUnavailable("No providers available".into()).into_response()
        }
    };

    let req = TranscribeRequest {
        model: model.clone(),
        file_bytes: bytes,
        filename,
        language,
        prompt,
        response_format,
        temperature,
    };

    let start = Instant::now();
    let result = provider.audio_transcribe(req).await;
    let latency_ms = start.elapsed().as_millis() as i32;

    match result {
        Ok(resp) => {
            let (_, price_per_second, _) =
                db::requests::find_modality_pricing(&state.pool, provider.name(), &model)
                    .await
                    .unwrap_or((None, None, None));
            let cost = match (resp.duration, price_per_second) {
                (Some(d), Some(p)) => pricing::calculate_audio_cost(d, p),
                _ => None,
            };

            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &model,
                cost,
                latency_ms,
                "success",
                "audio_transcribe",
                "/v1/audio/transcriptions",
            )
            .await;

            Json(resp).into_response()
        }
        Err(e) => {
            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &model,
                None,
                latency_ms,
                "error",
                "audio_transcribe",
                "/v1/audio/transcriptions",
            )
            .await;
            crate::gateway::pipeline::map_provider_error(e).into_response()
        }
    }
}

// ── POST /v1/audio/speech ─────────────────────────────────────────────────────

/// POST /v1/audio/speech
///
/// Text-to-speech. Returns a streaming binary audio body.
/// Cost is per-input-character from `model_pricing.price_per_character`.
#[utoipa::path(
    post,
    path = "/v1/audio/speech",
    tag = "Gateway",
    request_body(content = serde_json::Value, description = "OpenAI-format TTS request: model, input, voice, response_format"),
    responses(
        (status = 200, description = "Binary audio stream (Content-Type matches response_format)"),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded"),
    ),
    security(("api_key" = [])),
)]
pub async fn audio_speech(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<SpeechRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_budget(&api_key, &state.config.budget_downgrade) {
        return e.into_response();
    }

    let provider = match crate::gateway::router::select_provider(&state.providers, &request.model) {
        Some(p) => p,
        None => {
            return AppError::ProviderUnavailable("No providers available".into()).into_response()
        }
    };

    let start = Instant::now();
    let result = provider.audio_speech(&request).await;
    let latency_ms = start.elapsed().as_millis() as i32;

    match result {
        Ok(stream) => {
            // Compute cost up-front from input character count; the provider
            // returns audio whose size we don't pre-know.
            let char_count = request.input.chars().count() as u32;
            let (_, _, price_per_char) =
                db::requests::find_modality_pricing(&state.pool, provider.name(), &request.model)
                    .await
                    .unwrap_or((None, None, None));
            let cost = price_per_char.map(|p| pricing::calculate_character_cost(char_count, p));

            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &request.model,
                cost,
                latency_ms,
                "success",
                "audio_speech",
                "/v1/audio/speech",
            )
            .await;

            let content_type = stream.content_type.clone();
            let body_stream = stream
                .bytes
                .map(|chunk| chunk.map_err(std::io::Error::other));
            let body = Body::from_stream(body_stream);

            let mut response = axum::http::Response::new(body);
            if let Ok(v) = HeaderValue::from_str(&content_type) {
                response.headers_mut().insert("content-type", v);
            }
            response.into_response()
        }
        Err(e) => {
            let _ = db::requests::insert_modality_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &request.model,
                None,
                latency_ms,
                "error",
                "audio_speech",
                "/v1/audio/speech",
            )
            .await;
            crate::gateway::pipeline::map_provider_error(e).into_response()
        }
    }
}

// ── Prompt injection helpers ──────────────────────────────────────────────────

/// Prepend prompt content (and optionally a system message) to the request's
/// messages array.  The caller's original messages follow the injected ones.
fn inject_prompt(request: &mut ChatCompletionRequest, system_prompt: Option<&str>, content: &str) {
    let mut prepend: Vec<ChatMessage> = Vec::new();
    if let Some(sys) = system_prompt {
        prepend.push(ChatMessage {
            role: "system".to_string(),
            content: serde_json::Value::String(sys.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }
    prepend.push(ChatMessage {
        role: "user".to_string(),
        content: serde_json::Value::String(content.to_string()),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    });
    prepend.extend(std::mem::take(&mut request.messages));
    request.messages = prepend;
}

/// Weighted-random selection among active prompt versions.
/// Versions with `ab_weight = 0` are never selected.
fn select_version_by_weight(
    versions: &[crate::db::prompts::PromptVersion],
) -> &crate::db::prompts::PromptVersion {
    use rand::Rng;
    let total: i32 = versions.iter().map(|v| v.ab_weight.max(0)).sum();
    if total <= 0 {
        return &versions[0];
    }
    let mut pick = rand::thread_rng().gen_range(0..total);
    for v in versions {
        let w = v.ab_weight.max(0);
        if pick < w {
            return v;
        }
        pick -= w;
    }
    &versions[versions.len() - 1]
}

/// Extract cost tags from the request body (`metadata` field) and `X-Janus-Tags` header.
///
/// Merge order: body metadata first, then header values (header wins on key collision).
/// Returns an empty JSON object when neither source provides tags.
pub(crate) fn extract_tags(
    request: &ChatCompletionRequest,
    headers: &HeaderMap,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    if let Some(meta) = &request.metadata {
        if let Some(obj) = meta.as_object() {
            for (k, v) in obj {
                map.insert(k.clone(), v.clone());
            }
        }
    }

    if let Some(header_val) = headers.get("x-janus-tags").and_then(|v| v.to_str().ok()) {
        for pair in header_val.split(',') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                map.insert(
                    k.trim().to_string(),
                    serde_json::Value::String(v.trim().to_string()),
                );
            }
        }
    }

    serde_json::Value::Object(map)
}

/// Parse `X-Janus-Variables: {"key": "value"}` header into a HashMap.
/// Returns an empty map if the header is absent or invalid JSON.
fn parse_variables_header(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .get("x-janus-variables")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(s).ok())
        .unwrap_or_default()
}

// ── Token estimation ──────────────────────────────────────────────────────────

/// Rough token count estimate used for pre-flight TPM checks only.
/// 4 chars ≈ 1 token (standard rule-of-thumb); cheaper than a real tokeniser.
fn estimate_request_tokens(request: &crate::providers::ChatCompletionRequest) -> i64 {
    let char_count: usize = request
        .messages
        .iter()
        .map(|m| m.content.as_str().map(str::len).unwrap_or(50))
        .sum();
    ((char_count / 4) + 4).max(1) as i64
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

// ── POST /v1/embeddings ───────────────────────────────────────────────────────

/// POST /v1/embeddings
///
/// OpenAI-compatible embeddings endpoint. Supports exact caching (no semantic
/// cache — embeddings are the index, not the queries). Logged with `request_type = 'embedding'`.
#[utoipa::path(
    post,
    path = "/v1/embeddings",
    tag = "Gateway",
    request_body(content = serde_json::Value, description = "OpenAI-format embeddings request"),
    responses(
        (status = 200, description = "OpenAI-format embeddings response", body = serde_json::Value),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded"),
        (status = 503, description = "Provider unavailable"),
    ),
    security(("api_key" = [])),
)]
pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<EmbeddingRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_budget(&api_key, &state.config.budget_downgrade) {
        return e.into_response();
    }

    let bypass_cache = !state.runtime_config.load().cache_enabled;

    let hash = exact::compute_embedding_hash(&request);

    if !bypass_cache {
        if let Some(cached) = state.cache.lookup_embedding(&hash) {
            return Json((*cached).clone()).into_response();
        }
    }

    let start = Instant::now();

    let provider = match crate::gateway::router::select_provider(&state.providers, &request.model) {
        Some(p) => p,
        None => {
            return AppError::ProviderUnavailable("No providers available".into()).into_response()
        }
    };

    match provider.embeddings(&request).await {
        Ok(resp) => {
            let latency_ms = start.elapsed().as_millis() as i32;

            if !bypass_cache {
                state
                    .cache
                    .insert_embedding(hash.clone(), Arc::new(resp.clone()));
            }

            let cost = db::requests::find_pricing(&state.pool, provider.name(), &resp.model)
                .await
                .ok()
                .flatten()
                .map(|(inp, _out)| {
                    calculate_cost(
                        resp.usage.prompt_tokens,
                        0,
                        inp,
                        rust_decimal::Decimal::ZERO,
                    )
                });

            let _ = db::requests::insert_embedding_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &resp.model,
                Some(resp.usage.prompt_tokens as i32),
                Some(resp.usage.total_tokens as i32),
                cost,
                latency_ms,
                "success",
            )
            .await;

            Json(resp).into_response()
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as i32;
            let _ = db::requests::insert_embedding_request(
                &state.pool,
                Some(api_key.id),
                api_key.workspace_id,
                provider.name(),
                &request.model,
                None,
                None,
                None,
                latency_ms,
                "error",
            )
            .await;
            AppError::ProviderUnavailable(e.to_string()).into_response()
        }
    }
}

// ── POST /v1/completions (legacy) ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct LegacyCompletionRequest {
    pub model: String,
    pub prompt: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// POST /v1/completions
///
/// Legacy completions endpoint. Converts `prompt` → single user message, calls
/// the chat pipeline internally, and returns in legacy Completions response format.
#[utoipa::path(
    post,
    path = "/v1/completions",
    tag = "Gateway",
    request_body(content = serde_json::Value, description = "Legacy OpenAI completions request (prompt + model)"),
    responses(
        (status = 200, description = "Legacy OpenAI-format completion (JSON or SSE stream)"),
        (status = 401, description = "Invalid API key"),
        (status = 402, description = "Budget exceeded"),
    ),
    security(("api_key" = [])),
)]
pub async fn legacy_completions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<LegacyCompletionRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_budget(&api_key, &state.config.budget_downgrade) {
        return e.into_response();
    }

    // Convert prompt (string or array) to a single user chat message.
    let prompt_text = match &request.prompt {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    };

    let chat_request = ChatCompletionRequest {
        model: request.model.clone(),
        messages: vec![crate::providers::ChatMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(prompt_text),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        stream: None,
        top_p: None,
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        seed: None,
        user: None,
        logit_bias: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        response_format: None,
        metadata: None,
    };

    let rc = state.runtime_config.load();
    let bypass_cache = !rc.cache_enabled;
    let max_retries = rc.max_retries;
    drop(rc);

    let strategy = RoutingStrategy::from_db_str(&api_key.routing_strategy);
    let fallback_models: Vec<String> = state
        .config
        .routing
        .fallbacks
        .get(&chat_request.model)
        .cloned()
        .unwrap_or_default();

    let bypass_semantic_legacy = bypass_cache
        || !state
            .semantic_policy
            .allows(&chat_request.model, "/v1/completions", &api_key.name);

    match pipeline::run(
        &state.pool,
        &state.providers,
        &chat_request,
        &api_key,
        max_retries,
        &state.cache,
        bypass_cache,
        bypass_semantic_legacy,
        &strategy,
        &fallback_models,
        None, // legacy completions don't support prompt management
        &state.plugins,
        &state.dedup,
        0,     // no TTL for legacy completions endpoint
        false, // legacy completions bypass downgrade — budget check above uses simple path
        None,
        &serde_json::Value::Object(serde_json::Map::new()),
        "/v1/completions",
        &state.audit,
    )
    .await
    {
        Ok((resp, _cache_hit)) => {
            let text = resp
                .choices
                .first()
                .and_then(|c| c.message.content.as_str())
                .unwrap_or("")
                .to_string();
            let finish_reason = resp
                .choices
                .first()
                .and_then(|c| c.finish_reason.clone())
                .unwrap_or_else(|| "stop".to_string());
            Json(serde_json::json!({
                "id": resp.id,
                "object": "text_completion",
                "created": resp.created,
                "model": resp.model,
                "choices": [{
                    "text": text,
                    "index": 0,
                    "logprobs": null,
                    "finish_reason": finish_reason
                }],
                "usage": {
                    "prompt_tokens": resp.usage.prompt_tokens,
                    "completion_tokens": resp.usage.completion_tokens,
                    "total_tokens": resp.usage.total_tokens
                }
            }))
            .into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// Attach smart-routing observability headers when the router was invoked.
///
/// `X-Janus-Model-Selected` — the model the router chose (only when model was absent).
/// `X-Janus-Routing-Reason` — why that model was chosen (tier, rule, tag, etc.).
fn attach_routing_headers(
    headers: &mut axum::http::HeaderMap,
    decision: &Option<crate::gateway::smart_router::RoutingDecision>,
) {
    if let Some(d) = decision {
        if let Ok(v) = HeaderValue::from_str(&d.model) {
            headers.insert("x-janus-model-selected", v);
        }
        if let Ok(v) = HeaderValue::from_str(&d.reason.header_value()) {
            headers.insert("x-janus-routing-reason", v);
        }
    }
}

fn attach_cache_headers(headers: &mut axum::http::HeaderMap, hit: &CacheHit) {
    match hit {
        CacheHit::None => {}
        CacheHit::Exact => {
            headers.insert("x-janus-cache-hit", HeaderValue::from_static("exact"));
        }
        CacheHit::Semantic(score) => {
            headers.insert("x-janus-cache-hit", HeaderValue::from_static("semantic"));
            if let Ok(v) = HeaderValue::from_str(&format!("{score:.4}")) {
                headers.insert("x-janus-cache-similarity", v);
            }
        }
    }
}
