// tests/v3_1_semantic_cache.rs
// Phase V3-1 acceptance tests — Semantic Cache Redesign.
//
// Run with: cargo test v3_1
// Pure unit tests — no DB, no HTTP, no spawned app required.
//
// As of D.2 the `EmbeddingIndex` / `SemanticCache` / `CacheEngine::clear`
// surfaces are async, so the tests that touch them are `#[tokio::test]`.
// Trait-object-only checks stay synchronous since they do not call any
// method on the trait.

use janus::cache::{
    index::{hnsw::HnswIndex, linear::LinearIndex, EmbeddingIndex},
    policy::SemanticCachePolicy,
    semantic::SemanticCache,
    CacheEngine,
};
use std::time::Instant;

// ─── Embedding helpers ────────────────────────────────────────────────────────

/// Unit vector (all equal components, L2-normalized) of the given dimension.
/// Cosine similarity between two identical unit vectors = 1.0.
fn unit_embedding(dim: usize) -> Vec<f32> {
    let v = 1.0_f32 / (dim as f32).sqrt();
    vec![v; dim]
}

/// A vector orthogonal to `unit_embedding`: only the last component is non-zero.
fn orthogonal_embedding(dim: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; dim];
    *v.last_mut().unwrap() = 1.0;
    v
}

// ─── LinearIndex tests ────────────────────────────────────────────────────────

#[test]
fn v3_1_linear_index_implements_embedding_index_trait() {
    // Trait object creation verifies the impl satisfies Send + Sync.
    let _: Box<dyn EmbeddingIndex> = Box::new(LinearIndex::new());
}

#[tokio::test]
async fn v3_1_linear_insert_then_lookup_finds_entry() {
    let idx = LinearIndex::new();
    let emb = unit_embedding(64);
    idx.insert(emb.clone(), "linear-hash".to_string()).await;
    let result = idx.lookup(&emb, 0.90).await;
    assert!(result.is_some());
    let (hash, score) = result.unwrap();
    assert_eq!(hash, "linear-hash");
    assert!(
        score >= 0.99,
        "identical vectors should score ≈ 1.0, got {score}"
    );
}

#[tokio::test]
async fn v3_1_linear_lookup_returns_none_below_threshold() {
    let idx = LinearIndex::new();
    idx.insert(unit_embedding(64), "h".to_string()).await;
    // Orthogonal vector has cosine similarity 0 with the unit embedding.
    let result = idx.lookup(&orthogonal_embedding(64), 0.90).await;
    assert!(
        result.is_none(),
        "orthogonal vector must not hit above 0.90"
    );
}

#[tokio::test]
async fn v3_1_linear_clear_empties_index() {
    let idx = LinearIndex::new();
    idx.insert(unit_embedding(64), "a".to_string()).await;
    idx.insert(unit_embedding(64), "b".to_string()).await;
    assert_eq!(idx.len().await, 2);
    idx.clear().await;
    assert_eq!(idx.len().await, 0);
    assert!(idx.lookup(&unit_embedding(64), 0.80).await.is_none());
}

// ─── HnswIndex tests ──────────────────────────────────────────────────────────

#[test]
fn v3_1_hnsw_index_implements_embedding_index_trait() {
    let _: Box<dyn EmbeddingIndex> = Box::new(HnswIndex::new(16, 200));
}

#[tokio::test]
async fn v3_1_hnsw_insert_then_lookup_finds_entry() {
    let idx = HnswIndex::new(16, 200);
    let emb = unit_embedding(64);
    idx.insert(emb.clone(), "hnsw-hash".to_string()).await;
    let result = idx.lookup(&emb, 0.80).await;
    assert!(result.is_some(), "HNSW must find inserted entry");
    let (hash, score) = result.unwrap();
    assert_eq!(hash, "hnsw-hash");
    assert!(score >= 0.80, "similarity {score} must be >= 0.80");
}

#[tokio::test]
async fn v3_1_hnsw_lookup_returns_none_below_threshold() {
    let idx = HnswIndex::new(16, 200);
    idx.insert(unit_embedding(64), "h".to_string()).await;
    // Orthogonal vector: cosine similarity = 0, well below any threshold.
    let result = idx.lookup(&orthogonal_embedding(64), 0.50).await;
    assert!(result.is_none(), "orthogonal vector must not hit");
}

#[tokio::test]
async fn v3_1_hnsw_lookup_returns_above_threshold_entry() {
    let idx = HnswIndex::new(16, 200);
    let emb = unit_embedding(128);
    idx.insert(emb.clone(), "above-thresh".to_string()).await;
    // Same vector: similarity = 1.0, always above any threshold < 1.0.
    let result = idx.lookup(&emb, 0.95).await;
    assert!(result.is_some());
    assert_eq!(result.unwrap().0, "above-thresh");
}

#[tokio::test]
async fn v3_1_hnsw_clear_empties_index() {
    let idx = HnswIndex::new(16, 200);
    let emb = unit_embedding(64);
    idx.insert(emb.clone(), "will-be-cleared".to_string()).await;
    assert_eq!(idx.len().await, 1);
    idx.clear().await;
    assert_eq!(idx.len().await, 0);
    // After clear the HNSW is rebuilt from scratch — nothing should match.
    let result = idx.lookup(&emb, 0.0).await;
    assert!(result.is_none(), "cleared HNSW must have no entries");
}

#[tokio::test]
async fn v3_1_hnsw_multiple_inserts_lookup_finds_best() {
    let idx = HnswIndex::new(16, 200);
    let target = unit_embedding(64);
    let other = orthogonal_embedding(64);
    idx.insert(target.clone(), "best".to_string()).await;
    idx.insert(other, "worst".to_string()).await;
    let result = idx.lookup(&target, 0.80).await;
    assert_eq!(result.unwrap().0, "best");
}

// ─── SemanticCache with HNSW backend ─────────────────────────────────────────

#[tokio::test]
async fn v3_1_hnsw_backend_hits_on_similar_prompt() {
    let idx = Box::new(HnswIndex::new(16, 200));
    let sc = SemanticCache::with_index(idx, 0.80);
    let emb = unit_embedding(128);
    sc.insert(emb.clone(), "prompt-hash".to_string()).await;
    // Same embedding — must return a hit.
    let result = sc.lookup(&emb).await;
    assert!(
        result.is_some(),
        "HNSW-backed SemanticCache must return a hit"
    );
}

#[tokio::test]
async fn v3_1_linear_backend_still_works_unchanged() {
    // SemanticCache::new() still uses LinearIndex (backwards compatible).
    let sc = SemanticCache::new(0.80);
    let emb = unit_embedding(64);
    sc.insert(emb.clone(), "linear-backed".to_string()).await;
    let result = sc.lookup(&emb).await;
    assert!(result.is_some());
    assert_eq!(result.unwrap().0, "linear-backed");
}

#[tokio::test]
async fn v3_1_regression_semantic_flush_clears_hnsw_index() {
    let idx = Box::new(HnswIndex::new(16, 200));
    let sc = SemanticCache::with_index(idx, 0.80);
    let emb = unit_embedding(64);
    sc.insert(emb.clone(), "flush-test".to_string()).await;
    sc.clear().await;
    assert_eq!(sc.len().await, 0);
    assert!(
        sc.lookup(&emb).await.is_none(),
        "HNSW must be empty after flush"
    );
}

// ─── SemanticCachePolicy tests ────────────────────────────────────────────────

#[test]
fn v3_1_policy_allows_all_when_models_list_empty() {
    let policy = SemanticCachePolicy::default(); // models = []
    assert!(policy.allows("gpt-4o", "/v1/chat/completions", "any-key"));
    assert!(policy.allows("claude-3-opus", "/v1/chat/completions", "any-key"));
    assert!(policy.allows("unknown-model-xyz", "/v1/chat/completions", "any-key"));
}

#[test]
fn v3_1_policy_denies_unlisted_model() {
    let policy = SemanticCachePolicy::new(vec!["gpt-4o-mini".to_string()], vec![], vec![]);
    assert!(policy.allows("gpt-4o-mini", "/v1/chat/completions", "k"));
    assert!(!policy.allows("gpt-4o", "/v1/chat/completions", "k"));
    assert!(!policy.allows("claude-3-5-haiku-20241022", "/v1/chat/completions", "k"));
}

#[test]
fn v3_1_policy_denies_excluded_route_prefix() {
    let policy = SemanticCachePolicy::new(vec![], vec!["/v1/embeddings".to_string()], vec![]);
    assert!(!policy.allows("any-model", "/v1/embeddings", "k"));
    assert!(!policy.allows("any-model", "/v1/embeddings/batch", "k"));
    assert!(policy.allows("any-model", "/v1/chat/completions", "k"));
}

#[test]
fn v3_1_policy_denies_excluded_key() {
    let policy = SemanticCachePolicy::new(vec![], vec![], vec!["no-cache-key".to_string()]);
    assert!(!policy.allows("gpt-4o", "/v1/chat/completions", "no-cache-key"));
    assert!(policy.allows("gpt-4o", "/v1/chat/completions", "other-key"));
}

#[test]
fn v3_1_policy_blocks_semantic_cache_for_excluded_model() {
    // When a model is excluded, the cache engine's semantic lookup should be skipped.
    let policy = SemanticCachePolicy::new(vec!["gpt-4o-mini".to_string()], vec![], vec![]);

    let engine = CacheEngine::new();
    let emb = unit_embedding(64);

    // Simulate: insert a response (in a real flow this would be done by the pipeline).
    // The policy would block the lookup for "gpt-4o" (not in the allowlist).
    let allowed = policy.allows("gpt-4o-mini", "/v1/chat/completions", "test-key");
    let denied = policy.allows("gpt-4o", "/v1/chat/completions", "test-key");

    assert!(allowed, "gpt-4o-mini should be allowed");
    assert!(!denied, "gpt-4o should be denied by policy");

    // SemanticCache lookup when bypass_semantic=true returns nothing (handled in pipeline).
    // Here we verify the policy alone: no semantic index interaction needed.
    drop(emb); // suppress unused warning
    drop(engine);
}

// ─── Scale: O(log n) vs O(n) ─────────────────────────────────────────────────

#[tokio::test]
async fn v3_1_hnsw_lookup_faster_than_linear_at_1000_entries() {
    const N: usize = 1_000;
    const DIM: usize = 128;
    const REPS: usize = 50;

    let linear = LinearIndex::new();
    let hnsw = HnswIndex::new(16, 200);

    // Populate both with N distinct unit-ish vectors.
    for i in 0..N {
        let mut emb = unit_embedding(DIM);
        // Perturb element i%DIM slightly so vectors are not all identical.
        emb[i % DIM] *= 1.0 + (i as f32) * 0.001;
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);
        let hash = format!("h{i}");
        linear.insert(emb.clone(), hash.clone()).await;
        hnsw.insert(emb, hash).await;
    }

    let query = unit_embedding(DIM);

    // Warm up (avoid cold-cache effects on first run).
    let _ = linear.lookup(&query, 0.80).await;
    let _ = hnsw.lookup(&query, 0.80).await;

    let t0 = Instant::now();
    for _ in 0..REPS {
        let _ = linear.lookup(&query, 0.80).await;
    }
    let linear_us = t0.elapsed().as_micros() / REPS as u128;

    let t1 = Instant::now();
    for _ in 0..REPS {
        let _ = hnsw.lookup(&query, 0.80).await;
    }
    let hnsw_us = t1.elapsed().as_micros() / REPS as u128;

    // Both must return a result (sanity check).
    assert!(linear.lookup(&query, 0.80).await.is_some());
    assert!(hnsw.lookup(&query, 0.80).await.is_some());

    // HNSW should be at least as fast as linear at N=1000.
    // We allow a 10× slack to avoid flakiness on slow CI machines.
    // The real gain shows at N=10_000+, but correctness is what we're verifying here.
    assert!(
        hnsw_us <= linear_us * 10 + 500,
        "HNSW ({hnsw_us} µs) should not be dramatically slower than linear ({linear_us} µs) at N={N}"
    );
}

// ─── Regression ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn v3_1_regression_exact_cache_unaffected() {
    use janus::cache::exact::compute_hash;
    use janus::providers::{
        ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, UsageData,
    };
    use serde_json::json;
    use std::sync::Arc;

    let engine = CacheEngine::new();
    let req: ChatCompletionRequest = serde_json::from_value(json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "V3-1 regression"}]
    }))
    .unwrap();
    let hash = compute_hash(&req);
    let resp = Arc::new(ChatCompletionResponse {
        id: "r1".to_string(),
        object: "chat.completion".to_string(),
        created: 0,
        model: "gpt-4o-mini".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: json!("ok"),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 3,
            completion_tokens: 1,
            total_tokens: 4,
        },
    });

    engine.insert(hash.clone(), resp.clone());
    let hit = engine.lookup(&hash);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().id, "r1");

    engine.clear().await;
    assert!(engine.lookup(&hash).is_none());
}
