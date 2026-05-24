use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

/// Velox runtime configuration.
///
/// Loading priority (high → low):
///   1. Environment variables
///   2. velox.toml (optional, in working directory)
///   3. Default values below
///
/// Env var convention: field name uppercased, e.g. `database_url` → `DATABASE_URL`.
/// For nested future use, double-underscore acts as separator: `CACHE__TTL_SECONDS`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // ── Server ────────────────────────────────────────────────────────────────
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_request_timeout_ms")]
    #[allow(dead_code)] // used in Phase 1 provider HTTP client
    pub request_timeout_ms: u64,

    // ── Database ──────────────────────────────────────────────────────────────
    pub database_url: String,
    #[serde(default = "default_db_pool_max_connections")]
    pub db_pool_max_connections: u32,

    // ── Auth (admin dashboard JWT) ─────────────────────────────────────────────
    pub jwt_secret: String,
    #[serde(default = "default_jwt_expiration_hours")]
    pub jwt_expiration_hours: i64,
    /// AES-256-GCM key for encrypting provider API keys at rest (Phase 1+).
    /// Generate with: openssl rand -base64 32
    #[serde(default)]
    #[allow(dead_code)] // used in Phase 1 provider key encryption
    pub encryption_key: String,

    // ── Logging ───────────────────────────────────────────────────────────────
    #[serde(default = "default_log_level")]
    #[allow(dead_code)] // used in Phase 7 logging middleware
    pub log_level: String,
    #[serde(default)]
    pub log_request_bodies: bool,
    #[serde(default)]
    pub log_response_bodies: bool,

    // ── Cache ─────────────────────────────────────────────────────────────────
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    #[serde(default = "default_cache_ttl_seconds")]
    #[allow(dead_code)] // used in Phase 4 cache TTL enforcement
    pub cache_ttl_seconds: u64,
    #[serde(default = "default_cache_max_entries")]
    #[allow(dead_code)] // used in Phase 4 cache size limit
    pub cache_max_entries: u64,
    #[serde(default = "default_semantic_threshold")]
    #[allow(dead_code)] // used in Phase 5 semantic cache similarity gate
    pub semantic_cache_threshold: f64,

    // ── Semantic cache (Phase 5 + V3-1) ──────────────────────────────────────
    #[serde(default = "default_embedding_model_path")]
    pub embedding_model_path: String,
    #[serde(default = "default_embedding_tokenizer_path")]
    pub embedding_tokenizer_path: String,
    /// Index backend: "linear" (default, O(n)) or "hnsw" (O(log n), V3-1).
    #[serde(default = "default_semantic_backend")]
    pub semantic_cache_backend: String,
    /// HNSW ef parameter for both construction and search. Default: 200.
    #[serde(default = "default_hnsw_ef")]
    pub semantic_cache_hnsw_ef: usize,
    /// HNSW M parameter (connections per node). Default: 16.
    #[serde(default = "default_hnsw_connections")]
    pub semantic_cache_hnsw_connections: usize,
    /// If non-empty, only requests for these model IDs use semantic cache.
    #[serde(default)]
    pub semantic_cache_models: Vec<String>,
    /// Route prefixes excluded from semantic cache.
    #[serde(default)]
    pub semantic_cache_exclude_routes: Vec<String>,

    // ── Provider API keys (Phase 1+) ──────────────────────────────────────────
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub anthropic_api_key: String,
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default)]
    pub groq_api_key: String,
    #[serde(default)]
    pub deepseek_api_key: String,

    // ── Rate limiting & reliability (Phase 3) ────────────────────────────────
    /// Sliding window size in seconds for per-key rate limiting.
    #[serde(default = "default_rate_limit_window_secs")]
    pub rate_limit_window_secs: u64,
    /// Max retry attempts per provider before failing over to the next.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    // ── Metrics ───────────────────────────────────────────────────────────────
    #[serde(default = "default_true")]
    #[allow(dead_code)] // used in Phase 7 Prometheus endpoint
    pub prometheus_enabled: bool,

    // ── Routing (V2-4) ────────────────────────────────────────────────────────
    /// Model fallback chains for intelligent routing.
    /// When a request fails for a model, fallback models are tried in order.
    ///
    /// Example in velox.toml:
    /// ```toml
    /// [routing.fallbacks]
    /// "gpt-4o" = ["claude-3-5-sonnet-20241022", "gpt-4o-mini"]
    /// ```
    #[serde(default)]
    pub routing: RoutingConfig,

    // ── Clustering (V2-6) ─────────────────────────────────────────────────────
    /// Multi-node clustering configuration.
    /// When enabled, rate limits use the shared `rate_limit_windows` DB table
    /// and key revocations propagate via PostgreSQL LISTEN/NOTIFY.
    ///
    /// Example in velox.toml:
    /// ```toml
    /// [cluster]
    /// enabled = true
    /// node_id = "node-1"
    /// ```
    #[serde(default)]
    pub cluster: ClusterConfig,
}

/// Routing configuration for intelligent provider selection.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RoutingConfig {
    /// Per-model fallback chains: model → ordered list of fallback model IDs.
    #[serde(default)]
    pub fallbacks: HashMap<String, Vec<String>>,
}

/// Multi-node clustering configuration (V2-6).
#[derive(Debug, Clone, Deserialize)]
pub struct ClusterConfig {
    /// When true, rate limits use the shared DB table and key revocations
    /// propagate via PostgreSQL LISTEN/NOTIFY.
    /// Default: false (single-node in-memory mode).
    #[serde(default)]
    pub enabled: bool,
    /// Identifier for this node, used in log correlation.
    #[serde(default = "default_node_id")]
    pub node_id: String,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: default_node_id(),
        }
    }
}

fn default_node_id() -> String {
    "node-1".to_string()
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    8080
}
fn default_request_timeout_ms() -> u64 {
    60_000
}
fn default_db_pool_max_connections() -> u32 {
    5
}
fn default_jwt_expiration_hours() -> i64 {
    24
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_true() -> bool {
    true
}
fn default_cache_ttl_seconds() -> u64 {
    3_600
}
fn default_cache_max_entries() -> u64 {
    100_000
}
fn default_semantic_threshold() -> f64 {
    0.90
}
fn default_embedding_model_path() -> String {
    "models/all-MiniLM-L6-v2.onnx".to_string()
}
fn default_embedding_tokenizer_path() -> String {
    "models/tokenizer.json".to_string()
}
fn default_semantic_backend() -> String {
    "linear".to_string()
}
fn default_hnsw_ef() -> usize {
    200
}
fn default_hnsw_connections() -> usize {
    16
}
fn default_rate_limit_window_secs() -> u64 {
    60
}
fn default_max_retries() -> u32 {
    1
}

/// Runtime-mutable subset of Config.
///
/// These fields can be toggled via `PATCH /admin/config` without restart.
/// Stored separately in `AppState` behind an `Arc<tokio::sync::RwLock<>>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeConfig {
    pub log_request_bodies: bool,
    pub log_response_bodies: bool,
    pub cache_enabled: bool,
    pub max_retries: u32,
    pub semantic_cache_threshold: f64,
}

impl From<&Config> for RuntimeConfig {
    fn from(c: &Config) -> Self {
        Self {
            log_request_bodies: c.log_request_bodies,
            log_response_bodies: c.log_response_bodies,
            cache_enabled: c.cache_enabled,
            max_retries: c.max_retries,
            semantic_cache_threshold: c.semantic_cache_threshold,
        }
    }
}

impl Config {
    /// Load configuration from velox.toml (optional) then environment variables.
    ///
    /// Environment variables always win. `DATABASE_URL` maps to `database_url`,
    /// `JWT_SECRET` maps to `jwt_secret`, etc.
    pub fn load() -> Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::File::with_name("velox").required(false))
            .add_source(
                config::Environment::default()
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        Ok(cfg.try_deserialize()?)
    }
}
