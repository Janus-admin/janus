use crate::{
    errors::AppError, gateway::pipeline, middleware::api_key_auth::GatewayAuth,
    middleware::budget::check_budget, providers::ChatCompletionRequest, state::AppState,
};
use axum::{
    extract::{rejection::JsonRejection, FromRequest, State},
    http::Request,
    response::IntoResponse,
    Json,
};
use serde_json::Value;
use std::sync::Arc;

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
/// Drop-in replacement for the OpenAI Chat Completions endpoint. Clients set
/// `base_url = "http://your-velox-host/v1"` and change nothing else.
/// Supports both non-streaming (default) and SSE streaming (`"stream": true`).
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<ChatCompletionRequest>,
) -> impl IntoResponse {
    // Budget gate — check before touching any provider.
    if let Err(e) = check_budget(&api_key) {
        return e.into_response();
    }

    // Rate limit gate (Phase 3) — per-key sliding window check.
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

    let max_retries = state.config.max_retries;

    if request.stream == Some(true) {
        match pipeline::run_streaming(
            state.pool.clone(),
            state.providers.clone(),
            request,
            api_key,
            max_retries,
        )
        .await
        {
            Ok(response) => response,
            Err(e) => e.into_response(),
        }
    } else {
        match pipeline::run(
            &state.pool,
            &state.providers,
            &request,
            &api_key,
            max_retries,
        )
        .await
        {
            Ok(resp) => match serde_json::to_value(resp) {
                Ok(v) => Json::<Value>(v).into_response(),
                Err(e) => AppError::Anyhow(anyhow::anyhow!("Failed to serialize response: {e}"))
                    .into_response(),
            },
            Err(e) => e.into_response(),
        }
    }
}
