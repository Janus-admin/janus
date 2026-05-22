use crate::{
    errors::{AppError, AppResult},
    gateway::pipeline,
    middleware::api_key_auth::GatewayAuth,
    middleware::budget::check_budget,
    providers::ChatCompletionRequest,
    state::AppState,
};
use axum::{
    extract::{rejection::JsonRejection, FromRequest, State},
    http::Request,
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
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    ValidatedJson(request): ValidatedJson<ChatCompletionRequest>,
) -> AppResult<Json<Value>> {
    // Budget gate — check before touching any provider
    check_budget(&api_key)?;

    // Check allowed models if the key has model restrictions
    if let Some(ref allowed) = api_key.allowed_models {
        if !allowed.is_empty() && !allowed.contains(&request.model) {
            return Err(AppError::Forbidden(format!(
                "Model '{}' is not permitted for this API key",
                request.model
            )));
        }
    }

    let response = pipeline::run(&state.pool, &state.providers, &request, &api_key).await?;

    Ok(Json(serde_json::to_value(response).map_err(|e| {
        AppError::Anyhow(anyhow::anyhow!("Failed to serialize response: {e}"))
    })?))
}
