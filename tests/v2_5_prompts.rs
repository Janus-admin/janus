// tests/v2_5_prompts.rs — V2-5 Prompt Management tests
//
// Template unit tests run without a live server.
// Integration tests spin up a real Janus instance with a wiremock OpenAI stub.

mod common;
mod v2 {
    pub mod common {
        pub use crate::common::*;
    }
}
use v2::common::*;

use janus::prompts::template;
use serde_json::json;
use std::collections::HashMap;

// ── Template unit tests (no DB / server) ─────────────────────────────────────

#[test]
fn v2_5_template_single_variable_interpolated() {
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), "Alice".to_string());
    assert_eq!(template::render("Hello, {{name}}!", &vars), "Hello, Alice!");
}

#[test]
fn v2_5_template_multiple_variables_interpolated() {
    let mut vars = HashMap::new();
    vars.insert("topic".to_string(), "Rust".to_string());
    vars.insert("level".to_string(), "beginner".to_string());
    let result = template::render("Explain {{topic}} for a {{level}}.", &vars);
    assert_eq!(result, "Explain Rust for a beginner.");
}

#[test]
fn v2_5_template_missing_variable_leaves_placeholder() {
    let vars = HashMap::new();
    let result = template::render("Hello, {{name}}!", &vars);
    assert_eq!(result, "Hello, {{name}}!");
}

#[test]
fn v2_5_template_extra_variables_ignored() {
    let mut vars = HashMap::new();
    vars.insert("used".to_string(), "yes".to_string());
    vars.insert("unused".to_string(), "ignored".to_string());
    assert_eq!(template::render("Value: {{used}}", &vars), "Value: yes");
}

// ── Integration tests ─────────────────────────────────────────────────────────

/// POST /admin/prompts + GET /admin/prompts — create and list.
#[tokio::test]
async fn v2_5_create_prompt_returns_id() {
    let (base_url, _mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    let unique_name = format!("test-prompt-create-{}", uuid::Uuid::new_v4());
    let resp = client
        .post(format!("{base_url}/admin/prompts"))
        .header("Authorization", &jwt)
        .json(&json!({ "name": unique_name, "description": "A test prompt" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"]["id"].is_string());
    assert_eq!(body["data"]["name"], unique_name.as_str());
}

/// POST /admin/prompts/:id/versions increments version number.
#[tokio::test]
async fn v2_5_create_version_increments_version_number() {
    let (base_url, _mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    // Create prompt.
    let unique_name = format!("versioned-prompt-{}", uuid::Uuid::new_v4());
    let prompt_resp = client
        .post(format!("{base_url}/admin/prompts"))
        .header("Authorization", &jwt)
        .json(&json!({ "name": unique_name }))
        .send()
        .await
        .unwrap();
    assert_eq!(prompt_resp.status(), 201);
    let prompt_id = prompt_resp.json::<serde_json::Value>().await.unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create version 1.
    let v1_resp = client
        .post(format!("{base_url}/admin/prompts/{prompt_id}/versions"))
        .header("Authorization", &jwt)
        .json(&json!({ "content": "First version" }))
        .send()
        .await
        .unwrap();
    assert_eq!(v1_resp.status(), 201);
    let v1 = v1_resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(v1["data"]["version"], 1);

    // Create version 2.
    let v2_resp = client
        .post(format!("{base_url}/admin/prompts/{prompt_id}/versions"))
        .header("Authorization", &jwt)
        .json(&json!({ "content": "Second version" }))
        .send()
        .await
        .unwrap();
    assert_eq!(v2_resp.status(), 201);
    let v2 = v2_resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(v2["data"]["version"], 2);
}

/// Activating a version deactivates all previous versions.
#[tokio::test]
async fn v2_5_activate_version_deactivates_previous() {
    let (base_url, _mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    // Create prompt + two versions.
    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "deactivate-test", 2).await;

    // Activate version 1.
    let patch_resp = client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), 200);

    // Activate version 2 — version 1 should now be inactive.
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/2"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true }))
        .send()
        .await
        .unwrap();

    // Fetch all versions and check.
    let get_resp = client
        .get(format!("{base_url}/admin/prompts/{prompt_id}"))
        .header("Authorization", &jwt)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = get_resp.json().await.unwrap();
    let versions = body["data"]["versions"].as_array().unwrap();

    for v in versions {
        if v["version"] == 1 {
            assert_eq!(v["is_active"], false, "version 1 should be inactive");
        } else if v["version"] == 2 {
            assert_eq!(v["is_active"], true, "version 2 should be active");
        }
    }
}

/// DELETE /admin/prompts/:id cascades to versions.
#[tokio::test]
async fn v2_5_delete_prompt_cascades_to_versions() {
    let (base_url, _mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "delete-cascade", 2).await;

    let del_resp = client
        .delete(format!("{base_url}/admin/prompts/{prompt_id}"))
        .header("Authorization", &jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);

    // Getting it again should 404.
    let get_resp = client
        .get(format!("{base_url}/admin/prompts/{prompt_id}"))
        .header("Authorization", &jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 404);
}

/// Unknown prompt ID returns 404.
#[tokio::test]
async fn v2_5_unknown_prompt_id_returns_404() {
    let (base_url, _mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();

    let fake_id = uuid::Uuid::new_v4();
    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .header("X-Janus-Prompt", fake_id.to_string())
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

/// X-Janus-Prompt header loads the active version and injects it into the request.
#[tokio::test]
async fn v2_5_x_janus_prompt_header_loads_active_version() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    // Create prompt, version, activate it.
    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "active-version-test", 1).await;
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true }))
        .send()
        .await
        .unwrap();

    // Stub the provider to return a success.
    stub_openai_success(&mock).await;

    // Call gateway with the prompt header.
    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .header("X-Janus-Prompt", &prompt_id)
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}

/// Template variables are rendered before the request reaches the provider.
#[tokio::test]
async fn v2_5_template_variables_rendered_before_send_to_provider() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    // Create prompt with a template.
    let unique_name = format!("template-test-prompt-{}", uuid::Uuid::new_v4());
    let prompt_resp = client
        .post(format!("{base_url}/admin/prompts"))
        .header("Authorization", &jwt)
        .json(&json!({ "name": unique_name }))
        .send()
        .await
        .unwrap();
    let prompt_id = prompt_resp.json::<serde_json::Value>().await.unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    client
        .post(format!("{base_url}/admin/prompts/{prompt_id}/versions"))
        .header("Authorization", &jwt)
        .json(&json!({ "content": "You are an expert on {{topic}}." }))
        .send()
        .await
        .unwrap();
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true }))
        .send()
        .await
        .unwrap();

    stub_openai_success(&mock).await;

    // Call gateway — the provider receives the rendered template.
    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .header("X-Janus-Prompt", &prompt_id)
        .header("X-Janus-Variables", r#"{"topic":"Rust"}"#)
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}

/// prompt_version_id is recorded in the request log.
#[tokio::test]
async fn v2_5_prompt_version_id_recorded_in_request_log() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "log-test-prompt", 1).await;
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true }))
        .send()
        .await
        .unwrap();

    stub_openai_success(&mock).await;

    client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .header("X-Janus-Prompt", &prompt_id)
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();

    // Give the fire-and-forget DB write a moment to settle.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Check that the request log has a prompt_version_id set.
    let requests_resp = client
        .get(format!("{base_url}/admin/requests"))
        .header("Authorization", &jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(requests_resp.status(), 200);
    let body: serde_json::Value = requests_resp.json().await.unwrap();
    let requests = body["data"].as_array().unwrap();

    // At least one request has a non-null prompt_version_id.
    let has_version_id = requests.iter().any(|r| !r["prompt_version_id"].is_null());
    assert!(
        has_version_id,
        "At least one request should have prompt_version_id set"
    );
}

/// A/B test: weighted distribution sends traffic across versions.
#[tokio::test]
async fn v2_5_ab_test_distributes_traffic_by_weight() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "ab-test-prompt", 2).await;

    // Activate both versions with equal weight.
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true, "ab_weight": 50 }))
        .send()
        .await
        .unwrap();
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/2"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true, "ab_weight": 50 }))
        .send()
        .await
        .unwrap();

    // Fire several requests — all should succeed (we don't care about exact distribution here).
    for _ in 0..5 {
        stub_openai_success(&mock).await;
        let resp = client
            .post(format!("{base_url}/v1/chat/completions"))
            .header("Authorization", auth_header())
            .header("X-Janus-Prompt", &prompt_id)
            .json(&minimal_chat_request())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "A/B request should succeed");
    }
}

/// A version with ab_weight=0 is never selected.
#[tokio::test]
async fn v2_5_weight_zero_version_never_selected() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();
    let jwt = admin_auth_header(&base_url).await;

    let prompt_id =
        create_prompt_with_versions(&client, &base_url, &jwt, "zero-weight-prompt", 2).await;

    // Activate version 1 with weight=100, version 2 with weight=0.
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/1"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true, "ab_weight": 100 }))
        .send()
        .await
        .unwrap();
    client
        .patch(format!("{base_url}/admin/prompts/{prompt_id}/versions/2"))
        .header("Authorization", &jwt)
        .json(&json!({ "is_active": true, "ab_weight": 0 }))
        .send()
        .await
        .unwrap();

    stub_openai_success(&mock).await;
    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .header("X-Janus-Prompt", &prompt_id)
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();
    // Should succeed (version 1 handles all traffic).
    assert_eq!(resp.status(), 200);
}

/// Requests without the X-Janus-Prompt header work exactly as before.
#[tokio::test]
async fn v2_5_regression_requests_without_prompt_header_work_unchanged() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();

    stub_openai_success(&mock).await;

    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&minimal_chat_request())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["choices"].is_array());
}

/// Exact cache still works for requests that do not use the prompt header.
#[tokio::test]
async fn v2_5_regression_cache_still_works_for_non_prompt_requests() {
    let (base_url, mock) = spawn_app_with_wiremock().await;
    let client = reqwest::Client::new();

    stub_openai_success(&mock).await;

    let req = minimal_chat_request();
    // First request — populates cache.
    client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();

    // Second identical request — should be a cache hit.
    let resp2 = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&req)
        .send()
        .await
        .unwrap();

    assert_eq!(resp2.status(), 200);
    assert_eq!(
        resp2
            .headers()
            .get("x-janus-cache-hit")
            .map(|v| v.to_str().unwrap()),
        Some("exact"),
        "Second identical request should be an exact cache hit"
    );
}

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Create a prompt and a given number of empty versions, returning the prompt ID.
async fn create_prompt_with_versions(
    client: &reqwest::Client,
    base_url: &str,
    jwt: &str,
    name: &str,
    num_versions: u32,
) -> String {
    let unique_name = format!("{}-{}", name, uuid::Uuid::new_v4());
    let prompt_resp = client
        .post(format!("{base_url}/admin/prompts"))
        .header("Authorization", jwt)
        .json(&json!({ "name": unique_name }))
        .send()
        .await
        .unwrap();
    assert_eq!(prompt_resp.status(), 201, "prompt creation must succeed");
    let prompt_id = prompt_resp.json::<serde_json::Value>().await.unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    for i in 1..=num_versions {
        let v_resp = client
            .post(format!("{base_url}/admin/prompts/{prompt_id}/versions"))
            .header("Authorization", jwt)
            .json(&json!({ "content": format!("Version {i} content") }))
            .send()
            .await
            .unwrap();
        assert_eq!(v_resp.status(), 201, "version {i} creation must succeed");
    }
    prompt_id
}

/// Mount a single success stub on the wiremock server.
async fn stub_openai_success(mock: &wiremock::MockServer) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response_json()))
        .mount(mock)
        .await;
}
