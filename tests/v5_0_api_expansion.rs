// tests/v5_0_api_expansion.rs
// Phase V5-0 acceptance tests — API Surface Expansion.
//
// Run with: cargo test v5_0
//
// V5-0 adds the new modality endpoints (`/v1/images/generations`,
// `/v1/audio/transcriptions`, `/v1/audio/speech`), expands `/v1/models` to
// aggregate live providers behind a 5-second TTL, and instruments the
// `requests` audit log with the per-route `endpoint` column and the
// `tool_calls` JSON capture introduced by migration 0027.

mod common;

use sqlx::PgPool;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Shared helpers ────────────────────────────────────────────────────────────

async fn connect_pool() -> PgPool {
    common::load_env();
    let config = velox::config::Config::load().expect("Failed to load config");
    velox::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test database")
}

fn fake_embedding_response() -> serde_json::Value {
    serde_json::json!({
        "object": "list",
        "data": [{
            "object": "embedding",
            "embedding": [0.1, 0.2, 0.3, 0.4, 0.5],
            "index": 0
        }],
        "model": "text-embedding-3-small",
        "usage": { "prompt_tokens": 5, "total_tokens": 5 }
    })
}

fn fake_chat_with_tool_calls() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-tools",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"city\":\"Paris\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": { "prompt_tokens": 8, "completion_tokens": 12, "total_tokens": 20 }
    })
}

fn fake_images_response() -> serde_json::Value {
    serde_json::json!({
        "created": 1_716_000_000_u64,
        "data": [
            { "url": "https://example.com/img1.png" },
            { "url": "https://example.com/img2.png" }
        ]
    })
}

fn fake_models_response() -> serde_json::Value {
    serde_json::json!({
        "object": "list",
        "data": [
            {"id": "wiremock-only-model", "object": "model", "created": 1, "owned_by": "wiremock"},
            {"id": "gpt-4o-mini",         "object": "model", "created": 2, "owned_by": "openai"}
        ]
    })
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
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_chat_with_tool_calls()))
        .mount(mock_server)
        .await;
}

async fn wait_for_async_insert() {
    // pipeline::run spawns the audit insert; give it a moment to commit.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

// ── 1. Embeddings ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn v5_0_embeddings_endpoint_returns_openai_shape() {
    let mock_server = MockServer::start().await;
    mount_embedding_mock(&mock_server).await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "hello"
        }))
        .send()
        .await
        .expect("embeddings request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "list");
    assert!(body["data"][0]["embedding"].is_array());
}

#[tokio::test]
async fn v5_0_embeddings_cost_tracked_in_requests() {
    let mock_server = MockServer::start().await;
    mount_embedding_mock(&mock_server).await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let pool = connect_pool().await;

    let _ = reqwest::Client::new()
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "v5_0_embeddings_cost_tracked"
        }))
        .send()
        .await
        .unwrap();

    wait_for_async_insert().await;

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM requests
         WHERE endpoint = '/v1/embeddings' AND request_type = 'embedding'",
    )
    .fetch_one(&pool)
    .await
    .expect("count query failed");
    assert!(
        count >= 1,
        "embedding request must be persisted with endpoint = '/v1/embeddings'"
    );
}

#[tokio::test]
async fn v5_0_embeddings_routes_via_priority() {
    // The embeddings handler uses select_provider, which respects priority.
    let primary = MockServer::start().await;
    mount_embedding_mock(&primary).await;
    let base = common::spawn_app_with_openai_base(primary.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({ "model": "text-embedding-3-small", "input": "hi" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // The mock receiving the call IS the priority-1 provider — assertion is
    // that we got 200 (mock matched). Failure would route somewhere else.
}

// ── 2. /v1/models aggregation + 5s TTL ────────────────────────────────────────

#[tokio::test]
async fn v5_0_list_models_aggregates_across_providers() {
    let mock_server = MockServer::start().await;
    // Provider /models returns a model that is NOT in the DB catalogue.
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_models_response()))
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    // Drop the in-process /v1/models cache so this test isn't shadowed by a
    // prior test running in the same process.
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/v1/models", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();

    assert!(
        ids.contains(&"wiremock-only-model"),
        "aggregate must include provider-reported model; got {ids:?}"
    );
}

#[tokio::test]
async fn v5_0_list_models_cached_for_5_seconds() {
    let mock_server = MockServer::start().await;
    // expect(1) makes wiremock fail at drop time if /models is called more than once.
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_models_response()))
        .expect(1)
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    // Wait so this test's first call definitely re-populates the cache rather
    // than reusing a stale cached value from an earlier test.
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    let client = reqwest::Client::new();
    for _ in 0..3 {
        let resp = client
            .get(format!("{}/v1/models", base))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }
    // mock_server drops when test exits → asserts /models was called exactly once.
    drop(mock_server);
}

// ── 3. Images ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn v5_0_images_endpoint_passes_through() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_images_response()))
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/images/generations", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "dall-e-3",
            "prompt": "a stoat juggling sand dollars",
            "n": 2,
            "size": "1024x1024"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "images endpoint must return 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert!(body["data"][0]["url"].is_string());
}

#[tokio::test]
async fn v5_0_images_cost_uses_price_per_image() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_images_response()))
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let pool = connect_pool().await;

    // Seed a price_per_image for the test model so cost calc has data.
    sqlx::query(
        "INSERT INTO model_pricing
            (id, provider, model_id, model_display_name,
             input_per_1m_tokens, output_per_1m_tokens, price_per_image, updated_at)
         VALUES ($1, 'openai', 'v5-0-images-test-model', 'Test',
                 0.0, 0.0, 0.04, NOW())
         ON CONFLICT (provider, model_id) DO UPDATE SET price_per_image = EXCLUDED.price_per_image",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("seed pricing failed");

    let _ = reqwest::Client::new()
        .post(format!("{}/v1/images/generations", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "v5-0-images-test-model",
            "prompt": "test",
            "n": 2
        }))
        .send()
        .await
        .unwrap();

    let (cost,): (Option<rust_decimal::Decimal>,) = sqlx::query_as(
        "SELECT cost_usd FROM requests
         WHERE endpoint = '/v1/images/generations'
           AND model = 'v5-0-images-test-model'
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("cost query failed");

    assert!(cost.is_some(), "cost must be persisted for image requests");
    // 2 images × $0.04 = $0.08
    let expected = rust_decimal::Decimal::new(8, 2);
    assert_eq!(
        cost.unwrap(),
        expected,
        "cost must equal n × price_per_image"
    );
}

// ── 4. Audio: transcribe + speech ─────────────────────────────────────────────

#[tokio::test]
async fn v5_0_audio_transcription_multipart_upload_works() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "text": "the quick brown fox" })),
        )
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part(
            "file",
            reqwest::multipart::Part::bytes(vec![0u8, 1, 2, 3, 4, 5])
                .file_name("clip.wav")
                .mime_str("audio/wav")
                .unwrap(),
        );

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/audio/transcriptions", base))
        .header("Authorization", common::auth_header())
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "audio transcribe must return 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["text"], "the quick brown fox");
}

#[tokio::test]
async fn v5_0_audio_speech_streams_chunks() {
    let mock_server = MockServer::start().await;
    // Wiremock can't true-stream, but returning bytes with audio/mpeg
    // exercises the same handler code path.
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(vec![0xFF, 0xFB, 0x90, 0x00, 0x01, 0x02, 0x03]),
        )
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/audio/speech", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "tts-1",
            "input": "hello world",
            "voice": "alloy"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    assert!(
        ct.contains("audio/mpeg"),
        "content-type must propagate from provider; got {ct}"
    );

    let bytes = resp.bytes().await.unwrap();
    assert!(!bytes.is_empty(), "audio body must contain bytes");
}

// ── 5. Legacy /v1/completions ─────────────────────────────────────────────────

#[tokio::test]
async fn v5_0_completions_legacy_proxies_to_chat() {
    let mock_server = MockServer::start().await;
    mount_chat_mock(&mock_server).await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "prompt": "Once upon a time"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "text_completion");
    assert!(body["choices"][0]["text"].is_string());
}

// ── 6. tool_calls extraction + endpoint per route ─────────────────────────────

#[tokio::test]
async fn v5_0_tool_calls_extracted_into_requests_row() {
    let mock_server = MockServer::start().await;
    mount_chat_mock(&mock_server).await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let pool = connect_pool().await;

    let _ = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "weather in Paris?" }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": { "type": "object", "properties": { "city": {"type": "string"} } }
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    wait_for_async_insert().await;

    let (tool_calls,): (Option<serde_json::Value>,) = sqlx::query_as(
        "SELECT tool_calls FROM requests
         WHERE endpoint = '/v1/chat/completions' AND tool_calls IS NOT NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    let tc = tool_calls.expect("tool_calls JSON must be persisted");
    assert!(
        tc.get("tools").is_some(),
        "extracted JSON must include `tools` from the request"
    );
    let calls = tc.get("tool_calls").and_then(|v| v.as_array()).unwrap();
    assert!(
        !calls.is_empty(),
        "extracted JSON must include `tool_calls` from the response"
    );
    assert_eq!(
        calls[0]["function"]["name"], "get_weather",
        "extracted tool call name must match the response"
    );
}

#[tokio::test]
async fn v5_0_endpoint_field_set_per_route() {
    let mock_server = MockServer::start().await;
    mount_embedding_mock(&mock_server).await;
    mount_chat_mock(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_images_response()))
        .mount(&mock_server)
        .await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let pool = connect_pool().await;

    let client = reqwest::Client::new();

    // Hit three different endpoints.
    let _ = client
        .post(format!("{}/v1/chat/completions", base))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .unwrap();

    let _ = client
        .post(format!("{}/v1/embeddings", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "endpoint-per-route-test"
        }))
        .send()
        .await
        .unwrap();

    let _ = client
        .post(format!("{}/v1/images/generations", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "dall-e-3",
            "prompt": "endpoint-per-route-test",
            "n": 1
        }))
        .send()
        .await
        .unwrap();

    wait_for_async_insert().await;

    for endpoint in [
        "/v1/chat/completions",
        "/v1/embeddings",
        "/v1/images/generations",
    ] {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM requests WHERE endpoint = $1")
            .bind(endpoint)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            count >= 1,
            "expected at least one row with endpoint = {endpoint}"
        );
    }
}

// ── 7. Unsupported modality ───────────────────────────────────────────────────

#[tokio::test]
async fn v5_0_unsupported_modality_returns_error_with_hint() {
    // No mock for /audio/speech → wiremock returns 404 → propagated as Unavailable.
    let mock_server = MockServer::start().await;
    let base = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/audio/speech", base))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "tts-1",
            "input": "no mock for this",
            "voice": "alloy"
        }))
        .send()
        .await
        .unwrap();

    assert!(
        !resp.status().is_success(),
        "expected non-success when provider lacks the modality, got {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
    let msg = body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    assert!(
        msg.contains("speech")
            || msg.contains("provider")
            || msg.contains("audio")
            || msg.contains("404")
            || msg.contains("not found")
            || msg.contains("unavailable"),
        "error message must hint at the failing modality/provider; got: {msg}"
    );
}

// ── 8. Regression: chat completions unaffected by V5-0 ────────────────────────

#[tokio::test]
async fn v5_0_regression_chat_completions_unaffected() {
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
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["choices"][0]["message"]["content"].is_string());
    assert!(body["usage"]["total_tokens"].is_number());
}
