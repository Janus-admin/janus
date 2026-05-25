// tests/v5_l5_onboarding.rs
// V5-L5 acceptance tests — Onboarding & Docs Polish.
//
// Run with: cargo test v5_l5

mod common;

use serde_json::Value;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Register a fresh unique user and log in; returns (base_url, token).
async fn register_and_login(base_url: &str, email: &str) -> String {
    let client = reqwest::Client::new();

    client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": email,
            "password": "test-password",
            "name": "Test User"
        }))
        .send()
        .await
        .expect("register failed");

    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({
            "email": email,
            "password": "test-password"
        }))
        .send()
        .await
        .expect("login failed");

    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().expect("no token in login response");
    format!("Bearer {}", token)
}

// ── Test 1: New user has tour_completed_at = null ────────────────────────────

/// A freshly registered user has tour_completed_at = null in the /me response.
#[tokio::test]
async fn v5_l5_new_user_tour_not_completed() {
    let base_url = common::spawn_app().await;
    let email = format!("tour-new-{}@janus.test", uuid::Uuid::new_v4());
    let token = register_and_login(&base_url, &email).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/auth/me", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .expect("me request failed");

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["tour_completed_at"].is_null(),
        "new user should have tour_completed_at = null, got: {}",
        body["tour_completed_at"]
    );
}

// ── Test 2: POST /api/v1/auth/tour-complete marks the tour done ───────────────

/// After calling tour-complete, tour_completed_at is set on the user.
#[tokio::test]
async fn v5_l5_tour_complete_sets_timestamp() {
    let base_url = common::spawn_app().await;
    let email = format!("tour-done-{}@janus.test", uuid::Uuid::new_v4());
    let token = register_and_login(&base_url, &email).await;
    let client = reqwest::Client::new();

    // Mark tour complete.
    let mark_resp = client
        .post(format!("{}/api/v1/auth/tour-complete", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .expect("tour-complete request failed");
    assert_eq!(mark_resp.status(), 204, "tour-complete should return 204");

    // Verify via /me.
    let me_resp = client
        .get(format!("{}/api/v1/auth/me", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .expect("me request failed");

    let body: Value = me_resp.json().await.unwrap();
    assert!(
        !body["tour_completed_at"].is_null(),
        "tour_completed_at should be set after tour-complete"
    );
}

// ── Test 3: Calling tour-complete twice is idempotent ────────────────────────

/// A second call to tour-complete must still return 204 and not overwrite
/// the original timestamp.
#[tokio::test]
async fn v5_l5_tour_complete_is_idempotent() {
    let base_url = common::spawn_app().await;
    let email = format!("tour-idem-{}@janus.test", uuid::Uuid::new_v4());
    let token = register_and_login(&base_url, &email).await;
    let client = reqwest::Client::new();

    let url = format!("{}/api/v1/auth/tour-complete", base_url);

    // First call.
    let r1 = client
        .post(&url)
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 204);

    let ts1: Value = client
        .get(format!("{}/api/v1/auth/me", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ts1 = ts1["tour_completed_at"].clone();

    // Second call.
    let r2 = client
        .post(&url)
        .header("Authorization", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 204);

    let ts2: Value = client
        .get(format!("{}/api/v1/auth/me", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ts2 = ts2["tour_completed_at"].clone();

    // Timestamp must not change on the second call.
    assert_eq!(
        ts1, ts2,
        "second tour-complete must not overwrite the original timestamp"
    );
}

// ── Test 4: tour-complete requires authentication ─────────────────────────────

/// Unauthenticated call to tour-complete must return 401.
#[tokio::test]
async fn v5_l5_tour_complete_requires_auth() {
    let base_url = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/auth/tour-complete", base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        401,
        "unauthenticated tour-complete must return 401"
    );
}

// ── Test 5: /me returns tour_completed_at in the response body ───────────────

/// After login, the auth /me endpoint includes the tour_completed_at field
/// even when null (field must always be present).
#[tokio::test]
async fn v5_l5_me_includes_tour_field() {
    let base_url = common::spawn_app().await;
    let email = format!("tour-field-{}@janus.test", uuid::Uuid::new_v4());
    let token = register_and_login(&base_url, &email).await;

    let body: Value = reqwest::Client::new()
        .get(format!("{}/api/v1/auth/me", base_url))
        .header("Authorization", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(
        body.get("tour_completed_at").is_some(),
        "/me response must include tour_completed_at field"
    );
}
