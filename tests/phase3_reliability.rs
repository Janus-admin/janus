// tests/phase3_reliability.rs
// Phase 3 acceptance tests — Rate Limiting & Reliability.
// All provider-interaction tests use wiremock; zero real API calls.

mod common;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Send a single non-streaming chat request and return the response.
async fn send_chat(base: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("request must reach server")
}

/// Mount a wiremock stub that returns a valid 200 OpenAI JSON response for every POST.
async fn mount_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(server)
        .await;
}

/// Mount a wiremock stub that returns HTTP 500 for every POST.
async fn mount_500(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(server)
        .await;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Exceeding rate limit must return 429 with Retry-After header.
#[tokio::test]
async fn phase3_rate_limit_returns_429_with_retry_after() {
    // Mock server so the first two requests actually complete (200) rather
    // than failing on "no provider" — we want clean signals.
    let mock_server = MockServer::start().await;
    mount_ok(&mock_server).await;

    // Key with rpm=2, 1-second window for test speed.
    let base = common::spawn_app_with_rate_limit(mock_server.uri(), 2).await;

    // Requests 1 and 2: within limit — must NOT be 429.
    for i in 0..2 {
        let resp = send_chat(&base).await;
        assert_ne!(
            resp.status().as_u16(),
            429,
            "request {} should be within rate limit",
            i + 1
        );
    }

    // Request 3: over limit — must be 429 with Retry-After header.
    let resp = send_chat(&base).await;
    assert_eq!(
        resp.status().as_u16(),
        429,
        "third request must exceed rpm=2 limit"
    );
    assert!(
        resp.headers().contains_key("retry-after"),
        "429 response must include Retry-After header; headers: {:?}",
        resp.headers()
    );
    let retry_after: u64 = resp
        .headers()
        .get("retry-after")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .expect("Retry-After must be a number");
    assert!(retry_after >= 1, "Retry-After must be at least 1 second");
}

/// After rate limit window resets, requests must succeed again.
#[tokio::test]
async fn phase3_rate_limit_resets_after_window() {
    let mock_server = MockServer::start().await;
    mount_ok(&mock_server).await;

    // rpm=2, window=1s (set internally by spawn_app_with_rate_limit).
    let base = common::spawn_app_with_rate_limit(mock_server.uri(), 2).await;

    // Fill the window (2 requests).
    for _ in 0..2 {
        let resp = send_chat(&base).await;
        assert_ne!(
            resp.status().as_u16(),
            429,
            "should not be rate limited yet"
        );
    }

    // Third is rate-limited.
    let resp = send_chat(&base).await;
    assert_eq!(
        resp.status().as_u16(),
        429,
        "third request must be rate limited"
    );

    // Wait for the 1-second window to expire.
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;

    // Now the window has reset — should succeed.
    let resp = send_chat(&base).await;
    assert_ne!(
        resp.status().as_u16(),
        429,
        "request after window reset must not be rate limited"
    );
    assert_eq!(
        resp.status().as_u16(),
        200,
        "request after window reset must return 200"
    );
}

/// When primary provider returns 500, request must be retried transparently.
#[tokio::test]
async fn phase3_provider_500_triggers_retry() {
    let mock_server = MockServer::start().await;

    // First POST → 500; all subsequent POSTs → 200.
    // `up_to_n_times(1)` makes this stub fire exactly once, then fall through
    // to the next matching stub (the 200 stub below).
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    // The client sees a transparent 200 — retry is invisible.
    let resp = send_chat(&base).await;
    assert_eq!(
        resp.status().as_u16(),
        200,
        "response must be 200 after transparent retry; got {}",
        resp.status()
    );

    // Provider received 2 calls: the initial 500 + the retry 200.
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(
        received.len(),
        2,
        "provider must have received exactly 2 calls (initial 500 + retry); got {}",
        received.len()
    );
}

/// When primary provider is down, secondary provider must be used automatically.
#[tokio::test]
async fn phase3_provider_failover_uses_next_priority() {
    // Primary: always returns 500 (exhausts retries → failover).
    let primary = MockServer::start().await;
    mount_500(&primary).await;

    // Secondary: returns 200.
    let secondary = MockServer::start().await;
    mount_ok(&secondary).await;

    let base = common::spawn_app_with_two_providers(primary.uri(), secondary.uri()).await;

    // Client sees 200 — failover is transparent.
    let resp = send_chat(&base).await;
    assert_eq!(
        resp.status().as_u16(),
        200,
        "failover must produce a successful response; got {}",
        resp.status()
    );

    // Secondary received at least one request.
    let secondary_calls = secondary.received_requests().await.unwrap();
    assert!(
        !secondary_calls.is_empty(),
        "secondary provider must have been called during failover"
    );
}

/// When all providers are down, must return 503.
#[tokio::test]
async fn phase3_all_providers_down_returns_503() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    mount_500(&primary).await;
    mount_500(&secondary).await;

    let base = common::spawn_app_with_two_providers(primary.uri(), secondary.uri()).await;

    let resp = send_chat(&base).await;
    assert_eq!(
        resp.status().as_u16(),
        503,
        "all providers down must return 503; got {}",
        resp.status()
    );
}
