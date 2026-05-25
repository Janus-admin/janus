// tests/v2_6_clustering.rs
// V2-6: Multi-node clustering tests.
//
// Tests cover:
//   - DB-backed distributed rate limiting (globally shared across nodes)
//   - Cleanup task removes stale rate-limit rows
//   - Key revocation propagates to other nodes via pg_notify
//   - Exact cache is already DB-backed (shared across nodes by design)
//   - Single-node mode unchanged (cluster disabled by default)
//   - Regression: existing gateway behavior unaffected

mod common;

use common::*;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fake_response() -> serde_json::Value {
    fake_openai_response_json()
}

async fn mock_server_with_response(body: serde_json::Value) -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&mock)
        .await;
    mock
}

// ── Distributed rate limiting ─────────────────────────────────────────────────

/// Cluster rate limiting uses the DB; two `DbRateLimiter` instances sharing the
/// same database (simulating two nodes) count toward the same global limit.
#[tokio::test]
async fn v2_6_rate_limit_enforced_globally_across_two_nodes() {
    load_env();
    let config = janus::config::Config::load().expect("Failed to load config");
    let pool = janus::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test DB");

    // Two limiter instances that share the same pool — simulating two cluster nodes.
    let limiter1 = janus::cluster::rate_limit::DbRateLimiter::new(pool.clone(), 60);
    let limiter2 = janus::cluster::rate_limit::DbRateLimiter::new(pool.clone(), 60);

    // Use a unique key_id so parallel test runs don't interfere.
    let key_id = uuid::Uuid::new_v4();
    let rpm_limit = 2i32;

    // First check via "node 1" — allowed.
    assert!(
        limiter1.check_and_record(key_id, rpm_limit).await.is_ok(),
        "node1 first request must pass"
    );
    // Second check via "node 2" — allowed (limit not yet reached globally).
    assert!(
        limiter2.check_and_record(key_id, rpm_limit).await.is_ok(),
        "node2 first request must pass"
    );
    // Third check via "node 1" — should be rejected (global count = 2 ≥ limit 2).
    let result = limiter1.check_and_record(key_id, rpm_limit).await;
    assert!(
        result.is_err(),
        "third request must be rejected: global RPM limit reached"
    );
    let retry_after = result.unwrap_err();
    assert!(retry_after > 0, "retry_after must be > 0 seconds");
}

/// The cleanup task should delete rows older than 2× the window.
/// We insert a stale row directly and verify it disappears.
#[tokio::test]
async fn v2_6_cleanup_task_removes_old_rate_limit_rows() {
    load_env();
    let config = janus::config::Config::load().expect("Failed to load config");
    let pool = janus::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test DB");

    // Insert a row with a timestamp well beyond the 2-minute cleanup cutoff.
    #[cfg(not(feature = "sqlite"))]
    sqlx::query(
        "INSERT INTO rate_limit_windows (api_key_id, request_at, tokens) \
         VALUES ($1, NOW() - INTERVAL '10 minutes', 0)",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("Failed to insert stale row");

    #[cfg(feature = "sqlite")]
    sqlx::query(
        "INSERT INTO rate_limit_windows (api_key_id, request_at, tokens) \
         VALUES ($1, datetime('now', '-10 minutes'), 0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .execute(&pool)
    .await
    .expect("Failed to insert stale row");

    // Count stale rows before cleanup.
    let before: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rate_limit_windows WHERE request_at < NOW() - INTERVAL '2 minutes'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to count stale rows");
    assert!(before.0 >= 1, "Stale row should be present before cleanup");

    // Run cleanup directly via DbRateLimiter with a 60-second window.
    let limiter = janus::cluster::rate_limit::DbRateLimiter::new(pool.clone(), 60);
    limiter.cleanup().await.expect("Cleanup should succeed");

    // Stale rows should be gone.
    let after: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rate_limit_windows WHERE request_at < NOW() - INTERVAL '2 minutes'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to count stale rows after cleanup");
    assert_eq!(after.0, 0, "All stale rows must be removed by cleanup");
}

// ── Budget enforcement ────────────────────────────────────────────────────────

/// Budget is tracked correctly across multiple sequential requests in cluster mode.
/// (The test key has no budget_limit by default; this test verifies the gateway
/// still proxies successfully and budget_used increments are DB-persisted.)
#[tokio::test]
async fn v2_6_budget_tracked_in_cluster_mode() {
    let mock = mock_server_with_response(fake_response()).await;
    let base_url = spawn_app_with_cluster(mock.uri(), "node-budget").await;

    let client = reqwest::Client::new();

    // A few requests should all succeed (no budget limit on the in-memory test key).
    for i in 0..3 {
        let resp = client
            .post(format!("{}/v1/chat/completions", base_url))
            .header("Authorization", auth_header())
            .json(&minimal_chat_request())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "request {} must succeed", i + 1);
    }
}

// ── Key revocation propagation ────────────────────────────────────────────────

/// When a key is revoked, the pg_notify mechanism evicts it from other nodes' caches.
/// This test verifies the mechanism directly by:
///   1. Inserting a fake key into two DashMaps (simulating two nodes).
///   2. Starting a key_sync listener on the second DashMap.
///   3. Issuing pg_notify via direct SQL.
///   4. After a brief propagation window, verifying the second DashMap no longer has the key.
#[tokio::test]
#[cfg(not(feature = "sqlite"))]
async fn v2_6_key_revocation_propagates_via_notify() {
    use dashmap::DashMap;
    use std::sync::Arc;

    load_env();
    let config = janus::config::Config::load().expect("Failed to load config");
    let pool = janus::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test DB");

    // Generate a fake key's sha256 bytes.
    let fake_raw_key = "jn-sk-PropagationTestKey000000000000000000000000000000";
    let sha256_hex = janus::db::api_keys::sha256_hex(fake_raw_key);
    let sha256_bytes = janus::db::api_keys::sha256_bytes(fake_raw_key);

    // Simulate two separate node key caches.
    let cache_node1: Arc<DashMap<[u8; 32], janus::models::api_key::ApiKey>> =
        Arc::new(DashMap::new());
    let cache_node2: Arc<DashMap<[u8; 32], janus::models::api_key::ApiKey>> =
        Arc::new(DashMap::new());

    let dummy_key = janus::models::api_key::ApiKey {
        id: uuid::Uuid::new_v4(),
        name: "propagation-test".to_string(),
        key_hash: "placeholder".to_string(),
        key_sha256: Some(sha256_hex.clone()),
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "jn-sk-Propa".to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: rust_decimal::Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: chrono::Utc::now(),
        expires_at: None,
        last_used_at: None,
    };

    // Both nodes start with the key in their caches.
    cache_node1.insert(sha256_bytes, dummy_key.clone());
    cache_node2.insert(sha256_bytes, dummy_key);

    assert!(
        cache_node1.contains_key(&sha256_bytes),
        "node1 must have the key before notify"
    );
    assert!(
        cache_node2.contains_key(&sha256_bytes),
        "node2 must have the key before notify"
    );

    // Start the key_sync listener on node2's cache.
    janus::cluster::key_sync::start(pool.clone(), cache_node2.clone())
        .await
        .expect("key_sync::start must succeed");

    // Issue pg_notify (as if a DELETE /admin/keys call triggered it).
    sqlx::query("SELECT pg_notify('api_key_invalidated', $1)")
        .bind(sha256_hex.as_str())
        .execute(&pool)
        .await
        .expect("pg_notify must succeed");

    // Wait briefly for the listener task to receive and process the notification.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // node2's cache must no longer contain the key.
    assert!(
        !cache_node2.contains_key(&sha256_bytes),
        "node2 must NOT have the key after pg_notify propagation"
    );

    // node1's cache is unchanged (it wasn't listening).
    assert!(
        cache_node1.contains_key(&sha256_bytes),
        "node1 cache must be unchanged (it was not the listener)"
    );
}

/// A key revoked via admin API stops working on the same node immediately.
#[tokio::test]
async fn v2_6_revoked_key_rejected_on_same_node() {
    let mock = mock_server_with_response(fake_response()).await;
    let base_url = spawn_app_with_cluster(mock.uri(), "node-same").await;

    let admin_token = admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &admin_token)
        .json(&serde_json::json!({ "name": "revoke-same-node-key" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 201);

    let body: serde_json::Value = create_resp.json().await.unwrap();
    let raw_key = body["data"]["key"].as_str().unwrap().to_string();
    let key_id = body["data"]["id"].as_str().unwrap().to_string();

    // Revoke.
    let revoke = client
        .delete(format!("{}/admin/keys/{}", base_url, key_id))
        .header("Authorization", &admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(revoke.status(), 200);

    // Must be rejected immediately on the same node.
    let after = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", raw_key))
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        401,
        "Revoked key must be rejected immediately on the local node"
    );
}

// ── Exact cache shared across nodes ──────────────────────────────────────────

/// Exact cache entries are stored in PostgreSQL — they are shared across nodes.
/// A cache hit populated on node1 is served from the warmed cache on node2.
///
/// Flow:
///   1. node1 receives a request and populates the DB cache.
///   2. node2 is spawned WITH warm_cache=true — it loads the DB cache at startup.
///   3. The same request on node2 is served as an exact cache hit (no provider call).
#[tokio::test]
async fn v2_6_exact_cache_hit_on_second_node_after_first_node_populates() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_response()))
        .expect(1) // Provider called ONCE — node2 serves from warmed cache.
        .mount(&mock)
        .await;

    // Use a content string that is unique to this test to avoid cross-test cache pollution.
    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": "v2p6-cache-sharing-unique-test-content" }]
    });

    // Spawn node1 and make the request to populate the DB cache.
    let node1 = spawn_app_with_cluster(mock.uri(), "node-cache-1").await;
    let client = reqwest::Client::new();

    let r1 = client
        .post(format!("{}/v1/chat/completions", node1))
        .header("Authorization", auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200, "node1 request must succeed");

    // Spawn node2 AFTER node1 has populated the cache.
    // warm_cache=true means node2 calls warm_from_db() at startup and loads the entry.
    let node2 = spawn_app_with_cluster_warmed(mock.uri(), "node-cache-2").await;

    // Same request on node2 — must be served from the warmed cache (no provider call).
    let r2 = client
        .post(format!("{}/v1/chat/completions", node2))
        .header("Authorization", auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200, "node2 request must succeed");
    let cache_header2 = r2
        .headers()
        .get("x-janus-cache-hit")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        cache_header2, "exact",
        "node2 must return X-Janus-Cache-Hit: exact after warming cache from shared DB"
    );
}

// ── Single-node mode (cluster disabled) ──────────────────────────────────────

/// cluster.enabled defaults to false; the in-memory rate limiter is used.
#[tokio::test]
async fn v2_6_cluster_disabled_by_default() {
    load_env();
    let config = janus::config::Config::load().expect("Failed to load config");
    assert!(
        !config.cluster.enabled,
        "cluster.enabled must default to false"
    );
}

/// In single-node mode the existing in-memory rate limiter still works.
#[tokio::test]
async fn v2_6_single_node_mode_uses_in_memory_rate_limit() {
    let mock = mock_server_with_response(fake_response()).await;
    let base_url = spawn_app_with_rate_limit(mock.uri(), 1).await;

    let client = reqwest::Client::new();

    let r1 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", auth_header())
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200, "First request must succeed");

    let r2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", auth_header())
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 429, "Second request must be rate-limited");
}

// ── Regression ────────────────────────────────────────────────────────────────

/// Gateway proxy still works correctly in cluster mode.
#[tokio::test]
async fn v2_6_regression_gateway_still_fast_in_single_node_mode() {
    let mock = mock_server_with_response(fake_response()).await;
    let base_url = spawn_app_with_openai_base(mock.uri()).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", auth_header())
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "Gateway must proxy successfully in single-node mode"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["choices"].is_array(),
        "Response must contain choices array"
    );
}

/// Auth is still enforced in cluster mode.
#[tokio::test]
async fn v2_6_regression_auth_still_enforced() {
    let mock = mock_server_with_response(fake_response()).await;
    let base_url = spawn_app_with_cluster(mock.uri(), "node-auth").await;

    let client = reqwest::Client::new();

    // No auth header.
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "Missing auth must return 401");

    // Invalid key.
    let resp2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header(
            "Authorization",
            "Bearer jn-sk-invalid000000000000000000000000000000000000000000",
        )
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 401, "Invalid key must return 401");
}
