// tests/phase1_proxy.rs
// Phase 1 acceptance tests — Core Proxy.
//
// Run with: cargo test phase1

mod common;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Authentication Tests ─────────────────────────────────────────────────────

/// A request with no Authorization header must return 401.
#[tokio::test]
async fn phase1_missing_auth_header_returns_401() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Content-Type", "application/json")
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(
        response.status(),
        401,
        "Missing auth header must return 401"
    );
}

/// A request with a malformed API key must return 401.
#[tokio::test]
async fn phase1_invalid_api_key_returns_401() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", "Bearer not-a-valid-key")
        .header("Content-Type", "application/json")
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 401, "Invalid API key must return 401");

    let body: serde_json::Value = response.json().await.expect("Error response must be JSON");
    assert!(
        body["error"]["code"].is_string(),
        "Error response must have error.code field"
    );
    assert!(
        body["error"]["message"].is_string(),
        "Error response must have error.message field"
    );
}

/// An API key with the correct format but not in the database must return 401.
#[tokio::test]
async fn phase1_nonexistent_api_key_returns_401() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header(
            "Authorization",
            "Bearer vx-sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .header("Content-Type", "application/json")
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(
        response.status(),
        401,
        "Non-existent API key must return 401"
    );
}

// ─── Request Format Tests ─────────────────────────────────────────────────────

/// A request without a model field must return 400.
#[tokio::test]
async fn phase1_missing_model_field_returns_400() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "messages": [{ "role": "user", "content": "hello" }]
            // "model" intentionally missing
        }))
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(
        response.status(),
        400,
        "Request without model field must return 400"
    );
}

/// A request without messages must return 400.
#[tokio::test]
async fn phase1_missing_messages_field_returns_400() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-4o-mini"
            // "messages" intentionally missing
        }))
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(
        response.status(),
        400,
        "Request without messages must return 400"
    );
}

// ─── Proxy Behavior Tests ─────────────────────────────────────────────────────

/// A valid request must be proxied to the provider and return OpenAI-format response.
/// Uses wiremock to mock the provider so no real API key is needed.
#[tokio::test]
async fn phase1_valid_request_returns_openai_format_response() {
    // Start a mock server that mimics the OpenAI chat completions endpoint
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-test123",
            "object": "chat.completion",
            "created": 1716000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello! How can I help you?", "name": null },
                "finish_reason": "stop",
                "logprobs": null
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        })))
        .mount(&mock_server)
        .await;

    // Spawn app with the OpenAI provider pointed at our mock server.
    // The provider calls {base_url}/chat/completions, so pass mock_server.uri() as base_url.
    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("Content-Type", "application/json")
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");

    assert!(body["id"].is_string(), "Response must have id");
    assert_eq!(body["object"], "chat.completion");
    assert!(
        body["choices"].is_array(),
        "Response must have choices array"
    );
    assert!(
        body["choices"][0]["message"]["content"].is_string(),
        "Response must have message content"
    );
    assert!(body["usage"]["prompt_tokens"].is_number());
    assert!(body["usage"]["completion_tokens"].is_number());
    assert!(body["usage"]["total_tokens"].is_number());
}

// ─── Cost Tracking Tests ──────────────────────────────────────────────────────

/// After a successful request, a record must exist in the requests table
/// with correct token counts and non-zero cost.
#[tokio::test]
#[ignore = "Requires pricing data in model_pricing table and DB query access in test"]
async fn phase1_request_is_logged_with_cost() {
    let _base_url = common::spawn_app().await;
    // TODO: make a proxied request via mock provider, then SELECT from requests table
    // and assert prompt_tokens, completion_tokens, cost_usd > 0.
}

// ─── Admin API Tests ─────────────────────────────────────────────────────────

/// Creating an API key must return the key with vx-sk- prefix.
/// The full key is only shown once.
#[tokio::test]
async fn phase1_create_api_key_returns_prefixed_key() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/admin/keys", base_url))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "name": "Test Key"
        }))
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 201);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");
    let key = body["data"]["key"]
        .as_str()
        .expect("Response must contain key");

    assert!(
        key.starts_with("vx-sk-"),
        "API key must start with vx-sk-. Got: {}",
        key
    );
    assert_eq!(
        key.len(),
        54, // "vx-sk-" (6) + 48 chars
        "API key must be 54 characters total. Got: {}",
        key.len()
    );
}

/// Listing API keys must not return full key values — only prefixes.
#[tokio::test]
async fn phase1_list_api_keys_never_returns_full_key() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/admin/keys", base_url))
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");
    let keys = body["data"]
        .as_array()
        .expect("Response must have data array");

    for key in keys {
        assert!(
            key["key_prefix"].is_string(),
            "Each key must have key_prefix"
        );
        assert!(
            key.get("key").is_none(),
            "Full key must NEVER appear in list response"
        );
    }
}
