// tests/v4_5_cost_sim.rs
// Phase V4-5 acceptance tests — Cost Simulator + Provider Quality Scoring.
//
// Run with: cargo test v4_5
//
// Quality-score pure unit tests live in src/analytics/quality_score.rs.
// This file covers HTTP integration tests for the simulate endpoint and the
// quality_score field on the providers list.

mod common;

use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// OpenAI-format response for gpt-4o with 100 prompt + 50 completion tokens.
fn gpt4o_mock_response() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-sim-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Simulated response" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150 }
    })
}

// ── Simulate: empty period ────────────────────────────────────────────────────

/// GET /admin/analytics/simulate responds with the expected JSON shape,
/// including all required top-level fields, regardless of request volume.
#[tokio::test]
async fn v4_5_simulate_empty_period_returns_zero() {
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let _mock_server = mock_server; // keep alive
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{}/admin/analytics/simulate?strategy=cost_optimized&period=7d",
            base_url
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("simulate request failed");

    assert_eq!(resp.status(), 200, "simulate must return 200");

    let body: serde_json::Value = resp.json().await.expect("response must be JSON");
    let data = &body["data"];

    assert_eq!(data["strategy"], "cost_optimized");
    assert_eq!(data["period"], "7d");
    assert!(
        data["original_cost_usd"].is_number(),
        "original_cost_usd must be a number"
    );
    assert!(
        data["simulated_cost_usd"].is_number(),
        "simulated_cost_usd must be a number"
    );
    assert!(
        data["savings_usd"].is_number(),
        "savings_usd must be a number"
    );
    assert!(
        data["savings_percent"].is_number(),
        "savings_percent must be a number"
    );
    assert!(
        data["request_count"].is_number(),
        "request_count must be a number"
    );
    assert!(data["by_model"].is_array(), "by_model must be an array");

    // Costs must be non-negative.
    assert!(
        data["original_cost_usd"].as_f64().unwrap() >= 0.0,
        "original_cost_usd must be >= 0"
    );
    assert!(
        data["simulated_cost_usd"].as_f64().unwrap() >= 0.0,
        "simulated_cost_usd must be >= 0"
    );
}

// ── Simulate: priority strategy ───────────────────────────────────────────────

/// priority strategy must always return simulated_cost_usd == original_cost_usd.
#[tokio::test]
async fn v4_5_simulate_priority_returns_same_cost_as_actual() {
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Register a wiremock response so the gateway request succeeds.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gpt4o_mock_response()))
        .mount(&mock_server)
        .await;

    // Make a gateway request so there is some data.
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Cache", "false")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "priority strategy test unique v4_5" }]
        }))
        .send()
        .await
        .expect("gateway request failed");

    let resp = client
        .get(format!(
            "{}/admin/analytics/simulate?strategy=priority&period=30d",
            base_url
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("simulate request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let data = &body["data"];

    let original = data["original_cost_usd"].as_f64().unwrap();
    let simulated = data["simulated_cost_usd"].as_f64().unwrap();

    assert_eq!(
        original, simulated,
        "priority strategy: simulated cost must equal original cost"
    );
    assert_eq!(
        data["savings_usd"].as_f64().unwrap(),
        0.0,
        "priority strategy savings must be zero"
    );
}

// ── Simulate: cost_optimized with model override ──────────────────────────────

/// cost_optimized + model_overrides={"gpt-4o":"gpt-4o-mini"} must show positive savings
/// since gpt-4o ($5/$15 per 1M) >> gpt-4o-mini ($0.15/$0.60 per 1M).
#[tokio::test]
async fn v4_5_simulate_cost_optimized_returns_lower_cost() {
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gpt4o_mock_response()))
        .mount(&mock_server)
        .await;

    // Make a gpt-4o request so we have expensive-model data.
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Cache", "false")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "cost optimized test unique v4_5" }]
        }))
        .send()
        .await
        .expect("gateway request failed");

    // Simulate: use gpt-4o-mini pricing for all gpt-4o requests.
    let overrides = serde_json::json!({"gpt-4o": "gpt-4o-mini"}).to_string();
    let resp = client
        .get(format!("{}/admin/analytics/simulate", base_url))
        .query(&[
            ("strategy", "cost_optimized"),
            ("period", "30d"),
            ("model_overrides", &overrides),
        ])
        .header("Authorization", &auth)
        .send()
        .await
        .expect("simulate request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let data = &body["data"];

    let savings = data["savings_usd"].as_f64().unwrap();
    assert!(
        savings > 0.0,
        "savings_usd must be > 0 when replacing gpt-4o with gpt-4o-mini"
    );
    assert!(
        data["simulated_cost_usd"].as_f64().unwrap() < data["original_cost_usd"].as_f64().unwrap(),
        "simulated cost must be less than original when downgrading model"
    );
}

// ── Simulate: model override applies correct pricing ─────────────────────────

/// With model_overrides={"gpt-4o":"gpt-4o-mini"}, the simulated cost for gpt-4o requests
/// must reflect gpt-4o-mini pricing (savings_percent must be > 90%).
#[tokio::test]
async fn v4_5_simulate_model_override_applies_new_pricing() {
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gpt4o_mock_response()))
        .mount(&mock_server)
        .await;

    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Cache", "false")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "model override pricing test unique v4_5" }]
        }))
        .send()
        .await
        .expect("gateway request failed");

    let overrides = serde_json::json!({"gpt-4o": "gpt-4o-mini"}).to_string();
    let resp = client
        .get(format!("{}/admin/analytics/simulate", base_url))
        .query(&[
            ("strategy", "cost_optimized"),
            ("period", "30d"),
            ("model_overrides", overrides.as_str()),
        ])
        .header("Authorization", &auth)
        .send()
        .await
        .expect("simulate request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let data = &body["data"];

    // gpt-4o is ~33x more expensive than gpt-4o-mini on input tokens.
    // savings_percent must exceed 90%.
    let savings_pct = data["savings_percent"].as_f64().unwrap();
    assert!(
        savings_pct > 90.0,
        "savings_percent with gpt-4o → gpt-4o-mini should exceed 90%, got {savings_pct}"
    );
}

// ── Simulate: per-model breakdown ─────────────────────────────────────────────

/// simulate response must include a by_model array with entries for each model
/// that had successful requests in the period.
#[tokio::test]
async fn v4_5_simulate_includes_per_model_breakdown() {
    let (base_url, mock_server) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(gpt4o_mock_response()))
        .mount(&mock_server)
        .await;

    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        // Bypass cache so the request is always logged as 'success', not 'cached'.
        .header("X-Velox-Cache", "false")
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "breakdown test unique v4_5" }]
        }))
        .send()
        .await
        .expect("gateway request failed");

    let resp = client
        .get(format!(
            "{}/admin/analytics/simulate?strategy=cost_optimized&period=30d",
            base_url
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("simulate request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let by_model = body["data"]["by_model"].as_array().unwrap();

    // Find the gpt-4o entry.
    let gpt4o_entry = by_model.iter().find(|e| e["model"] == "gpt-4o");
    assert!(
        gpt4o_entry.is_some(),
        "by_model must contain a gpt-4o entry"
    );

    let entry = gpt4o_entry.unwrap();
    assert!(entry["request_count"].as_i64().unwrap() > 0);
    assert!(entry["original_cost_usd"].is_number());
    assert!(entry["simulated_cost_usd"].is_number());
}

// ── Simulate: invalid inputs ──────────────────────────────────────────────────

/// Invalid strategy must return 400.
#[tokio::test]
async fn v4_5_simulate_invalid_strategy_returns_400() {
    let (base_url, _mock) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!(
            "{}/admin/analytics/simulate?strategy=random&period=30d",
            base_url
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 400);
}

// ── Provider quality_score field ──────────────────────────────────────────────

/// GET /admin/providers must include quality_score and quality_updated_at fields.
#[tokio::test]
async fn v4_5_quality_score_visible_in_provider_list() {
    let (base_url, _mock) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/providers", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("providers request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let providers = body["data"].as_array().unwrap();

    assert!(!providers.is_empty(), "providers list must not be empty");

    for p in providers {
        assert!(
            p["quality_score"].is_number(),
            "provider {} must have a numeric quality_score",
            p["id"]
        );
        let score = p["quality_score"].as_f64().unwrap();
        assert!(
            (0.0..=1.0).contains(&score),
            "quality_score must be in [0, 1], got {score}"
        );
    }
}

// ── Regression: overview endpoint unaffected ─────────────────────────────────

/// GET /admin/analytics/overview must still return the expected shape after V4-5.
#[tokio::test]
async fn v4_5_regression_analytics_overview_unaffected() {
    let (base_url, _mock) = common::spawn_app_with_wiremock().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("overview request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert!(body["today"].is_object(), "overview must have today field");
    assert!(
        body["last_7d"].is_object(),
        "overview must have last_7d field"
    );
    assert!(
        body["last_30d"].is_object(),
        "overview must have last_30d field"
    );
}
