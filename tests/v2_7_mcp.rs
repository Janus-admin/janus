// tests/v2_7_mcp.rs
// Phase V2-7 acceptance tests — MCP Server.
//
// Run with: cargo test v2_7

mod common;

use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// POST a JSON-RPC request to /mcp/rpc without auth (no Authorization header).
async fn rpc_no_auth(base: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp/rpc", base))
        .json(&body)
        .send()
        .await
        .expect("rpc request failed")
}

/// POST a JSON-RPC request to /mcp/rpc with an admin JWT token.
async fn rpc(base: &str, jwt_token: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp/rpc", base))
        .header("Authorization", format!("Bearer {}", jwt_token))
        .json(&body)
        .send()
        .await
        .expect("rpc request failed")
}

fn initialize_request(token: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0" },
            "token": token
        }
    })
}

fn tools_list_request() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    })
}

fn tools_call(id: u32, tool: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": args
        }
    })
}

// ── Tool schema validation (sync, no server needed) ───────────────────────────

#[test]
fn v2p7_all_tools_have_valid_json_schema() {
    let tools = velox::mcp::tools::tool_list();
    assert!(!tools.is_empty(), "tool list must not be empty");

    for tool in &tools {
        assert!(!tool.name.is_empty(), "tool must have a name");
        assert!(!tool.description.is_empty(), "tool must have a description");
        let schema = &tool.input_schema;
        assert_eq!(
            schema["type"], "object",
            "input schema type must be 'object'"
        );
        assert!(
            schema.get("properties").is_some(),
            "input schema must have 'properties'"
        );
    }
}

#[test]
fn v2p7_tool_inputs_correctly_validated() {
    let tools = velox::mcp::tools::tool_list();

    let proxy = tools
        .iter()
        .find(|t| t.name == "proxy_llm_request")
        .unwrap();
    let required = proxy.input_schema["required"].as_array().unwrap();
    assert!(
        required.iter().any(|v| v.as_str() == Some("model")),
        "proxy_llm_request must require 'model'"
    );
    assert!(
        required.iter().any(|v| v.as_str() == Some("messages")),
        "proxy_llm_request must require 'messages'"
    );

    let create = tools.iter().find(|t| t.name == "create_api_key").unwrap();
    let required = create.input_schema["required"].as_array().unwrap();
    assert!(
        required.iter().any(|v| v.as_str() == Some("name")),
        "create_api_key must require 'name'"
    );

    let stats = tools.iter().find(|t| t.name == "get_usage_stats").unwrap();
    let required = stats.input_schema["required"].as_array().unwrap();
    assert!(
        required.iter().any(|v| v.as_str() == Some("period")),
        "get_usage_stats must require 'period'"
    );
}

// ── Tool execution: get_usage_stats ──────────────────────────────────────────

#[tokio::test]
async fn v2p7_get_usage_stats_returns_valid_data() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(
            1,
            "get_usage_stats",
            serde_json::json!({ "period": "today" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0", "jsonrpc version must be 2.0");
    assert!(body.get("error").is_none(), "must not have error field");
    assert!(
        body["result"]["content"].is_array(),
        "result must contain a content array"
    );
}

// ── Tool execution: list_api_keys ─────────────────────────────────────────────

#[tokio::test]
async fn v2p7_list_api_keys_returns_array() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(1, "list_api_keys", serde_json::json!({})),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body.get("error").is_none(), "must not have error field");

    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(
        parsed["keys"].is_array(),
        "payload must contain a 'keys' array"
    );
}

// ── Tool execution: create_api_key ────────────────────────────────────────────

#[tokio::test]
async fn v2p7_create_api_key_returns_new_key() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(
            1,
            "create_api_key",
            serde_json::json!({ "name": "mcp-test-key" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body.get("error").is_none(), "must not have error field");

    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed["id"].is_string(), "response must contain 'id'");
    let key = parsed["key"].as_str().unwrap();
    assert!(key.starts_with("vx-sk-"), "key must start with 'vx-sk-'");
}

// ── Tool execution: get_cache_stats ──────────────────────────────────────────

#[tokio::test]
async fn v2p7_get_cache_stats_returns_valid_data() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(1, "get_cache_stats", serde_json::json!({})),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body.get("error").is_none(), "must not have error field");
    assert!(
        body["result"]["content"].is_array(),
        "must have content array"
    );
}

// ── Tool execution: flush_cache ───────────────────────────────────────────────

#[tokio::test]
async fn v2p7_flush_cache_clears_entries() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(1, "flush_cache", serde_json::json!({})),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body.get("error").is_none(), "must not have error field");

    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(
        parsed["flushed"].is_number(),
        "response must contain a 'flushed' count"
    );
}

// ── Tool execution: proxy_llm_request ────────────────────────────────────────

#[tokio::test]
async fn v2p7_proxy_llm_request_returns_completion() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        tools_call(
            1,
            "proxy_llm_request",
            serde_json::json!({
                "model": "gpt-4o-mini",
                "messages": [{ "role": "user", "content": "Hello" }]
            }),
        ),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body.get("error").is_none(), "must not have error field");
    assert!(
        body["result"]["content"].is_array(),
        "must have content array"
    );

    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let completion: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        completion["choices"][0]["message"]["content"], "Hello!",
        "completion must contain the provider response"
    );
}

// ── Transport: SSE ────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2p7_sse_transport_sends_json_rpc_events() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/mcp/sse", base))
        .header("Authorization", &jwt)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .expect("SSE request failed");

    assert_eq!(resp.status(), 200, "SSE endpoint must return 200");

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "Content-Type must be text/event-stream, got: {content_type}"
    );

    // Read the response body — the SSE stream begins with an `endpoint` event.
    let bytes = resp.bytes().await.expect("failed to read SSE body");
    let raw = std::str::from_utf8(&bytes).expect("SSE body must be UTF-8");
    assert!(
        raw.contains("endpoint") || raw.contains("mcp/rpc"),
        "SSE stream must contain endpoint event pointing to /mcp/rpc, got: {raw}"
    );
}

// ── Auth enforcement ──────────────────────────────────────────────────────────

#[tokio::test]
async fn v2p7_unauthenticated_request_rejected() {
    let base = common::spawn_app().await;

    let resp = rpc_no_auth(&base, tools_list_request()).await;

    assert_eq!(resp.status(), 200, "JSON-RPC errors still return HTTP 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    let err = &body["error"];
    assert!(err.is_object(), "response must have an 'error' field");
    assert_eq!(
        err["code"],
        velox::mcp::UNAUTHORIZED,
        "error code must be UNAUTHORIZED (-32001)"
    );
}

#[tokio::test]
async fn v2p7_sse_unauthenticated_returns_401() {
    let base = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .get(format!("{}/mcp/sse", base))
        .send()
        .await
        .expect("SSE request failed");

    assert_eq!(resp.status(), 401, "SSE without JWT must return 401");
}

#[tokio::test]
async fn v2p7_invalid_method_returns_error_response() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    let resp = rpc(
        &base,
        token,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "nonexistent/method",
            "params": {}
        }),
    )
    .await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let err = &body["error"];
    assert!(err.is_object(), "must have 'error' field");
    assert_eq!(
        err["code"],
        velox::mcp::METHOD_NOT_FOUND,
        "code must be METHOD_NOT_FOUND (-32601)"
    );
}

// ── Initialize with token in params ──────────────────────────────────────────

#[tokio::test]
async fn v2p7_initialize_accepts_token_in_params() {
    let base = common::spawn_app().await;
    let jwt = common::admin_auth_header(&base).await;
    let token = jwt.strip_prefix("Bearer ").unwrap();

    // No Authorization header — token lives inside params.token.
    let resp = rpc_no_auth(&base, initialize_request(token)).await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("error").is_none(),
        "valid token in params must succeed, got: {body}"
    );
    assert_eq!(body["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(body["result"]["serverInfo"]["name"], "velox");
}

// ── Regression ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2p7_regression_gateway_unaffected_by_mcp_server() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("gateway request failed");

    assert_eq!(resp.status(), 200, "gateway proxy must still return 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["choices"][0]["message"]["content"], "Hello!",
        "gateway must return the provider completion"
    );
}
