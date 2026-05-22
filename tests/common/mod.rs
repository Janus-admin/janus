// tests/common/mod.rs
// Shared test helpers used across all phase test files.
#![allow(dead_code)] // helpers used in future phase tests

use chrono::Utc;
use rust_decimal::Decimal;
use std::net::TcpListener;
use uuid::Uuid;

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

/// Authorization header value for the test API key.
pub fn auth_header() -> String {
    format!("Bearer {}", test_api_key())
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

/// Start the full Velox application on a random port and return the base URL.
///
/// The test API key (`test_api_key()`) is pre-seeded into the in-memory key cache
/// so gateway auth tests work without inserting DB rows.
pub async fn spawn_app() -> String {
    spawn_app_inner(None).await
}

/// Like `spawn_app()` but wires the OpenAI provider to use `openai_base_url` instead of
/// the real OpenAI API. Use this with a wiremock `MockServer` to test the full proxy path.
pub async fn spawn_app_with_openai_base(openai_base_url: String) -> String {
    spawn_app_inner(Some(openai_base_url)).await
}

async fn spawn_app_inner(openai_base_url: Option<String>) -> String {
    load_env();

    let config = velox::config::Config::load().expect("Failed to load config");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&config.database_url)
        .await
        .expect("Failed to connect to test database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    // ── Build provider list ───────────────────────────────────────────────────
    let key_cache: std::sync::Arc<dashmap::DashMap<[u8; 32], velox::models::api_key::ApiKey>> =
        std::sync::Arc::new(dashmap::DashMap::new());

    let mut providers: Vec<std::sync::Arc<dyn velox::providers::Provider>> = Vec::new();

    // If an override URL was provided, always add an OpenAI provider pointed at it.
    if let Some(ref base_url) = openai_base_url {
        let api_key = if config.openai_api_key.is_empty() {
            "test-key".to_string()
        } else {
            config.openai_api_key.clone()
        };
        providers.push(std::sync::Arc::new(
            velox::providers::openai::OpenAIProvider::with_base_url(api_key, base_url.clone(), 10),
        ));
    } else {
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

    // ── Seed the test API key into the in-memory cache ────────────────────────
    // This avoids bcrypt hashing and DB inserts in every test that needs auth.
    let test_key_str = test_api_key();
    let test_key_bytes = velox::db::api_keys::sha256_bytes(test_key_str);
    let test_key_entry = velox::models::api_key::ApiKey {
        id: Uuid::new_v4(),
        name: "Test Key".to_string(),
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
    key_cache.insert(test_key_bytes, test_key_entry);

    let registry = std::sync::Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));

    let state = std::sync::Arc::new(velox::state::AppState {
        pool,
        config,
        providers: registry,
        key_cache,
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
