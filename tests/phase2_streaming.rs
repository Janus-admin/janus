// tests/phase2_streaming.rs
// Phase 2 acceptance tests — Streaming.
// All four tests use wiremock to mock the OpenAI SSE endpoint; zero real API calls.

mod common;

use sqlx::Row;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── SSE test helpers ──────────────────────────────────────────────────────────

/// Builds a minimal but complete OpenAI-format SSE response body.
///
/// Events emitted (in order):
///   1. Role delta  — announces `role: "assistant"`, `content: ""`
///   2. Content "Hi"
///   3. Content "!"
///   4. Stop chunk  — empty delta, `finish_reason: "stop"`
///   5. Usage chunk — `prompt_tokens: 10, completion_tokens: 3` (no choices)
///   6. `[DONE]` sentinel
///
/// Using model `"gpt-4o-mini"` so the pricing lookup in `model_pricing` succeeds.
fn openai_sse_body() -> String {
    let chunks: &[serde_json::Value] = &[
        serde_json::json!({
            "id": "chatcmpl-teststream",
            "object": "chat.completion.chunk",
            "created": 1_716_000_000_u64,
            "model": "gpt-4o-mini",
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]
        }),
        serde_json::json!({
            "id": "chatcmpl-teststream",
            "object": "chat.completion.chunk",
            "created": 1_716_000_000_u64,
            "model": "gpt-4o-mini",
            "choices": [{"index": 0, "delta": {"content": "Hi"}, "finish_reason": null}]
        }),
        serde_json::json!({
            "id": "chatcmpl-teststream",
            "object": "chat.completion.chunk",
            "created": 1_716_000_000_u64,
            "model": "gpt-4o-mini",
            "choices": [{"index": 0, "delta": {"content": "!"}, "finish_reason": null}]
        }),
        serde_json::json!({
            "id": "chatcmpl-teststream",
            "object": "chat.completion.chunk",
            "created": 1_716_000_000_u64,
            "model": "gpt-4o-mini",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        }),
        // Usage chunk: carries token counts, no choices. The pipeline reads
        // prompt_tokens / completion_tokens from here and writes them to the DB.
        serde_json::json!({
            "id": "chatcmpl-teststream",
            "object": "chat.completion.chunk",
            "created": 1_716_000_000_u64,
            "model": "gpt-4o-mini",
            "choices": [],
            "usage": {"prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13}
        }),
    ];

    let mut body = String::new();
    for chunk in chunks {
        body.push_str("data: ");
        body.push_str(&chunk.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// Mount the SSE mock on `mock_server`.
/// Intercepts `POST /chat/completions` (the path the OpenAI provider appends to base_url).
async fn mount_sse_mock(mock_server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            // set_body_raw sets both the response body and the Content-Type header.
            ResponseTemplate::new(200).set_body_raw(openai_sse_body(), "text/event-stream"),
        )
        .mount(mock_server)
        .await;
}

/// Collect every `data:` value from a raw SSE response body.
/// Handles both `data: value` (OpenAI convention) and `data:value` forms.
/// Returns values in document order, including the final `"[DONE]"` entry.
fn parse_sse_data_lines(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            line.strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A streaming request must return Content-Type: text/event-stream.
#[tokio::test]
async fn phase2_streaming_response_has_correct_content_type() {
    let mock_server = MockServer::start().await;
    mount_sse_mock(&mock_server).await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_streaming_request())
        .send()
        .await
        .expect("request must reach server");

    assert_eq!(response.status(), 200);

    let content_type = response
        .headers()
        .get("content-type")
        .expect("response must have Content-Type header")
        .to_str()
        .unwrap();

    assert!(
        content_type.contains("text/event-stream"),
        "streaming response must have Content-Type: text/event-stream; got: {content_type}"
    );
}

/// A streaming response must end with the `[DONE]` sentinel.
/// Clients depend on this to know the stream has finished.
#[tokio::test]
async fn phase2_streaming_response_ends_with_done_sentinel() {
    let mock_server = MockServer::start().await;
    mount_sse_mock(&mock_server).await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_streaming_request())
        .send()
        .await
        .expect("request must reach server");

    assert_eq!(response.status(), 200);

    let body = response.text().await.expect("body must be readable text");
    let data_lines = parse_sse_data_lines(&body);

    assert!(
        !data_lines.is_empty(),
        "SSE response must contain at least one data event"
    );
    assert_eq!(
        data_lines.last().map(String::as_str),
        Some("[DONE]"),
        "last SSE event must be [DONE]; all events: {data_lines:?}"
    );
}

/// Every streaming chunk must carry the OpenAI `chat.completion.chunk` format:
///   - `object == "chat.completion.chunk"`
///   - `choices` array present
///   - first chunk announces `role: "assistant"`
///   - at least one chunk carries a `delta.content` string
///   - exactly one chunk has `finish_reason: "stop"`
#[tokio::test]
async fn phase2_streaming_chunks_have_openai_delta_format() {
    let mock_server = MockServer::start().await;
    mount_sse_mock(&mock_server).await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_streaming_request())
        .send()
        .await
        .expect("request must reach server");

    let body = response.text().await.expect("body must be readable text");
    let data_lines = parse_sse_data_lines(&body);

    // Parse every non-[DONE] data line as JSON.
    let chunks: Vec<serde_json::Value> = data_lines
        .iter()
        .filter(|d| d.as_str() != "[DONE]")
        .map(|d| {
            serde_json::from_str(d)
                .unwrap_or_else(|e| panic!("non-DONE data line must be valid JSON: {e}\ndata: {d}"))
        })
        .collect();

    assert!(!chunks.is_empty(), "must receive at least one chunk");

    // Structural check on every chunk.
    for chunk in &chunks {
        assert_eq!(
            chunk["object"], "chat.completion.chunk",
            "object must be chat.completion.chunk; chunk: {chunk}"
        );
        assert!(
            chunk["id"].is_string(),
            "id must be a string; chunk: {chunk}"
        );
        assert!(
            chunk["choices"].is_array(),
            "choices must be an array; chunk: {chunk}"
        );
    }

    // First chunk must announce the assistant role.
    let first = &chunks[0];
    assert_eq!(
        first["choices"][0]["delta"]["role"], "assistant",
        "first chunk must have delta.role == \"assistant\"; chunk: {first}"
    );

    // At least one chunk must carry a non-null content string.
    let has_content = chunks.iter().any(|c| {
        c["choices"]
            .as_array()
            .and_then(|a| a.first())
            .map(|choice| choice["delta"]["content"].is_string())
            .unwrap_or(false)
    });
    assert!(has_content, "at least one chunk must carry delta.content");

    // Exactly one chunk must have finish_reason: "stop".
    let stop_count = chunks
        .iter()
        .filter(|c| {
            c["choices"]
                .as_array()
                .and_then(|a| a.first())
                .map(|choice| choice["finish_reason"] == "stop")
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        stop_count, 1,
        "exactly one chunk must have finish_reason: stop; all chunks: {chunks:?}"
    );
}

/// After a streaming response completes the `requests` table must contain a row with:
///   - `prompt_tokens  == 10`  (from mock usage chunk)
///   - `completion_tokens == 3`  (from mock usage chunk)
///   - `cost_usd > 0`  (gpt-4o-mini pricing is seeded in model_pricing)
///   - `ttfb_ms IS NOT NULL`  (first-byte time recorded by the pipeline)
#[tokio::test]
async fn phase2_streaming_request_cost_tracked_correctly() {
    let mock_server = MockServer::start().await;
    mount_sse_mock(&mock_server).await;

    // Build a direct DB connection to query the audit log.
    // Uses the same connection string the app uses; migrations are already applied.
    common::load_env();
    let config = velox::config::Config::load().expect("config must load");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.database_url)
        .await
        .expect("must connect to test DB");

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;

    // Timestamp before the request so we can pinpoint our row.
    let before_ts = chrono::Utc::now();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_streaming_request())
        .send()
        .await
        .expect("request must reach server");

    // Drain the full response so the server-side SSE stream is guaranteed complete.
    response
        .bytes()
        .await
        .expect("response body must be readable");

    // The pipeline fires two nested tokio::spawn calls after the stream closes —
    // one to collect final state, one to write to the DB. 500 ms is ample time
    // for both to finish against a local Postgres instance.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let row = sqlx::query(
        "SELECT prompt_tokens, completion_tokens, cost_usd, ttfb_ms
         FROM   requests
         WHERE  stream = TRUE AND created_at > $1
         ORDER  BY created_at DESC
         LIMIT  1",
    )
    .bind(before_ts)
    .fetch_one(&pool)
    .await
    .expect("streaming request must appear in the DB within 500 ms");

    let prompt_tokens: Option<i32> = row.try_get("prompt_tokens").unwrap();
    let completion_tokens: Option<i32> = row.try_get("completion_tokens").unwrap();
    let cost_usd: Option<rust_decimal::Decimal> = row.try_get("cost_usd").unwrap();
    let ttfb_ms: Option<i32> = row.try_get("ttfb_ms").unwrap();

    assert_eq!(
        prompt_tokens,
        Some(10),
        "prompt_tokens must match mock usage"
    );
    assert_eq!(
        completion_tokens,
        Some(3),
        "completion_tokens must match mock usage"
    );
    assert!(
        cost_usd.map_or(false, |c| c > rust_decimal::Decimal::ZERO),
        "cost_usd must be > 0 — gpt-4o-mini is priced in model_pricing; got {cost_usd:?}"
    );
    assert!(
        ttfb_ms.is_some(),
        "ttfb_ms must be non-null for streaming requests"
    );
}
