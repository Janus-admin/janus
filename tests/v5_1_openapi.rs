// tests/v5_1_openapi.rs
// Phase V5-1 acceptance tests — OpenAPI spec + Swagger UI.
//
// Run with: cargo test v5_1
//
// V5-1 adds compile-time OpenAPI 3.1 spec generation via `utoipa`, served at:
//   - GET /admin/openapi.json   — machine-readable JSON
//   - GET /admin/docs           — Swagger UI (HTML + assets)

mod common;

use serde_json::Value;

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn fetch_spec(base_url: &str) -> Value {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/openapi.json", base_url))
        .send()
        .await
        .expect("openapi.json request failed");
    assert_eq!(resp.status(), 200, "openapi.json must return 200");
    assert!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("application/json"),
        "openapi.json must be served as application/json"
    );
    resp.json().await.expect("openapi.json must parse")
}

fn path_methods<'a>(spec: &'a Value, path: &str) -> Vec<&'a str> {
    let Some(obj) = spec
        .get("paths")
        .and_then(|p| p.get(path))
        .and_then(|p| p.as_object())
    else {
        return Vec::new();
    };
    obj.keys().map(String::as_str).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// The endpoint exists, returns 200, and produces parseable JSON whose shape
/// matches OpenAPI 3.1 at the top level (`openapi`, `info`, `paths`).
#[tokio::test]
async fn v5_1_openapi_json_endpoint_returns_valid_spec() {
    let base_url = common::spawn_app().await;
    let spec = fetch_spec(&base_url).await;

    assert!(
        spec.get("openapi")
            .and_then(Value::as_str)
            .unwrap_or("")
            .starts_with("3."),
        "spec.openapi must be a 3.x version string"
    );
    assert!(
        spec.get("info").and_then(|i| i.get("title")).is_some(),
        "spec.info.title must be present"
    );
    assert!(
        spec.get("paths").map(Value::is_object).unwrap_or(false),
        "spec.paths must be an object"
    );
}

/// Every admin endpoint mounted by the router must appear in the spec. We do not
/// check every single endpoint — that becomes maintenance bait — but we check a
/// representative spread across all admin tags.
#[tokio::test]
async fn v5_1_openapi_includes_all_admin_endpoints() {
    let base_url = common::spawn_app().await;
    let spec = fetch_spec(&base_url).await;

    let must_have: &[(&str, &[&str])] = &[
        ("/admin/keys", &["get", "post"]),
        ("/admin/keys/{id}", &["get", "patch", "delete"]),
        ("/admin/keys/{id}/rotate", &["post"]),
        ("/admin/requests", &["get"]),
        ("/admin/requests/export", &["get"]),
        ("/admin/requests/{id}", &["get"]),
        ("/admin/requests/{id}/replay", &["post"]),
        ("/admin/playground", &["post"]),
        ("/admin/analytics/overview", &["get"]),
        ("/admin/analytics/costs", &["get"]),
        ("/admin/analytics/latency", &["get"]),
        ("/admin/analytics/cache", &["get"]),
        ("/admin/analytics/simulate", &["get"]),
        ("/admin/models", &["get"]),
        ("/admin/providers", &["get"]),
        ("/admin/providers/{id}", &["patch"]),
        ("/admin/providers/{id}/test", &["post"]),
        ("/admin/alerts", &["get", "post"]),
        ("/admin/alerts/{id}", &["get", "patch", "delete"]),
        ("/admin/alerts/{id}/test", &["post"]),
        ("/admin/cache/stats", &["get"]),
        ("/admin/cache", &["delete"]),
        ("/admin/cache/entries/{id}", &["delete"]),
        ("/admin/prompts", &["get", "post"]),
        ("/admin/prompts/{id}", &["get", "delete"]),
        ("/admin/prompts/{id}/versions", &["post"]),
        ("/admin/prompts/{id}/versions/{version}", &["patch"]),
        ("/admin/config", &["get", "patch"]),
        ("/admin/system/readiness", &["get"]),
        ("/admin/workspaces", &["get"]),
        ("/admin/workspaces/{workspace_id}/members", &["get", "post"]),
        (
            "/admin/workspaces/{workspace_id}/members/{user_id}",
            &["patch", "delete"],
        ),
    ];

    for (path, methods) in must_have {
        let present = path_methods(&spec, path);
        for m in *methods {
            assert!(
                present.contains(m),
                "spec must contain {} {}; got methods {:?}",
                m.to_uppercase(),
                path,
                present
            );
        }
    }

    // Gateway endpoints
    let gateway: &[(&str, &str)] = &[
        ("/v1/chat/completions", "post"),
        ("/v1/embeddings", "post"),
        ("/v1/completions", "post"),
        ("/v1/models", "get"),
        ("/v1/images/generations", "post"),
        ("/v1/audio/transcriptions", "post"),
        ("/v1/audio/speech", "post"),
    ];
    for (path, method) in gateway {
        assert!(
            path_methods(&spec, path).contains(method),
            "gateway spec must contain {} {}",
            method.to_uppercase(),
            path
        );
    }
}

/// Shallow check that the spec validates against OpenAPI 3.1 structural rules
/// we care about: every operation has a `responses` map, paths use `{name}` style
/// parameters (not `:name`), and security schemes are declared at the component level.
#[tokio::test]
async fn v5_1_openapi_spec_validates_against_3_1_schema() {
    let base_url = common::spawn_app().await;
    let spec = fetch_spec(&base_url).await;

    // 3.1 marker
    let version = spec
        .get("openapi")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        version.starts_with("3.1"),
        "must declare openapi 3.1, got {:?}",
        version
    );

    // Security schemes declared
    let schemes = spec
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .and_then(Value::as_object)
        .expect("components.securitySchemes must exist");
    assert!(
        schemes.contains_key("bearer_jwt"),
        "bearer_jwt security scheme must be declared"
    );
    assert!(
        schemes.contains_key("api_key"),
        "api_key security scheme must be declared"
    );

    // Per-operation invariants
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .expect("paths must be an object");
    for (path, ops) in paths {
        assert!(
            !path.contains(":"),
            "path {:?} uses axum-style colon param; OpenAPI requires {{name}}",
            path
        );
        for (method, op) in ops.as_object().expect("path entry must be object") {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace"
            ) {
                continue;
            }
            assert!(
                op.get("responses").is_some(),
                "{} {} missing responses map",
                method.to_uppercase(),
                path
            );
        }
    }
}

/// `GET /admin/docs` redirects to the trailing-slash form and the trailing-slash
/// form returns an HTML page (the bundled Swagger UI index).
#[tokio::test]
async fn v5_1_swagger_ui_endpoint_returns_200() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Redirect step
    let resp = client
        .get(format!("{}/admin/docs", base_url))
        .send()
        .await
        .expect("docs redirect failed");
    assert!(
        resp.status().is_redirection(),
        "GET /admin/docs should redirect, got {}",
        resp.status()
    );

    // Trailing-slash returns 200 HTML
    let resp = client
        .get(format!("{}/admin/docs/", base_url))
        .send()
        .await
        .expect("docs index failed");
    assert_eq!(resp.status(), 200, "docs index must return 200");
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("text/html"),
        "docs index must be text/html, got {:?}",
        ct
    );

    // A static asset must also serve
    let resp = client
        .get(format!("{}/admin/docs/swagger-ui.css", base_url))
        .send()
        .await
        .expect("swagger-ui.css fetch failed");
    assert_eq!(resp.status(), 200, "swagger-ui.css must return 200");
}

/// Regression: handlers still behave exactly as before after annotation.
/// We exercise one unauthenticated public route (`/v1/models`) and one admin
/// route under JWT (`/admin/keys` requires JWT — we expect 401 without one).
#[tokio::test]
async fn v5_1_regression_existing_handler_responses_unchanged() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    // /v1/models — public, returns OpenAI list envelope
    let resp = client
        .get(format!("{}/v1/models", base_url))
        .send()
        .await
        .expect("/v1/models failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("/v1/models must be JSON");
    assert_eq!(
        body.get("object").and_then(Value::as_str),
        Some("list"),
        "/v1/models response envelope must still be {{object: \"list\", data: [...]}}"
    );
    assert!(
        body.get("data").map(Value::is_array).unwrap_or(false),
        "/v1/models.data must still be an array"
    );

    // /admin/keys without JWT → 401 (unchanged behaviour)
    let resp = client
        .get(format!("{}/admin/keys", base_url))
        .send()
        .await
        .expect("/admin/keys failed");
    assert_eq!(
        resp.status(),
        401,
        "/admin/keys must still require JWT (got {})",
        resp.status()
    );
}
