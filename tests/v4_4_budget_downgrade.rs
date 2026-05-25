// tests/v4_4_budget_downgrade.rs
// Phase V4-4 acceptance tests — Budget-Aware Auto-Downgrade.
//
// Run with: cargo test v4_4
//
// Most tests here are pure unit tests of `check_budget()` — no network or
// database required.  One integration test uses wiremock to verify the
// X-Janus-Downgraded header is present on a live request.

mod common;

use janus::{
    config::BudgetDowngradeConfig,
    middleware::budget::{check_budget, DowngradeDecision},
    models::api_key::ApiKey,
};
use rust_decimal::Decimal;
use std::str::FromStr;
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_key(budget_limit: Option<&str>, budget_used: &str) -> ApiKey {
    ApiKey {
        id: uuid::Uuid::new_v4(),
        name: "test".into(),
        key_hash: "".into(),
        key_sha256: None,
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "jn-sk-test".into(),
        workspace_id: None,
        budget_limit: budget_limit.map(|s| Decimal::from_str(s).unwrap()),
        budget_used: Decimal::from_str(budget_used).unwrap(),
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".into(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: chrono::Utc::now(),
        expires_at: None,
        last_used_at: None,
    }
}

fn global_disabled() -> BudgetDowngradeConfig {
    BudgetDowngradeConfig::default() // enabled = false
}

fn global_enabled_at(pct: u8, strategy: &str) -> BudgetDowngradeConfig {
    BudgetDowngradeConfig {
        enabled: true,
        threshold_percent: pct,
        strategy: strategy.to_string(),
        fallback_model: String::new(),
    }
}

fn openai_mock_response() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10}
    })
}

// ── Unit tests: check_budget() ───────────────────────────────────────────────

#[tokio::test]
async fn v4_4_downgrade_disabled_by_default() {
    // No budget_limit on key, global disabled → None.
    let key = make_key(None, "0");
    let result = check_budget(&key, &global_disabled());
    assert_eq!(result.unwrap(), DowngradeDecision::None);
}

#[tokio::test]
async fn v4_4_no_downgrade_when_under_threshold() {
    // 50% used, threshold 80% → no downgrade.
    let key = make_key(Some("1.00"), "0.50");
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(result.unwrap(), DowngradeDecision::None);
}

#[tokio::test]
async fn v4_4_strategy_downgrade_triggered_at_threshold() {
    // 85% used, threshold 80% → UseStrategy("cost_optimized").
    let key = make_key(Some("1.00"), "0.85");
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(
        result.unwrap(),
        DowngradeDecision::UseStrategy("cost_optimized".into())
    );
}

#[tokio::test]
async fn v4_4_strategy_downgrade_triggered_at_exactly_threshold() {
    // exactly 80% used, threshold 80% → triggers.
    let key = make_key(Some("1.00"), "0.80");
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(
        result.unwrap(),
        DowngradeDecision::UseStrategy("cost_optimized".into())
    );
}

#[tokio::test]
async fn v4_4_budget_block_still_fires_at_100_percent() {
    // 100% used → BudgetExceeded regardless of downgrade config.
    let key = make_key(Some("1.00"), "1.00");
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert!(result.is_err(), "must block when budget is fully consumed");
}

#[tokio::test]
async fn v4_4_budget_block_fires_above_100_percent() {
    // Over budget → also blocks.
    let key = make_key(Some("1.00"), "1.05");
    let cfg = global_disabled();
    let result = check_budget(&key, &cfg);
    assert!(result.is_err());
}

#[tokio::test]
async fn v4_4_specific_model_downgrade_overrides_strategy() {
    // Per-key downgrade_to_model set → UseModel wins over strategy.
    let mut key = make_key(Some("1.00"), "0.90");
    key.downgrade_at_percent = Some(80);
    key.downgrade_strategy = Some("specific_model".into());
    key.downgrade_to_model = Some("gpt-4o-mini".into());
    let result = check_budget(&key, &global_disabled());
    assert_eq!(
        result.unwrap(),
        DowngradeDecision::UseModel("gpt-4o-mini".into())
    );
}

#[tokio::test]
async fn v4_4_per_key_threshold_overrides_global() {
    // Per-key threshold = 90, global = 80; spend = 85% → no per-key trigger.
    let mut key = make_key(Some("1.00"), "0.85");
    key.downgrade_at_percent = Some(90);
    key.downgrade_strategy = Some("cost_optimized".into());
    // Global would trigger at 80% but per-key overrides to 90%.
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(result.unwrap(), DowngradeDecision::None);
}

#[tokio::test]
async fn v4_4_per_key_strategy_overrides_global_strategy() {
    // Per-key strategy = "latency_optimized", global = "cost_optimized".
    let mut key = make_key(Some("1.00"), "0.85");
    key.downgrade_at_percent = Some(80);
    key.downgrade_strategy = Some("latency_optimized".into());
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(
        result.unwrap(),
        DowngradeDecision::UseStrategy("latency_optimized".into())
    );
}

#[tokio::test]
async fn v4_4_no_downgrade_without_budget_limit() {
    // Key has no budget_limit — downgrade never triggers even if global enabled.
    let key = make_key(None, "999.00");
    let cfg = global_enabled_at(80, "cost_optimized");
    let result = check_budget(&key, &cfg);
    assert_eq!(result.unwrap(), DowngradeDecision::None);
}

#[tokio::test]
async fn v4_4_keys_without_downgrade_config_unaffected() {
    // Key without downgrade fields, global disabled → passes through unchanged.
    let key = make_key(Some("10.00"), "2.00");
    let result = check_budget(&key, &global_disabled());
    assert_eq!(result.unwrap(), DowngradeDecision::None);
}

// ── header_value() helper ─────────────────────────────────────────────────────

#[test]
fn v4_4_header_value_returns_strategy_name() {
    let d = DowngradeDecision::UseStrategy("cost_optimized".into());
    assert_eq!(d.header_value(), "cost_optimized");
}

#[test]
fn v4_4_header_value_returns_specific_model_for_use_model() {
    let d = DowngradeDecision::UseModel("gpt-4o-mini".into());
    assert_eq!(d.header_value(), "specific_model");
}

#[test]
fn v4_4_header_value_empty_for_none() {
    assert_eq!(DowngradeDecision::None.header_value(), "");
}

// ── Integration test: X-Janus-Downgraded header ───────────────────────────────

#[tokio::test]
async fn v4_4_downgrade_header_set_when_triggered() {
    let mock_server = wiremock::MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_mock_response()))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Key is 85% spent with an 80% threshold and "cost_optimized" downgrade.
    let base_url = common::spawn_app_with_budget_key(
        mock_server.uri(),
        Decimal::from_str("1.00").unwrap(),
        Decimal::from_str("0.85").unwrap(),
        80,
        "cost_optimized",
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let downgraded_header = resp
        .headers()
        .get("x-janus-downgraded")
        .and_then(|h| h.to_str().ok());
    assert_eq!(
        downgraded_header,
        Some("cost_optimized"),
        "X-Janus-Downgraded header must be set when downgrade triggers"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn v4_4_no_downgrade_header_when_under_threshold() {
    let mock_server = wiremock::MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_mock_response()))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Key is 50% spent — under the 80% threshold.
    let base_url = common::spawn_app_with_budget_key(
        mock_server.uri(),
        Decimal::from_str("1.00").unwrap(),
        Decimal::from_str("0.50").unwrap(),
        80,
        "cost_optimized",
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers().get("x-janus-downgraded").is_none(),
        "X-Janus-Downgraded must not be set when under threshold"
    );

    mock_server.verify().await;
}
