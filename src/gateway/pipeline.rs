use super::{router, ProviderRegistry};
use crate::{
    cache::{self, CacheEngine, CacheHit},
    db,
    errors::{AppError, AppResult},
    models::api_key::ApiKey,
    pricing,
    providers::{
        ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChunkChoice,
        ChunkDelta, ProviderError,
    },
};
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use futures_util::StreamExt;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::{convert::Infallible, sync::Arc, time::Instant};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

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
pub async fn run(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    max_retries: u32,
    cache: &CacheEngine,
    bypass_cache: bool,
) -> AppResult<(ChatCompletionResponse, CacheHit)> {
    // ── Exact cache lookup ────────────────────────────────────────────────────
    let hash = cache::exact::compute_hash(request);

    if !bypass_cache {
        if let Some(cached) = cache.lookup(&hash) {
            let tokens = cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
            let _ = db::cache::record_hit(pool, &hash, tokens, Decimal::ZERO).await;
            return Ok(((*cached).clone(), CacheHit::Exact));
        }
    }

    // ── Semantic cache lookup ─────────────────────────────────────────────────
    // Compute embedding once; reuse it after provider call to populate the index.
    let embedding: Option<Vec<f32>> = if !bypass_cache && cache.model.is_some() {
        let text = prompt_text(request);
        cache.model.as_ref().and_then(|m| m.embed(&text).ok())
    } else {
        None
    };

    if !bypass_cache {
        if let Some(ref emb) = embedding {
            if let Some((hit_hash, score)) = cache.semantic_lookup(emb) {
                if let Some(cached) = cache.lookup(&hit_hash) {
                    let tokens =
                        cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
                    let _ = db::cache::record_hit(pool, &hit_hash, tokens, Decimal::ZERO).await;
                    return Ok(((*cached).clone(), CacheHit::Semantic(score)));
                }
            }
        }
    }

    // ── Provider loop ─────────────────────────────────────────────────────────
    let providers = router::select_all_providers(registry);
    if providers.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "No enabled providers available".to_string(),
        ));
    }

    let mut last_error: Option<AppError> = None;

    'provider: for provider in &providers {
        let mut attempts = 0u32;

        loop {
            let start = Instant::now();
            let result = provider.chat_completion(request).await;
            let latency_ms = start.elapsed().as_millis() as i32;

            match result {
                Ok(resp) => {
                    let usage = &resp.usage;
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

                    // Store in hot cache + persist to DB synchronously.
                    if !bypass_cache {
                        cache.insert(hash.clone(), Arc::new(resp.clone()));

                        let req_body = serde_json::to_string(request).unwrap_or_default();
                        let resp_body = serde_json::to_string(&resp).unwrap_or_default();
                        let (pt, ct) = (usage.prompt_tokens as i32, usage.completion_tokens as i32);
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
                        )
                        .await;

                        // Persist embedding and populate semantic index.
                        if let Some(emb) = embedding {
                            cache.semantic_insert(emb.clone(), hash.clone());
                            let emb_bytes = crate::cache::semantic::f32_vec_to_bytes(&emb);
                            let _ = db::cache::save_embedding(pool, &hash, &emb_bytes).await;
                        }
                    }

                    // Fire-and-forget: log request + update budget + last_used.
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
                        tokio::spawn(async move {
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

                    {
                        let pool = pool.clone();
                        let api_key_id = api_key.id;
                        let workspace_id = api_key.workspace_id;
                        let provider_name = provider.name();
                        let model = request.model.clone();
                        tokio::spawn(async move {
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

    Err(last_error
        .unwrap_or_else(|| AppError::ProviderUnavailable("All providers unavailable".to_string())))
}

// ── Streaming pipeline ────────────────────────────────────────────────────────

/// Run the streaming proxy pipeline.
///
/// Returns `(response, CacheHit)`. On an exact or semantic cache hit the cached
/// response is synthesized as a valid SSE stream.
pub async fn run_streaming(
    pool: PgPool,
    registry: Arc<ProviderRegistry>,
    request: ChatCompletionRequest,
    api_key: ApiKey,
    max_retries: u32,
    cache: Arc<CacheEngine>,
    bypass_cache: bool,
) -> AppResult<(Response, CacheHit)> {
    // ── Exact cache lookup ────────────────────────────────────────────────────
    let hash = cache::exact::compute_hash(&request);

    if !bypass_cache {
        if let Some(cached) = cache.lookup(&hash) {
            let tokens = cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
            let _ = db::cache::record_hit(&pool, &hash, tokens, Decimal::ZERO).await;

            let sse = synthesize_sse_from_cached(&cached);
            return Ok((sse, CacheHit::Exact));
        }
    }

    // ── Semantic cache lookup ─────────────────────────────────────────────────
    let embedding: Option<Vec<f32>> = if !bypass_cache && cache.model.is_some() {
        let text = prompt_text(&request);
        cache.model.as_ref().and_then(|m| m.embed(&text).ok())
    } else {
        None
    };

    if !bypass_cache {
        if let Some(ref emb) = embedding {
            if let Some((hit_hash, score)) = cache.semantic_lookup(emb) {
                if let Some(cached) = cache.lookup(&hit_hash) {
                    let tokens =
                        cached.usage.prompt_tokens as i64 + cached.usage.completion_tokens as i64;
                    let _ = db::cache::record_hit(&pool, &hit_hash, tokens, Decimal::ZERO).await;
                    let sse = synthesize_sse_from_cached(&cached);
                    return Ok((sse, CacheHit::Semantic(score)));
                }
            }
        }
    }

    // ── Provider loop ─────────────────────────────────────────────────────────
    let providers = router::select_all_providers(&registry);
    if providers.is_empty() {
        return Err(AppError::ProviderUnavailable(
            "No enabled providers available".to_string(),
        ));
    }

    let wall_start = Instant::now();
    let mut last_error: Option<AppError> = None;

    for provider in &providers {
        let mut attempts = 0u32;

        loop {
            let stream_result = provider.chat_completion_stream(&request).await;

            if let Err(ref e) = stream_result {
                let retriable = matches!(e, ProviderError::Unavailable(_) | ProviderError::Timeout);
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
                    let provider_name = provider.name();
                    let api_key_id = api_key.id;
                    let workspace_id = api_key.workspace_id;
                    let model = request.model.clone();

                    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

                    tokio::spawn(async move {
                        let mut prompt_tokens: u32 = 0;
                        let mut completion_tokens: u32 = 0;
                        let mut ttfb_ms: Option<i32> = None;
                        let mut final_model = model.clone();

                        tokio::pin!(provider_stream);

                        while let Some(chunk_result) = provider_stream.next().await {
                            if ttfb_ms.is_none() {
                                ttfb_ms = Some(wall_start.elapsed().as_millis() as i32);
                            }

                            match chunk_result {
                                Err(e) => {
                                    tracing::error!(provider = provider_name, "Stream error: {e}");
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

                                    let data = match serde_json::to_string(&chunk) {
                                        Ok(s) => s,
                                        Err(e) => {
                                            tracing::warn!("Chunk serialise error: {e}");
                                            continue;
                                        }
                                    };

                                    if tx.send(Ok(Event::default().data(data))).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }

                        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
                        drop(tx);

                        let latency_ms = wall_start.elapsed().as_millis() as i32;
                        let total_tokens = prompt_tokens + completion_tokens;

                        tokio::spawn(async move {
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
                                "success",
                                true,
                                ttfb_ms,
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
                    });

                    let sse = Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default());
                    return Ok((sse.into_response(), CacheHit::None));
                }
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| AppError::ProviderUnavailable("All providers unavailable".to_string())))
}

// ── Convenience wrapper ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_with_workspace(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    workspace_id: Option<Uuid>,
    max_retries: u32,
    cache: &CacheEngine,
    bypass_cache: bool,
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

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(4);
    tokio::spawn(async move {
        if let Ok(data) = serde_json::to_string(&chunk_content) {
            let _ = tx.send(Ok(Event::default().data(data))).await;
        }
        if let Ok(data) = serde_json::to_string(&chunk_done) {
            let _ = tx.send(Ok(Event::default().data(data))).await;
        }
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
    });

    Sse::new(ReceiverStream::new(rx))
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
    }
}
