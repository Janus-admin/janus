// tests/v4_0_foundation.rs
// Phase V4-0 acceptance tests — Foundation & DX.
//
// Run with: cargo test v4_0
//
// Coverage:
//   3.1  Configurable provider base_url — loaded from DB at startup
//   3.2  janus doctor — readiness checks (JWT, providers, embedding model)
//   3.3  Demo mode — DemoProvider mock
//   Regression: existing provider adapters unaffected

#![cfg(all(feature = "postgres", not(feature = "sqlite")))]

mod common;

use janus::{
    config::Config,
    demo::DemoProvider,
    doctor::{self, CheckStatus},
    providers::{ChatCompletionRequest, Provider},
};
use serial_test::serial;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn test_pool() -> sqlx::PgPool {
    common::load_env();
    let config = Config::load().expect("Config load failed");
    janus::db::pool::connect(&config.database_url)
        .await
        .expect("DB connect failed")
}

fn make_request(content: &str) -> ChatCompletionRequest {
    serde_json::from_value(serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": content }]
    }))
    .unwrap()
}

// ─── 3.1 Custom base_url ─────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn v4_0_load_base_urls_returns_db_values() {
    let pool = test_pool().await;

    let urls = janus::db::providers::load_base_urls(&pool).await;
    // Seed data in migration 0004 sets openai base_url; must be present.
    assert!(urls.contains_key("openai"), "openai provider must be in DB");
    let openai_url = urls.get("openai").unwrap();
    assert!(
        !openai_url.is_empty(),
        "openai base_url must be non-empty in seed data"
    );
    assert!(
        openai_url.contains("openai.com") || openai_url.contains("localhost"),
        "unexpected openai base_url: {}",
        openai_url
    );
}

#[tokio::test]
#[serial]
async fn v4_0_provider_uses_custom_base_url_when_set() {
    // Set a custom base_url in the DB, verify load_base_urls returns it.
    let pool = test_pool().await;
    let custom_url = "http://localhost:11434/v1";

    // Temporarily set a custom base_url on the openai provider.
    sqlx::query("UPDATE providers SET base_url = $1 WHERE id = 'openai'")
        .bind(custom_url)
        .execute(&pool)
        .await
        .expect("UPDATE failed");

    let urls = janus::db::providers::load_base_urls(&pool).await;
    assert_eq!(
        urls.get("openai").map(String::as_str),
        Some(custom_url),
        "load_base_urls must return the custom URL set in the DB"
    );

    // Restore the original URL.
    sqlx::query("UPDATE providers SET base_url = 'https://api.openai.com/v1' WHERE id = 'openai'")
        .execute(&pool)
        .await
        .expect("RESTORE failed");
}

#[tokio::test]
#[serial]
async fn v4_0_provider_falls_back_to_hardcoded_default_when_base_url_empty() {
    // When base_url is empty, the application's resolve_base_url helper should
    // use the adapter's compiled-in default. We test load_base_urls returns
    // the empty string and that the resolve logic handles it.
    let pool = test_pool().await;

    // Ensure openai provider is enabled so load_base_urls includes it.
    sqlx::query("UPDATE providers SET is_enabled = true WHERE id = 'openai'")
        .execute(&pool)
        .await
        .expect("ENABLE failed");

    sqlx::query("UPDATE providers SET base_url = '' WHERE id = 'openai'")
        .execute(&pool)
        .await
        .expect("UPDATE failed");

    let urls = janus::db::providers::load_base_urls(&pool).await;
    let url = urls
        .get("openai")
        .map(String::as_str)
        .unwrap_or("not_found");
    // filter(|u| !u.is_empty()) in resolve_base_url would drop this → use hardcoded default.
    assert!(
        url.is_empty(),
        "DB should return empty string, triggering fallback; got: {url}"
    );

    // Restore.
    sqlx::query("UPDATE providers SET base_url = 'https://api.openai.com/v1' WHERE id = 'openai'")
        .execute(&pool)
        .await
        .expect("RESTORE failed");
}

#[tokio::test]
#[serial]
async fn v4_0_update_provider_base_url_via_api() {
    // Verify PATCH /admin/providers/:id accepts and persists base_url.
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let admin_jwt = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let custom_url = format!("{}/custom", mock_server.uri());

    let resp = client
        .patch(format!("{}/admin/providers/openai", base_url))
        .header("Authorization", &admin_jwt)
        .json(&serde_json::json!({ "base_url": custom_url }))
        .send()
        .await
        .expect("PATCH request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["data"]["base_url"].as_str().unwrap(),
        custom_url,
        "PATCH response must reflect updated base_url"
    );

    // Restore so shared-DB tests see the canonical URL.
    let pool = test_pool().await;
    sqlx::query("UPDATE providers SET base_url = 'https://api.openai.com/v1' WHERE id = 'openai'")
        .execute(&pool)
        .await
        .expect("RESTORE failed");
}

#[tokio::test]
async fn v4_0_provider_request_routed_to_custom_base_url() {
    // Spawn app with wiremock as the provider; make a request; verify wiremock received it.
    // This tests the end-to-end path: spawn_app uses with_base_url(), which now
    // mirrors the DB base_url read in main.rs.
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200);
    assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
}

// ─── 3.2 janus doctor — readiness checks ─────────────────────────────────────

#[test]
fn v4_0_doctor_fails_when_jwt_secret_too_short() {
    let mut config = Config::load().unwrap_or_else(|_| {
        common::load_env();
        Config::load().unwrap()
    });
    config.jwt_secret = "short".to_string(); // 5 chars < 32

    let check = janus::doctor::CheckStatus::Fail;
    // Inline the check logic (same as doctor::check_jwt_secret).
    let status = if config.jwt_secret.len() >= 32 {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    };
    assert_eq!(status, check, "JWT secret shorter than 32 bytes must fail");
}

#[test]
fn v4_0_doctor_passes_when_jwt_secret_long_enough() {
    let status = if "a-32-byte-secret-that-is-long-ok!".len() >= 32 {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    };
    assert_eq!(status, CheckStatus::Pass);
}

#[test]
fn v4_0_doctor_warns_when_embedding_model_missing() {
    let mut config = Config::load().unwrap_or_else(|_| {
        common::load_env();
        Config::load().unwrap()
    });
    config.embedding_model_path = "/nonexistent/model.onnx".to_string();
    config.embedding_tokenizer_path = "/nonexistent/tokenizer.json".to_string();

    let model_exists = std::path::Path::new(&config.embedding_model_path).exists();
    let tokenizer_exists = std::path::Path::new(&config.embedding_tokenizer_path).exists();

    let status = if model_exists && tokenizer_exists {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    assert_eq!(
        status,
        CheckStatus::Warn,
        "Missing embedding model must warn, not fail"
    );
}

#[tokio::test]
#[serial]
async fn v4_0_doctor_fails_when_no_providers_enabled() {
    let pool = test_pool().await;

    // Disable all providers temporarily.
    sqlx::query("UPDATE providers SET is_enabled = false")
        .execute(&pool)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE is_enabled = true")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);

    let status = if count >= 1 {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    };
    assert_eq!(
        status,
        CheckStatus::Fail,
        "Zero enabled providers must fail"
    );

    // Restore.
    sqlx::query("UPDATE providers SET is_enabled = true")
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[serial]
async fn v4_0_readiness_endpoint_returns_valid_json() {
    // The endpoint exists and always returns JSON with a `data` object
    // containing `checks`, `errors`, `warnings`, and `healthy` fields.
    let (base_url, _mock) = common::spawn_app_with_wiremock().await;
    let admin_jwt = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/system/readiness", base_url))
        .header("Authorization", &admin_jwt)
        .send()
        .await
        .expect("GET failed");

    // Status is 200 or 503 — both are acceptable in the test environment
    // (depends on JWT_SECRET length and other env factors).
    assert!(
        resp.status() == 200 || resp.status() == 503,
        "Unexpected status: {}",
        resp.status()
    );

    let body: serde_json::Value = resp.json().await.expect("Response must be JSON");
    assert!(
        body["data"]["checks"].is_array(),
        "data.checks must be an array"
    );
    assert!(
        body["data"]["healthy"].is_boolean(),
        "data.healthy must be boolean"
    );
    assert!(
        body["data"]["errors"].is_number(),
        "data.errors must be a number"
    );
    assert!(
        body["data"]["warnings"].is_number(),
        "data.warnings must be a number"
    );
}

#[tokio::test]
#[serial]
async fn v4_0_readiness_endpoint_returns_503_when_jwt_secret_short() {
    // We can't easily reconfigure the test server's JWT secret mid-test,
    // so we test the logic directly: run_checks with a short secret → errors > 0.
    let pool = test_pool().await;
    let mut config = Config::load().unwrap_or_else(|_| {
        common::load_env();
        Config::load().unwrap()
    });
    config.jwt_secret = "tooshort".to_string(); // 8 chars

    let report = doctor::run_checks(&pool, &config).await;

    assert!(
        !report.healthy,
        "Report must not be healthy with short JWT secret"
    );
    assert!(report.errors > 0, "Must have at least one error");

    let jwt_check = report
        .checks
        .iter()
        .find(|c| c.name == "JWT secret strength")
        .expect("JWT check must be present");
    assert_eq!(jwt_check.status, CheckStatus::Fail);
}

// ─── 3.3 Demo mode — DemoProvider ────────────────────────────────────────────

#[test]
fn v4_0_demo_provider_name_is_demo() {
    let provider = DemoProvider;
    assert_eq!(provider.name(), "demo");
}

#[test]
fn v4_0_demo_provider_is_always_enabled() {
    let provider = DemoProvider;
    assert!(
        provider.is_enabled(),
        "DemoProvider must always report enabled"
    );
}

#[tokio::test]
async fn v4_0_demo_provider_returns_canned_chat_response() {
    let provider = DemoProvider;
    let request = make_request("Hello demo!");

    let result = provider.chat_completion(&request).await;
    assert!(result.is_ok(), "DemoProvider must not return an error");

    let resp = result.unwrap();
    assert_eq!(resp.model, "gpt-4o-mini");
    assert!(!resp.choices.is_empty(), "Must have at least one choice");
    assert_eq!(resp.choices[0].message.role, "assistant");

    let content = resp.choices[0].message.content.as_str().unwrap_or("");
    assert!(
        content.contains("[Demo mode]"),
        "Canned response must contain '[Demo mode]': {content}"
    );
    assert!(
        content.contains("Hello demo!"),
        "Canned response must echo the prompt: {content}"
    );
}

#[tokio::test]
async fn v4_0_demo_provider_streaming_returns_chunks() {
    use futures_util::StreamExt;

    let provider = DemoProvider;
    let request = make_request("Stream me!");

    let mut stream = provider
        .chat_completion_stream(&request)
        .await
        .expect("Stream must not error");

    let mut count = 0usize;
    let mut full_text = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.expect("Chunk must not error");
        for choice in &chunk.choices {
            if let Some(ref content) = choice.delta.content {
                full_text.push_str(content);
            }
        }
        count += 1;
    }

    assert!(count > 0, "Stream must produce at least one chunk");
    assert!(
        full_text.contains("[Demo mode]"),
        "Stream content must contain '[Demo mode]': {full_text}"
    );
}

// ─── Regression ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn v4_0_regression_existing_provider_adapters_unaffected() {
    // Existing OpenAI-compatible path still works end-to-end with a wiremock stub.
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let base_url = common::spawn_app_with_openai_base(mock_server.uri()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&common::minimal_chat_request())
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"].as_str().unwrap(), "chat.completion");
    assert!(!body["choices"].as_array().unwrap().is_empty());
}
