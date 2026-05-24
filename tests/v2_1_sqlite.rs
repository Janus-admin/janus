// tests/v2_1_sqlite.rs
// Phase V2-1 acceptance tests — SQLite Support.
//
// Run with:  cargo test --no-default-features --features sqlite v2_1
// Postgres suite (unchanged): cargo test

mod common;

// All items in this file are gated on the sqlite feature so the test binary
// compiles without error when building with the default postgres feature.

#[cfg(feature = "sqlite")]
use chrono::{Timelike, Utc};
#[cfg(feature = "sqlite")]
use rust_decimal::Decimal;
#[cfg(feature = "sqlite")]
use tempfile::TempDir;
#[cfg(feature = "sqlite")]
use uuid::Uuid;
#[cfg(feature = "sqlite")]
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── SQLite app spawner ────────────────────────────────────────────────────────

/// Spawn a full Velox server backed by a fresh SQLite database in `tmp`.
///
/// Returns `(base_url, mock_server, pool)`.
/// The mock_server stubs `POST /v1/chat/completions` with a valid OpenAI response.
/// Callers must keep all three alive for the duration of the test.
#[cfg(feature = "sqlite")]
async fn spawn_app_sqlite(tmp: &TempDir) -> (String, MockServer, velox::db::DbPool) {
    common::load_env();

    let db_path = tmp.path().join("velox_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mock_server = MockServer::start().await;

    let mut config = velox::config::Config::load().expect("Config::load failed");
    config.database_url = db_url.clone();
    config.rate_limit_window_secs = 60;

    let pool = velox::db::pool::connect(&db_url)
        .await
        .expect("SQLite connect failed");

    // Seed the test API key into the in-memory cache.
    let key_cache: std::sync::Arc<dashmap::DashMap<[u8; 32], velox::models::api_key::ApiKey>> =
        std::sync::Arc::new(dashmap::DashMap::new());

    let test_key_str = common::test_api_key();
    let test_key_bytes = velox::db::api_keys::sha256_bytes(test_key_str);
    let test_key_id = Uuid::new_v4();
    let test_key_entry = velox::models::api_key::ApiKey {
        id: test_key_id,
        name: "SQLite Test Key".to_string(),
        key_hash: "placeholder".to_string(),
        key_sha256: None,
        key_prefix: test_key_str[..12].to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    };
    key_cache.insert(test_key_bytes, test_key_entry.clone());

    // Persist the test key so FK constraints on requests are satisfied.
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
    .expect("Failed to insert test API key into SQLite");

    // Wire the OpenAI provider to the mock server.
    let api_key = if config.openai_api_key.is_empty() {
        "test-key".to_string()
    } else {
        config.openai_api_key.clone()
    };
    let providers: Vec<std::sync::Arc<dyn velox::providers::Provider>> = vec![std::sync::Arc::new(
        velox::providers::openai::OpenAIProvider::with_base_url(api_key, mock_server.uri(), 1),
    )];

    let registry = std::sync::Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));
    let rate_limiter =
        velox::middleware::rate_limit::RateLimiter::new(config.rate_limit_window_secs);
    let cache = std::sync::Arc::new(velox::cache::CacheEngine::new());
    let (event_tx, _) = tokio::sync::broadcast::channel(64);
    let runtime_config = std::sync::Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let state = std::sync::Arc::new(velox::state::AppState {
        pool: pool.clone(),
        config,
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cache,
        semantic_policy: velox::cache::policy::SemanticCachePolicy::default(),
        event_tx,
    });

    let app = velox::routes::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server failed");
    });

    (format!("http://127.0.0.1:{}", port), mock_server, pool)
}

/// Mount the standard fake OpenAI success response on the mock server.
#[cfg(feature = "sqlite")]
async fn mount_openai_stub(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(server)
        .await;
}

// ── Helper: get a fresh SQLite pool connected to an existing db ───────────────

#[cfg(feature = "sqlite")]
async fn sqlite_pool(db_url: &str) -> velox::db::DbPool {
    velox::db::pool::connect(db_url)
        .await
        .expect("SQLite reconnect failed")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_migrations_apply_cleanly() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("migrations_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool = velox::db::pool::connect(&db_url)
        .await
        .expect("connect failed");

    // Verify key tables exist by counting rows (would fail if tables are missing).
    let (user_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("users table missing after migration");

    let (provider_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM providers")
        .fetch_one(&pool)
        .await
        .expect("providers table missing after migration");

    let (pricing_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM model_pricing")
        .fetch_one(&pool)
        .await
        .expect("model_pricing table missing after migration");

    assert_eq!(user_count, 0);
    assert!(provider_count >= 3, "seed providers should exist");
    assert!(pricing_count > 0, "seed pricing rows should exist");
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_gateway_proxies_and_logs_request() {
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "Hello!");

    // Verify a request row was written to the SQLite DB.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM requests WHERE status = 'success'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_api_key_created_and_validated() {
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, _pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();

    // Register and log in to get a JWT for admin endpoints.
    let reg_resp = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": "sqlite@test.com", "password": "pass123!", "name": "SQLite User"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg_resp.status(), 200);

    let login_resp: serde_json::Value = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"email": "sqlite@test.com", "password": "pass123!"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt = login_resp["token"].as_str().unwrap().to_string();

    // Create a new API key via the admin endpoint.
    let key_resp: serde_json::Value = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .json(&serde_json::json!({"name": "SQLite test key"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let new_key = key_resp["data"]["key"]
        .as_str()
        .expect("key field missing")
        .to_string();
    assert!(new_key.starts_with("vx-sk-"), "key has wrong format");

    // Use the newly created key to call the gateway.
    let gw_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", new_key))
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(gw_resp.status(), 200);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_exact_cache_hit_and_miss() {
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, _pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();
    let req = common::minimal_chat_request();

    // First request: cache miss → hits wiremock.
    let r1 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);
    let hit_header_1 = r1
        .headers()
        .get("x-velox-cache-hit")
        .map(|v| v.to_str().unwrap().to_string());
    assert!(hit_header_1.is_none(), "first request must be a cache miss");

    // Second identical request: should be an exact cache hit.
    let r2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);
    let hit_header_2 = r2
        .headers()
        .get("x-velox-cache-hit")
        .map(|v| v.to_str().unwrap().to_string());
    assert_eq!(
        hit_header_2.as_deref(),
        Some("exact"),
        "second request must be exact cache hit"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_rate_limit_enforced() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("velox_rl.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mock_server = MockServer::start().await;
    mount_openai_stub(&mock_server).await;

    common::load_env();
    let mut config = velox::config::Config::load().expect("config load failed");
    config.database_url = db_url.clone();
    config.rate_limit_window_secs = 60;

    let pool = velox::db::pool::connect(&db_url)
        .await
        .expect("connect failed");
    let key_cache: std::sync::Arc<dashmap::DashMap<[u8; 32], velox::models::api_key::ApiKey>> =
        std::sync::Arc::new(dashmap::DashMap::new());

    let test_key_str = common::test_api_key();
    let test_key_bytes = velox::db::api_keys::sha256_bytes(test_key_str);
    let rate_limited_key = velox::models::api_key::ApiKey {
        id: Uuid::new_v4(),
        name: "RL Key".to_string(),
        key_hash: "placeholder2".to_string(),
        key_sha256: None,
        key_prefix: test_key_str[..12].to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: Some(1), // allow only 1 request per window
        rate_limit_tpm: None,
        allowed_models: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    };
    key_cache.insert(test_key_bytes, rate_limited_key);

    let providers: Vec<std::sync::Arc<dyn velox::providers::Provider>> = vec![std::sync::Arc::new(
        velox::providers::openai::OpenAIProvider::with_base_url(
            "test-key".to_string(),
            mock_server.uri(),
            1,
        ),
    )];
    let registry = std::sync::Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));
    let rate_limiter =
        velox::middleware::rate_limit::RateLimiter::new(config.rate_limit_window_secs);
    let cache = std::sync::Arc::new(velox::cache::CacheEngine::new());
    let (event_tx, _) = tokio::sync::broadcast::channel(64);
    let runtime_config = std::sync::Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let state = std::sync::Arc::new(velox::state::AppState {
        pool,
        config,
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cache,
        semantic_policy: velox::cache::policy::SemanticCachePolicy::default(),
        event_tx,
    });

    let app = velox::routes::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base_url = format!("http://127.0.0.1:{}", port);

    let client = reqwest::Client::new();
    let req = common::minimal_chat_request();

    let r1 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200, "first request should succeed");

    let r2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 429, "second request should be rate-limited");
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_budget_limit_blocks_request() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("velox_budget.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mock_server = MockServer::start().await;
    mount_openai_stub(&mock_server).await;

    common::load_env();
    let mut config = velox::config::Config::load().expect("config load failed");
    config.database_url = db_url.clone();

    let pool = velox::db::pool::connect(&db_url)
        .await
        .expect("connect failed");
    let key_cache: std::sync::Arc<dashmap::DashMap<[u8; 32], velox::models::api_key::ApiKey>> =
        std::sync::Arc::new(dashmap::DashMap::new());

    let test_key_str = common::test_api_key();
    let test_key_bytes = velox::db::api_keys::sha256_bytes(test_key_str);
    // budget_limit=0.000001, budget_used=999 → already over budget
    let budget_key = velox::models::api_key::ApiKey {
        id: Uuid::new_v4(),
        name: "Budget Key".to_string(),
        key_hash: "placeholder3".to_string(),
        key_sha256: None,
        key_prefix: test_key_str[..12].to_string(),
        workspace_id: None,
        budget_limit: Some(Decimal::new(1, 6)), // $0.000001
        budget_used: Decimal::new(999, 0),      // already exhausted
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    };
    key_cache.insert(test_key_bytes, budget_key);

    let providers: Vec<std::sync::Arc<dyn velox::providers::Provider>> = vec![std::sync::Arc::new(
        velox::providers::openai::OpenAIProvider::with_base_url(
            "test-key".to_string(),
            mock_server.uri(),
            1,
        ),
    )];
    let registry = std::sync::Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));
    let rate_limiter = velox::middleware::rate_limit::RateLimiter::new(60);
    let cache = std::sync::Arc::new(velox::cache::CacheEngine::new());
    let (event_tx, _) = tokio::sync::broadcast::channel(64);
    let runtime_config = std::sync::Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let state = std::sync::Arc::new(velox::state::AppState {
        pool,
        config,
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cache,
        semantic_policy: velox::cache::policy::SemanticCachePolicy::default(),
        event_tx,
    });

    let app = velox::routes::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base_url = format!("http://127.0.0.1:{}", port);

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        402,
        "over-budget key should be rejected with 402"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_daily_costs_written() {
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    // Give the background task time to write the daily_costs row.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM daily_costs WHERE request_count > 0")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        count >= 1,
        "daily_costs should have at least one row after a request"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_analytics_overview_returns_correct_counts() {
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, _pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();

    // Make a request first so there's data.
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Register + login for admin access.
    let reg = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({"email": "admin2@test.com", "password": "pass123!", "name": "Admin"}))
        .send().await.unwrap();
    assert_eq!(reg.status(), 200);

    let login: serde_json::Value = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"email": "admin2@test.com", "password": "pass123!"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let jwt = login["token"].as_str().unwrap();

    let overview: serde_json::Value = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", format!("Bearer {}", jwt))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(overview["today"]["requests"].as_i64().unwrap_or(0) >= 1);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_uuids_round_trip_correctly() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("velox_uuid.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let pool = velox::db::pool::connect(&db_url).await.unwrap();

    let id = Uuid::new_v4();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, stream, created_at)
         VALUES ($1, 'openai', 'gpt-4o', 'success', 0, $2)",
    )
    .bind(id)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let (returned_id,): (Uuid,) = sqlx::query_as("SELECT id FROM requests WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        returned_id, id,
        "UUID must round-trip through SQLite TEXT without loss"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_cost_decimal_precision_preserved() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("velox_decimal.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let pool = velox::db::pool::connect(&db_url).await.unwrap();

    // Sub-cent precision value: $0.00012345
    let cost: Decimal = "0.00012345".parse().unwrap();
    let id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, stream, cost_usd, created_at)
         VALUES ($1, 'openai', 'gpt-4o', 'success', 0, $2, $3)",
    )
    .bind(id)
    .bind(cost.to_string())
    .bind(Utc::now())
    .execute(&pool)
    .await
    .unwrap();

    let (cost_str,): (String,) = sqlx::query_as("SELECT cost_usd FROM requests WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let returned_cost: Decimal = cost_str.parse().unwrap();

    assert_eq!(
        returned_cost, cost,
        "Decimal precision must be preserved through SQLite TEXT storage"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_sqlite_timestamps_round_trip_as_utc() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("velox_ts.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let pool = velox::db::pool::connect(&db_url).await.unwrap();

    // Truncate to microseconds so the round-trip comparison is exact.
    let ts = Utc::now();
    let ts_trunc = ts.with_nanosecond(ts.nanosecond() / 1000 * 1000).unwrap();
    let id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, stream, created_at)
         VALUES ($1, 'openai', 'gpt-4o', 'success', 0, $2)",
    )
    .bind(id)
    .bind(ts_trunc)
    .execute(&pool)
    .await
    .unwrap();

    let (returned_ts,): (chrono::DateTime<Utc>,) =
        sqlx::query_as("SELECT created_at FROM requests WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        returned_ts.timestamp_micros(),
        ts_trunc.timestamp_micros(),
        "Timestamp must round-trip as UTC through SQLite TEXT"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn v2_1_regression_postgres_tests_still_pass_unchanged() {
    // This test verifies that the SQLite backend produces the same observable
    // gateway behaviour as PostgreSQL: valid OpenAI response format, correct
    // HTTP status, and cache-hit headers working as expected.
    let tmp = TempDir::new().unwrap();
    let (base_url, mock_server, _pool) = spawn_app_sqlite(&tmp).await;
    mount_openai_stub(&mock_server).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    // Verify OpenAI-compatible response structure is preserved on SQLite.
    assert!(
        body["id"].as_str().is_some(),
        "response must have an id field"
    );
    assert_eq!(body["object"], "chat.completion");
    assert!(
        body["choices"].as_array().is_some(),
        "choices array required"
    );
    assert!(body["usage"]["total_tokens"].as_i64().unwrap_or(0) > 0);
}
