// tests/v2_2_webhooks.rs
// Phase V2-2 acceptance tests — Webhook Alerts.
//
// Run with: cargo test v2_2
#![cfg(all(feature = "postgres", not(feature = "sqlite")))]

mod common;

use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;
use uuid::Uuid;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

async fn test_pool() -> sqlx::PgPool {
    common::load_env();
    let config = velox::config::Config::load().expect("config load failed");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&config.database_url)
        .await
        .expect("db connect failed")
}

async fn default_workspace(pool: &sqlx::PgPool) -> Uuid {
    let row: (Uuid,) = sqlx::query_as("SELECT id FROM workspaces LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("need at least one workspace");
    row.0
}

// ─── Webhook delivery unit tests ──────────────────────────────────────────────

#[tokio::test]
async fn v2_2_slack_payload_format_is_valid_json() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let ctx = velox::alerts::webhook::WebhookContext {
        alert_id: Uuid::new_v4(),
        alert_type: "spend_threshold",
        alert_name: "Test Alert",
        message: "Threshold exceeded",
        value: 45.20,
        threshold: 40.00,
        triggered_at: Utc::now(),
    };
    let format = velox::alerts::webhook::WebhookFormat::Slack;
    velox::alerts::webhook::deliver(&client, &mock_server.uri(), &format, None, &ctx)
        .await
        .expect("slack delivery should succeed");

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("body must be valid JSON");
    assert!(
        body.get("text").is_some(),
        "Slack payload must have 'text' field"
    );
    let text = body["text"].as_str().unwrap();
    assert!(
        text.contains("spend_threshold"),
        "Slack text must mention alert type"
    );
}

#[tokio::test]
async fn v2_2_discord_payload_format_is_valid_json() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let ctx = velox::alerts::webhook::WebhookContext {
        alert_id: Uuid::new_v4(),
        alert_type: "error_rate",
        alert_name: "Error Rate Alert",
        message: "High error rate",
        value: 0.25,
        threshold: 0.10,
        triggered_at: Utc::now(),
    };
    let format = velox::alerts::webhook::WebhookFormat::Discord;
    velox::alerts::webhook::deliver(&client, &mock_server.uri(), &format, None, &ctx)
        .await
        .expect("discord delivery should succeed");

    let reqs = mock_server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert!(
        body.get("content").is_some(),
        "Discord payload must have 'content' field"
    );
}

#[tokio::test]
async fn v2_2_generic_payload_contains_all_required_fields() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let alert_id = Uuid::new_v4();
    let client = reqwest::Client::new();
    let ctx = velox::alerts::webhook::WebhookContext {
        alert_id,
        alert_type: "latency_spike",
        alert_name: "Latency Alert",
        message: "High latency detected",
        value: 5000.0,
        threshold: 3000.0,
        triggered_at: Utc::now(),
    };
    let format = velox::alerts::webhook::WebhookFormat::Generic;
    velox::alerts::webhook::deliver(&client, &mock_server.uri(), &format, None, &ctx)
        .await
        .expect("generic delivery should succeed");

    let reqs = mock_server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["alert_id"].as_str().unwrap(), alert_id.to_string());
    assert_eq!(body["type"].as_str().unwrap(), "latency_spike");
    assert!(body.get("message").is_some());
    assert!(body.get("value").is_some());
    assert!(body.get("threshold").is_some());
    assert!(body.get("triggered_at").is_some());
}

#[tokio::test]
async fn v2_2_webhook_secret_hmac_header_added_when_configured() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let secret = "super-secret-key";
    let client = reqwest::Client::new();
    let ctx = velox::alerts::webhook::WebhookContext {
        alert_id: Uuid::new_v4(),
        alert_type: "spend_threshold",
        alert_name: "Spend Alert",
        message: "Spend exceeded",
        value: 100.0,
        threshold: 80.0,
        triggered_at: Utc::now(),
    };
    velox::alerts::webhook::deliver(
        &client,
        &mock_server.uri(),
        &velox::alerts::webhook::WebhookFormat::Generic,
        Some(secret),
        &ctx,
    )
    .await
    .expect("delivery with secret should succeed");

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);

    let sig_header = reqs[0]
        .headers
        .get("x-velox-signature")
        .expect("X-Velox-Signature header must be present");
    let sig = sig_header.to_str().expect("header is utf8");

    // Independently compute expected signature.
    let body_str = String::from_utf8(reqs[0].body.to_vec()).unwrap();
    let expected_sig = velox::alerts::webhook::sign(secret, &body_str);
    assert_eq!(sig, expected_sig, "HMAC signature must match");
}

// ─── AlertEngine integration tests ────────────────────────────────────────────

#[tokio::test]
async fn v2_2_spend_threshold_fires_when_budget_exceeded() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, created_at)
         VALUES ($1,$2,'Spend Alert','spend_threshold',0.000001,60,TRUE,$3,'generic',NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.001,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.expect("evaluate should not error");

    let reqs = mock_server.received_requests().await.unwrap();
    assert!(
        !reqs.is_empty(),
        "webhook must have been called when threshold exceeded"
    );

    let last_triggered: (Option<chrono::DateTime<Utc>>,) =
        sqlx::query_as("SELECT last_triggered FROM alerts WHERE id = $1")
            .bind(alert_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(last_triggered.0.is_some(), "last_triggered must be set");

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_webhook_post_reaches_configured_url() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, created_at)
         VALUES ($1,$2,'Reach Test','spend_threshold',0.000001,60,TRUE,$3,'generic',NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap();

    assert!(
        mock_server.received_requests().await.unwrap().len() >= 1,
        "webhook endpoint must be reached"
    );

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_alert_does_not_fire_twice_within_cooldown_window() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    // Set last_triggered to now so cooldown is active.
    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, last_triggered, created_at)
         VALUES ($1,$2,'Cooldown Test','spend_threshold',0.000001,60,TRUE,$3,'generic',NOW(),NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap();

    let reqs = mock_server.received_requests().await.unwrap_or_default();
    assert_eq!(reqs.len(), 0, "alert within cooldown must not fire again");

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_alert_fires_again_after_cooldown_expires() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    // Set last_triggered to 2 hours ago (well past the 60-min window).
    let two_hours_ago = Utc::now() - chrono::Duration::hours(2);
    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, last_triggered, created_at)
         VALUES ($1,$2,'Cooldown Expired','spend_threshold',0.000001,60,TRUE,$3,'generic',$4,NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .bind(two_hours_ago)
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap();

    let reqs = mock_server.received_requests().await.unwrap_or_default();
    assert!(!reqs.is_empty(), "alert with expired cooldown must fire");

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_inactive_alert_does_not_fire() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, created_at)
         VALUES ($1,$2,'Inactive Alert','spend_threshold',0.000001,60,FALSE,$3,'generic',NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap();

    assert_eq!(
        mock_server
            .received_requests()
            .await
            .unwrap_or_default()
            .len(),
        0,
        "inactive alert must not fire"
    );

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_alert_history_recorded_after_firing() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, created_at)
         VALUES ($1,$2,'History Test','spend_threshold',0.000001,60,TRUE,$3,'generic',NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(mock_server.uri())
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap();

    let history_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM alert_history WHERE alert_id = $1")
            .bind(alert_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        history_count.0 >= 1,
        "at least one history entry must be recorded"
    );

    let row: (bool, Option<String>) =
        sqlx::query_as("SELECT delivered, error FROM alert_history WHERE alert_id = $1")
            .bind(alert_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(row.0, "history row must be marked delivered");
    assert!(row.1.is_none(), "no error on successful delivery");

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_failed_delivery_recorded_with_error() {
    let pool = test_pool().await;
    let ws_id = default_workspace(&pool).await;

    // Point to a non-existent URL to force delivery failure.
    let bad_url = "http://127.0.0.1:19999/nonexistent";
    let alert_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO alerts (id, workspace_id, name, type, threshold, window_minutes,
                             is_active, webhook_url, webhook_format, created_at)
         VALUES ($1,$2,'Fail Test','spend_threshold',0.000001,60,TRUE,$3,'generic',NOW())",
    )
    .bind(alert_id)
    .bind(ws_id)
    .bind(bad_url)
    .execute(&pool)
    .await
    .unwrap();

    let req_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO requests (id, provider, model, status, cost_usd, created_at)
         VALUES ($1,'openai','gpt-4o-mini','success',0.01,NOW())",
    )
    .bind(req_id)
    .execute(&pool)
    .await
    .unwrap();

    let engine = velox::alerts::AlertEngine::new(pool.clone());
    engine.evaluate().await.unwrap(); // engine must not propagate delivery errors

    let row: (bool, Option<String>) =
        sqlx::query_as("SELECT delivered, error FROM alert_history WHERE alert_id = $1")
            .bind(alert_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!row.0, "delivered must be false on failed delivery");
    assert!(row.1.is_some(), "error message must be recorded");

    sqlx::query("DELETE FROM requests WHERE id = $1")
        .bind(req_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(alert_id)
        .execute(&pool)
        .await
        .ok();
}

// ─── CRUD tests via HTTP API ──────────────────────────────────────────────────

#[tokio::test]
async fn v2_2_create_alert_returns_id() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/admin/alerts", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "name": "Test Create Alert",
            "type": "spend_threshold",
            "threshold": 100.0,
            "window_minutes": 60
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["data"]["id"].is_string(),
        "response must contain alert id"
    );
    assert_eq!(body["data"]["type"].as_str().unwrap(), "spend_threshold");

    // Cleanup
    let id = body["data"]["id"].as_str().unwrap();
    let pool = test_pool().await;
    sqlx::query("DELETE FROM alerts WHERE id = $1::uuid")
        .bind(id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_update_alert_changes_threshold() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/admin/alerts", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "name": "Update Threshold Test",
            "type": "spend_threshold",
            "threshold": 50.0
        }))
        .send()
        .await
        .unwrap();
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let id = created["data"]["id"].as_str().unwrap().to_string();

    let patch_resp = client
        .patch(format!("{}/admin/alerts/{}", base_url, id))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({ "threshold": 75.0 }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), 200);
    let patched: serde_json::Value = patch_resp.json().await.unwrap();
    let new_threshold = patched["data"]["threshold"].as_f64().unwrap();
    assert!(
        (new_threshold - 75.0).abs() < 0.001,
        "threshold must be updated to 75"
    );

    let pool = test_pool().await;
    sqlx::query("DELETE FROM alerts WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
async fn v2_2_delete_alert_removes_it() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/admin/alerts", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "name": "Delete Me",
            "type": "error_rate",
            "threshold": 0.5
        }))
        .send()
        .await
        .unwrap();
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let id = created["data"]["id"].as_str().unwrap().to_string();

    let del_resp = client
        .delete(format!("{}/admin/alerts/{}", base_url, id))
        .header("Authorization", common::auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 200);

    let get_resp = client
        .get(format!("{}/admin/alerts/{}", base_url, id))
        .header("Authorization", common::auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 404, "deleted alert must return 404");
}

#[tokio::test]
async fn v2_2_test_endpoint_delivers_webhook_regardless_of_threshold() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/admin/alerts", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "name": "Test Endpoint Alert",
            "type": "spend_threshold",
            "threshold": 99999.0,    // very high — would never fire naturally
            "webhook_url": mock_server.uri(),
            "webhook_format": "generic"
        }))
        .send()
        .await
        .unwrap();
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let id = created["data"]["id"].as_str().unwrap().to_string();

    let test_resp = client
        .post(format!("{}/admin/alerts/{}/test", base_url, id))
        .header("Authorization", common::auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(test_resp.status(), 200);

    assert!(
        mock_server.received_requests().await.unwrap().len() >= 1,
        "test endpoint must have triggered webhook delivery"
    );

    let pool = test_pool().await;
    sqlx::query("DELETE FROM alerts WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .ok();
}

// ─── Regression ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn v2_2_regression_gateway_proxy_unaffected() {
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
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "gateway proxy must still work after V2-2"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"].as_str().unwrap(), "chat.completion");
}
