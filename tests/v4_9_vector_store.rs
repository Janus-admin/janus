// tests/v4_9_vector_store.rs
// Phase V4-9 acceptance tests — External Vector Stores (Qdrant).
//
// Run with: cargo test v4_9
//
// Tests that require a live Qdrant instance connect to the URL in the
// QDRANT_TEST_URL environment variable (default: http://localhost:6334).
// When Qdrant is not reachable the test prints a skip message and returns
// successfully — this keeps CI green on machines without Qdrant installed.
//
// To run the full suite locally:
//   docker run -p 6334:6334 qdrant/qdrant
//   cargo test v4_9

mod common;

use velox::cache::index::{qdrant::QdrantIndex, EmbeddingIndex};

const DEFAULT_QDRANT_URL: &str = "http://localhost:6334";

fn qdrant_test_url() -> String {
    std::env::var("QDRANT_TEST_URL").unwrap_or_else(|_| DEFAULT_QDRANT_URL.to_string())
}

fn test_collection(suffix: &str) -> String {
    format!("velox_test_{}", suffix)
}

/// Try to connect to Qdrant. Returns `None` and prints a skip notice if unreachable.
async fn try_connect(collection: &str) -> Option<QdrantIndex> {
    let url = qdrant_test_url();
    match QdrantIndex::new(&url, collection, 4).await {
        Ok(idx) => Some(idx),
        Err(_) => {
            eprintln!(
                "[SKIP] Qdrant not reachable at {url} — set QDRANT_TEST_URL or start Qdrant to run this test"
            );
            None
        }
    }
}

// ── V4-9 tests ────────────────────────────────────────────────────────────────

/// Compile-time assertion: QdrantIndex implements EmbeddingIndex.
/// This test body never runs — it exists purely to fail at compile time
/// if the trait implementation is removed.
#[test]
fn v4_9_qdrant_index_implements_embedding_index_trait() {
    fn assert_impl<T: EmbeddingIndex>() {}
    assert_impl::<QdrantIndex>();
}

/// Connecting to a non-existent Qdrant URL must return an error.
#[tokio::test(flavor = "multi_thread")]
async fn v4_9_qdrant_unavailable_at_startup_returns_error() {
    // Port 1 is not in use in any sane test environment.
    let result = QdrantIndex::new("http://localhost:1", "test_unavailable", 4).await;
    assert!(
        result.is_err(),
        "Expected Err when Qdrant is not reachable, got Ok"
    );
}

/// Insert a vector, then look it up with a near-identical query above the threshold.
#[tokio::test(flavor = "multi_thread")]
async fn v4_9_qdrant_insert_and_find() {
    let collection = test_collection("insert_and_find");
    let Some(index) = try_connect(&collection).await else {
        return;
    };

    // Clean state.
    index.clear();

    let embedding = vec![1.0_f32, 0.0, 0.0, 0.0];
    let hash = "aabbccdd00112233".repeat(4); // 64-char fake SHA-256 hash

    index.insert(embedding.clone(), hash.clone());

    // Exact query should be above any reasonable threshold (similarity = 1.0).
    let result = index.lookup(&embedding, 0.9);
    assert!(result.is_some(), "Exact lookup after insert must succeed");
    let (found_hash, score) = result.unwrap();
    assert_eq!(found_hash, hash, "Returned hash must match inserted hash");
    assert!(score >= 0.9, "Score must be >= threshold; got {score}");
}

/// A query sufficiently similar to an inserted vector must return above threshold.
#[tokio::test(flavor = "multi_thread")]
async fn v4_9_qdrant_lookup_returns_above_threshold_match() {
    let collection = test_collection("above_threshold");
    let Some(index) = try_connect(&collection).await else {
        return;
    };

    index.clear();

    // Insert a unit vector pointing in the (1, 0, 0, 0) direction.
    let base: Vec<f32> = vec![1.0, 0.0, 0.0, 0.0];
    let hash = "deadbeef00000000".repeat(4);
    index.insert(base.clone(), hash.clone());

    // Query with a close-but-not-identical unit vector in the same direction.
    // Cosine similarity ≈ 1.0 after normalization — should exceed 0.9.
    let query = vec![0.99_f32, 0.01, 0.0, 0.0];
    // Normalize manually so Qdrant cosine distance is computed correctly.
    let len = (query.iter().map(|x| x * x).sum::<f32>()).sqrt();
    let query_norm: Vec<f32> = query.iter().map(|x| x / len).collect();

    let result = index.lookup(&query_norm, 0.9);
    assert!(
        result.is_some(),
        "Similar vector lookup must return a match above threshold"
    );
}

/// After calling `clear()` the collection must be empty and lookups must return None.
#[tokio::test(flavor = "multi_thread")]
async fn v4_9_qdrant_clear_empties_collection() {
    let collection = test_collection("clear_empties");
    let Some(index) = try_connect(&collection).await else {
        return;
    };

    let embedding = vec![1.0_f32, 0.0, 0.0, 0.0];
    let hash = "cafebabe11223344".repeat(4);
    index.insert(embedding.clone(), hash);

    index.clear();

    // len() resets to 0.
    assert_eq!(index.len(), 0, "len() must be 0 after clear()");

    // Lookup must return None on an empty collection.
    let result = index.lookup(&embedding, 0.5);
    assert!(
        result.is_none(),
        "Lookup on cleared collection must return None"
    );
}

/// The HNSW backend must be unaffected by the Qdrant feature.
#[test]
fn v4_9_regression_hnsw_backend_unchanged() {
    use velox::cache::index::{hnsw::HnswIndex, EmbeddingIndex};

    let index = HnswIndex::new(16, 200);
    assert_eq!(index.len(), 0);

    let embedding = vec![1.0_f32, 0.0, 0.0, 0.0];
    let hash = "ffffffff00000000".repeat(4);
    index.insert(embedding.clone(), hash.clone());

    assert_eq!(index.len(), 1);

    let result = index.lookup(&embedding, 0.9);
    assert!(result.is_some(), "HNSW lookup must still work after V4-9");
    assert_eq!(result.unwrap().0, hash);

    index.clear();
    assert_eq!(index.len(), 0);
}
