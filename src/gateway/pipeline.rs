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
    IntoResponse,
};
use futures_util::StreamExt;
use sqlx::PgPool;
use std::{convert::Infallible, sync::Arc, time::Instant};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// Run the full non-streaming proxy pipeline:
///   select provider → call provider → calculate cost → log to DB → return response.
pub async fn run(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
) -> AppResult<ChatCompletionResponse> {
    // 1. Select provider
    let provider = router::select_provider(registry, &request.model).ok_or_else(|| {
        AppError::ProviderUnavailable("No enabled providers available".to_string())
    })?;

    // 2. Call provider
    let start = Instant::now();
    let result = provider.chat_completion(request).await;
    let latency_ms = start.elapsed().as_millis() as i32;

    // 3. Handle provider error vs success
    let (response, status) = match result {
        Ok(resp) => (Some(resp), "success"),
        Err(e) => {
            let status = match &e {
                ProviderError::RateLimit => "rate_limit",
                ProviderError::Unauthorized => "auth_error",
                ProviderError::Timeout => "timeout",
                ProviderError::BadRequest(_) => "bad_request",
                _ => "error",
            };
            // Fire-and-forget: log the failed request without blocking the error response
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
            return Err(map_provider_error(e));
        }
    };

    let resp = response.unwrap();

    // 4. Calculate cost (synchronous — needed before we spawn the log task)
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

    // 5–7. Fire-and-forget: log + budget + last_used without blocking the response
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
                status,
                false,
                None,
            )
            .await;
            if let Some(cost_value) = cost {
                if cost_value > rust_decimal::Decimal::ZERO {
                    let _ = db::api_keys::add_budget_used(&pool, api_key_id, cost_value).await;
                }
            }
            let _ = db::api_keys::update_last_used(&pool, api_key_id).await;
        });
    }

    Ok(resp)
}

/// Run the streaming proxy pipeline.
/// Selects a provider, opens a streaming call, normalises chunks to OpenAI SSE
/// format, forwards them to the client, then logs cost + token counts after the
/// stream closes.
pub async fn run_streaming(
    pool: PgPool,
    registry: Arc<ProviderRegistry>,
    request: ChatCompletionRequest,
    api_key: ApiKey,
) -> AppResult<impl IntoResponse> {
    let wall_start = Instant::now();

    let provider = router::select_provider(&registry, &request.model).ok_or_else(|| {
        AppError::ProviderUnavailable("No enabled providers available".to_string())
    })?;

    let provider_name = provider.name();
    let api_key_id = api_key.id;
    let workspace_id = api_key.workspace_id;
    let model = request.model.clone();

    let provider_stream = provider
        .chat_completion_stream(&request)
        .await
        .map_err(map_provider_error)?;

    // Channel that carries SSE Event items to axum's Sse responder.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    // Drive the provider stream in a background task so we can return the SSE
    // response immediately. Token counts and DB logging happen here too.
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
                    // Record the model name from the first chunk that carries it.
                    if !chunk.model.is_empty() {
                        final_model = chunk.model.clone();
                    }

                    // Accumulate token counts. Prefer explicit usage from the
                    // provider (final chunk); otherwise count content deltas.
                    if let Some(usage) = &chunk.usage {
                        prompt_tokens = usage.prompt_tokens;
                        completion_tokens = usage.completion_tokens;
                    } else {
                        for choice in &chunk.choices {
                            if !choice.delta.content.as_deref().unwrap_or("").is_empty() {
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
                        // Client disconnected — stop streaming but still log.
                        break;
                    }
                }
            }
        }

        // Send the [DONE] sentinel (OpenAI SSE convention).
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
        drop(tx);

        // Log request + update budget + last_used after the stream closes.
        let latency_ms = wall_start.elapsed().as_millis() as i32;
        let total_tokens = prompt_tokens + completion_tokens;

        tokio::spawn(async move {
            let cost = db::requests::find_pricing(&pool, provider_name, &final_model)
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
                    let _ = db::api_keys::add_budget_used(&pool, api_key_id, cost_value).await;
                }
            }
            let _ = db::api_keys::update_last_used(&pool, api_key_id).await;
        });
    });

    let sse = Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default());
    Ok(sse)
}

/// Update api_key.id usage stats and handle workspace_id being optional
pub async fn run_with_workspace(
    pool: &PgPool,
    registry: &ProviderRegistry,
    request: &ChatCompletionRequest,
    api_key: &ApiKey,
    workspace_id: Option<Uuid>,
) -> AppResult<ChatCompletionResponse> {
    // Build a synthetic key view with the overridden workspace_id
    let key_with_workspace = ApiKey {
        workspace_id,
        ..api_key.clone()
    };
    run(pool, registry, request, &key_with_workspace).await
}

pub fn map_provider_error(e: ProviderError) -> AppError {
    match e {
        ProviderError::RateLimit => AppError::RateLimitExceeded,
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
