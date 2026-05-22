use crate::{
    db::api_keys::sha256_bytes, errors::AppError, models::api_key::ApiKey, state::AppState,
};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use std::sync::Arc;

/// Axum extractor that authenticates a gateway request via `Authorization: Bearer vx-sk-...`.
///
/// On success, injects the validated `ApiKey` into the handler. On failure, returns
/// `401 Unauthorized` with a JSON error body matching the Velox admin API format.
pub struct GatewayAuth(pub ApiKey);

#[axum::async_trait]
impl<S> FromRequestParts<S> for GatewayAuth
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = Arc::<AppState>::from_ref(state);

        // Extract the Bearer token from the Authorization header
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            AppError::Unauthorized("Authorization header must use Bearer scheme".to_string())
        })?;

        if !token.starts_with("vx-sk-") {
            return Err(AppError::Unauthorized("Invalid API key format".to_string()));
        }

        // SHA-256 lookup in the in-memory dashmap (O(1), ~microseconds)
        let key_bytes = sha256_bytes(token);
        let api_key = app_state
            .key_cache
            .get(&key_bytes)
            .map(|r| r.value().clone())
            .ok_or_else(|| AppError::Unauthorized("Invalid or inactive API key".to_string()))?;

        if !api_key.is_active {
            return Err(AppError::Unauthorized("API key is inactive".to_string()));
        }

        // Check expiry
        if let Some(expires_at) = api_key.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(AppError::Unauthorized("API key has expired".to_string()));
            }
        }

        Ok(GatewayAuth(api_key))
    }
}
