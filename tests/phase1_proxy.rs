// tests/phase1_proxy.rs
// Phase 1 acceptance tests — Core Proxy.
//
// These tests WILL FAIL until Phase 1 is implemented. That is expected.
// They define the contract for Phase 1.
//
// Run with: cargo test phase1

mod common;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Authentication Tests ─────────────────────────────────────────────────────

/// A request with no Authorization header must return 401.
#[tokio::test]
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_missing_auth_header_returns_401() {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
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
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_invalid_api_key_returns_401() {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
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
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_nonexistent_api_key_returns_401() {
    let client = reqwest::Client::new();
    // Valid format but doesn't exist in DB
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
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
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_missing_model_field_returns_400() {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
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
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_missing_messages_field_returns_400() {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
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
#[ignore = "Phase 1 not yet implemented — requires test server setup"]
async fn phase1_valid_request_returns_openai_format_response() {
    // Mock OpenAI provider
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-test123",
            "object": "chat.completion",
            "created": 1716000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello! How can I help you?" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        })))
        .mount(&mock_server)
        .await;

    // The response from Velox must match OpenAI format exactly
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/v1/chat/completions")
        .header("Authorization", common::auth_header())
        .header("Content-Type", "application/json")
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");

    // Verify OpenAI-compatible response shape
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
#[ignore = "Phase 1 not yet implemented — requires test server + DB setup"]
async fn phase1_request_is_logged_with_cost() {
    // This test requires:
    // 1. A test API key that exists in the database
    // 2. A mocked provider
    // 3. Database access to verify the log entry
    //
    // Implementation: see Phase 1 development session
    todo!("Implement in Phase 1 development session")
}

// ─── Admin API Tests ─────────────────────────────────────────────────────────

/// Creating an API key must return the key with vx-sk- prefix.
/// The full key is only shown once.
#[tokio::test]
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_create_api_key_returns_prefixed_key() {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8080/admin/keys")
        .header("Authorization", "Bearer admin-jwt-token-here")
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
#[ignore = "Phase 1 not yet implemented"]
async fn phase1_list_api_keys_never_returns_full_key() {
    let client = reqwest::Client::new();
    let response = client
        .get("http://localhost:8080/admin/keys")
        .header("Authorization", "Bearer admin-jwt-token-here")
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");
    let keys = body["data"]
        .as_array()
        .expect("Response must have data array");

    for key in keys {
        // key_prefix should exist but full key must NOT be in the response
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
