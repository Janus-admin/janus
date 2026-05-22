use crate::{
    cache::CacheEngine, config::Config, gateway::ProviderRegistry,
    middleware::rate_limit::RateLimiter, models::api_key::ApiKey,
};
use dashmap::DashMap;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub providers: Arc<ProviderRegistry>,
    pub key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub cache: Arc<CacheEngine>,
    /// Broadcast channel for the live WebSocket feed (/admin/stream).
    /// Each completed gateway request sends one JSON event here.
    pub event_tx: broadcast::Sender<Value>,
}
