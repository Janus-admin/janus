// tests/phase5_semantic_cache.rs
// Phase 5 acceptance tests — Semantic Cache.

mod common;

use common::{
    auth_header, fake_openai_response_json, spawn_app_with_embedding_and_wiremock,
    spawn_app_with_embedding_base,
};
use serial_test::serial;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

// ── Shared setup ──────────────────────────────────────────────────────────────

/// Flush all cache entries so each test starts with an empty slate.
async fn flush_cache(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM cache_entries")
        .execute(pool)
        .await
        .expect("Failed to flush cache_entries");
}

/// Get a pool from the default test database URL.
async fn test_pool() -> sqlx::PgPool {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("Failed to connect to test database")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Semantically similar (but not identical) prompts must return a semantic cache hit.
#[tokio::test]
#[serial]
async fn phase5_semantically_similar_prompt_returns_cache_hit() {
    let pool = test_pool().await;
    flush_cache(&pool).await;

    let (base_url, mock_server) = spawn_app_with_embedding_and_wiremock().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First request — must miss (no cache entry yet).
    let resp1 = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        }))
        .send()
        .await
        .expect("request 1 failed");

    assert_eq!(resp1.status(), 200, "request 1 must succeed");
    assert!(
        resp1.headers().get("x-velox-cache-hit").is_none(),
        "first request must be a cache miss"
    );

    // Second request — semantically equivalent; must hit semantic cache.
    let resp2 = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Tell me the capital city of France"}]
        }))
        .send()
        .await
        .expect("request 2 failed");

    assert_eq!(resp2.status(), 200, "request 2 must succeed");
    let hit = resp2
        .headers()
        .get("x-velox-cache-hit")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        hit, "semantic",
        "second request must be a semantic cache hit"
    );
}

/// On a semantic hit the response must carry both cache headers.
#[tokio::test]
#[serial]
async fn phase5_semantic_hit_includes_similarity_header() {
    let pool = test_pool().await;
    flush_cache(&pool).await;

    let (base_url, mock_server) = spawn_app_with_embedding_and_wiremock().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Seed the cache.
    client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        }))
        .send()
        .await
        .expect("seed request failed");

    // Semantic hit request.
    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Tell me the capital city of France"}]
        }))
        .send()
        .await
        .expect("hit request failed");

    assert_eq!(resp.status(), 200);

    let hit = resp
        .headers()
        .get("x-velox-cache-hit")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(hit, "semantic", "X-Velox-Cache-Hit must be 'semantic'");

    let similarity_str = resp
        .headers()
        .get("x-velox-cache-similarity")
        .and_then(|v| v.to_str().ok())
        .expect("X-Velox-Cache-Similarity header must be present on semantic hits");

    let score: f32 = similarity_str
        .parse()
        .expect("X-Velox-Cache-Similarity must be a float");
    assert!(
        score >= 0.90,
        "similarity score {score} must be >= 0.90 for these two prompts"
    );
}

/// Completely different prompts must NOT hit the semantic cache.
#[tokio::test]
#[serial]
async fn phase5_different_prompts_do_not_return_cache_hit() {
    let pool = test_pool().await;
    flush_cache(&pool).await;

    let (base_url, mock_server) = spawn_app_with_embedding_and_wiremock().await;

    // Expect exactly 2 provider calls — both requests must reach the provider.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response_json()))
        .expect(2)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First prompt: geography question.
    let resp1 = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        }))
        .send()
        .await
        .expect("request 1 failed");
    assert_eq!(resp1.status(), 200);

    // Second prompt: completely different topic — must not hit.
    let resp2 = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Write me a Python function to sort a list"}]
        }))
        .send()
        .await
        .expect("request 2 failed");
    assert_eq!(resp2.status(), 200);

    assert!(
        resp2.headers().get("x-velox-cache-hit").is_none(),
        "different prompts must not return a cache hit"
    );
}

/// Semantic cache must survive a server restart by loading embeddings from the database.
#[tokio::test]
#[serial]
async fn phase5_semantic_cache_survives_restart() {
    let pool = test_pool().await;
    flush_cache(&pool).await;

    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_openai_response_json()))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Instance 1: seed the cache.
    let base_url1 = spawn_app_with_embedding_base(mock_server.uri()).await;

    client
        .post(format!("{base_url1}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        }))
        .send()
        .await
        .expect("seed request on instance 1 failed");

    // Instance 2: fresh in-memory state but same database.
    // warm_from_db() loads the embedding persisted by instance 1.
    let base_url2 = spawn_app_with_embedding_base(mock_server.uri()).await;

    let resp = client
        .post(format!("{base_url2}/v1/chat/completions"))
        .header("Authorization", auth_header())
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "Tell me the capital city of France"}]
        }))
        .send()
        .await
        .expect("hit request on instance 2 failed");

    assert_eq!(resp.status(), 200);
    let hit = resp
        .headers()
        .get("x-velox-cache-hit")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        hit, "semantic",
        "semantic cache must survive restart via DB-persisted embeddings"
    );
}
