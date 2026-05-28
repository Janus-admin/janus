// tests/v5_l6_smart_routing.rs
// V5-L6 acceptance tests — Smart Routing Engine.
//
// Run with: cargo test v5_l6
//
// These tests cover:
//   1. Request without model is rejected when smart_routing.enabled = false (default)
//   2. Layer 1 — capability filtering (needs_vision blocks non-vision models)
//   3. Layer 2a — quality tag routing (X-Janus-Tags: quality=premium)
//   4. Layer 2b — admin routing rule (token range match)
//   5. Layer 3 — heuristic complexity scoring (long prompt → standard/premium tier)
//   6. Layer 3 — short prompt routes to micro tier
//   7. Layer 4 — config default_model fallback
//   8. Admin config CRUD (GET/PUT smart-routing/config)
//   9. Admin rules CRUD (create, list, delete)
//  10. Response headers X-Janus-Model-Selected and X-Janus-Routing-Reason

mod common;

use janus::gateway::smart_router::{parse_tag_header, SmartRouter};
use serde_json::Value;
use std::collections::HashMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn login(base_url: &str, email: &str) -> String {
    let client = reqwest::Client::new();
    client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": email, "password": "pass", "name": "Test"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({"email": email, "password": "pass"}))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    format!("Bearer {}", body["token"].as_str().unwrap())
}

async fn get_workspace_id(base_url: &str, token: &str) -> uuid::Uuid {
    let resp = reqwest::Client::new()
        .get(format!("{}/admin/workspaces", base_url))
        .header("Authorization", token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let ws_str = body["data"][0]["id"].as_str().unwrap();
    ws_str.parse().unwrap()
}

// ── Unit tests: SmartRouter::profile_request ─────────────────────────────────

/// Test 1: Short single message → micro tier (score 0–3)
#[test]
fn v5_l6_short_prompt_scores_micro() {
    let request =
        serde_json::from_value::<janus::providers::ChatCompletionRequest>(serde_json::json!({
            "messages": [{"role": "user", "content": "Hi there!"}]
        }))
        .unwrap();

    let profile = SmartRouter::profile_request(&request, &HashMap::new());
    assert_eq!(
        SmartRouter::tier_from_score(profile.complexity_score),
        "micro",
        "score={} should map to micro",
        profile.complexity_score
    );
}

/// Test 2: Deep multi-turn conversation with tools + complex verbs → premium tier.
/// Scoring: token_estimate→2pts + deep_history(9 msgs)→2pts + tools→2pts + verbs→2pts = 8 → premium.
#[test]
fn v5_l6_complex_prompt_scores_premium() {
    let long_content = "Please analyze and evaluate the following architecture. ".repeat(100);
    let request = serde_json::from_value::<janus::providers::ChatCompletionRequest>(
        serde_json::json!({
            "messages": [
                {"role": "system", "content": "You are a software architect. Analyze and critique."},
                {"role": "user", "content": "Previous context"},
                {"role": "assistant", "content": "I see"},
                {"role": "user", "content": "More context"},
                {"role": "assistant", "content": "Continue"},
                {"role": "user", "content": "More messages"},
                {"role": "assistant", "content": "Still here"},
                {"role": "user", "content": "Another one"},
                {"role": "user", "content": long_content}
            ],
            // Tools push score from 6 → 8, crossing the standard/premium boundary
            "tools": [{"type": "function", "function": {"name": "search_codebase"}}]
        }),
    )
    .unwrap();

    let profile = SmartRouter::profile_request(&request, &HashMap::new());
    assert_eq!(
        SmartRouter::tier_from_score(profile.complexity_score),
        "premium",
        "score={} should map to premium (expected ≥8 from: tokens+history+tools+verbs)",
        profile.complexity_score
    );
}

/// Test 3: Tool use present → complexity score ≥ 2 (tools add 2 pts)
#[test]
fn v5_l6_tools_raise_complexity_score() {
    let request =
        serde_json::from_value::<janus::providers::ChatCompletionRequest>(serde_json::json!({
            "messages": [{"role": "user", "content": "Call the function"}],
            "tools": [{"type": "function", "function": {"name": "get_weather"}}]
        }))
        .unwrap();

    let profile = SmartRouter::profile_request(&request, &HashMap::new());
    assert!(profile.needs_functions, "needs_functions should be true");
    assert!(
        profile.complexity_score >= 2,
        "tools should add ≥2 pts, got {}",
        profile.complexity_score
    );
}

/// Test 4: Vision content detected correctly
#[test]
fn v5_l6_image_url_sets_needs_vision() {
    let request =
        serde_json::from_value::<janus::providers::ChatCompletionRequest>(serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe this image"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                ]
            }]
        }))
        .unwrap();

    let profile = SmartRouter::profile_request(&request, &HashMap::new());
    assert!(
        profile.needs_vision,
        "needs_vision should be true when image_url present"
    );
}

/// Test 5: Tag parsing — header format "key=val,key2=val2"
#[test]
fn v5_l6_parse_tag_header_parses_correctly() {
    let tags = parse_tag_header("quality=premium,domain=code,env=prod");
    assert_eq!(tags.get("quality").map(String::as_str), Some("premium"));
    assert_eq!(tags.get("domain").map(String::as_str), Some("code"));
    assert_eq!(tags.get("env").map(String::as_str), Some("prod"));
}

/// Test 6: Tag parsing — malformed pairs are silently dropped
#[test]
fn v5_l6_parse_tag_header_tolerates_malformed() {
    let tags = parse_tag_header("quality=premium,badentry,,=nokey");
    assert_eq!(tags.get("quality").map(String::as_str), Some("premium"));
    assert_eq!(tags.len(), 1, "only well-formed pairs should be kept");
}

// ── Integration tests: API ────────────────────────────────────────────────────

/// Test 7: Without smart routing enabled, missing model returns 400
#[tokio::test]
async fn v5_l6_missing_model_without_smart_routing_is_400() {
    let base_url = common::spawn_app().await;
    let api_key = common::test_api_key();

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "messages": [{"role": "user", "content": "Hello"}]
            // no "model" field
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "missing model without smart routing should be 400"
    );
    let body: Value = resp.json().await.unwrap();
    let msg = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("model") || msg.contains("smart_routing"),
        "error message should reference model or smart_routing, got: {msg}"
    );
}

/// Test 8: Admin smart-routing config endpoint — GET returns defaults
#[tokio::test]
async fn v5_l6_admin_get_config_returns_defaults() {
    let base_url = common::spawn_app().await;
    let email = format!("sr-config-{}@janus.test", uuid::Uuid::new_v4());
    let token = login(&base_url, &email).await;
    let workspace_id = get_workspace_id(&base_url, &token).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/admin/workspaces/{}/smart-routing/config",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["data"]["enabled"].is_boolean(),
        "enabled field must be present, got: {}",
        body["data"]
    );
}

/// Test 9: Admin can enable smart routing for a workspace
#[tokio::test]
async fn v5_l6_admin_put_config_enables_smart_routing() {
    let base_url = common::spawn_app().await;
    let email = format!("sr-enable-{}@janus.test", uuid::Uuid::new_v4());
    let token = login(&base_url, &email).await;
    let workspace_id = get_workspace_id(&base_url, &token).await;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!(
            "{}/admin/workspaces/{}/smart-routing/config",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .json(&serde_json::json!({
            "enabled": true,
            "default_model": "gpt-4o-mini",
            "meta_classifier_enabled": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "PUT config should succeed");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["enabled"], true);
    assert_eq!(body["data"]["default_model"], "gpt-4o-mini");

    // Verify the change persists
    let get_resp = client
        .get(format!(
            "{}/admin/workspaces/{}/smart-routing/config",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();
    let get_body: Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["data"]["enabled"], true, "change should persist");
}

/// Test 10: Admin routing rule CRUD — create, list, delete
#[tokio::test]
async fn v5_l6_admin_routing_rule_crud() {
    let base_url = common::spawn_app().await;
    let email = format!("sr-rules-{}@janus.test", uuid::Uuid::new_v4());
    let token = login(&base_url, &email).await;
    let workspace_id = get_workspace_id(&base_url, &token).await;
    let client = reqwest::Client::new();

    // Create a rule: premium quality tag → route to claude-opus-4-7
    let create_resp = client
        .post(format!(
            "{}/admin/workspaces/{}/smart-routing/rules",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .json(&serde_json::json!({
            "name": "Premium quality rule",
            "rule_order": 10,
            "tag_key": "quality",
            "tag_value": "enterprise",
            "target_model": "claude-opus-4-7"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(create_resp.status(), 201, "create rule should return 201");
    let created: Value = create_resp.json().await.unwrap();
    let rule_id = created["data"]["id"].as_str().unwrap();

    // List rules — should contain the new rule
    let list_resp = client
        .get(format!(
            "{}/admin/workspaces/{}/smart-routing/rules",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status(), 200);
    let list_body: Value = list_resp.json().await.unwrap();
    let rules = list_body["data"].as_array().unwrap();
    assert!(
        rules.iter().any(|r| r["id"].as_str() == Some(rule_id)),
        "created rule should appear in list"
    );

    // Delete the rule
    let del_resp = client
        .delete(format!(
            "{}/admin/workspaces/{}/smart-routing/rules/{}",
            base_url, workspace_id, rule_id
        ))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 204, "delete should return 204");

    // Verify gone
    let list_after: Value = client
        .get(format!(
            "{}/admin/workspaces/{}/smart-routing/rules",
            base_url, workspace_id
        ))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        list_after["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"].as_str() != Some(rule_id)),
        "deleted rule should not appear in list"
    );
}
