// tests/phase2_streaming.rs
// Phase 2 acceptance tests — Streaming.
// These tests verify that SSE token streaming works correctly.

mod common;

/// A streaming request must return Content-Type: text/event-stream.
#[tokio::test]
#[ignore = "Phase 2 not yet implemented"]
async fn phase2_streaming_response_has_correct_content_type() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("Content-Type", "application/json")
        .json(&common::minimal_streaming_request())
        .send()
        .await
        .expect("Failed to reach server");

    assert_eq!(response.status(), 200);

    let content_type = response
        .headers()
        .get("content-type")
        .expect("Response must have Content-Type header")
        .to_str()
        .unwrap();

    assert!(
        content_type.contains("text/event-stream"),
        "Streaming response must have Content-Type: text/event-stream. Got: {}",
        content_type
    );
}

/// A streaming response must end with the [DONE] sentinel.
/// This is required for OpenAI-compatible streaming.
#[tokio::test]
#[ignore = "Phase 2 not yet implemented"]
async fn phase2_streaming_response_ends_with_done_sentinel() {
    // A streaming response must contain "data: [DONE]" as the last event.
    // This is the OpenAI SSE convention — clients rely on this to know the stream ended.
    // TODO: implement full stream collection and assertion in Phase 2 dev session
}

/// Each streaming chunk must have the correct OpenAI delta format.
#[tokio::test]
#[ignore = "Phase 2 not yet implemented"]
async fn phase2_streaming_chunks_have_openai_delta_format() {
    // Each SSE event data must be JSON with:
    // { "choices": [{ "delta": { "content": "..." }, "finish_reason": null }] }
    // The last chunk must have finish_reason: "stop"
    // TODO: implement stream collection and delta format assertions in Phase 2 dev session
}

/// Streaming cost must be calculated correctly from accumulated token chunks.
#[tokio::test]
#[ignore = "Phase 2 not yet implemented"]
async fn phase2_streaming_request_cost_tracked_correctly() {
    // After a streaming response completes, the request log must contain:
    // - correct prompt_tokens
    // - correct completion_tokens (accumulated from stream)
    // - non-zero cost_usd
    // - non-null ttfb_ms (time to first byte)
    // TODO: implement DB assertions after streaming in Phase 2 dev session
}
