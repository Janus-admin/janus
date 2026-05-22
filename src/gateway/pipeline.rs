use super::{router, ProviderRegistry};
use crate::{
    db,
    errors::{AppError, AppResult},
    models::api_key::ApiKey,
    pricing,
    providers::{ChatCompletionRequest, ChatCompletionResponse, ProviderError},
};
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use futures_util::StreamExt;
use sqlx::PgPool;
use std::{convert::Infallible, sync::Arc, time::Instant};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

// ── Non-streaming pipeline ────────────────────────────────────────────────────

/// Run the full non-streaming proxy pipeline with retry and provider failover:
///   for each provider (ascending priority):
///     retry up to `max_retries` times on retriable errors
///     → on success: calculate cost, log to DB, return response
///     → on non-retriable error: abort immediately
///   if all providers fail → 503
pub async fn run(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    max_retries: u32,
) -> AppResult<ChatCompletionResponse> {
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
                    // Calculate cost before spawning the log task.
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

                    // Fire-and-forget: log + budget + last_used without blocking.
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
                                if cost_value > rust_decimal::Decimal::ZERO {
                                    let _ = db::api_keys::add_budget_used(
                                        &pool, api_key_id, cost_value,
                                    )
                                    .await;
                                }
                            }
                            let _ = db::api_keys::update_last_used(&pool, api_key_id).await;
                        });
                    }

                    return Ok(resp);
                }

                Err(e) => {
                    let status = match &e {
                        ProviderError::RateLimit => "rate_limit",
                        ProviderError::Unauthorized => "auth_error",
                        ProviderError::Timeout => "timeout",
                        ProviderError::BadRequest(_) => "bad_request",
                        _ => "error",
                    };

                    // Fire-and-forget: log the failed attempt.
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

                    // Auth and bad-request errors are not recoverable on any provider.
                    if matches!(
                        &e,
                        ProviderError::Unauthorized | ProviderError::BadRequest(_)
                    ) {
                        last_error = Some(map_provider_error(e));
                        break 'provider;
                    }

                    // Retriable errors: retry the same provider.
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
                        continue; // retry
                    }

                    // Exhausted retries (or non-retriable like RateLimit/Http) — try next provider.
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

/// Run the streaming proxy pipeline with provider failover.
/// Tries each enabled provider in priority order; on initial connection failure
/// moves to the next. No per-provider retry for streams (can't rewind SSE).
/// All providers fail → 503.
pub async fn run_streaming(
    pool: PgPool,
    registry: Arc<ProviderRegistry>,
    request: ChatCompletionRequest,
    api_key: ApiKey,
    max_retries: u32,
) -> AppResult<Response> {
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
                    break; // try next provider
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
                                if cost_value > rust_decimal::Decimal::ZERO {
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
                    return Ok(sse.into_response());
                }
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| AppError::ProviderUnavailable("All providers unavailable".to_string())))
}

// ── Convenience wrapper ───────────────────────────────────────────────────────

pub async fn run_with_workspace(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    workspace_id: Option<Uuid>,
    max_retries: u32,
) -> AppResult<ChatCompletionResponse> {
    let key_with_workspace = ApiKey {
        workspace_id,
        ..api_key.clone()
    };
    run(pool, registry, request, &key_with_workspace, max_retries).await
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
