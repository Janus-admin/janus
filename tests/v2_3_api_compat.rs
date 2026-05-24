// tests/v2_3_api_compat.rs
// Phase V2-3 acceptance tests — Extended API Compatibility.
//
// Run with: cargo test v2_3

mod common;

use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn fake_embedding_response() -> serde_json::Value {
    serde_json::json!({
        "object": "list",
        "data": [{
            "object": "embedding",
            "embedding": [0.1, 0.2, 0.3, 0.4, 0.5],
            "index": 0
        }],
        "model": "text-embedding-3-small",
        "usage": {
            "prompt_tokens": 5,
            "total_tokens": 5
        }
    })
}

fn fake_chat_response() -> serde_json::Value {
    common::fake_openai_response_json()
}

async fn mount_embedding_mock(mock_server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_embedding_response()))
        .mount(mock_server)
        .await;
}

async fn mount_chat_mock(mock_server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .mount(mock_server)
        .await;
}

// ── Embeddings ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2_3_embeddings_endpoint_returns_openai_format() {
    let mock_server = MockServer::start().await;
    mount_embedding_mock(&mock_server).await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "The sky is blue"
        }))
        .send()
        .await
        .expect("embeddings request failed");

    assert_eq!(resp.status(), 200, "embeddings must return 200");

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["object"], "list", "object must be 'list'");
    assert!(body["data"].is_array(), "data must be an array");
    assert!(
        !body["data"].as_array().unwrap().is_empty(),
        "data must be non-empty"
    );
    assert!(
        body["usage"]["prompt_tokens"].is_number(),
        "usage must be present"
    );
    assert!(body["model"].is_string(), "model must be present");
}

#[tokio::test]
async fn v2_3_embeddings_string_and_array_inputs_both_work() {
    let mock_server = MockServer::start().await;

    // Mount mock to handle both requests (no request limit so it handles both).
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_embedding_response()))
        .expect(2)
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    // String input
    let resp_string = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "Hello world"
        }))
        .send()
        .await
        .expect("string input request failed");
    assert_eq!(resp_string.status(), 200, "string input must return 200");

    // Array input — use unique content so it isn't a cache hit
    let resp_array = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["Foo bar baz unique array input xyz"]
        }))
        .send()
        .await
        .expect("array input request failed");
    assert_eq!(resp_array.status(), 200, "array input must return 200");
}

#[tokio::test]
async fn v2_3_embeddings_exact_cache_hit_on_identical_input() {
    let mock_server = MockServer::start().await;

    // The mock expects exactly ONE real call — the second must come from cache.
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_embedding_response()))
        .expect(1)
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "text-embedding-3-small",
        "input": "Cache hit test unique string 42"
    });

    // First request — hits provider.
    let r1 = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);

    // Second identical request — must come from cache (mock only allowed 1 call).
    let r2 = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);

    // Wiremock verifies expect(1) on drop — no assertion needed here.
}

// ── Models list ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2_3_models_endpoint_returns_active_models() {
    let base = common::spawn_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/models", base))
        .send()
        .await
        .expect("models request failed");

    assert_eq!(resp.status(), 200, "GET /v1/models must return 200");

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(body["object"], "list", "object must be 'list'");
    assert!(body["data"].is_array(), "data must be an array");

    let models = body["data"].as_array().unwrap();
    assert!(!models.is_empty(), "there must be at least one model");

    // Every entry must have the OpenAI model shape.
    for m in models {
        assert!(m["id"].is_string(), "model must have id");
        assert_eq!(m["object"], "model", "model object must be 'model'");
        assert!(
            m["created"].is_number(),
            "model must have created timestamp"
        );
        assert!(m["owned_by"].is_string(), "model must have owned_by");
    }
}

#[tokio::test]
async fn v2_3_models_endpoint_requires_no_auth() {
    let base = common::spawn_app().await;
    let client = reqwest::Client::new();

    // No Authorization header — must still succeed (matches OpenAI behaviour).
    let resp = client
        .get(format!("{}/v1/models", base))
        .send()
        .await
        .expect("unauthenticated models request failed");

    assert_eq!(
        resp.status(),
        200,
        "GET /v1/models must work without auth (OpenAI-compatible)"
    );
}

#[tokio::test]
async fn v2_3_models_list_contains_all_enabled_providers() {
    let base = common::spawn_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/v1/models", base))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let models = body["data"].as_array().unwrap();

    // The seeded model_pricing table has at least openai models.
    // Verify at least one model is owned by 'openai'.
    let has_openai = models
        .iter()
        .any(|m| m["owned_by"].as_str().unwrap_or("") == "openai");
    assert!(
        has_openai,
        "models list must contain at least one openai model"
    );
}

// ── Tool use / Function calling pass-through ──────────────────────────────────

#[tokio::test]
async fn v2_3_tool_call_fields_passed_through_to_provider() {
    let mock_server = MockServer::start().await;

    // The mock accepts the request and captures it — we verify the tools field was forwarded.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let tools = serde_json::json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the weather",
            "parameters": {
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }
        }
    }]);

    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "What is the weather?" }],
            "tools": tools,
            "tool_choice": "auto"
        }))
        .send()
        .await
        .expect("tool call request failed");

    assert_eq!(resp.status(), 200, "tool call request must succeed");

    // Verify the provider received the tools field.
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1, "provider must have been called once");
    let forwarded: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("provider request must be JSON");
    assert!(
        forwarded["tools"].is_array(),
        "tools field must have been forwarded to provider"
    );
    assert_eq!(
        forwarded["tool_choice"], "auto",
        "tool_choice must have been forwarded"
    );
}

#[tokio::test]
async fn v2_3_tool_call_included_in_exact_cache_key() {
    let mock_server = MockServer::start().await;

    // Both requests must reach the provider (different tools = different cache keys).
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .expect(2)
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let base_msg = serde_json::json!([{ "role": "user", "content": "Tool cache key test" }]);

    // Request with tool A.
    let r1 = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": base_msg,
            "tools": [{"type": "function", "function": {"name": "tool_a", "description": "a"}}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);

    // Request with tool B — must NOT hit cache (different tools).
    let r2 = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": base_msg,
            "tools": [{"type": "function", "function": {"name": "tool_b", "description": "b"}}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);

    // Wiremock verifies expect(2) — both requests reached the provider.
}

#[tokio::test]
async fn v2_3_identical_tool_call_request_returns_cache_hit() {
    let mock_server = MockServer::start().await;

    // Expect exactly 1 provider call — second must be a cache hit.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .expect(1)
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": "Unique tool cache hit content xyz789" }],
        "tools": [{"type": "function", "function": {"name": "my_tool", "description": "test"}}],
        "tool_choice": "auto"
    });

    let r1 = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);

    let r2 = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);
    assert_eq!(
        r2.headers()
            .get("x-velox-cache-hit")
            .and_then(|v| v.to_str().ok()),
        Some("exact"),
        "second identical tool-call request must be a cache hit"
    );
}

// ── Vision / Multi-modal pass-through ─────────────────────────────────────────

#[tokio::test]
async fn v2_3_image_url_content_passes_through_unchanged() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let image_content = serde_json::json!([
        { "type": "text", "text": "What is in this image?" },
        { "type": "image_url", "image_url": { "url": "https://example.com/image.png" } }
    ]);

    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": image_content }]
        }))
        .send()
        .await
        .expect("vision request failed");

    assert_eq!(resp.status(), 200, "vision request must succeed");

    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let forwarded: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    let forwarded_content = &forwarded["messages"][0]["content"];
    assert!(
        forwarded_content.is_array(),
        "multimodal content must be forwarded as array"
    );
    assert_eq!(
        forwarded_content[1]["type"], "image_url",
        "image_url type must be preserved"
    );
    assert_eq!(
        forwarded_content[1]["image_url"]["url"], "https://example.com/image.png",
        "image URL must be forwarded unchanged"
    );
}

#[tokio::test]
async fn v2_3_base64_image_content_passes_through_unchanged() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_response()))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let base64_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let image_content = serde_json::json!([
        { "type": "text", "text": "Describe this image" },
        {
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/png;base64,{}", base64_data)
            }
        }
    ]);

    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": image_content }]
        }))
        .send()
        .await
        .expect("base64 image request failed");

    assert_eq!(resp.status(), 200);

    let received = mock_server.received_requests().await.unwrap();
    let forwarded: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    let forwarded_url = forwarded["messages"][0]["content"][1]["image_url"]["url"]
        .as_str()
        .expect("image url must be a string");
    assert!(
        forwarded_url.starts_with("data:image/png;base64,"),
        "base64 image URL must be forwarded unchanged"
    );
    assert!(
        forwarded_url.contains(base64_data),
        "base64 data must be forwarded intact"
    );
}

// ── Legacy completions ────────────────────────────────────────────────────────

#[tokio::test]
async fn v2_3_completions_endpoint_accepts_legacy_format() {
    let mock_server = MockServer::start().await;
    mount_chat_mock(&mock_server).await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-3.5-turbo-instruct",
            "prompt": "Once upon a time",
            "max_tokens": 50
        }))
        .send()
        .await
        .expect("legacy completions request failed");

    assert_eq!(resp.status(), 200, "POST /v1/completions must return 200");

    let body: serde_json::Value = resp.json().await.expect("response must be JSON");
    assert_eq!(
        body["object"], "text_completion",
        "object must be 'text_completion'"
    );
    assert!(body["choices"].is_array(), "choices must be present");
    assert!(
        body["choices"][0]["text"].is_string(),
        "choice must have 'text' field (not 'message')"
    );
    assert!(
        body["usage"]["prompt_tokens"].is_number(),
        "usage must be present"
    );
}

// ── Regression ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2_3_regression_chat_completions_still_work() {
    let mock_server = MockServer::start().await;
    mount_chat_mock(&mock_server).await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("chat completions regression request failed");

    assert_eq!(resp.status(), 200, "chat completions must still return 200");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["choices"].is_array());
}

#[tokio::test]
async fn v2_3_regression_streaming_still_works() {
    let mock_server = MockServer::start().await;

    let sse_body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1716000000,\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n";

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&mock_server)
        .await;

    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "Stream regression test" }],
            "stream": true
        }))
        .send()
        .await
        .expect("streaming regression request failed");

    assert_eq!(resp.status(), 200, "streaming must still return 200");
    assert!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false),
        "streaming response must have text/event-stream content-type"
    );
}
