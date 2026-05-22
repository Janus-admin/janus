use crate::{
    cache::CacheEngine, config::Config, gateway::ProviderRegistry,
    middleware::rate_limit::RateLimiter, models::api_key::ApiKey,
};
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub providers: Arc<ProviderRegistry>,
    pub key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub cache: Arc<CacheEngine>,
}
