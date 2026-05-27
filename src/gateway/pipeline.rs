use super::{
    dedup::{DedupRole, DeduplicatedResult, InFlightDeduplicator},
    router,
    strategies::RoutingStrategy,
    tool_extract, ProviderRegistry,
};
use crate::db::DbPool;
use crate::{
    cache::{self, CacheEngine, CacheHit},
    db,
    errors::{AppError, AppResult},
    models::api_key::ApiKey,
    plugins::{self, PluginError, RequestPlugin},
    pricing,
    providers::{
        ChatChoice, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
        ChatMessage, ChunkChoice, ChunkDelta, ProviderError, UsageData,
    },
};
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use futures_util::StreamExt;
use metrics::{counter, histogram};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_stream::wrappers::ReceiverStream;
use tracing::Instrument;
use uuid::Uuid;

// ── Audit-log task admission control ──────────────────────────────────────────

/// Spawn a fire-and-forget DB write task, but only if the audit semaphore has
/// capacity. Under extreme sustained load (e.g. >10k RPS where PostgreSQL can't
/// keep up with per-request inserts), this prevents tokio's task queue from
/// growing without bound and OOM-killing the process. When permits are
/// exhausted, the record is dropped and `janus_audit_dropped_total` is bumped.
fn spawn_audit<F>(sem: &Arc<tokio::sync::Semaphore>, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match sem.clone().try_acquire_owned() {
        Ok(permit) => {
            tokio::spawn(async move {
                let _permit = permit; // released on drop, frees one slot
                fut.await;
            });
        }
        Err(_) => {
            counter!("janus_audit_dropped_total").increment(1);
        }
    }
}

// ── Prompt text extraction ────────────────────────────────────────────────────

/// Concatenate all message content strings for embedding.
fn prompt_text(request: &ChatCompletionRequest) -> String {
    request
        .messages
        .iter()
        .map(|m| {
            if let Some(s) = m.content.as_str() {
                s.to_string()
            } else if let Some(arr) = m.content.as_array() {
                arr.iter()
                    .filter_map(|item| {
                        if item["type"].as_str() == Some("text") {
                            item["text"].as_str().map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Non-streaming pipeline ────────────────────────────────────────────────────

/// Run the full non-streaming proxy pipeline.
///
/// Returns `(response, CacheHit)` where `CacheHit` describes how the response
/// was sourced (live provider, exact cache, or semantic cache).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    pool: &DbPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    max_retries: u32,
    cache: &CacheEngine,
    bypass_cache: bool,
    bypass_semantic: bool,
    strategy: &RoutingStrategy,
    fallback_models: &[String],
    prompt_version_id: Option<Uuid>,
    plugin_chain: &[Box<dyn RequestPlugin>],
    dedup: &InFlightDeduplicator,
    // Cache TTL in seconds. 0 = no expiry.
    cache_ttl_secs: u64,
    // Whether a budget downgrade was triggered for this request.
    downgrade_triggered: bool,
    // When Some, only the provider with this string ID is tried (replay override).
    only_provider: Option<&str>,
    // V5-L3: tags extracted from request metadata + X-Janus-Tags header.
    tags: &serde_json::Value,
    // V5-0: route this request is being served on, written to requests.endpoint.
    endpoint: &str,
    // Bounds concurrent fire-and-forget audit-log tasks (see `spawn_audit`).
    audit_sem: &Arc<tokio::sync::Semaphore>,
) -> AppResult<(ChatCompletionResponse, CacheHit)> {
    // V5-0: own the endpoint string so it can be moved into spawned audit-log tasks.
    let endpoint_log_arc: Arc<str> = Arc::from(endpoint);
    // ── Exact cache lookup ────────────────────────────────────────────────────
    let hash = cache::exact::compute_hash(request);

    if !bypass_cache {
        let cached =
            tracing::info_span!("janus.cache.exact_lookup").in_scope(|| cache.lookup(&hash));
        if let Some(cached) = cached {
            let tokens = cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
            counter!("janus_requests_total", "cache_type" => "exact", "status" => "success")
                .increment(1);
            {
                let pool = pool.clone();
                let hash = hash.clone();
                spawn_audit(audit_sem, async move {
                    let _ = db::cache::record_hit(&pool, &hash, tokens, Decimal::ZERO).await;
                });
            }
            crate::metrics::record_request(true);
            let mut resp = (*cached).clone();
            match plugins::run_after(plugin_chain, request, &mut resp, api_key).await {
                Ok(()) | Err(PluginError::Warning(_)) => {}
                Err(PluginError::BadRequest(msg)) => return Err(AppError::BadRequest(msg)),
                Err(PluginError::Forbidden(msg)) => return Err(AppError::Forbidden(msg)),
            }
            return Ok((resp, CacheHit::Exact));
        }
    }

    // ── Semantic cache lookup ─────────────────────────────────────────────────
    // Compute embedding once; reuse it after provider call to populate the index.
    // Skip when bypass_semantic is set (policy exclusion or cache fully bypassed).
    let embedding: Option<Vec<f32>> = if !bypass_cache && !bypass_semantic && cache.model.is_some()
    {
        let text = prompt_text(request);
        cache.model.as_ref().and_then(|m| m.embed(&text).ok())
    } else {
        None
    };

    if !bypass_cache && !bypass_semantic {
        if let Some(ref emb) = embedding {
            let semantic_hit = tracing::info_span!("janus.cache.semantic_lookup")
                .in_scope(|| cache.semantic_lookup(emb));
            if let Some((hit_hash, score)) = semantic_hit {
                if let Some(cached) = cache.lookup(&hit_hash) {
                    let tokens =
                        cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
                    counter!("janus_requests_total", "cache_type" => "semantic", "status" => "success").increment(1);
                    {
                        let pool = pool.clone();
                        let hash = hit_hash.clone();
                        spawn_audit(audit_sem, async move {
                            let _ = db::cache::record_hit(&pool, &hash, tokens, Decimal::ZERO).await;
                        });
                    }
                    crate::metrics::record_request(true);
                    let mut resp = (*cached).clone();
                    match plugins::run_after(plugin_chain, request, &mut resp, api_key).await {
                        Ok(()) | Err(PluginError::Warning(_)) => {}
                        Err(PluginError::BadRequest(msg)) => return Err(AppError::BadRequest(msg)),
                        Err(PluginError::Forbidden(msg)) => return Err(AppError::Forbidden(msg)),
                    }
                    return Ok((resp, CacheHit::Semantic(score)));
                }
            }
        }
    }

    // Clone request so plugins can mutate it without affecting the caller.
    let mut request = request.clone();

    // ── Plugin before_request chain ───────────────────────────────────────────
    match plugins::run_before(plugin_chain, &mut request, api_key).await {
        Ok(()) => {}
        Err(PluginError::BadRequest(msg)) => return Err(AppError::BadRequest(msg)),
        Err(PluginError::Forbidden(msg)) => return Err(AppError::Forbidden(msg)),
        Err(PluginError::Warning(_)) => {} // already logged in run_before
    }

    let request = &request; // reborrow as shared ref for the rest of the pipeline

    // ── In-flight deduplication ───────────────────────────────────────────────
    // Reuses the SHA-256 already computed for exact-cache — no extra hashing.
    // Streaming requests skip this entirely (SSE cannot be broadcast).
    let is_dedup_primary = if !bypass_cache {
        match dedup.register_or_subscribe(&hash) {
            DedupRole::Primary => true,
            DedupRole::Waiter(mut rx) => {
                let timeout = Duration::from_secs(InFlightDeduplicator::waiter_timeout_secs());
                let dedup_result = tokio::time::timeout(timeout, rx.recv()).await;
                return match dedup_result {
                    Ok(Ok(result)) => match result.as_ref() {
                        DeduplicatedResult::Response(resp) => {
                            let mut resp = resp.clone();
                            match plugins::run_after(plugin_chain, request, &mut resp, api_key)
                                .await
                            {
                                Ok(()) | Err(PluginError::Warning(_)) => {}
                                Err(PluginError::BadRequest(msg)) => {
                                    return Err(AppError::BadRequest(msg))
                                }
                                Err(PluginError::Forbidden(msg)) => {
                                    return Err(AppError::Forbidden(msg))
                                }
                            }
                            crate::metrics::record_request(false);
                            Ok((resp, CacheHit::None))
                        }
                        DeduplicatedResult::Error(msg) => {
                            Err(AppError::ProviderUnavailable(msg.clone()))
                        }
                    },
                    Ok(Err(_)) => {
                        // Sender dropped without broadcasting (primary panicked or
                        // released early). The cache may already have the result,
                        // so this request will be retried by the client and hit cache.
                        Err(AppError::ProviderUnavailable(
                            "Dedup channel closed — please retry".to_string(),
                        ))
                    }
                    Err(_) => {
                        // Primary took too long. Surface as a retriable error.
                        Err(AppError::ProviderUnavailable(
                            "Dedup wait timed out — please retry".to_string(),
                        ))
                    }
                };
            }
        }
    } else {
        false
    };

    // ── Provider loop ─────────────────────────────────────────────────────────
    let mut providers =
        router::select_providers_for_strategy(pool, registry, strategy, &request.model).await;
    if let Some(name) = only_provider {
        providers.retain(|p| p.name() == name);
    }
    if providers.is_empty() {
        if is_dedup_primary {
            dedup.broadcast_result(
                &hash,
                Arc::new(DeduplicatedResult::Error(
                    "No enabled providers available".to_string(),
                )),
            );
            dedup.release(&hash);
        }
        return Err(AppError::ProviderUnavailable(
            "No enabled providers available".to_string(),
        ));
    }

    let mut last_error: Option<AppError> = None;

    // Build the list of models to attempt: original model first, then fallbacks.
    let mut models_to_try = vec![request.model.clone()];
    models_to_try.extend(fallback_models.iter().cloned());

    for model_attempt in &models_to_try {
        let mut request_for_model;
        let effective_request = if model_attempt == &request.model {
            request
        } else {
            request_for_model = request.clone();
            request_for_model.model = model_attempt.clone();
            &request_for_model
        };

        let is_primary_model = model_attempt == &request.model;

        'provider: for provider in &providers {
            // Skip providers whose circuit breaker is open.
            if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                if cb.is_open() {
                    tracing::debug!(
                        provider = provider.name(),
                        "Circuit open — skipping provider"
                    );
                    last_error = Some(AppError::ProviderUnavailable(format!(
                        "{} circuit open",
                        provider.name()
                    )));
                    continue 'provider;
                }
            }

            let mut attempts = 0u32;

            loop {
                let start = Instant::now();
                let provider_span = tracing::info_span!(
                    "janus.provider.call",
                    janus.provider = provider.name(),
                    janus.model = %effective_request.model,
                    janus.prompt_tokens = tracing::field::Empty,
                    janus.completion_tokens = tracing::field::Empty,
                    janus.cost_usd = tracing::field::Empty,
                );
                // Clone before move into .instrument() so we can record attributes
                // on the span after the future completes (Span is cheaply Clone).
                let provider_span_ref = provider_span.clone();
                let result = provider
                    .chat_completion(effective_request)
                    .instrument(provider_span)
                    .await;
                let latency_ms = start.elapsed().as_millis() as i32;

                match result {
                    Ok(mut resp) => {
                        if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                            cb.record_success();
                        }
                        let usage = &resp.usage;
                        let elapsed_secs = start.elapsed().as_secs_f64();

                        let cost = db::requests::find_pricing(pool, provider.name(), &resp.model)
                            .await
                            .ok()
                            .flatten()
                            .map(|(input_price, output_price)| {
                                pricing::calculate_cost(
                                    usage.prompt_tokens,
                                    usage.completion_tokens,
                                    input_price,
                                    output_price,
                                )
                            });

                        // Record token and cost attributes on the provider call span.
                        provider_span_ref
                            .record("janus.prompt_tokens", usage.prompt_tokens)
                            .record("janus.completion_tokens", usage.completion_tokens);

                        // Record metrics
                        counter!("janus_requests_total", "provider" => provider.name(), "model" => resp.model.clone(), "status" => "success", "cache_type" => "none").increment(1);
                        histogram!("janus_request_duration_seconds", "provider" => provider.name(), "model" => resp.model.clone()).record(elapsed_secs);
                        counter!("janus_tokens_total", "provider" => provider.name(), "model" => resp.model.clone(), "direction" => "prompt").increment(usage.prompt_tokens as u64);
                        counter!("janus_tokens_total", "provider" => provider.name(), "model" => resp.model.clone(), "direction" => "completion").increment(usage.completion_tokens as u64);

                        if let Some(cost_value) = cost {
                            if cost_value > Decimal::ZERO {
                                let cost_microdollars =
                                    (cost_value.to_f64().unwrap_or(0.0) * 1_000_000.0) as u64;
                                counter!("janus_cost_usd_total", "provider" => provider.name(), "model" => resp.model.clone()).increment(cost_microdollars);
                                provider_span_ref
                                    .record("janus.cost_usd", cost_value.to_f64().unwrap_or(0.0));
                            }
                        }

                        // Only cache responses for the primary (non-fallback) model so
                        // the cache key (hash of original request) remains coherent.
                        if !bypass_cache && is_primary_model {
                            tracing::info_span!("janus.cache.insert").in_scope(|| {
                                cache.insert_with_ttl(
                                    hash.clone(),
                                    Arc::new(resp.clone()),
                                    cache_ttl_secs,
                                );
                            });

                            let req_body = crate::pii::scrub(
                                &serde_json::to_string(request).unwrap_or_default(),
                            )
                            .into_owned();
                            let resp_body = serde_json::to_string(&resp).unwrap_or_default();
                            let (pt, ct) =
                                (usage.prompt_tokens as i32, usage.completion_tokens as i32);
                            let _ = db::cache::upsert_entry(
                                pool,
                                &hash,
                                provider.name(),
                                &resp.model,
                                &req_body,
                                &resp_body,
                                Some(pt),
                                Some(ct),
                                cost,
                                cache_ttl_secs,
                            )
                            .await;

                            if let Some(emb) = embedding.as_ref() {
                                cache.semantic_insert(emb.clone(), hash.clone());
                                let emb_bytes = crate::cache::semantic::f32_vec_to_bytes(emb);
                                let _ = db::cache::save_embedding(pool, &hash, &emb_bytes).await;
                            }
                        }

                        // Broadcast result to dedup waiters AFTER cache insert so that
                        // any new requests arriving after the slot is released will get
                        // exact cache hits rather than becoming new primaries.
                        if is_dedup_primary {
                            dedup.broadcast_result(
                                &hash,
                                Arc::new(DeduplicatedResult::Response(resp.clone())),
                            );
                            dedup.release(&hash);
                        }

                        // V5-0: extract function-calling audit (tools + tool_calls) once,
                        // before the response is moved into the spawned task.
                        let tool_calls = tool_extract::extract(effective_request, &resp);

                        // Fire-and-forget: log request + update budget + last_used + daily_costs.
                        {
                            let pool = pool.clone();
                            let api_key_id = api_key.id;
                            let workspace_id = api_key.workspace_id;
                            let provider_name = provider.name();
                            let model = resp.model.clone();
                            let (pt, ct, tt) = (
                                usage.prompt_tokens as i32,
                                usage.completion_tokens as i32,
                                usage.total_tokens as i32,
                            );
                            let endpoint_log = endpoint_log_arc.clone();
                            let tags_log = tags.clone();
                            spawn_audit(audit_sem, async move {
                                let _ = db::requests::insert_request(
                                    &pool,
                                    Some(api_key_id),
                                    workspace_id,
                                    provider_name,
                                    &model,
                                    Some(pt),
                                    Some(ct),
                                    Some(tt),
                                    cost,
                                    latency_ms,
                                    "success",
                                    false,
                                    None,
                                    prompt_version_id,
                                    downgrade_triggered,
                                    &endpoint_log,
                                    tool_calls.as_ref(),
                                    &tags_log,
                                )
                                .await;
                                let _ = db::analytics::upsert_daily_cost(
                                    &pool,
                                    Some(api_key_id),
                                    workspace_id,
                                    provider_name,
                                    &model,
                                    pt as i64,
                                    ct as i64,
                                    cost,
                                    false,
                                )
                                .await;
                                if let Some(cost_value) = cost {
                                    if cost_value > Decimal::ZERO {
                                        let _ = db::api_keys::add_budget_used(
                                            &pool, api_key_id, cost_value,
                                        )
                                        .await;
                                    }
                                }
                                let _ = db::api_keys::update_last_used(&pool, api_key_id).await;
                            });
                        }

                        crate::metrics::record_request(false);
                        match plugins::run_after(plugin_chain, request, &mut resp, api_key).await {
                            Ok(()) | Err(PluginError::Warning(_)) => {}
                            Err(PluginError::BadRequest(msg)) => {
                                return Err(AppError::BadRequest(msg))
                            }
                            Err(PluginError::Forbidden(msg)) => {
                                return Err(AppError::Forbidden(msg))
                            }
                        }
                        return Ok((resp, CacheHit::None));
                    }

                    Err(e) => {
                        let status = match &e {
                            ProviderError::RateLimit => "rate_limit",
                            ProviderError::Unauthorized => "auth_error",
                            ProviderError::Timeout => "timeout",
                            ProviderError::BadRequest(_) => "bad_request",
                            _ => "error",
                        };

                        // Record failure in circuit breaker for retriable errors.
                        if matches!(&e, ProviderError::Unavailable(_) | ProviderError::Timeout) {
                            if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                                cb.record_failure();
                            }
                        }

                        // Record error metrics
                        counter!("janus_requests_total", "provider" => provider.name(), "model" => effective_request.model.clone(), "status" => status, "cache_type" => "none").increment(1);
                        let elapsed_secs = start.elapsed().as_secs_f64();
                        histogram!("janus_request_duration_seconds", "provider" => provider.name(), "model" => effective_request.model.clone()).record(elapsed_secs);

                        {
                            let pool = pool.clone();
                            let api_key_id = api_key.id;
                            let workspace_id = api_key.workspace_id;
                            let provider_name = provider.name();
                            let model = effective_request.model.clone();
                            let endpoint_log = endpoint_log_arc.clone();
                            let tags_err = tags.clone();
                            spawn_audit(audit_sem, async move {
                                let _ = db::requests::insert_request(
                                    &pool,
                                    Some(api_key_id),
                                    workspace_id,
                                    provider_name,
                                    &model,
                                    None,
                                    None,
                                    None,
                                    None,
                                    latency_ms,
                                    status,
                                    false,
                                    None,
                                    None,
                                    downgrade_triggered,
                                    &endpoint_log,
                                    None,
                                    &tags_err,
                                )
                                .await;
                            });
                        }

                        if matches!(
                            &e,
                            ProviderError::Unauthorized | ProviderError::BadRequest(_)
                        ) {
                            last_error = Some(map_provider_error(e));
                            break 'provider;
                        }

                        if matches!(&e, ProviderError::Unavailable(_) | ProviderError::Timeout)
                            && attempts < max_retries
                        {
                            tracing::warn!(
                                provider = provider.name(),
                                attempt = attempts + 1,
                                max = max_retries,
                                "Retrying after provider error: {e}"
                            );
                            attempts += 1;
                            continue;
                        }

                        tracing::warn!(
                            provider = provider.name(),
                            "Failing over after {} attempt(s): {e}",
                            attempts + 1
                        );
                        last_error = Some(map_provider_error(e));
                        continue 'provider;
                    }
                }
            }
        }
        // All providers failed for this model; if there are fallback models, try them next.
    } // end for model_attempt

    // Propagate failure to any dedup waiters before returning.
    if is_dedup_primary {
        let msg = last_error
            .as_ref()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "All providers unavailable".to_string());
        dedup.broadcast_result(&hash, Arc::new(DeduplicatedResult::Error(msg)));
        dedup.release(&hash);
    }

    Err(last_error
        .unwrap_or_else(|| AppError::ProviderUnavailable("All providers unavailable".to_string())))
}

// ── Streaming pipeline ────────────────────────────────────────────────────────

/// Run the streaming proxy pipeline.
///
/// Returns `(response, CacheHit)`. On an exact or semantic cache hit the cached
/// response is synthesized as a valid SSE stream.
#[allow(clippy::too_many_arguments)]
pub async fn run_streaming(
    pool: DbPool,
    registry: Arc<ProviderRegistry>,
    mut request: ChatCompletionRequest,
    api_key: ApiKey,
    max_retries: u32,
    cache: Arc<CacheEngine>,
    bypass_cache: bool,
    bypass_semantic: bool,
    strategy: RoutingStrategy,
    fallback_models: Vec<String>,
    prompt_version_id: Option<Uuid>,
    plugin_chain: Arc<Vec<Box<dyn RequestPlugin>>>,
    // Cache TTL in seconds. Streaming responses are buffered as they pass through
    // and persisted as a non-streaming response on clean completion, so subsequent
    // identical requests (streaming or not) hit cache via synthesize_sse_from_cached.
    cache_ttl_secs: u64,
    downgrade_triggered: bool,
    // V5-L3: tags extracted from request metadata + X-Janus-Tags header.
    tags: serde_json::Value,
    // V5-0: route this stream is being served on, written to requests.endpoint.
    endpoint: &str,
    // Bounds concurrent fire-and-forget audit-log tasks (see `spawn_audit`).
    audit_sem: Arc<tokio::sync::Semaphore>,
) -> AppResult<(Response, CacheHit)> {
    let endpoint_log_arc: Arc<str> = Arc::from(endpoint);
    // ── Exact cache lookup ────────────────────────────────────────────────────
    let hash = cache::exact::compute_hash(&request);

    if !bypass_cache {
        let cached =
            tracing::info_span!("janus.cache.exact_lookup").in_scope(|| cache.lookup(&hash));
        if let Some(cached) = cached {
            let tokens = cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
            counter!("janus_requests_total", "cache_type" => "exact", "status" => "success")
                .increment(1);
            {
                let pool = pool.clone();
                let hash = hash.clone();
                spawn_audit(&audit_sem, async move {
                    let _ = db::cache::record_hit(&pool, &hash, tokens, Decimal::ZERO).await;
                });
            }
            crate::metrics::record_request(true);
            let sse = synthesize_sse_from_cached(&cached);
            return Ok((sse, CacheHit::Exact));
        }
    }

    // ── Semantic cache lookup ─────────────────────────────────────────────────
    let embedding: Option<Vec<f32>> = if !bypass_cache && !bypass_semantic && cache.model.is_some()
    {
        let text = prompt_text(&request);
        cache.model.as_ref().and_then(|m| m.embed(&text).ok())
    } else {
        None
    };

    if !bypass_cache && !bypass_semantic {
        if let Some(ref emb) = embedding {
            let semantic_hit = tracing::info_span!("janus.cache.semantic_lookup")
                .in_scope(|| cache.semantic_lookup(emb));
            if let Some((hit_hash, score)) = semantic_hit {
                if let Some(cached) = cache.lookup(&hit_hash) {
                    let tokens =
                        cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
                    counter!("janus_requests_total", "cache_type" => "semantic", "status" => "success").increment(1);
                    {
                        let pool = pool.clone();
                        let hash = hit_hash.clone();
                        spawn_audit(&audit_sem, async move {
                            let _ = db::cache::record_hit(&pool, &hash, tokens, Decimal::ZERO).await;
                        });
                    }
                    crate::metrics::record_request(true);
                    let sse = synthesize_sse_from_cached(&cached);
                    return Ok((sse, CacheHit::Semantic(score)));
                }
            }
        }
    }

    // ── Plugin before_request chain ───────────────────────────────────────────
    match plugins::run_before(&plugin_chain, &mut request, &api_key).await {
        Ok(()) => {}
        Err(PluginError::BadRequest(msg)) => return Err(AppError::BadRequest(msg)),
        Err(PluginError::Forbidden(msg)) => return Err(AppError::Forbidden(msg)),
        Err(PluginError::Warning(_)) => {}
    }

    // ── Provider loop ─────────────────────────────────────────────────────────
    let providers =
        router::select_providers_for_strategy(&pool, &registry, &strategy, &request.model).await;
    if providers.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "No enabled providers available".to_string(),
        ));
    }

    let wall_start = Instant::now();
    let mut last_error: Option<AppError> = None;

    // Build the list of models to try: original first, then fallbacks.
    let mut models_to_try = vec![request.model.clone()];
    models_to_try.extend(fallback_models.iter().cloned());

    for model_attempt in &models_to_try {
        // Mirror the non-streaming path (line ~259): only cache when serving the
        // primary model — fallback responses would poison the cache key (hash of
        // the original request) with content the user never explicitly requested.
        let is_primary_model = model_attempt == &request.model;

        let mut request_for_model;
        let effective_request = if model_attempt == &request.model {
            &request
        } else {
            request_for_model = request.clone();
            request_for_model.model = model_attempt.clone();
            &request_for_model
        };

        for provider in &providers {
            // Skip providers whose circuit breaker is open.
            if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                if cb.is_open() {
                    tracing::debug!(
                        provider = provider.name(),
                        "Circuit open — skipping provider (stream)"
                    );
                    last_error = Some(AppError::ProviderUnavailable(format!(
                        "{} circuit open",
                        provider.name()
                    )));
                    continue;
                }
            }

            let mut attempts = 0u32;

            loop {
                let stream_result = provider.chat_completion_stream(effective_request).await;

                if let Err(ref e) = stream_result {
                    let retriable =
                        matches!(e, ProviderError::Unavailable(_) | ProviderError::Timeout);
                    if retriable {
                        if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                            cb.record_failure();
                        }
                    }
                    if retriable && attempts < max_retries {
                        tracing::warn!(
                            provider = provider.name(),
                            attempt = attempts + 1,
                            "Retrying stream open after: {e}"
                        );
                        attempts += 1;
                        continue;
                    }
                }

                match stream_result {
                    Err(e) => {
                        tracing::warn!(provider = provider.name(), "Stream open failed: {e}");
                        last_error = Some(map_provider_error(e));
                        break;
                    }
                    Ok(provider_stream) => {
                        if let Some(cb) = registry.circuit_breakers.get(&provider.priority()) {
                            cb.record_success();
                        }
                        let provider_name = provider.name();
                        let api_key_id = api_key.id;
                        let workspace_id = api_key.workspace_id;
                        let model = effective_request.model.clone();
                        let tags_stream = tags.clone();

                        // State captured into the spawn so the stream can be persisted
                        // to cache after a clean finish (mirrors the non-streaming path).
                        // Pre-serialise the ORIGINAL request (not effective_request) so
                        // the stored req_body matches the hash key.
                        let hash_for_cache = hash.clone();
                        let cache_for_write = cache.clone();
                        let embedding_for_write = embedding.clone();
                        let request_body_for_cache =
                            serde_json::to_string(&request).unwrap_or_default();
                        let audit_sem_stream = audit_sem.clone();

                        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

                        tokio::spawn(async move {
                            let mut prompt_tokens: u32 = 0;
                            let mut completion_tokens: u32 = 0;
                            let mut ttfb_ms: Option<i32> = None;
                            let mut final_model = model.clone();
                            // 6.3: Track whether the stream ended cleanly or with a
                            // provider error so the request log reflects reality.
                            let mut stream_status = "success";

                            // Cache-write state: accumulate text content, capture
                            // finish_reason, distinguish clean-end from client-abort.
                            let mut accumulated_content = String::new();
                            let mut finish_reason_str: Option<String> = None;
                            let mut stream_completed = false;
                            let mut client_disconnected = false;

                            tokio::pin!(provider_stream);

                            loop {
                                tokio::select! {
                                    biased;

                                    // 6.1: Detect client disconnect.
                                    // tx.closed() resolves when the ReceiverStream
                                    // (SSE body) is dropped by axum — i.e. the client
                                    // disconnected.  Abort the provider stream
                                    // immediately instead of letting it run to
                                    // completion and burning tokens for nobody.
                                    _ = tx.closed() => {
                                        tracing::debug!(
                                            provider = provider_name,
                                            "Client disconnected — cancelling provider stream"
                                        );
                                        client_disconnected = true;
                                        break;
                                    }

                                    chunk_opt = provider_stream.next() => {
                                        let Some(chunk_result) = chunk_opt else {
                                            // Provider closed the stream naturally.
                                            stream_completed = true;
                                            break;
                                        };

                                        // Record TTFB on first byte received (error or data).
                                        if ttfb_ms.is_none() {
                                            ttfb_ms = Some(wall_start.elapsed().as_millis() as i32);
                                        }

                                        match chunk_result {
                                            Err(e) => {
                                                // 6.3: Provider returned an error mid-stream
                                                // (HTTP 200 then error SSE event).  Set
                                                // status = "error" so the audit log is correct.
                                                tracing::error!(
                                                    provider = provider_name,
                                                    error = %e,
                                                    "Mid-stream provider error"
                                                );
                                                stream_status = "error";
                                                break;
                                            }
                                            Ok(chunk) => {
                                                if !chunk.model.is_empty() {
                                                    final_model = chunk.model.clone();
                                                }

                                                if let Some(usage) = &chunk.usage {
                                                    prompt_tokens = usage.prompt_tokens;
                                                    completion_tokens = usage.completion_tokens;
                                                } else {
                                                    for choice in &chunk.choices {
                                                        if !choice
                                                            .delta
                                                            .content
                                                            .as_deref()
                                                            .unwrap_or("")
                                                            .is_empty()
                                                        {
                                                            completion_tokens += 1;
                                                        }
                                                    }
                                                }

                                                // Buffer content + capture finish_reason
                                                // for the cache-write step below.
                                                for choice in &chunk.choices {
                                                    if let Some(c) = choice.delta.content.as_deref()
                                                    {
                                                        accumulated_content.push_str(c);
                                                    }
                                                    if let Some(fr) = &choice.finish_reason {
                                                        finish_reason_str = Some(fr.clone());
                                                        // Provider has signalled end-of-completion.
                                                        // We may still get a trailing usage chunk
                                                        // before the stream closes.
                                                        stream_completed = true;
                                                    }
                                                }

                                                let data = match serde_json::to_string(&chunk) {
                                                    Ok(s) => s,
                                                    Err(e) => {
                                                        tracing::warn!("Chunk serialise error: {e}");
                                                        continue;
                                                    }
                                                };

                                                // 6.2: send().await provides natural
                                                // backpressure: the task suspends when
                                                // the channel is full, preventing the
                                                // provider from being consumed faster
                                                // than the client can receive.  If the
                                                // receiver was dropped between select!
                                                // arms, is_err() returns immediately.
                                                if tx.send(Ok(Event::default().data(data))).await.is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
                            drop(tx);

                            let latency_ms = wall_start.elapsed().as_millis() as i32;
                            let elapsed_secs = wall_start.elapsed().as_secs_f64();
                            let total_tokens = prompt_tokens + completion_tokens;

                            // Record streaming metrics
                            counter!("janus_requests_total", "provider" => provider_name.to_string(), "model" => final_model.clone(), "status" => stream_status, "cache_type" => "none").increment(1);
                            histogram!("janus_request_duration_seconds", "provider" => provider_name.to_string(), "model" => final_model.clone()).record(elapsed_secs);
                            counter!("janus_tokens_total", "provider" => provider_name.to_string(), "model" => final_model.clone(), "direction" => "prompt").increment(prompt_tokens as u64);
                            counter!("janus_tokens_total", "provider" => provider_name.to_string(), "model" => final_model.clone(), "direction" => "completion").increment(completion_tokens as u64);

                            let provider_for_metrics = provider_name.to_string();
                            let model_for_metrics = final_model.clone();
                            let endpoint_log = endpoint_log_arc.clone();
                            spawn_audit(&audit_sem_stream, async move {
                                let cost =
                                    db::requests::find_pricing(&pool, provider_name, &final_model)
                                        .await
                                        .ok()
                                        .flatten()
                                        .map(|(input_price, output_price)| {
                                            pricing::calculate_cost(
                                                prompt_tokens,
                                                completion_tokens,
                                                input_price,
                                                output_price,
                                            )
                                        });

                                // Record cost metrics in async block
                                if let Some(cost_value) = cost {
                                    if cost_value > Decimal::ZERO {
                                        let cost_microdollars = (cost_value.to_f64().unwrap_or(0.0)
                                            * 1_000_000.0)
                                            as u64;
                                        counter!("janus_cost_usd_total", "provider" => provider_for_metrics.clone(), "model" => model_for_metrics.clone()).increment(cost_microdollars);
                                    }
                                }

                                let _ = db::requests::insert_request(
                                    &pool,
                                    Some(api_key_id),
                                    workspace_id,
                                    provider_name,
                                    &final_model,
                                    Some(prompt_tokens as i32),
                                    Some(completion_tokens as i32),
                                    Some(total_tokens as i32),
                                    cost,
                                    latency_ms,
                                    stream_status,
                                    true,
                                    ttfb_ms,
                                    prompt_version_id,
                                    downgrade_triggered,
                                    &endpoint_log,
                                    None, // streaming tool_calls audit deferred to V5-1
                                    &tags_stream,
                                )
                                .await;
                                let _ = db::analytics::upsert_daily_cost(
                                    &pool,
                                    Some(api_key_id),
                                    workspace_id,
                                    provider_name,
                                    &final_model,
                                    prompt_tokens as i64,
                                    completion_tokens as i64,
                                    cost,
                                    false,
                                )
                                .await;

                                if let Some(cost_value) = cost {
                                    if cost_value > Decimal::ZERO {
                                        let _ = db::api_keys::add_budget_used(
                                            &pool, api_key_id, cost_value,
                                        )
                                        .await;
                                    }
                                }
                                let _ = db::api_keys::update_last_used(&pool, api_key_id).await;

                                // ── Streaming cache write ─────────────────
                                // Persist the assembled response so subsequent
                                // identical requests (streaming or not) hit
                                // cache via synthesize_sse_from_cached().
                                //
                                // Guards:
                                //  - !bypass_cache:        client opted in
                                //  - is_primary_model:     don't cache fallback
                                //                          model under the
                                //                          original request hash
                                //  - stream_completed:     provider closed the
                                //                          stream cleanly
                                //  - !client_disconnected: don't cache truncated
                                //                          responses
                                //  - stream_status=success not a mid-stream error
                                //  - completion_tokens > 0 we have content
                                //  - !accumulated_content.is_empty()
                                let should_cache = !bypass_cache
                                    && is_primary_model
                                    && stream_completed
                                    && !client_disconnected
                                    && stream_status == "success"
                                    && completion_tokens > 0
                                    && !accumulated_content.is_empty();

                                if should_cache {
                                    let synthetic_resp = ChatCompletionResponse {
                                        id: format!("chatcmpl-{}", Uuid::new_v4()),
                                        object: "chat.completion".to_string(),
                                        created: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_secs())
                                            .unwrap_or(0),
                                        model: final_model.clone(),
                                        choices: vec![ChatChoice {
                                            index: 0,
                                            message: ChatMessage {
                                                role: "assistant".to_string(),
                                                content: serde_json::Value::String(
                                                    accumulated_content,
                                                ),
                                                name: None,
                                                tool_calls: None,
                                                tool_call_id: None,
                                            },
                                            finish_reason: finish_reason_str
                                                .or_else(|| Some("stop".to_string())),
                                            logprobs: None,
                                        }],
                                        usage: UsageData {
                                            prompt_tokens,
                                            completion_tokens,
                                            total_tokens: prompt_tokens + completion_tokens,
                                        },
                                    };

                                    let resp_body = serde_json::to_string(&synthetic_resp)
                                        .unwrap_or_default();

                                    cache_for_write.insert_with_ttl(
                                        hash_for_cache.clone(),
                                        Arc::new(synthetic_resp),
                                        cache_ttl_secs,
                                    );

                                    let req_body = crate::pii::scrub(&request_body_for_cache)
                                        .into_owned();
                                    let _ = db::cache::upsert_entry(
                                        &pool,
                                        &hash_for_cache,
                                        provider_name,
                                        &final_model,
                                        &req_body,
                                        &resp_body,
                                        Some(prompt_tokens as i32),
                                        Some(completion_tokens as i32),
                                        cost,
                                        cache_ttl_secs,
                                    )
                                    .await;

                                    if let Some(emb) = embedding_for_write {
                                        cache_for_write
                                            .semantic_insert(emb.clone(), hash_for_cache.clone());
                                        let emb_bytes =
                                            crate::cache::semantic::f32_vec_to_bytes(&emb);
                                        let _ = db::cache::save_embedding(
                                            &pool,
                                            &hash_for_cache,
                                            &emb_bytes,
                                        )
                                        .await;
                                    }
                                }
                            });
                        });

                        crate::metrics::record_request(false);
                        let sse =
                            Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default());
                        return Ok((sse.into_response(), CacheHit::None));
                    }
                }
            }
        }
        // All providers failed for this model; try next fallback model if any.
    } // end for model_attempt

    Err(last_error
        .unwrap_or_else(|| AppError::ProviderUnavailable("All providers unavailable".to_string())))
}

// ── Convenience wrapper ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_with_workspace(
    pool: &DbPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    workspace_id: Option<Uuid>,
    max_retries: u32,
    cache: &CacheEngine,
    bypass_cache: bool,
    bypass_semantic: bool,
    strategy: &RoutingStrategy,
    fallback_models: &[String],
    dedup: &InFlightDeduplicator,
    cache_ttl_secs: u64,
    downgrade_triggered: bool,
    endpoint: &str,
    audit_sem: &Arc<tokio::sync::Semaphore>,
) -> AppResult<(ChatCompletionResponse, CacheHit)> {
    let key_with_workspace = ApiKey {
        workspace_id,
        ..api_key.clone()
    };
    run(
        pool,
        registry,
        request,
        &key_with_workspace,
        max_retries,
        cache,
        bypass_cache,
        bypass_semantic,
        strategy,
        fallback_models,
        None,
        &[], // no plugins in workspace convenience wrapper
        dedup,
        cache_ttl_secs,
        downgrade_triggered,
        None,
        &serde_json::Value::Object(serde_json::Map::new()),
        endpoint,
        audit_sem,
    )
    .await
}

// ── SSE synthesis for cache hits ──────────────────────────────────────────────

/// Build a valid SSE response from a cached non-streaming response.
fn synthesize_sse_from_cached(resp: &ChatCompletionResponse) -> Response {
    let content = resp
        .choices
        .first()
        .map(|c| c.message.content.as_str().unwrap_or("").to_string())
        .unwrap_or_default();

    let chunk_content = ChatCompletionChunk {
        id: resp.id.clone(),
        object: "chat.completion.chunk".to_string(),
        created: resp.created,
        model: resp.model.clone(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".to_string()),
                content: Some(content),
            },
            finish_reason: None,
        }],
        usage: None,
    };

    let chunk_done = ChatCompletionChunk {
        id: resp.id.clone(),
        object: "chat.completion.chunk".to_string(),
        created: resp.created,
        model: resp.model.clone(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(resp.usage.clone()),
    };

    let mut events: Vec<Result<Event, Infallible>> = Vec::new();
    if let Ok(data) = serde_json::to_string(&chunk_content) {
        events.push(Ok(Event::default().data(data)));
    }
    if let Ok(data) = serde_json::to_string(&chunk_done) {
        events.push(Ok(Event::default().data(data)));
    }
    events.push(Ok(Event::default().data("[DONE]")));

    Sse::new(futures_util::stream::iter(events))
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ── Error mapping ─────────────────────────────────────────────────────────────

pub fn map_provider_error(e: ProviderError) -> AppError {
    match e {
        ProviderError::RateLimit => AppError::RateLimitExceeded(None),
        ProviderError::Unauthorized => {
            AppError::ProviderUnavailable("Provider authentication failed".to_string())
        }
        ProviderError::Unavailable(msg) => AppError::ProviderUnavailable(msg),
        ProviderError::Timeout => {
            AppError::ProviderUnavailable("Provider request timed out".to_string())
        }
        ProviderError::BadRequest(msg) => AppError::BadRequest(msg),
        ProviderError::Http(e) => AppError::ProviderUnavailable(e.to_string()),
        ProviderError::ParseError(msg) => AppError::ProviderUnavailable(msg),
        ProviderError::Unsupported(name) => {
            AppError::BadRequest(format!("Modality not supported by provider '{name}'"))
        }
    }
}
