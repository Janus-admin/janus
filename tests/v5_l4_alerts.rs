// tests/v5_l4_alerts.rs
// V5-L4 acceptance tests — Polished Alerts (Slack + Email).
//
// Run with: cargo test v5_l4

mod common;

use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn create_alert(base_url: &str, body: Value) -> Value {
    let client = reqwest::Client::new();
    let admin = common::admin_auth_header(base_url).await;
    let resp = client
        .post(format!("{}/admin/alerts", base_url))
        .header("Authorization", &admin)
        .json(&body)
        .send()
        .await
        .expect("create alert request failed");
    assert_eq!(resp.status(), 201, "create alert must return 201");
    let body: Value = resp
        .json()
        .await
        .expect("create alert response must be JSON");
    body["data"].clone()
}

async fn test_alert(base_url: &str, alert_id: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    let admin = common::admin_auth_header(base_url).await;
    client
        .post(format!("{}/admin/alerts/{}/test", base_url, alert_id))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("test alert request failed")
}

// ── Test 1: Slack block payload ───────────────────────────────────────────────

/// The test endpoint sends a Slack block-kit payload with a `blocks` array
/// (not just a plain `text` field) when `slack_webhook_url` is configured.
#[tokio::test]
async fn v5_l4_slack_webhook_sends_block_payload() {
    // Mock Slack incoming-webhook endpoint.
    let slack_mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&slack_mock)
        .await;

    let base_url = common::spawn_app().await;

    let alert = create_alert(
        &base_url,
        json!({
            "name": "Budget Alert",
            "type": "spend_threshold",
            "threshold": 100.0,
            "slack_webhook_url": slack_mock.uri()
        }),
    )
    .await;

    let alert_id = alert["id"].as_str().expect("alert id must be a string");

    let resp = test_alert(&base_url, alert_id).await;
    assert_eq!(resp.status(), 200, "test delivery must succeed");

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["delivered"], json!(true));

    // Verify the Slack mock received exactly one request.
    slack_mock.verify().await;

    // Retrieve the captured request body and confirm block-kit format.
    let reqs = slack_mock.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let payload: Value = serde_json::from_slice(&reqs[0].body).expect("Slack body must be JSON");
    assert!(
        payload.get("blocks").is_some(),
        "Slack payload must contain 'blocks' key (block-kit format), got: {payload}"
    );
    let blocks = payload["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty(), "Slack blocks array must be non-empty");
    // First block must be a header.
    assert_eq!(blocks[0]["type"], json!("header"));
}

// ── Test 2: Email file transport ──────────────────────────────────────────────

/// When `smtp.file_dir` is set, the email dispatcher writes a .eml file for
/// each recipient instead of sending over SMTP. This test verifies the file
/// is created and contains expected content.
#[tokio::test]
async fn v5_l4_email_sends_via_smtp() {
    let tmp = tempfile::TempDir::new().expect("tempdir creation failed");
    let dir = tmp.path().to_str().unwrap().to_string();

    let base_url = common::spawn_app_with_smtp_file_dir(&dir).await;

    let alert = create_alert(
        &base_url,
        json!({
            "name": "Error Rate Alert",
            "type": "error_rate",
            "threshold": 0.05,
            "email_to": ["oncall@acme.com"]
        }),
    )
    .await;

    let alert_id = alert["id"].as_str().unwrap();

    let resp = test_alert(&base_url, alert_id).await;
    assert_eq!(
        resp.status(),
        200,
        "test delivery must succeed when smtp.file_dir is set"
    );

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["delivered"], json!(true));

    // Verify that at least one .eml file was written to the temp directory.
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("temp dir must be readable")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "eml"))
        .collect();

    assert!(
        !entries.is_empty(),
        "at least one .eml file must be written to smtp.file_dir"
    );

    // Read the first .eml and check it contains the alert name.
    let content = std::fs::read_to_string(entries[0].path()).expect("eml file must be readable");
    assert!(
        content.contains("Error Rate Alert"),
        "eml file must reference the alert name"
    );
}

// ── Test 3: Both channels dispatched ─────────────────────────────────────────

/// When an alert has both `slack_webhook_url` and `email_to` configured,
/// the test endpoint dispatches to both and returns delivered=true.
#[tokio::test]
async fn v5_l4_alert_dispatches_to_both_channels_when_configured() {
    let slack_mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&slack_mock)
        .await;

    let tmp = tempfile::TempDir::new().expect("tempdir creation failed");
    let dir = tmp.path().to_str().unwrap().to_string();

    let base_url = common::spawn_app_with_smtp_file_dir(&dir).await;

    let alert = create_alert(
        &base_url,
        json!({
            "name": "Latency Alert",
            "type": "latency_spike",
            "threshold": 5000.0,
            "slack_webhook_url": slack_mock.uri(),
            "email_to": ["sre@acme.com"]
        }),
    )
    .await;

    let alert_id = alert["id"].as_str().unwrap();

    let resp = test_alert(&base_url, alert_id).await;
    assert_eq!(resp.status(), 200, "both-channel delivery must succeed");

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["delivered"], json!(true));

    // Verify Slack received its request.
    slack_mock.verify().await;

    // Verify email file was written.
    let eml_count = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "eml"))
        .count();
    assert!(eml_count >= 1, "at least one .eml must be written");
}

// ── Test 4: Missing channel falls back gracefully ─────────────────────────────

/// An alert with no delivery channels (no webhook_url, no slack_webhook_url,
/// no email_to) returns 400 with a descriptive error — it does not panic
/// or return 500.
#[tokio::test]
async fn v5_l4_missing_channel_config_falls_back_gracefully() {
    let base_url = common::spawn_app().await;

    let alert = create_alert(
        &base_url,
        json!({
            "name": "No Channel Alert",
            "type": "spend_threshold",
            "threshold": 50.0
        }),
    )
    .await;

    let alert_id = alert["id"].as_str().unwrap();

    let resp = test_alert(&base_url, alert_id).await;
    assert_eq!(
        resp.status(),
        400,
        "test endpoint must return 400 when no channels are configured"
    );

    let body: Value = resp.json().await.unwrap();
    let msg = body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    assert!(
        msg.contains("channel") || msg.contains("webhook") || msg.contains("email"),
        "error message must mention channels, got: {msg}"
    );
}

// ── Test 5: Test endpoint round-trip ─────────────────────────────────────────

/// Full round-trip: create alert with slack_webhook_url, POST /test, verify
/// the response is `{"data":{"delivered":true}}` and history is recorded.
#[tokio::test]
async fn v5_l4_test_endpoint_sends_sample_payload() {
    let slack_mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&slack_mock)
        .await;

    let base_url = common::spawn_app().await;
    let admin = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let alert = create_alert(
        &base_url,
        json!({
            "name": "Spend Limit",
            "type": "spend_threshold",
            "threshold": 200.0,
            "slack_webhook_url": slack_mock.uri()
        }),
    )
    .await;

    let alert_id = alert["id"].as_str().unwrap();

    // Deliver the test payload.
    let test_resp = test_alert(&base_url, alert_id).await;
    assert_eq!(test_resp.status(), 200);
    let test_body: Value = test_resp.json().await.unwrap();
    assert_eq!(test_body["data"]["delivered"], json!(true));

    // Confirm history was recorded via GET /admin/alerts/:id.
    let history_resp = client
        .get(format!("{}/admin/alerts/{}", base_url, alert_id))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("get alert request failed");
    assert_eq!(history_resp.status(), 200);

    let history_body: Value = history_resp.json().await.unwrap();
    let history = history_body["data"]["history"]
        .as_array()
        .expect("history must be an array");
    assert!(
        !history.is_empty(),
        "history must contain at least one entry after test delivery"
    );

    let latest = &history[0];
    assert_eq!(
        latest["delivered"],
        json!(true),
        "latest history entry must show delivered=true"
    );
    assert_eq!(
        latest["message"],
        json!("Test delivery"),
        "latest history entry must have message='Test delivery'"
    );
}
