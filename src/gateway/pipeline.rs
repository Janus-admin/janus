use super::{router, ProviderRegistry};
use crate::{
    db,
    errors::{AppError, AppResult},
    models::api_key::ApiKey,
    pricing,
    providers::{ChatCompletionRequest, ChatCompletionResponse, ProviderError},
};
use sqlx::PgPool;
use std::time::Instant;
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

fn map_provider_error(e: ProviderError) -> AppError {
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
