use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

/// Janus runtime configuration.
///
/// Loading priority (high → low):
///   1. Environment variables
///   2. janus.toml (optional, in working directory)
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

    // ── Cache TTL (V4-3) ──────────────────────────────────────────────────────
    /// Global cache TTL in seconds. 0 = no expiry (backward-compatible default).
    #[serde(default)]
    pub cache_ttl_secs: u64,
    /// Per-model TTL overrides. Key = model name, value = TTL in seconds.
    /// When set, takes precedence over `cache_ttl_secs` for that model.
    ///
    /// Example in janus.toml:
    /// ```toml
    /// [cache_ttl_overrides]
    /// "gpt-4o-mini" = 3600
    /// ```
    #[serde(default)]
    pub cache_ttl_overrides: HashMap<String, u64>,

    // ── Time-sensitive cache bypass (V4-3) ────────────────────────────────────
    /// Regex patterns checked against all message content before cache lookup.
    /// If any pattern matches, the request bypasses both cache lookup and write.
    /// Supports English, Persian, and Arabic time-related phrases by default.
    #[serde(default = "default_time_sensitive_patterns")]
    pub time_sensitive_patterns: Vec<String>,

    // ── Semantic cache (Phase 5 + V3-1) ──────────────────────────────────────
    #[serde(default = "default_embedding_model_path")]
    pub embedding_model_path: String,
    #[serde(default = "default_embedding_tokenizer_path")]
    pub embedding_tokenizer_path: String,
    /// Index backend: "linear" (default, O(n)), "hnsw" (O(log n), V3-1),
    /// or "qdrant" (external vector store, V4-9).
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

    // ── Qdrant external vector store (V4-9) ───────────────────────────────────
    /// gRPC URL for the Qdrant instance. Used when `semantic_cache_backend = "qdrant"`.
    #[serde(default = "default_qdrant_url")]
    pub qdrant_url: String,
    /// Qdrant collection name for the semantic cache index.
    #[serde(default = "default_qdrant_collection")]
    pub qdrant_collection: String,
    /// Embedding dimensionality for Qdrant collection creation. Default: 384 (all-MiniLM-L6-v2).
    #[serde(default = "default_qdrant_vector_size")]
    pub qdrant_vector_size: u64,

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
    /// Example in janus.toml:
    /// ```toml
    /// [routing.fallbacks]
    /// "gpt-4o" = ["claude-3-5-sonnet-20241022", "gpt-4o-mini"]
    /// ```
    #[serde(default)]
    pub routing: RoutingConfig,

    // ── Distributed tracing (V3-2) ───────────────────────────────────────────
    #[serde(default)]
    pub tracing: TracingConfig,

    // ── Plugin middleware (V3-4) ──────────────────────────────────────────────
    #[serde(default)]
    pub plugins: PluginsConfig,

    // ── TLS / mTLS for provider connections (V3-5) ───────────────────────────
    #[serde(default)]
    pub provider_tls: ProviderTlsConfig,

    // ── Key rotation grace period (V3-5) ─────────────────────────────────────
    /// Seconds the old key remains valid after a rotation. Default: 300 (5 min).
    #[serde(default = "default_rotation_grace_period_secs")]
    pub rotation_grace_period_secs: u64,

    // ── Budget-aware auto-downgrade (V4-4) ────────────────────────────────────
    /// When enabled, requests from keys nearing their budget limit are
    /// automatically routed to cheaper models/strategies instead of blocking.
    #[serde(default)]
    pub budget_downgrade: BudgetDowngradeConfig,

    // ── Clustering (V2-6) ─────────────────────────────────────────────────────
    /// Multi-node clustering configuration.
    /// When enabled, rate limits use the shared `rate_limit_windows` DB table
    /// and key revocations propagate via PostgreSQL LISTEN/NOTIFY.
    ///
    /// Example in janus.toml:
    /// ```toml
    /// [cluster]
    /// enabled = true
    /// node_id = "node-1"
    /// ```
    #[serde(default)]
    pub cluster: ClusterConfig,

    // ── SMTP / Email Alerts (V5-L4) ───────────────────────────────────────────
    #[serde(default)]
    pub smtp: SmtpConfig,

    // ── Smart Routing (V5-L6) ─────────────────────────────────────────────────
    /// Global smart-routing defaults. Per-workspace overrides live in the DB
    /// (smart_routing_config table). DB values always take precedence over these.
    #[serde(default)]
    pub smart_routing: SmartRoutingConfig,

    // ── Enterprise license (optional) ────────────────────────────────────────
    /// Signed RS256 JWT issued by Janus-admin to licensed customers.
    /// Set via `JANUS_LICENSE_JWT` env var or `license_jwt` in janus.toml.
    /// Absent or empty = community edition (all enterprise features disabled).
    #[serde(default)]
    pub license_jwt: String,
}

/// SMTP configuration for email alert delivery (V5-L4).
///
/// Set `smtp.host` in janus.toml or via `SMTP__HOST` env var to enable email alerts.
/// When `smtp.file_dir` is non-empty, emails are written to that directory as .eml
/// files instead of being sent — useful for testing and CI environments.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SmtpConfig {
    /// SMTP hostname. Leave empty to disable email alerts entirely.
    #[serde(default)]
    pub host: String,
    /// SMTP port. Default: 587 (STARTTLS).
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    /// SMTP login username.
    #[serde(default)]
    pub username: String,
    /// SMTP login password.
    #[serde(default)]
    pub password: String,
    /// RFC 5321 sender address (e.g. `janus@acme.com`). Defaults to `janus@<host>`.
    #[serde(default)]
    pub from_address: String,
    /// When non-empty, write emails as .eml files in this directory instead of sending.
    /// Intended for testing and CI — not for production use.
    #[serde(default)]
    pub file_dir: String,
}

fn default_smtp_port() -> u16 {
    587
}

/// Global smart-routing configuration (V5-L6).
///
/// Per-workspace overrides are stored in the `smart_routing_config` DB table and
/// always take precedence. These values act as the system-wide fallback.
///
/// Example in janus.toml:
/// ```toml
/// [smart_routing]
/// enabled       = true
/// default_model = "gpt-4o-mini"
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SmartRoutingConfig {
    /// Enable smart routing globally. When false, requests without a model field
    /// receive a 400 Bad Request. Default: false.
    #[serde(default)]
    pub enabled: bool,
    /// Fallback model used when no tier match is found and no workspace default
    /// is configured. Empty string = no global fallback (returns 400).
    #[serde(default)]
    pub default_model: String,
    /// Global per-request cost cap in USD. Overridden per workspace in DB.
    /// Models whose estimated cost exceeds this are excluded from Layer 1.
    /// None (default) = no cap.
    #[serde(default)]
    pub max_cost_per_request: Option<rust_decimal::Decimal>,
}

/// Routing configuration for intelligent provider selection.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RoutingConfig {
    /// Per-model fallback chains: model → ordered list of fallback model IDs.
    #[serde(default)]
    pub fallbacks: HashMap<String, Vec<String>>,
}

/// mTLS / TLS-pinning configuration for outbound provider connections (V3-5).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderTlsConfig {
    /// Path to a PEM-encoded CA certificate for TLS pinning. Empty = disabled.
    #[serde(default)]
    pub ca_cert_path: String,
    /// Path to a PEM-encoded client certificate for mTLS. Empty = disabled.
    /// Must be set together with `client_key_path`.
    #[serde(default)]
    pub client_cert_path: String,
    /// Path to the PEM-encoded private key for the client certificate.
    #[serde(default)]
    pub client_key_path: String,
}

impl ProviderTlsConfig {
    /// Validate paths and key/cert pairing at startup.
    /// Returns an error string on the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        if !self.ca_cert_path.is_empty() && !std::path::Path::new(&self.ca_cert_path).exists() {
            return Err(format!(
                "provider_tls.ca_cert_path not found: {}",
                self.ca_cert_path
            ));
        }
        if !self.client_cert_path.is_empty() && self.client_key_path.is_empty() {
            return Err(
                "provider_tls.client_key_path is required when client_cert_path is set".to_string(),
            );
        }
        if !self.client_key_path.is_empty() && self.client_cert_path.is_empty() {
            return Err(
                "provider_tls.client_cert_path is required when client_key_path is set".to_string(),
            );
        }
        if !self.client_cert_path.is_empty()
            && !std::path::Path::new(&self.client_cert_path).exists()
        {
            return Err(format!(
                "provider_tls.client_cert_path not found: {}",
                self.client_cert_path
            ));
        }
        if !self.client_key_path.is_empty() && !std::path::Path::new(&self.client_key_path).exists()
        {
            return Err(format!(
                "provider_tls.client_key_path not found: {}",
                self.client_key_path
            ));
        }
        Ok(())
    }
}

/// Global defaults for budget-aware auto-downgrade (V4-4).
///
/// Per-key `downgrade_at_percent` / `downgrade_strategy` / `downgrade_to_model`
/// columns take precedence over these global defaults when set.
///
/// Example in janus.toml:
/// ```toml
/// [budget_downgrade]
/// enabled           = true
/// threshold_percent = 80
/// strategy          = "cost_optimized"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct BudgetDowngradeConfig {
    /// Enable budget downgrade globally. Default: false.
    #[serde(default)]
    pub enabled: bool,
    /// Spend percentage at which downgrade kicks in (0–100). Default: 80.
    #[serde(default = "default_downgrade_threshold")]
    pub threshold_percent: u8,
    /// Routing strategy to apply when downgrade triggers: "cost_optimized",
    /// "latency_optimized", "round_robin". Default: "cost_optimized".
    #[serde(default = "default_downgrade_strategy")]
    pub strategy: String,
    /// Specific model to switch to when `strategy = "specific_model"`.
    #[serde(default)]
    pub fallback_model: String,
}

impl Default for BudgetDowngradeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_percent: default_downgrade_threshold(),
            strategy: default_downgrade_strategy(),
            fallback_model: String::new(),
        }
    }
}

fn default_downgrade_threshold() -> u8 {
    80
}
fn default_downgrade_strategy() -> String {
    "cost_optimized".to_string()
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

fn default_rotation_grace_period_secs() -> u64 {
    300
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
fn default_qdrant_url() -> String {
    "http://localhost:6334".to_string()
}
fn default_qdrant_collection() -> String {
    "janus_cache".to_string()
}
fn default_qdrant_vector_size() -> u64 {
    384
}
fn default_rate_limit_window_secs() -> u64 {
    60
}
fn default_max_retries() -> u32 {
    1
}
fn default_time_sensitive_patterns() -> Vec<String> {
    vec![
        // English
        r"\btoday\b".into(),
        r"\bright now\b".into(),
        r"\bcurrently\b".into(),
        r"\blatest\b".into(),
        r"\bcurrent price\b".into(),
        r"\bthis week\b".into(),
        r"\bat this moment\b".into(),
        // Persian
        "امروز".into(),
        "الان".into(),
        "هم‌اکنون".into(),
        "قیمت فعلی".into(),
        "این هفته".into(),
        "اخبار".into(),
        // Arabic
        "اليوم".into(),
        "الآن".into(),
        "السعر الحالي".into(),
    ]
}
fn default_otlp_endpoint() -> String {
    "http://localhost:4317".to_string()
}
fn default_service_name() -> String {
    "janus".to_string()
}
fn default_sample_rate() -> f64 {
    1.0
}

/// Distributed tracing configuration (V3-2).
#[derive(Debug, Clone, Deserialize)]
pub struct TracingConfig {
    /// Enable OTLP tracing export. Default: false (zero overhead when disabled).
    #[serde(default)]
    pub enabled: bool,
    /// gRPC OTLP collector endpoint. Default: "http://localhost:4317"
    #[serde(default = "default_otlp_endpoint")]
    pub otlp_endpoint: String,
    /// Service name embedded in trace metadata. Default: "janus"
    #[serde(default = "default_service_name")]
    pub service_name: String,
    /// Sampling rate [0.0, 1.0]. 1.0 = 100%, 0.1 = 10%. Default: 1.0
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
}

/// Plugin middleware configuration (V3-4).
#[derive(Debug, Clone, Deserialize)]
pub struct PluginsConfig {
    /// Enable PII redaction of request message content. Default: true.
    #[serde(default = "default_true")]
    pub pii_redaction: bool,
    /// Reject requests whose total message characters exceed this limit.
    /// 0 = no limit (default).
    #[serde(default)]
    pub max_prompt_chars: usize,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            pii_redaction: true,
            max_prompt_chars: 0,
        }
    }
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            otlp_endpoint: default_otlp_endpoint(),
            service_name: default_service_name(),
            sample_rate: default_sample_rate(),
        }
    }
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
    /// Load configuration from janus.toml (optional) then environment variables.
    ///
    /// Environment variables always win. `DATABASE_URL` maps to `database_url`,
    /// `JWT_SECRET` maps to `jwt_secret`, etc.
    pub fn load() -> Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::File::with_name("janus").required(false))
            .add_source(
                config::Environment::default()
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        Ok(cfg.try_deserialize()?)
    }
}
