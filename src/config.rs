use anyhow::Result;
use serde::Deserialize;

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
    #[allow(dead_code)] // used in Phase 7 request body logging
    pub log_request_bodies: bool,
    #[serde(default)]
    #[allow(dead_code)] // used in Phase 7 response body logging
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

    // ── Metrics ───────────────────────────────────────────────────────────────
    #[serde(default = "default_true")]
    #[allow(dead_code)] // used in Phase 7 Prometheus endpoint
    pub prometheus_enabled: bool,
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
    0.95
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
