use crate::db::DbPool;
use crate::{
    cache::{policy::SemanticCachePolicy, time_guard::TimeGuard, CacheEngine},
    cluster::rate_limit::DbRateLimiter,
    config::{Config, RuntimeConfig},
    gateway::{dedup::InFlightDeduplicator, ProviderRegistry},
    middleware::rate_limit::RateLimiter,
    models::api_key::ApiKey,
    plugins::RequestPlugin,
};
use dashmap::DashMap;
use serde_json::Value;
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
    /// DB-backed rate limiter for cluster mode (Some when cluster.enabled = true).
    pub cluster_rate_limiter: Option<Arc<DbRateLimiter>>,
    pub cache: Arc<CacheEngine>,
    /// Controls which model/route/key combinations are eligible for semantic cache.
    pub semantic_policy: SemanticCachePolicy,
    /// Broadcast channel for the live WebSocket feed (/admin/stream).
    /// Each completed gateway request sends one JSON event here.
    pub event_tx: broadcast::Sender<Value>,
    /// Ordered plugin chain executed for every gateway request.
    pub plugins: Arc<Vec<Box<dyn RequestPlugin>>>,
    /// In-flight request deduplicator — prevents N identical concurrent
    /// non-streaming requests from each making a separate provider call.
    pub dedup: Arc<InFlightDeduplicator>,
    /// Time-sensitive query detector — skips cache for prompts matching
    /// time-bound patterns (e.g. "today", "current price", "الآن").
    pub time_guard: Arc<TimeGuard>,
}
