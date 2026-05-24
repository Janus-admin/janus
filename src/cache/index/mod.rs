pub mod hnsw;
pub mod linear;

/// Pluggable vector index backend for the semantic cache.
///
/// Both implementations are `Send + Sync` so they can live inside `Arc<SemanticCache>`.
pub trait EmbeddingIndex: Send + Sync {
    /// Find the most similar entry. Returns `(hash, similarity)` if above threshold.
    fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)>;

    /// Insert a new entry into the index.
    fn insert(&self, embedding: Vec<f32>, hash: String);

    /// Remove all entries. Called on cache flush.
    fn clear(&self);

    /// Number of entries currently in the index.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
