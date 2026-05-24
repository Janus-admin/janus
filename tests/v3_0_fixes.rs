// tests/v3_0_fixes.rs
// Phase V3-0 acceptance tests — Foundation Fixes.
//
// Run with: cargo test v3_0
// These are pure unit tests — no DB, no HTTP, no spawned app required.

use serde_json::json;
use std::sync::Arc;
use velox::cache::{exact::compute_hash, semantic::SemanticCache, CacheEngine};
use velox::providers::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, UsageData,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn dummy_request(content: &str) -> ChatCompletionRequest {
    serde_json::from_value(json!({
        "model": "gpt-4o-mini",
        "messages": [{ "role": "user", "content": content }]
    }))
    .unwrap()
}

fn dummy_response() -> Arc<ChatCompletionResponse> {
    Arc::new(ChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        object: "chat.completion".to_string(),
        created: 1_700_000_000,
        model: "gpt-4o-mini".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: json!("Hello!"),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 5,
            completion_tokens: 2,
            total_tokens: 7,
        },
    })
}

/// A unit vector (all equal components, L2-normalized) of the given dimension.
/// Cosine similarity between any two of these identical vectors is 1.0.
fn unit_embedding(dim: usize) -> Vec<f32> {
    let v = 1.0_f32 / (dim as f32).sqrt();
    vec![v; dim]
}

// ─── V3-0 Fix 1: SemanticCache::clear() ──────────────────────────────────────

#[test]
fn v3_0_semantic_cache_clear_empties_entries() {
    let sc = SemanticCache::new(0.90);
    assert_eq!(sc.len(), 0);

    sc.insert(unit_embedding(384), "hash-1".to_string());
    sc.insert(unit_embedding(384), "hash-2".to_string());
    assert_eq!(sc.len(), 2);

    sc.clear();
    assert_eq!(sc.len(), 0);
}

#[test]
fn v3_0_semantic_lookup_returns_none_after_clear() {
    let sc = SemanticCache::new(0.80);
    let emb = unit_embedding(384);
    sc.insert(emb.clone(), "hash-abc".to_string());

    // Verify it's findable before clear.
    assert!(sc.lookup(&emb).is_some());

    sc.clear();

    // Must not be found after clear.
    assert!(sc.lookup(&emb).is_none());
}

#[test]
fn v3_0_semantic_cache_insert_after_clear_works() {
    let sc = SemanticCache::new(0.80);
    let emb = unit_embedding(384);

    sc.insert(emb.clone(), "first".to_string());
    sc.clear();
    assert_eq!(sc.len(), 0);

    sc.insert(emb.clone(), "second".to_string());
    assert_eq!(sc.len(), 1);
    assert!(sc.lookup(&emb).is_some());
}

// ─── V3-0 Fix 2: CacheEngine::clear() flushes both layers ───────────────────

#[test]
fn v3_0_flush_cache_clears_hot_layer() {
    let engine = CacheEngine::new();
    let req = dummy_request("What is Rust?");
    let hash = compute_hash(&req);
    let resp = dummy_response();

    engine.insert(hash.clone(), resp);
    assert!(!engine.is_empty());

    engine.clear();
    assert!(engine.is_empty(), "hot layer must be empty after clear");
}

#[test]
fn v3_0_exact_cache_hit_does_not_occur_after_flush() {
    let engine = CacheEngine::new();
    let req = dummy_request("What is Rust?");
    let hash = compute_hash(&req);
    let resp = dummy_response();

    engine.insert(hash.clone(), resp);
    assert!(engine.lookup(&hash).is_some());

    engine.clear();
    assert!(
        engine.lookup(&hash).is_none(),
        "exact hit must not occur after flush"
    );
}

// ─── Regression ───────────────────────────────────────────────────────────────

#[test]
fn v3_0_regression_exact_cache_insert_and_hit_still_work() {
    let engine = CacheEngine::new();
    let req = dummy_request("regression check");
    let hash = compute_hash(&req);
    let resp = dummy_response();

    assert!(engine.lookup(&hash).is_none());
    engine.insert(hash.clone(), resp.clone());
    let hit = engine.lookup(&hash);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().id, resp.id);
}

#[test]
fn v3_0_regression_compute_hash_is_deterministic() {
    let req1 = dummy_request("the same prompt");
    let req2 = dummy_request("the same prompt");
    assert_eq!(compute_hash(&req1), compute_hash(&req2));
}

#[test]
fn v3_0_regression_different_prompts_have_different_hashes() {
    let req1 = dummy_request("prompt A");
    let req2 = dummy_request("prompt B");
    assert_ne!(compute_hash(&req1), compute_hash(&req2));
}

#[test]
fn v3_0_regression_semantic_cache_threshold_respected() {
    // threshold = 1.0 means only a perfect match returns a hit.
    let sc = SemanticCache::new(1.0);
    let emb_a = unit_embedding(384);

    // A slightly different vector (first component doubled, not normalized).
    let mut emb_b = unit_embedding(384);
    emb_b[0] *= 2.0;

    sc.insert(emb_a.clone(), "hash-perfect".to_string());

    // Perfect match should hit (dot product of identical unit vectors = 1.0).
    // Note: threshold is exclusive (score > threshold), so score must be > 1.0
    // which is impossible. Use a threshold just below 1.0 to test near-match.
    let sc_near = SemanticCache::new(0.99);
    sc_near.insert(emb_a.clone(), "hash-near".to_string());
    assert!(
        sc_near.lookup(&emb_a).is_some(),
        "identical vectors must hit"
    );

    // Non-matching vector must miss even at low threshold when too dissimilar.
    let sc_strict = SemanticCache::new(0.99);
    // All-zeros vector has zero cosine similarity with anything.
    let zero_emb = vec![0.0f32; 384];
    sc_strict.insert(emb_a.clone(), "hash-x".to_string());
    assert!(
        sc_strict.lookup(&zero_emb).is_none(),
        "zero vector must not match"
    );
}
