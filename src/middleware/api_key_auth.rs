use crate::{
    db::api_keys::sha256_bytes,
    errors::AppError,
    models::api_key::ApiKey,
    state::AppState,
};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use std::sync::Arc;

/// Axum extractor that authenticates a gateway request via `Authorization: Bearer jn-sk-...`.
///
/// On success, injects the validated `ApiKey` into the handler. On failure, returns
/// `401 Unauthorized` with a JSON error body matching the Janus admin API format.
///
/// Key rotation (V3-5): if a key has been rotated, the DashMap contains entries for
/// both the new hash and the previous hash (pointing to the same ApiKey record).
/// When the presented hash matches `previous_key_sha256`, we check that
/// `rotation_expires_at` is still in the future; expired old keys are evicted and rejected.
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

        if !token.starts_with("jn-sk-") {
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

        // Rotation grace-period check (V3-5):
        // If the presented hash does not match the current key_sha256, the caller
        // is using the old (pre-rotation) key. Verify the grace window is still open.
        let presented_hex = {
            use std::fmt::Write;
            let mut s = String::with_capacity(64);
            for b in &key_bytes {
                let _ = write!(s, "{:02x}", b);
            }
            s
        };
        let is_previous_hash = api_key
            .key_sha256
            .as_deref()
            .map(|current| current != presented_hex.as_str())
            .unwrap_or(false);

        if is_previous_hash {
            match api_key.rotation_expires_at {
                Some(expires_at) if expires_at > chrono::Utc::now() => {
                    // Within grace period — allow the old key to pass.
                }
                _ => {
                    // Grace period has expired (or rotation_expires_at was never set).
                    // Evict the stale DashMap entry so future lookups fail fast.
                    app_state.key_cache.remove(&key_bytes);
                    return Err(AppError::Unauthorized(
                        "API key rotation grace period has expired".to_string(),
                    ));
                }
            }
        }

        Ok(GatewayAuth(api_key))
    }
}
