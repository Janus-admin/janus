use crate::{
    cache::CacheEngine,
    config::{Config, RuntimeConfig},
    gateway::ProviderRegistry,
    middleware::rate_limit::RateLimiter,
    models::api_key::ApiKey,
};
use dashmap::DashMap;
use serde_json::Value;
use crate::db::DbPool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
    /// Runtime-mutable config fields (logging flags, cache settings, max_retries).
    /// Updated by `PATCH /admin/config` without restart.
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    pub providers: Arc<ProviderRegistry>,
    pub key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub cache: Arc<CacheEngine>,
    /// Broadcast channel for the live WebSocket feed (/admin/stream).
    /// Each completed gateway request sends one JSON event here.
    pub event_tx: broadcast::Sender<Value>,
}
