use crate::db::DbPool;
use crate::{
    cache::{policy::SemanticCachePolicy, time_guard::TimeGuard, CacheEngine},
    cluster::rate_limit::DbRateLimiter,
    config::{Config, RuntimeConfig},
    enterprise::EnterpriseExt,
    gateway::{dedup::InFlightDeduplicator, ProviderRegistry},
    middleware::rate_limit::RateLimiter,
    models::api_key::ApiKey,
    plugins::RequestPlugin,
};
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Ephemeral state stored per OIDC login attempt.
/// Keyed by the CSRF state token; removed on callback or after TTL.
pub struct OidcState {
    pub code_verifier: String,
    pub nonce: String,
    pub idp_id: Uuid,
    pub created_at: std::time::Instant,
}

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
pub struct AppState {
    pub pool: DbPool,
    pub config: Config,
    /// Runtime-mutable config fields (logging flags, cache settings, max_retries).
    /// Updated by `PATCH /admin/config` without restart.
    pub runtime_config: Arc<arc_swap::ArcSwap<RuntimeConfig>>,
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
    /// Short-lived in-memory cache for GET /v1/models responses (5-second TTL).
    /// Per-AppState so tests don't share a global singleton.
    pub models_cache: Arc<std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>,
    /// In-flight OIDC login state: CSRF token → (PKCE verifier, nonce, idp_id).
    /// Entries are removed on callback (single use) or after 10 minutes (TTL).
    pub oidc_states: Arc<DashMap<String, OidcState>>,
    /// Enterprise capabilities (audit log, license, policy engine).
    /// Community builds hold `CommunityEnterprise` (all no-ops).
    /// Enterprise builds hold `EnterpriseState` (real DB writes + license).
    pub enterprise: Arc<dyn EnterpriseExt>,
    /// Bounds the number of concurrent fire-and-forget audit-log tasks.
    /// Why: each completed request spawns a task that writes 1–4 rows to PostgreSQL.
    /// Under sustained extreme load (e.g. >10k RPS), the DB can't keep up and the
    /// spawned tasks accumulate, consuming unbounded memory until OOM. This semaphore
    /// caps the in-flight audit set; when full, the record is dropped and the
    /// `janus_audit_dropped_total` counter increments. Normal load (< a few k RPS)
    /// never triggers this — DB finishes inserts faster than new requests arrive.
    pub audit_semaphore: Arc<tokio::sync::Semaphore>,
}
