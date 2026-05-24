//! Qdrant vector database backend for the semantic cache (V4-9).
//!
//! Implements `EmbeddingIndex` via gRPC calls to a Qdrant instance.
//! Suitable for deployments with > 500,000 semantic cache entries where
//! the in-process HNSW index would consume too much RAM.
//!
//! Configuration:
//! ```toml
//! semantic_cache_backend = "qdrant"
//! qdrant_url              = "http://localhost:6334"
//! qdrant_collection       = "velox_cache"
//! qdrant_vector_size      = 384
//! ```

use super::EmbeddingIndex;
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Remote Qdrant vector index that implements `EmbeddingIndex`.
///
/// Sync trait methods (`lookup`, `insert`, `clear`) bridge to async Qdrant gRPC
/// calls via `tokio::task::block_in_place`. This requires a multi-threaded tokio
/// runtime, which axum provides in production. Tests must use
/// `#[tokio::test(flavor = "multi_thread")]` when calling these methods directly.
pub struct QdrantIndex {
    client: qdrant_client::Qdrant,
    collection: String,
    vector_size: u64,
    /// Best-effort in-memory count; synced with Qdrant on construction and after clear().
    count: AtomicUsize,
}

impl QdrantIndex {
    /// Connect to Qdrant and ensure the named collection exists.
    ///
    /// Returns `Err` when Qdrant is unreachable or the collection cannot be created.
    /// `vector_size`: embedding dimensionality (384 for all-MiniLM-L6-v2).
    pub async fn new(url: &str, collection: &str, vector_size: u64) -> Result<Self> {
        let client = qdrant_client::Qdrant::from_url(url)
            .build()
            .context("Failed to build Qdrant client")?;

        let exists = client
            .collection_exists(collection)
            .await
            .context("Failed to check Qdrant collection existence — is Qdrant reachable?")?;

        if !exists {
            use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
            client
                .create_collection(
                    CreateCollectionBuilder::new(collection)
                        .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
                )
                .await
                .context("Failed to create Qdrant collection")?;
        }

        // Sync count with actual Qdrant state.
        let count = client
            .collection_info(collection)
            .await
            .ok()
            .and_then(|r| r.result)
            .and_then(|r| r.points_count)
            .unwrap_or(0) as usize;

        Ok(Self {
            client,
            collection: collection.to_string(),
            vector_size,
            count: AtomicUsize::new(count),
        })
    }
}

impl EmbeddingIndex for QdrantIndex {
    fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.qdrant_lookup(query, threshold))
        })
    }

    fn insert(&self, embedding: Vec<f32>, hash: String) {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.qdrant_insert(embedding, hash))
        });
    }

    fn clear(&self) {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.qdrant_clear())
        });
    }

    fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

// ── Async helpers (called from sync trait methods via block_in_place) ─────────

impl QdrantIndex {
    async fn qdrant_lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)> {
        use qdrant_client::qdrant::SearchPointsBuilder;

        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, query.to_vec(), 1)
                    .with_payload(true)
                    .score_threshold(threshold),
            )
            .await
            .ok()?;

        let point = results.result.into_iter().next()?;
        let hash = extract_string_payload(&point.payload, "hash")?;
        Some((hash, point.score))
    }

    async fn qdrant_insert(&self, embedding: Vec<f32>, hash: String) {
        use qdrant_client::qdrant::{PointStruct, UpsertPointsBuilder};

        let id = hash_to_point_id(&hash);
        let payload = build_hash_payload(&hash);

        if self
            .client
            .upsert_points(UpsertPointsBuilder::new(
                &self.collection,
                vec![PointStruct::new(id, embedding, payload)],
            ))
            .await
            .is_ok()
        {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn qdrant_clear(&self) {
        use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};

        // Delete and recreate the collection — the only reliable way to clear
        // all points without pagination.
        let _ = self.client.delete_collection(&self.collection).await;
        let _ = self
            .client
            .create_collection(
                CreateCollectionBuilder::new(&self.collection)
                    .vectors_config(VectorParamsBuilder::new(self.vector_size, Distance::Cosine)),
            )
            .await;
        self.count.store(0, Ordering::Relaxed);
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Convert the first 16 hex chars of a SHA-256 hash to a u64 point ID.
///
/// SHA-256 hex strings are always 64 chars. Taking the first 8 bytes (16 hex
/// digits) gives a deterministic, stable ID with negligible collision risk for
/// the scale at which Qdrant becomes relevant (> 500k entries).
fn hash_to_point_id(hash: &str) -> u64 {
    u64::from_str_radix(hash.get(..16).unwrap_or("0000000000000000"), 16).unwrap_or(0)
}

/// Build a Qdrant payload map containing a single "hash" string field.
fn build_hash_payload(
    hash: &str,
) -> std::collections::HashMap<String, qdrant_client::qdrant::Value> {
    use qdrant_client::qdrant::{value::Kind, Value};
    let mut map = std::collections::HashMap::new();
    map.insert(
        "hash".to_string(),
        Value {
            kind: Some(Kind::StringValue(hash.to_string())),
        },
    );
    map
}

/// Extract a string value from a Qdrant protobuf payload map.
fn extract_string_payload(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> Option<String> {
    use qdrant_client::qdrant::value::Kind;
    match payload.get(key)?.kind.as_ref()? {
        Kind::StringValue(s) => Some(s.clone()),
        _ => None,
    }
}
