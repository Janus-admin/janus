// tests/v5_l3_cost_tags.rs
// V5-L3 acceptance tests — Cost Tags.
//
// Run with: cargo test v5_l3

mod common;

use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn openai_success_response() -> Value {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_716_000_000_u64,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Hello!" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
}

async fn setup() -> (String, MockServer) {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_success_response()))
        .mount(&mock)
        .await;
    let base_url = common::spawn_app_with_openai_base(mock.uri()).await;
    (base_url, mock)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Tags from the `metadata` body field are stored in the `requests.tags` column.
#[tokio::test]
async fn v5_l3_metadata_field_tags_stored_in_requests() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "tag test via metadata" }],
            "metadata": { "team": "backend", "project": "rag" }
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200, "gateway must return 200");

    // Allow the fire-and-forget DB write to settle.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let admin = common::admin_auth_header(&base_url).await;
    let reqs: Value = client
        .get(format!("{}/admin/requests?per_page=20", base_url))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("requests list failed")
        .json()
        .await
        .expect("requests list must be JSON");

    let rows = reqs["data"].as_array().expect("data must be array");
    assert!(!rows.is_empty(), "must have at least one request");

    // Search for our specific row — concurrent tests share the DB so rows[0] may not be ours.
    let matching = rows
        .iter()
        .find(|r| r["tags"]["team"] == "backend" && r["tags"]["project"] == "rag");
    assert!(
        matching.is_some(),
        "must find a request with team=backend,project=rag; rows: {rows:?}"
    );
}

/// Tags from the `X-Velox-Tags` header are stored in `requests.tags`.
#[tokio::test]
async fn v5_l3_header_tags_stored_in_requests() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Tags", "team=ml,env=prod")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "tag test via header" }]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let admin = common::admin_auth_header(&base_url).await;
    let reqs: Value = client
        .get(format!("{}/admin/requests?per_page=20", base_url))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("requests list failed")
        .json()
        .await
        .expect("requests list must be JSON");

    let rows = reqs["data"].as_array().expect("data must be array");
    assert!(!rows.is_empty());

    let matching = rows
        .iter()
        .find(|r| r["tags"]["team"] == "ml" && r["tags"]["env"] == "prod");
    assert!(
        matching.is_some(),
        "must find a request with team=ml,env=prod; rows: {rows:?}"
    );
}

/// When both `metadata` and `X-Velox-Tags` are present, header values win on collision.
#[tokio::test]
async fn v5_l3_header_overrides_body_when_both_present() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Tags", "team=infra")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "override test" }],
            "metadata": { "team": "backend", "project": "rag" }
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let admin = common::admin_auth_header(&base_url).await;
    let reqs: Value = client
        .get(format!("{}/admin/requests?per_page=20", base_url))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("requests list failed")
        .json()
        .await
        .expect("requests list must be JSON");

    let rows = reqs["data"].as_array().expect("data must be array");
    assert!(!rows.is_empty());

    // Header wins for "team"; body-only key "project" is preserved.
    // Search by the unique combination (team=infra + project=rag) since tests share the DB.
    let matching = rows
        .iter()
        .find(|r| r["tags"]["team"] == "infra" && r["tags"]["project"] == "rag");
    assert!(
        matching.is_some(),
        "must find a request with team=infra (header override) and project=rag (body); rows: {rows:?}"
    );
}

/// The cost breakdown endpoint returns data grouped by a tag key.
#[tokio::test]
async fn v5_l3_cost_breakdown_by_tag_endpoint() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    // Send a tagged request so there is something to aggregate.
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .header("X-Velox-Tags", "team=frontend")
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "tag breakdown test" }]
        }))
        .send()
        .await
        .expect("request failed");

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let admin = common::admin_auth_header(&base_url).await;
    let body: Value = client
        .get(format!(
            "{}/admin/analytics/cost-by-tag?tag=team&days=1",
            base_url
        ))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("cost-by-tag request failed")
        .json()
        .await
        .expect("response must be JSON");

    assert!(body["data"].is_object(), "response must have data object");
    assert_eq!(body["data"]["tag_key"], "team");
    assert!(
        body["data"]["groups"].is_array(),
        "groups must be an array"
    );
    let groups = body["data"]["groups"].as_array().unwrap();
    assert!(
        groups.iter().any(|g| g["tag_value"] == "frontend"),
        "frontend team must appear in groups"
    );
}

/// A request with no tags produces a row with an empty tags object (not null).
#[tokio::test]
async fn v5_l3_missing_tag_key_returns_null_group() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();

    // Send a request with no tags at all.
    client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", common::auth_header())
        .json(&json!({
            "model": "gpt-4o-mini",
            "messages": [{ "role": "user", "content": "no tag test" }]
        }))
        .send()
        .await
        .expect("request failed");

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let admin = common::admin_auth_header(&base_url).await;
    let body: Value = client
        .get(format!(
            "{}/admin/analytics/cost-by-tag?tag=team&days=1",
            base_url
        ))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("cost-by-tag request failed")
        .json()
        .await
        .expect("response must be JSON");

    let groups = body["data"]["groups"]
        .as_array()
        .expect("groups must be an array");

    // There should be a group with null tag_value for requests without the tag.
    let has_null = groups.iter().any(|g| g["tag_value"].is_null());
    assert!(
        has_null,
        "untagged requests should appear as a null tag_value group"
    );
}

/// Invalid tag key (contains special characters) returns 400.
#[tokio::test]
async fn v5_l3_invalid_tag_key_returns_400() {
    let (base_url, _mock) = setup().await;
    let client = reqwest::Client::new();
    let admin = common::admin_auth_header(&base_url).await;

    let resp = client
        .get(format!(
            "{}/admin/analytics/cost-by-tag?tag=team%3Bname",
            base_url
        ))
        .header("Authorization", &admin)
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        400,
        "SQL-special characters in tag key must return 400"
    );
}
