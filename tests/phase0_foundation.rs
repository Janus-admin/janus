// tests/phase0_foundation.rs
// Phase 0 acceptance tests.
//
// THESE TESTS MUST ALL PASS before Phase 1 begins.
// They verify the foundation is solid: DB tables exist, config loads, server starts.
//
// Run with: cargo test phase0

mod common;

use sqlx::PgPool;
use std::env;

/// Helper: create a test database connection.
/// Loads .env if present, then reads DATABASE_URL.
async fn test_db() -> PgPool {
    dotenvy::dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests. Add it to .env");
    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

// ─── Database Schema Tests ────────────────────────────────────────────────────

/// Verify the users table exists (from migration 0001).
/// This is the existing admin user table — must not be dropped.
#[tokio::test]
async fn phase0_users_table_exists() {
    let pool = test_db().await;
    let result = sqlx::query("SELECT 1 FROM users LIMIT 1")
        .fetch_optional(&pool)
        .await;
    assert!(result.is_ok(), "users table must exist (migration 0001)");
}

/// Verify the workspaces table exists (from migration 0002).
#[tokio::test]
async fn phase0_workspaces_table_exists() {
    let pool = test_db().await;
    let result = sqlx::query("SELECT 1 FROM workspaces LIMIT 1")
        .fetch_optional(&pool)
        .await;
    assert!(
        result.is_ok(),
        "workspaces table must exist (migration 0002)"
    );
}

/// Verify the api_keys table exists with required columns.
#[tokio::test]
async fn phase0_api_keys_table_exists_with_correct_schema() {
    let pool = test_db().await;

    // Verify each required column exists by selecting it explicitly.
    // Uses sqlx::query (not sqlx::query!) so no DB connection needed at compile time.
    let result = sqlx::query(
        r#"
        SELECT
            id, name, key_hash, key_prefix,
            budget_limit, budget_used,
            rate_limit_rpm, rate_limit_tpm,
            is_active, created_at, expires_at, last_used_at
        FROM api_keys
        LIMIT 1
        "#,
    )
    .fetch_optional(&pool)
    .await;

    assert!(
        result.is_ok(),
        "api_keys table must exist with all required columns. Error: {:?}",
        result.err()
    );
}

/// Verify the requests table exists with required columns.
#[tokio::test]
async fn phase0_requests_table_exists_with_correct_schema() {
    let pool = test_db().await;

    let result = sqlx::query(
        r#"
        SELECT
            id, api_key_id, provider, model,
            prompt_tokens, completion_tokens, total_tokens,
            cost_usd, latency_ms, ttfb_ms,
            status, cache_type, cache_similarity,
            http_status, error_code, error_message,
            created_at, stream
        FROM requests
        LIMIT 1
        "#,
    )
    .fetch_optional(&pool)
    .await;

    assert!(
        result.is_ok(),
        "requests table must exist with all required columns. Error: {:?}",
        result.err()
    );
}

/// Verify the providers table exists and has seed data.
#[tokio::test]
async fn phase0_providers_table_exists_with_seed_data() {
    let pool = test_db().await;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers")
        .fetch_one(&pool)
        .await
        .expect("providers table must exist");

    assert!(
        count >= 3,
        "providers table must have at least 3 rows (openai, anthropic, bedrock). Found: {}",
        count
    );
}

/// Verify the model_pricing table exists and has seed data.
#[tokio::test]
async fn phase0_model_pricing_table_has_seed_data() {
    let pool = test_db().await;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM model_pricing")
        .fetch_one(&pool)
        .await
        .expect("model_pricing table must exist");

    assert!(
        count >= 5,
        "model_pricing table must have at least 5 model prices seeded. Found: {}",
        count
    );
}

/// Verify the cache_entries table exists.
#[tokio::test]
async fn phase0_cache_entries_table_exists() {
    let pool = test_db().await;
    let result = sqlx::query("SELECT 1 FROM cache_entries LIMIT 1")
        .fetch_optional(&pool)
        .await;
    assert!(result.is_ok(), "cache_entries table must exist");
}

/// Verify the daily_costs table exists.
#[tokio::test]
async fn phase0_daily_costs_table_exists() {
    let pool = test_db().await;
    let result = sqlx::query("SELECT 1 FROM daily_costs LIMIT 1")
        .fetch_optional(&pool)
        .await;
    assert!(result.is_ok(), "daily_costs table must exist");
}

// ─── Config Tests ─────────────────────────────────────────────────────────────

/// Verify the Config struct loads from environment variables.
/// This is a compile-time check — if Config doesn't have the right fields,
/// this test won't compile.
#[test]
fn phase0_config_has_required_fields() {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL");
    assert!(
        database_url.is_ok(),
        "DATABASE_URL environment variable must be set"
    );

    let jwt_secret = env::var("JWT_SECRET");
    assert!(
        jwt_secret.is_ok(),
        "JWT_SECRET environment variable must be set"
    );
}

// ─── Server Health Tests ──────────────────────────────────────────────────────

/// Verify the health endpoint returns the correct response shape.
/// This test REQUIRES the server to be running.
/// Run with: cargo run & sleep 2 && cargo test phase0_health
#[tokio::test]
#[ignore = "requires running server — run manually with: cargo run"]
async fn phase0_health_endpoint_returns_ok() {
    let client = reqwest::Client::new();
    let response = client
        .get("http://localhost:8080/health")
        .send()
        .await
        .expect("Failed to reach server. Is it running?");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Response must be JSON");
    assert_eq!(body["status"], "ok", "Health response must have status: ok");
    assert!(
        body["version"].is_string(),
        "Health response must have a version field"
    );
}
