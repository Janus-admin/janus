// tests/v4_3_cache_ttl.rs
// Phase V4-3 acceptance tests — Cache TTL + Time-sensitive Safety.
//
// Run with: cargo test v4_3
//
// These tests use the CacheEngine and TimeGuard directly (pure unit tests, no
// network required) and the full HTTP stack via wiremock for integration tests.

mod common;

use std::sync::Arc;
use velox::{
    cache::{time_guard::TimeGuard, CacheEngine},
    providers::ChatCompletionResponse,
};
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fake_response(content: &str) -> ChatCompletionResponse {
    serde_json::from_value(serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 0,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10 }
    }))
    .unwrap()
}

fn chat_request_body(content: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": content }]
    })
}

fn openai_mock_response() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Hello from provider!" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10 }
    })
}

// ── Unit tests: TTL in CacheEngine ───────────────────────────────────────────

#[tokio::test]
async fn v4_3_entry_returned_before_ttl_expires() {
    let cache = CacheEngine::new();
    let hash = "hash-not-expired".to_string();
    cache.insert_with_ttl(hash.clone(), Arc::new(fake_response("hi")), 3600);

    let result = cache.lookup(&hash);
    assert!(
        result.is_some(),
        "entry should be present before TTL expires"
    );
}

#[tokio::test]
async fn v4_3_entry_not_returned_after_ttl_expires() {
    let cache = CacheEngine::new();
    let hash = "hash-expired".to_string();
    // TTL of 1 second — we manually set the expiry to the past via a negative TTL equivalent.
    // Since we can't mock time directly, use insert_with_ttl then manually call evict_expired
    // by inserting a zero-second TTL entry (effectively already expired).
    //
    // Workaround: insert an entry and then directly manipulate via the API.
    // We insert normally, then call evict_expired with a 1ms TTL entry.
    // The simplest approach: use the internal expiry mechanism.
    //
    // Since we can't control clock in tests, we validate the evict_expired path:
    // insert with ttl=0 (no expiry), check it's present, then insert a second entry
    // with ttl=1 and run evict_expired after sleeping.
    cache.insert_with_ttl(hash.clone(), Arc::new(fake_response("hi")), 1);
    // 1 second TTL: entry is present now
    assert!(
        cache.lookup(&hash).is_some(),
        "should be present immediately after insert"
    );
    // Sleep past the TTL
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    // Next lookup should evict and return None
    let result = cache.lookup(&hash);
    assert!(result.is_none(), "entry should be evicted after TTL expiry");
}

#[tokio::test]
async fn v4_3_zero_ttl_means_no_expiry() {
    let cache = CacheEngine::new();
    let hash = "hash-no-expiry".to_string();
    cache.insert_with_ttl(hash.clone(), Arc::new(fake_response("hi")), 0);
    // With ttl=0, entry never expires
    assert!(cache.lookup(&hash).is_some(), "ttl=0 should mean no expiry");
    // Confirm no expiry entry in the expiry map by checking evict_expired finds nothing
    let evicted = cache.evict_expired();
    assert_eq!(evicted, 0, "no entries should be evicted when ttl=0");
    assert!(
        cache.lookup(&hash).is_some(),
        "entry still present after evict pass"
    );
}

#[tokio::test]
async fn v4_3_prune_task_removes_expired_entries() {
    let cache = CacheEngine::new();
    // Insert 3 entries: 2 with very short TTL, 1 permanent
    cache.insert_with_ttl("exp-a".to_string(), Arc::new(fake_response("a")), 1);
    cache.insert_with_ttl("exp-b".to_string(), Arc::new(fake_response("b")), 1);
    cache.insert_with_ttl("permanent".to_string(), Arc::new(fake_response("c")), 0);

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let evicted = cache.evict_expired();
    assert_eq!(evicted, 2, "both expired entries should be evicted");
    assert!(cache.lookup("exp-a").is_none());
    assert!(cache.lookup("exp-b").is_none());
    assert!(
        cache.lookup("permanent").is_some(),
        "permanent entry should survive"
    );
}

#[tokio::test]
async fn v4_3_plain_insert_has_no_expiry() {
    let cache = CacheEngine::new();
    cache.insert("hash-plain".to_string(), Arc::new(fake_response("hello")));
    // No TTL set → evict_expired should not touch it
    let evicted = cache.evict_expired();
    assert_eq!(evicted, 0);
    assert!(cache.lookup("hash-plain").is_some());
}

// ── Unit tests: TimeGuard ─────────────────────────────────────────────────────

fn req_with(content: &str) -> velox::providers::ChatCompletionRequest {
    serde_json::from_value(serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": content }]
    }))
    .unwrap()
}

#[test]
fn v4_3_time_sensitive_prompt_detected_english() {
    let guard = TimeGuard::new(&[r"\btoday\b".into()]);
    assert!(guard.is_time_sensitive(&req_with("What happened today?")));
}

#[test]
fn v4_3_time_sensitive_prompt_detected_persian() {
    let guard = TimeGuard::new(&["امروز".into()]);
    assert!(guard.is_time_sensitive(&req_with("امروز چه خبر است؟")));
}

#[test]
fn v4_3_non_time_sensitive_prompt_not_detected() {
    let guard = TimeGuard::new(&[r"\btoday\b".into()]);
    assert!(!guard.is_time_sensitive(&req_with("What is the capital of France?")));
}

#[test]
fn v4_3_empty_pattern_list_never_matches() {
    let guard = TimeGuard::new(&[]);
    // Even explicitly time-bound text should not match when patterns are empty
    assert!(!guard.is_time_sensitive(&req_with("today right now currently الآن")));
}

#[test]
fn v4_3_custom_pattern_added_via_config() {
    let guard = TimeGuard::new(&[r"\bbreaking\b".into()]);
    assert!(guard.is_time_sensitive(&req_with("Show me breaking news")));
    assert!(!guard.is_time_sensitive(&req_with("What is the weather forecast?")));
}

// ── Integration tests: X-Velox-Cache-Skip header ─────────────────────────────

#[tokio::test]
async fn v4_3_skip_header_set_on_time_sensitive_request() {
    let mock_server = wiremock::MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_mock_response()))
        .expect(1) // time-sensitive skips cache → always hits provider
        .mount(&mock_server)
        .await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&chat_request_body(
            "What is the current price of gold today?",
        ))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let skip_header = resp.headers().get("x-velox-cache-skip");
    assert_eq!(
        skip_header.and_then(|h| h.to_str().ok()),
        Some("time_sensitive"),
        "time-sensitive requests must have X-Velox-Cache-Skip: time_sensitive header"
    );
}

#[tokio::test]
async fn v4_3_regression_exact_cache_hit_still_works_without_ttl() {
    let mock_server = wiremock::MockServer::start().await;

    // Provider should be called exactly once — second request hits cache.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_mock_response()))
        .expect(1)
        .mount(&mock_server)
        .await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let body = chat_request_body("Explain quantum entanglement");

    // First request — hits provider, populates cache.
    let r1 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .expect("first request failed");
    assert_eq!(r1.status(), 200);
    assert!(
        r1.headers().get("x-velox-cache-hit").is_none()
            || r1
                .headers()
                .get("x-velox-cache-hit")
                .map(|h| h != "exact")
                .unwrap_or(true),
        "first request should not be a cache hit"
    );

    // Second identical request — should be served from cache (exact hit).
    let r2 = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .expect("second request failed");
    assert_eq!(r2.status(), 200);

    let hit_header = r2
        .headers()
        .get("x-velox-cache-hit")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        hit_header, "exact",
        "second request should be an exact cache hit"
    );

    mock_server.verify().await;
}
