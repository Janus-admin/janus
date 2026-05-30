// tests/multi_model.rs
// Acceptance tests for POST /v1/chat/completions/multi.
//
// Run with: cargo test multi_model

mod common;

use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn fake_openai_response(model: &str) -> Value {
    json!({
        "id": "chatcmpl-multi-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Hello from multi!" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 8,
            "completion_tokens": 4,
            "total_tokens": 12
        }
    })
}

/// Start the app with a wiremock provider that always returns a successful response.
async fn setup() -> (String, MockServer) {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response("gpt-4o-mini")))
        .mount(&mock)
        .await;
    let base_url = common::spawn_app_with_openai_base(mock.uri()).await;
    (base_url, mock)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Two models in parallel → both return successful responses.
#[tokio::test]
async fn multi_model_parallel_both_succeed() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions/multi", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "models": ["gpt-4o", "gpt-4o-mini"],
            "messages": [{ "role": "user", "content": "Say hi" }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "multi endpoint must return 200");

    let body: Value = resp.json().await.expect("body must be JSON");
    let results = body["results"]
        .as_array()
        .expect("results must be an array");

    assert_eq!(results.len(), 2, "must have one result per model");

    for result in results {
        assert!(
            result["error"].is_null(),
            "no errors expected; got: {result:?}"
        );
        assert!(
            result["response"].is_object(),
            "each result must have a response object"
        );
        assert!(
            result["latency_ms"].is_number(),
            "each result must have latency_ms"
        );
    }
}

/// Empty `models` array returns HTTP 400.
#[tokio::test]
async fn multi_model_empty_models_returns_400() {
    let (base_url, _mock) = setup().await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions/multi", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "models": [],
            "messages": [{ "role": "user", "content": "Hi" }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        400,
        "empty models array must return 400 Bad Request"
    );

    let body: Value = resp.json().await.expect("body must be JSON");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("models"),
        "error message must mention 'models'; got: {body:?}"
    );
}

/// Missing API key returns HTTP 401.
#[tokio::test]
async fn multi_model_no_auth_returns_401() {
    let (base_url, _mock) = setup().await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions/multi", base_url))
        .json(&json!({
            "models": ["gpt-4o"],
            "messages": [{ "role": "user", "content": "Hi" }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        401,
        "missing API key must return 401 Unauthorized"
    );
}

/// Each result has the correct shape: model name, response with choices/usage, latency_ms.
#[tokio::test]
async fn multi_model_result_shape_is_correct() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions/multi", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "models": ["gpt-4o"],
            "messages": [{ "role": "user", "content": "Shape test" }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.expect("body must be JSON");
    let result = &body["results"][0];

    assert_eq!(
        result["model"].as_str().unwrap_or(""),
        "gpt-4o",
        "result.model must echo the requested model"
    );
    assert!(
        result["response"]["choices"].is_array(),
        "response must contain choices array"
    );
    assert!(
        result["response"]["usage"].is_object(),
        "response must contain usage object"
    );
    assert!(
        result["latency_ms"].as_i64().unwrap_or(-1) >= 0,
        "latency_ms must be a non-negative integer"
    );
}

/// Each model call is logged independently to the audit trail.
/// After a 2-model request the request count increases by at least 2.
#[tokio::test]
async fn multi_model_each_model_logged_separately() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();
    let admin = common::admin_auth_header(&base_url).await;

    // Record baseline request count before the multi call.
    let before: Value = client
        .get(format!("{}/admin/requests?per_page=1", base_url))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("baseline request list failed")
        .json()
        .await
        .expect("baseline response must be JSON");
    let count_before = before["meta"]["total"].as_i64().unwrap_or(0);

    // Make the multi-model call (2 models).
    let resp = client
        .post(format!("{}/v1/chat/completions/multi", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "models": ["gpt-4o", "gpt-4o-mini"],
            "messages": [{ "role": "user", "content": "Audit log test" }]
        }))
        .send()
        .await
        .expect("multi call failed");

    assert_eq!(resp.status(), 200, "multi call must succeed");

    // Allow the fire-and-forget audit writes to settle.
    tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;

    let after: Value = client
        .get(format!("{}/admin/requests?per_page=1", base_url))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("after request list failed")
        .json()
        .await
        .expect("after response must be JSON");
    let count_after = after["meta"]["total"].as_i64().unwrap_or(0);

    assert!(
        count_after >= count_before + 2,
        "request count must increase by at least 2 (one per model); before={count_before}, after={count_after}"
    );
}
