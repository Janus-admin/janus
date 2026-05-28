mod common;

use reqwest::StatusCode;

// ── Registration lock tests ───────────────────────────────────────────────────

/// With allow_registration = false, any register attempt when users already
/// exist in the shared DB must return 403 Forbidden.
#[tokio::test]
async fn first_run_register_blocked_when_users_exist() {
    let base_url = common::spawn_app_registration_closed().await;
    let client = reqwest::Client::new();

    // The shared test DB already has users from parallel test runs.
    // allow_registration = false → any register attempt returns 403.
    let resp = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": "blocked@example.com",
            "password": "password123",
            "name": "Blocked User"
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "register must return 403 when allow_registration=false and users exist"
    );

    let body: serde_json::Value = resp.json().await.expect("response must be JSON");
    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "FORBIDDEN",
        "error code must be FORBIDDEN"
    );
}

/// With allow_registration = true (default for tests), registration always
/// works regardless of how many users already exist.
#[tokio::test]
async fn first_run_register_always_open_when_flag_true() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    for i in 0..3 {
        let email = format!("open-{}-{}@example.com", i, uuid::Uuid::new_v4());
        let resp = client
            .post(format!("{}/api/v1/auth/register", base_url))
            .json(&serde_json::json!({
                "email": email,
                "password": "password123",
                "name": format!("User {}", i)
            }))
            .send()
            .await
            .expect("request failed");

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "register #{i} must succeed when allow_registration=true"
        );
    }
}

/// A user created via the register endpoint can log in and receive a JWT.
#[tokio::test]
async fn first_run_created_user_can_login() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    let email = format!("login-test-{}@example.com", uuid::Uuid::new_v4());

    // Register.
    let reg = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": email,
            "password": "strongpassword",
            "name": "Admin"
        }))
        .send()
        .await
        .expect("register failed");
    assert_eq!(reg.status(), StatusCode::OK);

    // Login.
    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({
            "email": email,
            "password": "strongpassword"
        }))
        .send()
        .await
        .expect("login failed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("response must be JSON");
    assert!(
        body["token"].as_str().is_some(),
        "login must return a JWT token"
    );
}

/// The 403 response body includes a helpful message pointing to the config flag.
#[tokio::test]
async fn first_run_blocked_response_contains_helpful_message() {
    let base_url = common::spawn_app_registration_closed().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": "help@example.com",
            "password": "password123",
            "name": "Help"
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.expect("response must be JSON");
    let msg = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("ALLOW_REGISTRATION"),
        "error message must mention ALLOW_REGISTRATION, got: {msg}"
    );
}

/// count() returns a value ≥ 0 and increments correctly after a user is created.
/// This validates the DB building block used by the startup seeding path in main.rs.
#[tokio::test]
async fn first_run_user_count_increments_correctly() {
    common::load_env();
    let config = janus::config::Config::load().expect("config load failed");
    let pool = janus::db::pool::connect(&config.database_url)
        .await
        .expect("db connect failed");

    let before = janus::db::users::count(&pool).await.expect("count failed");

    let email = format!("count-test-{}@example.com", uuid::Uuid::new_v4());
    let hash = bcrypt::hash("password", bcrypt::DEFAULT_COST).unwrap();
    janus::db::users::create(&pool, &email, &hash, "Test")
        .await
        .expect("create failed");

    let after = janus::db::users::count(&pool).await.expect("count failed");
    assert_eq!(after, before + 1, "count must increment by 1 after create");
}
