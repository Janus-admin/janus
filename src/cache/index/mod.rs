pub mod hnsw;
pub mod linear;
pub mod qdrant;

use async_trait::async_trait;

/// Pluggable vector index backend for the semantic cache.
///
/// All methods are `async` so backends that need to perform real I/O
/// (`QdrantIndex` calls gRPC) can `.await` properly without parking a runtime
/// worker thread via `block_in_place`.  In-memory backends (`LinearIndex`,
/// `HnswIndex`) implement the trait with a plain sync body inside `async fn`;
/// the boxed-future overhead from `#[async_trait]` is negligible compared to
/// embedding cost, and avoids the cost of `spawn_blocking` for fast lookups.
///
/// Implementations must be `Send + Sync` so they can live inside `Arc<SemanticCache>`.
#[async_trait]
pub trait EmbeddingIndex: Send + Sync {
    /// Find the most similar entry. Returns `(hash, similarity)` if above threshold.
    async fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)>;

    /// Insert a new entry into the index.
    async fn insert(&self, embedding: Vec<f32>, hash: String);

    /// Remove all entries. Called on cache flush.
    async fn clear(&self);

    /// Number of entries currently in the index.
    async fn len(&self) -> usize;

    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}
