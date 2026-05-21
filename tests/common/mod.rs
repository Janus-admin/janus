// tests/common/mod.rs
// Shared test helpers used across all phase test files.
// Add helpers here when two or more test files need the same setup code.

use std::net::TcpListener;

/// Load .env file for tests. Call at the top of any test that needs env vars.
pub fn load_env() {
    dotenvy::dotenv().ok();
}

/// Binds to a random available port and returns the address.
/// Use this to start the test server on a port that won't conflict.
pub fn random_port_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();
    format!("127.0.0.1:{}", port)
}

/// A valid Velox API key format for testing.
/// In real tests, this key must exist in the test database.
pub fn test_api_key() -> &'static str {
    "vx-sk-testkey000000000000000000000000000000000000000"
}

/// Authorization header value for test API key.
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
