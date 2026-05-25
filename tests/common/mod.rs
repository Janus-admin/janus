// tests/common/mod.rs
// Shared test helpers used across all phase test files.
#![allow(dead_code)] // helpers used in future phase tests

use chrono::Utc;
use rust_decimal::Decimal;
use std::net::TcpListener;
use uuid::Uuid;

// ── Basic helpers ─────────────────────────────────────────────────────────────

/// Load .env file for tests. Call at the top of any test that needs env vars.
pub fn load_env() {
    dotenvy::dotenv().ok();
}

/// Binds to a random available port and returns the address string `127.0.0.1:<port>`.
/// Kept for callers that need a raw address rather than a full URL.
pub fn random_port_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();
    format!("127.0.0.1:{}", port)
}

/// A valid Velox API key format for testing (exactly 54 chars: "vx-sk-" + 48 alphanumeric).
/// This key is pre-seeded into the key_cache by spawn_app so all auth tests work.
pub fn test_api_key() -> &'static str {
    "vx-sk-TestAPIKey00000000000000000000000000000000000000"
}

/// Authorization header value for the test API key (gateway routes only).
pub fn auth_header() -> String {
    format!("Bearer {}", test_api_key())
}

/// Register a test admin user against the running app, log in, and return a
/// `Bearer <jwt>` string for use on `/admin/*` routes.
///
/// Uses a fixed email so concurrent tests reuse the same account — the register
/// call is idempotent (ignores 409 Conflict).
pub async fn admin_auth_header(base_url: &str) -> String {
    let client = reqwest::Client::new();

    // Register — ignore conflict if already exists from a parallel test.
    client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": "test-admin@velox.test",
            "password": "velox-test-password",
            "name": "Test Admin"
        }))
        .send()
        .await
        .expect("register request failed");

    // Login — always returns a fresh JWT.
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({
            "email": "test-admin@velox.test",
            "password": "velox-test-password"
        }))
        .send()
        .await
        .expect("login request failed");

    assert_eq!(resp.status(), 200, "admin login must succeed");

    let body: serde_json::Value = resp.json().await.expect("login response must be JSON");
    let token = body["token"]
        .as_str()
        .expect("login response must contain token");

    format!("Bearer {}", token)
}

/// Minimal valid OpenAI-format chat completion request body.
pub fn minimal_chat_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "user", "content": "Say hello" }
        ]
    })
}

/// Minimal valid streaming chat completion request body.
pub fn minimal_streaming_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "user", "content": "Say hello" }
        ],
        "stream": true
    })
}

/// A minimal but valid OpenAI-format non-streaming JSON response body.
/// Used by wiremock stubs in provider retry / failover tests.
pub fn fake_openai_response_json() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Hello!" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 2,
            "total_tokens": 7
        }
    })
}

// ── App spawn options ─────────────────────────────────────────────────────────

/// Options for `spawn_app_from_opts`.
/// All fields have sensible defaults via `Default`.
struct TestAppOpts {
    /// If set, point the primary OpenAI-compatible provider at this URL.
    openai_base_url: Option<String>,
    /// If set, add a second OpenAI-compatible provider at this URL (priority 2).
    secondary_openai_base_url: Option<String>,
    /// If set, the test API key will have this `rate_limit_rpm` value.
    rate_limit_rpm: Option<i32>,
    /// Override the rate-limit sliding window (seconds). Default: 60.
    /// Use 1 in rate-limit tests to avoid sleeping 60 s.
    rate_limit_window_secs: u64,
    /// Load the embedding model from config paths.
    /// Must be true for Phase 5 semantic cache tests. Panics if model files missing.
    load_embedding_model: bool,
    /// Warm the exact cache from the DB at startup (without loading the embedding model).
    /// Use when a previously-spawned node has already populated the DB cache.
    warm_cache: bool,
    /// Enable cluster mode (DB-backed rate limiting + pg_notify key invalidation).
    cluster_enabled: bool,
    /// Node ID to use when cluster_enabled = true.
    cluster_node_id: Option<String>,
    /// Load all active keys from the DB at startup (in addition to the pre-seeded test key).
    /// Required for tests where a key was created via admin API on another node.
    load_keys_from_db: bool,
    /// Budget limit for the test key. None = no limit.
    budget_limit: Option<Decimal>,
    /// Budget already used. Default: Decimal::ZERO.
    budget_used: Decimal,
    /// Per-key downgrade threshold (0–100). None = use global config.
    downgrade_at_percent: Option<i32>,
    /// Per-key downgrade strategy. None = use global config.
    downgrade_strategy: Option<String>,
    /// Per-key downgrade model. None = use global config.
    downgrade_to_model: Option<String>,
}

impl Default for TestAppOpts {
    fn default() -> Self {
        Self {
            openai_base_url: None,
            secondary_openai_base_url: None,
            rate_limit_rpm: None,
            rate_limit_window_secs: 60,
            load_embedding_model: false,
            warm_cache: false,
            cluster_enabled: false,
            cluster_node_id: None,
            load_keys_from_db: false,
            budget_limit: None,
            budget_used: Decimal::ZERO,
            downgrade_at_percent: None,
            downgrade_strategy: None,
            downgrade_to_model: None,
        }
    }
}

// ── Public spawn helpers ──────────────────────────────────────────────────────

/// Start the full Velox application on a random port and return the base URL.
///
/// The test API key (`test_api_key()`) is pre-seeded into the in-memory key cache
/// so gateway auth tests work without inserting DB rows.
pub async fn spawn_app() -> String {
    spawn_app_from_opts(TestAppOpts::default()).await
}

/// Like `spawn_app()` but wires the OpenAI provider to use `openai_base_url` instead of
/// the real OpenAI API. Use this with a wiremock `MockServer` to test the full proxy path.
pub async fn spawn_app_with_openai_base(openai_base_url: String) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        ..Default::default()
    })
    .await
}

/// Start app with a rate-limited test key.
///
/// - The test API key has `rate_limit_rpm = rpm`.
/// - The rate-limit window is set to **1 second** so tests don't need to sleep 60 s.
/// - The OpenAI provider is pointed at `openai_base_url` (typically a wiremock server).
pub async fn spawn_app_with_rate_limit(openai_base_url: String, rpm: i32) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        rate_limit_rpm: Some(rpm),
        rate_limit_window_secs: 1,
        ..Default::default()
    })
    .await
}

/// Start the app with a fresh wiremock `MockServer` as the sole OpenAI provider.
///
/// Returns `(base_url, mock_server)`. The caller **must** keep `mock_server` alive
/// for the duration of the test or the provider endpoint disappears.
pub async fn spawn_app_with_wiremock() -> (String, wiremock::MockServer) {
    let mock_server = wiremock::MockServer::start().await;
    let base_url = spawn_app_with_openai_base(mock_server.uri()).await;
    (base_url, mock_server)
}

/// Start app with two OpenAI-compatible providers at different priorities.
///
/// - Primary provider: `primary_url` at priority 1.
/// - Secondary provider: `secondary_url` at priority 2.
///
/// The pipeline tries the primary first; on exhausted retries it fails over to secondary.
pub async fn spawn_app_with_two_providers(primary_url: String, secondary_url: String) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(primary_url),
        secondary_openai_base_url: Some(secondary_url),
        ..Default::default()
    })
    .await
}

/// Start app with embedding model loaded + wiremock provider.
///
/// Returns `(base_url, mock_server)`. The embedding model is loaded from the default
/// path (`models/all-MiniLM-L6-v2.onnx` + `models/tokenizer.json`).
///
/// **Panics** if the model files are missing — this is intentional so Phase 5 tests
/// fail loudly rather than silently skip.
pub async fn spawn_app_with_embedding_and_wiremock() -> (String, wiremock::MockServer) {
    let mock_server = wiremock::MockServer::start().await;
    let base_url = spawn_app_with_embedding_base(mock_server.uri()).await;
    (base_url, mock_server)
}

/// Start app with embedding model loaded, pointing the provider at `openai_base_url`.
///
/// **Panics** if the model files are missing.
pub async fn spawn_app_with_embedding_base(openai_base_url: String) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        load_embedding_model: true,
        ..Default::default()
    })
    .await
}

/// Start app with a budget-limited test key that has downgrade configured.
///
/// - `budget_limit`: total budget cap in USD.
/// - `budget_used`: spend already recorded (set > threshold to trigger downgrade).
/// - `downgrade_at_percent`: threshold % at which downgrade kicks in.
/// - `downgrade_strategy`: routing strategy string used when threshold crossed.
pub async fn spawn_app_with_budget_key(
    openai_base_url: String,
    budget_limit: Decimal,
    budget_used: Decimal,
    downgrade_at_percent: i32,
    downgrade_strategy: &str,
) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        budget_limit: Some(budget_limit),
        budget_used,
        downgrade_at_percent: Some(downgrade_at_percent),
        downgrade_strategy: Some(downgrade_strategy.to_string()),
        ..Default::default()
    })
    .await
}

/// Start app with cluster mode enabled (DB-backed rate limiting + pg_notify key sync).
///
/// The `node_id` is used in log correlation when running multiple nodes in tests.
/// Both nodes must share the same PostgreSQL database.
pub async fn spawn_app_with_cluster(openai_base_url: String, node_id: &str) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        cluster_enabled: true,
        cluster_node_id: Some(node_id.to_string()),
        ..Default::default()
    })
    .await
}

/// Like `spawn_app_with_cluster` but also sets a rate limit on the test key.
pub async fn spawn_app_with_cluster_and_rate_limit(
    openai_base_url: String,
    node_id: &str,
    rpm: i32,
) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        cluster_enabled: true,
        cluster_node_id: Some(node_id.to_string()),
        rate_limit_rpm: Some(rpm),
        rate_limit_window_secs: 2,
        ..Default::default()
    })
    .await
}

/// Spawn app in cluster mode, warm the cache from DB, and load all active DB keys.
///
/// Use this when starting a "second node" that needs to see keys and cache entries
/// created by a previously-running first node.
pub async fn spawn_app_with_cluster_warmed(openai_base_url: String, node_id: &str) -> String {
    spawn_app_from_opts(TestAppOpts {
        openai_base_url: Some(openai_base_url),
        cluster_enabled: true,
        cluster_node_id: Some(node_id.to_string()),
        warm_cache: true,
        load_keys_from_db: true,
        ..Default::default()
    })
    .await
}

// ── Internal implementation ───────────────────────────────────────────────────

async fn spawn_app_from_opts(opts: TestAppOpts) -> String {
    load_env();

    let mut config = velox::config::Config::load().expect("Failed to load config");

    // Override the rate-limit window if requested (e.g. 1 s for fast tests).
    config.rate_limit_window_secs = opts.rate_limit_window_secs;

    // Enable cluster mode if requested.
    if opts.cluster_enabled {
        config.cluster.enabled = true;
        if let Some(ref node_id) = opts.cluster_node_id {
            config.cluster.node_id = node_id.clone();
        }
    }

    let pool = velox::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test database");

    // ── Build provider list ───────────────────────────────────────────────────
    let key_cache: std::sync::Arc<dashmap::DashMap<[u8; 32], velox::models::api_key::ApiKey>> =
        std::sync::Arc::new(dashmap::DashMap::new());

    let mut providers: Vec<std::sync::Arc<dyn velox::providers::Provider>> = Vec::new();

    if let Some(ref base_url) = opts.openai_base_url {
        // Explicit URL override (wiremock): always add, ignore env key emptiness.
        let api_key = if config.openai_api_key.is_empty() {
            "test-key".to_string()
        } else {
            config.openai_api_key.clone()
        };
        providers.push(std::sync::Arc::new(
            velox::providers::openai::OpenAIProvider::with_base_url(api_key, base_url.clone(), 1),
        ));
    } else {
        // No override: use real API keys from env if present.
        if !config.openai_api_key.is_empty() {
            providers.push(std::sync::Arc::new(
                velox::providers::openai::OpenAIProvider::new(config.openai_api_key.clone(), 10),
            ));
        }
        if !config.anthropic_api_key.is_empty() {
            providers.push(std::sync::Arc::new(
                velox::providers::anthropic::AnthropicProvider::new(
                    config.anthropic_api_key.clone(),
                    20,
                ),
            ));
        }
    }

    // Optional second provider for failover tests (priority 2, tried after primary).
    if let Some(ref secondary_url) = opts.secondary_openai_base_url {
        let api_key = if config.openai_api_key.is_empty() {
            "test-key".to_string()
        } else {
            config.openai_api_key.clone()
        };
        providers.push(std::sync::Arc::new(
            velox::providers::openai::OpenAIProvider::with_base_url(
                api_key,
                secondary_url.clone(),
                2,
            ),
        ));
    }

    // ── Seed the test API key into the in-memory cache ────────────────────────
    let test_key_str = test_api_key();
    let test_key_bytes = velox::db::api_keys::sha256_bytes(test_key_str);
    let test_key_entry = velox::models::api_key::ApiKey {
        id: Uuid::new_v4(),
        name: "Test Key".to_string(),
        key_hash: "placeholder".to_string(),
        key_sha256: None,
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: test_key_str[..12].to_string(),
        workspace_id: None,
        budget_limit: opts.budget_limit,
        budget_used: opts.budget_used,
        rate_limit_rpm: opts.rate_limit_rpm,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: opts.downgrade_at_percent,
        downgrade_strategy: opts.downgrade_strategy.clone(),
        downgrade_to_model: opts.downgrade_to_model.clone(),
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    };
    key_cache.insert(test_key_bytes, test_key_entry.clone());

    // Persist the test key into the DB so the FK constraint on requests.api_key_id
    // is satisfied when the pipeline logs streaming requests.
    // Use bound parameters for is_active ($5) and created_at ($6) so this query
    // works on both PostgreSQL (BOOLEAN / TIMESTAMPTZ) and SQLite (INTEGER / TEXT).
    sqlx::query(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, is_active, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING",
    )
    .bind(test_key_entry.id)
    .bind(&test_key_entry.name)
    .bind(format!("test-hash-{}", test_key_entry.id))
    .bind(&test_key_entry.key_prefix)
    .bind(true)
    .bind(Utc::now())
    .execute(&pool)
    .await
    .expect("Failed to insert test API key into DB");

    // ── Load additional DB keys if requested ─────────────────────────────────
    if opts.load_keys_from_db {
        if let Ok(entries) = velox::db::api_keys::load_all_active(&pool).await {
            for (hash, key) in entries {
                key_cache.insert(hash, key);
            }
        }
    }

    // ── Build cache engine ────────────────────────────────────────────────────
    let cache = if opts.load_embedding_model {
        let model = velox::cache::embedding::EmbeddingModel::load(
            &config.embedding_model_path,
            &config.embedding_tokenizer_path,
        )
        .expect(
            "Embedding model must be loadable for Phase 5 tests — \
             ensure models/all-MiniLM-L6-v2.onnx and models/tokenizer.json exist",
        );
        let engine = std::sync::Arc::new(velox::cache::CacheEngine::new_with_semantic(
            std::sync::Arc::new(model),
            config.semantic_cache_threshold as f32,
        ));
        // Warm from DB so restart-survival test picks up embeddings from the first instance.
        engine.warm_from_db(&pool).await;
        engine
    } else {
        let engine = std::sync::Arc::new(velox::cache::CacheEngine::new());
        if opts.warm_cache {
            // Warm the exact cache from DB without loading the embedding model.
            engine.warm_from_db(&pool).await;
        }
        engine
    };

    let registry = std::sync::Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));

    let rate_limiter =
        velox::middleware::rate_limit::RateLimiter::new(config.rate_limit_window_secs);

    let cluster_rate_limiter = if config.cluster.enabled {
        Some(velox::cluster::rate_limit::DbRateLimiter::new(
            pool.clone(),
            config.rate_limit_window_secs,
        ))
    } else {
        None
    };

    let (event_tx, _) = tokio::sync::broadcast::channel(64);

    let runtime_config = std::sync::Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let time_guard = std::sync::Arc::new(velox::cache::time_guard::TimeGuard::new(
        &config.time_sensitive_patterns,
    ));

    let state = std::sync::Arc::new(velox::state::AppState {
        pool,
        config,
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cluster_rate_limiter,
        cache,
        // Tests use the permissive default (allows all models/routes/keys).
        semantic_policy: velox::cache::policy::SemanticCachePolicy::default(),
        event_tx,
        // Tests start with an empty plugin chain so behavior is unchanged.
        plugins: std::sync::Arc::new(vec![]),
        dedup: std::sync::Arc::new(velox::gateway::dedup::InFlightDeduplicator::new()),
        time_guard,
        models_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        oidc_states: std::sync::Arc::new(dashmap::DashMap::new()),
    });

    let app = velox::routes::create_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    format!("http://127.0.0.1:{}", port)
}
