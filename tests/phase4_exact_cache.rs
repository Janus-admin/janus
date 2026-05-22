// tests/phase4_exact_cache.rs
// Phase 4 acceptance tests — Exact Cache.

mod common;

use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Mount a wiremock stub that returns a valid 200 OpenAI JSON response for every POST.
async fn mount_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(server)
        .await;
}

/// A chat request body with a unique UUID in the message so no two tests share
/// a cache key — prevents cross-test contamination via the shared PostgreSQL DB.
fn unique_chat_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            { "role": "user", "content": format!("Say hello {}", uuid::Uuid::new_v4()) }
        ]
    })
}

/// Send a non-streaming chat request and return the full response.
async fn send(base: &str, body: &serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(body)
        .send()
        .await
        .expect("request must reach server")
}

/// Send a request with `X-Velox-Cache: false` to bypass the cache.
async fn send_bypass(base: &str, body: &serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Cache", "false")
        .json(body)
        .send()
        .await
        .expect("request must reach server")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Identical requests must return `X-Velox-Cache-Hit: exact` on the second call.
#[tokio::test]
#[serial]
async fn phase4_identical_request_returns_exact_cache_hit() {
    let (base, mock_server) = common::spawn_app_with_wiremock().await;
    mount_ok(&mock_server).await;

    let body = unique_chat_request();

    // First request: cache miss — no cache-hit header.
    let resp1 = send(&base, &body).await;
    assert_eq!(resp1.status(), 200, "first request must succeed");
    assert!(
        resp1.headers().get("x-velox-cache-hit").is_none(),
        "first request must NOT have X-Velox-Cache-Hit header"
    );

    // Second request: cache hit — header must be present with value "exact".
    let resp2 = send(&base, &body).await;
    assert_eq!(resp2.status(), 200, "second request must succeed");
    assert_eq!(
        resp2
            .headers()
            .get("x-velox-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("exact"),
        "second request must have X-Velox-Cache-Hit: exact"
    );
}

/// Exact cache hit must respond in under 10 ms (DashMap lookup, no provider call).
#[tokio::test]
#[serial]
async fn phase4_exact_cache_response_time_under_10ms() {
    let (base, mock_server) = common::spawn_app_with_wiremock().await;
    mount_ok(&mock_server).await;

    let body = unique_chat_request();

    // Warm the cache with the first request.
    let warm = send(&base, &body).await;
    assert_eq!(warm.status(), 200, "warm-up request must succeed");

    // Measure the cache-hit round-trip.
    let start = std::time::Instant::now();
    let resp = send(&base, &body).await;
    let elapsed_ms = start.elapsed().as_millis();

    assert_eq!(resp.status(), 200, "cache-hit request must succeed");
    assert_eq!(
        resp.headers()
            .get("x-velox-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("exact"),
        "response must come from cache"
    );
    assert!(
        elapsed_ms < 10,
        "cache hit must respond in under 10 ms; got {} ms",
        elapsed_ms
    );
}

/// `X-Velox-Cache: false` request header must bypass the cache entirely.
/// Both requests must reach the provider; neither response has a cache-hit header.
#[tokio::test]
#[serial]
async fn phase4_cache_bypass_header_skips_cache() {
    let (base, mock_server) = common::spawn_app_with_wiremock().await;
    mount_ok(&mock_server).await;

    let body = unique_chat_request();

    let resp1 = send_bypass(&base, &body).await;
    assert_eq!(resp1.status(), 200, "first bypass request must succeed");
    assert!(
        resp1.headers().get("x-velox-cache-hit").is_none(),
        "bypass response must not have cache-hit header"
    );

    let resp2 = send_bypass(&base, &body).await;
    assert_eq!(resp2.status(), 200, "second bypass request must succeed");
    assert!(
        resp2.headers().get("x-velox-cache-hit").is_none(),
        "second bypass response must not have cache-hit header"
    );

    // Both requests must have reached the provider (not served from cache).
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(
        received.len(),
        2,
        "both bypass requests must reach the provider; got {} calls",
        received.len()
    );
}

/// Cache stats endpoint must report correct entry count and hit count.
#[tokio::test]
#[serial]
async fn phase4_cache_stats_show_correct_savings() {
    let (base, mock_server) = common::spawn_app_with_wiremock().await;
    mount_ok(&mock_server).await;

    let client = reqwest::Client::new();
    let body = unique_chat_request();

    // Request 1: cache miss — writes entry to DB.
    let r1 = send(&base, &body).await;
    assert_eq!(r1.status(), 200);

    // Request 2: cache hit — increments hit_count in DB (fire-and-forget spawn).
    let r2 = send(&base, &body).await;
    assert_eq!(r2.status(), 200);
    assert_eq!(
        r2.headers()
            .get("x-velox-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("exact")
    );

    // Both upsert and record_hit are awaited synchronously in the pipeline,
    // so no sleep needed here.

    // Check stats.
    let stats = client
        .get(format!("{}/admin/cache/stats", base))
        .send()
        .await
        .expect("stats request must reach server");

    assert_eq!(stats.status(), 200, "stats endpoint must return 200");

    let body: serde_json::Value = stats.json().await.expect("stats must parse as JSON");
    let data = &body["data"];

    assert!(
        data["total_entries"].as_i64().unwrap_or(0) >= 1,
        "must have at least 1 cache entry; data: {data}"
    );
    assert!(
        data["total_hits"].as_i64().unwrap_or(0) >= 1,
        "must have at least 1 hit recorded; data: {data}"
    );
    assert!(
        data["exact_entries"].as_i64().unwrap_or(0) >= 1,
        "must have at least 1 exact entry; data: {data}"
    );
    assert_eq!(
        data["semantic_entries"].as_i64().unwrap_or(-1),
        0,
        "semantic entries must be 0 in Phase 4; data: {data}"
    );
}

/// Flushing the cache must result in a provider call on the subsequent request.
#[tokio::test]
#[serial]
async fn phase4_flush_cache_causes_miss_on_next_request() {
    let (base, mock_server) = common::spawn_app_with_wiremock().await;
    mount_ok(&mock_server).await;

    let client = reqwest::Client::new();
    let body = unique_chat_request();

    // Warm the cache.
    let r1 = send(&base, &body).await;
    assert_eq!(r1.status(), 200);

    // Verify the cache is warm.
    let r2 = send(&base, &body).await;
    assert_eq!(r2.status(), 200);
    assert_eq!(
        r2.headers()
            .get("x-velox-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("exact"),
        "second request must be a cache hit before flush"
    );

    // Flush the cache.
    let flush = client
        .delete(format!("{}/admin/cache", base))
        .send()
        .await
        .expect("flush request must reach server");
    assert_eq!(flush.status(), 200, "flush must return 200");

    // After flush, same request must miss and call the provider.
    let provider_calls_before = mock_server.received_requests().await.unwrap().len();

    let r3 = send(&base, &body).await;
    assert_eq!(r3.status(), 200, "post-flush request must succeed");
    assert!(
        r3.headers().get("x-velox-cache-hit").is_none(),
        "post-flush request must NOT have cache-hit header"
    );

    let provider_calls_after = mock_server.received_requests().await.unwrap().len();
    assert!(
        provider_calls_after > provider_calls_before,
        "post-flush request must reach the provider; before={} after={}",
        provider_calls_before,
        provider_calls_after
    );
}
