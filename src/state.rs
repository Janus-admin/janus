use crate::{
    config::Config, gateway::ProviderRegistry, middleware::rate_limit::RateLimiter,
    models::api_key::ApiKey,
};
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
///
/// NOTE: The `pool` field will be renamed to `db` once Phase 1 allows touching
/// `src/handlers/auth.rs` which still references `state.pool`.
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub providers: Arc<ProviderRegistry>,
    pub key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    pub rate_limiter: Arc<RateLimiter>,
}
