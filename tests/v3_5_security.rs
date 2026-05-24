// tests/v3_5_security.rs
// Phase V3-5 acceptance tests — Security Hardening.
//
// Run with: cargo test v3_5
//
// Test areas:
//   8.1  mTLS config validation (startup checks)
//   8.2  API key rotation (new key valid immediately, old valid during grace, rejected after)
//   8.3  Audit log API (extended filters + X-Velox-Audit-Hash header)

use chrono::Utc;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;
use velox::{config::ProviderTlsConfig, db::api_keys as db_api_keys, models::api_key::ApiKey};

mod common;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_api_key(key_sha256: Option<&str>) -> ApiKey {
    ApiKey {
        id: Uuid::new_v4(),
        name: "test".to_string(),
        key_hash: String::new(),
        key_sha256: key_sha256.map(str::to_string),
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "vx-sk-test".to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    }
}

fn sha256_hex(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── §8.1  mTLS Config Validation ────────────────────────────────────────────

#[test]
fn v3_5_tls_config_empty_is_valid() {
    let cfg = ProviderTlsConfig::default();
    assert!(cfg.validate().is_ok(), "empty TLS config must be valid");
}

#[test]
fn v3_5_invalid_ca_cert_path_fails_at_startup() {
    let cfg = ProviderTlsConfig {
        ca_cert_path: "/nonexistent/path/ca.pem".to_string(),
        client_cert_path: String::new(),
        client_key_path: String::new(),
    };
    let result = cfg.validate();
    assert!(result.is_err(), "missing ca_cert_path must fail validation");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("ca_cert_path"),
        "error message must mention ca_cert_path; got: {msg}"
    );
}

#[test]
fn v3_5_missing_client_key_with_cert_fails_at_startup() {
    let cfg = ProviderTlsConfig {
        ca_cert_path: String::new(),
        client_cert_path: "/some/cert.pem".to_string(),
        client_key_path: String::new(), // missing key
    };
    let result = cfg.validate();
    assert!(result.is_err(), "cert without key must fail validation");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("client_key_path"),
        "error must mention client_key_path; got: {msg}"
    );
}

#[test]
fn v3_5_missing_client_cert_with_key_fails_at_startup() {
    let cfg = ProviderTlsConfig {
        ca_cert_path: String::new(),
        client_cert_path: String::new(), // missing cert
        client_key_path: "/some/key.pem".to_string(),
    };
    let result = cfg.validate();
    assert!(result.is_err(), "key without cert must fail validation");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("client_cert_path"),
        "error must mention client_cert_path; got: {msg}"
    );
}

// ─── §8.2  API Key Rotation ────────────────────────────────────────────────

#[tokio::test]
async fn v3_5_rotate_returns_new_key() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Create a key via the admin API.
    let create_resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &admin_auth)
        .json(&serde_json::json!({ "name": "rotate-test" }))
        .send()
        .await
        .expect("create key request failed");
    assert_eq!(create_resp.status(), 201);
    let body: serde_json::Value = create_resp.json().await.unwrap();
    let key_id = body["data"]["id"].as_str().unwrap().to_string();

    // Rotate it.
    let rotate_resp = client
        .post(format!("{}/admin/keys/{}/rotate", base_url, key_id))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .expect("rotate request failed");
    assert_eq!(rotate_resp.status(), 200, "rotate must return 200");

    let rotate_body: serde_json::Value = rotate_resp.json().await.unwrap();
    let new_key = rotate_body["data"]["key"].as_str().unwrap();
    assert!(
        new_key.starts_with("vx-sk-"),
        "rotated key must follow vx-sk- format"
    );
    assert!(
        rotate_body["data"]["rotation_expires_at"]
            .as_str()
            .is_some(),
        "rotation_expires_at must be returned"
    );
}

#[tokio::test]
async fn v3_5_new_key_valid_immediately_after_rotation() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Create a key.
    let create_resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &admin_auth)
        .json(&serde_json::json!({ "name": "new-key-immediate" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 201);
    let body: serde_json::Value = create_resp.json().await.unwrap();
    let key_id = body["data"]["id"].as_str().unwrap();

    // Rotate it.
    let rotate_resp = client
        .post(format!("{}/admin/keys/{}/rotate", base_url, key_id))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();
    assert_eq!(rotate_resp.status(), 200);
    let rotate_body: serde_json::Value = rotate_resp.json().await.unwrap();
    let new_key = rotate_body["data"]["key"].as_str().unwrap().to_string();

    // New key must be accepted by the gateway immediately.
    let gateway_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", new_key))
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();
    // We expect 200 (real API) or 503 (no provider configured) — NOT 401.
    assert_ne!(
        gateway_resp.status(),
        401,
        "new key must not be rejected immediately after rotation"
    );
}

#[tokio::test]
async fn v3_5_old_key_still_valid_within_grace_period() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Create a key via admin API (this inserts into DB + key_cache).
    let create_resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &admin_auth)
        .json(&serde_json::json!({ "name": "grace-test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 201);
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    let old_key = create_body["data"]["key"].as_str().unwrap().to_string();
    let key_id = create_body["data"]["id"].as_str().unwrap();

    // Rotate it — the server uses the default grace period (300 s), so old key is still valid.
    let rotate_resp = client
        .post(format!("{}/admin/keys/{}/rotate", base_url, key_id))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();
    assert_eq!(rotate_resp.status(), 200);

    // Old key must still be accepted (within grace period).
    let gateway_resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", old_key))
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_ne!(
        gateway_resp.status(),
        401,
        "old key must still be accepted within the grace period"
    );
}

#[tokio::test]
async fn v3_5_old_key_rejected_after_grace_period_expires() {
    common::load_env();

    // Build an app state with a key whose rotation has already expired.
    // We construct this directly rather than through the HTTP API to avoid
    // needing a real DB write + time manipulation.

    use dashmap::DashMap;

    common::load_env();
    let config = velox::config::Config::load().expect("config");
    let pool = velox::db::pool::connect(&config.database_url)
        .await
        .expect("db");

    let key_cache: Arc<DashMap<[u8; 32], ApiKey>> = Arc::new(DashMap::new());

    // Fabricate a key that is already past its rotation grace period.
    let old_key_str = "vx-sk-OldKeyExpiredGrace000000000000000000000000000";
    let new_key_sha256_str = sha256_hex("vx-sk-NewKeyAfterRotation00000000000000000000000000");
    let old_key_bytes = db_api_keys::sha256_bytes(old_key_str);

    let expired_key = ApiKey {
        id: Uuid::new_v4(),
        name: "expired-rotation".to_string(),
        key_hash: String::new(),
        // key_sha256 points to the NEW key, not the old one being presented.
        key_sha256: Some(new_key_sha256_str),
        previous_key_sha256: Some(sha256_hex(old_key_str)),
        // Grace period already expired (1 second ago).
        rotation_expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
        key_prefix: "vx-sk-Old".to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    };
    key_cache.insert(old_key_bytes, expired_key);

    let providers: Vec<Arc<dyn velox::providers::Provider>> = vec![];
    let registry = Arc::new(velox::gateway::ProviderRegistry::new(
        providers,
        key_cache.clone(),
    ));
    let rate_limiter = velox::middleware::rate_limit::RateLimiter::new(60);
    let cache = Arc::new(velox::cache::CacheEngine::new());
    let (event_tx, _) = tokio::sync::broadcast::channel(64);
    let runtime_config = Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let state = Arc::new(velox::state::AppState {
        pool,
        config: config.clone(),
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cluster_rate_limiter: None,
        cache,
        semantic_policy: velox::cache::policy::SemanticCachePolicy::default(),
        event_tx,
        plugins: Arc::new(vec![]),
        dedup: std::sync::Arc::new(velox::gateway::dedup::InFlightDeduplicator::new()),
        time_guard: std::sync::Arc::new(velox::cache::time_guard::TimeGuard::new(
            &config.time_sensitive_patterns,
        )),
    });

    let app = velox::routes::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    let base_url = format!("http://127.0.0.1:{}", port);
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", old_key_str))
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        401,
        "old key must be rejected after grace period expires"
    );
}

#[tokio::test]
async fn v3_5_rotate_nonexistent_key_returns_404() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let nonexistent_id = Uuid::new_v4();
    let resp = client
        .post(format!("{}/admin/keys/{}/rotate", base_url, nonexistent_id))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        404,
        "rotating a nonexistent key must return 404"
    );
}

// ─── §8.3  Audit Log API ──────────────────────────────────────────────────────

#[tokio::test]
async fn v3_5_audit_log_response_includes_hash_header() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/requests", base_url))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().contains_key("x-velox-audit-hash"),
        "response must include X-Velox-Audit-Hash header"
    );
}

#[tokio::test]
async fn v3_5_audit_log_hash_matches_body_sha256() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/requests", base_url))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let reported_hash = resp
        .headers()
        .get("x-velox-audit-hash")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let body_bytes = resp.bytes().await.unwrap();
    let computed_hash: String = {
        let mut h = Sha256::new();
        h.update(&body_bytes);
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
    };

    assert_eq!(
        reported_hash, computed_hash,
        "X-Velox-Audit-Hash must equal SHA-256 of the response body"
    );
}

#[tokio::test]
async fn v3_5_audit_log_filters_by_date_range() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // A date range that excludes everything (far future).
    let future = "2099-01-01T00:00:00Z";
    let resp = client
        .get(format!(
            "{}/admin/requests?start_time={}&end_time={}",
            base_url, future, future
        ))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["meta"]["total"].as_i64().unwrap_or(1),
        0,
        "date range in the far future must return 0 rows"
    );
}

#[tokio::test]
async fn v3_5_audit_log_filters_by_status() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/requests?status=success", base_url))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Every returned row must have status = "success".
    for row in body["data"].as_array().unwrap_or(&vec![]) {
        assert_eq!(
            row["status"].as_str().unwrap_or(""),
            "success",
            "filter by status=success must return only successful requests"
        );
    }
}

#[tokio::test]
async fn v3_5_audit_log_filters_by_api_key_id() {
    common::load_env();
    let base_url = common::spawn_app().await;
    let admin_auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Filter by a random (non-existent) api_key_id — must return 0 rows.
    let fake_id = Uuid::new_v4();
    let resp = client
        .get(format!(
            "{}/admin/requests?api_key_id={}",
            base_url, fake_id
        ))
        .header("Authorization", &admin_auth)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["meta"]["total"].as_i64().unwrap_or(1),
        0,
        "filtering by a nonexistent api_key_id must return 0 rows"
    );
}

// ─── Regression ───────────────────────────────────────────────────────────────

#[test]
fn v3_5_regression_existing_key_auth_unaffected() {
    // Keys with no rotation state (previous_key_sha256 = None) should be authenticated
    // exactly as before: key_sha256 matches the presented token's hash → accepted.
    let key_str = "vx-sk-NormalKeyNoRotation0000000000000000000000000";
    let hex = sha256_hex(key_str);
    let api_key = make_api_key(Some(&hex));

    // Simulate what the middleware does: compare presented hash to key_sha256.
    let presented_hex = sha256_hex(key_str);
    let is_previous = api_key
        .key_sha256
        .as_deref()
        .map(|current| current != presented_hex.as_str())
        .unwrap_or(false);

    assert!(
        !is_previous,
        "normal key (no rotation) must not trigger the previous-hash code path"
    );
    assert!(api_key.rotation_expires_at.is_none());
}

#[tokio::test]
async fn v3_5_regression_gateway_proxy_unaffected() {
    common::load_env();
    let base_url = common::spawn_app().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    // Must not be 401 — gateway auth for a normal (non-rotated) key is unchanged.
    assert_ne!(
        resp.status(),
        401,
        "V3-5 changes must not break existing gateway auth"
    );
}
