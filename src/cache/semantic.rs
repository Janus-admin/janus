use crate::cache::index::{linear::LinearIndex, EmbeddingIndex};

/// In-memory semantic cache backed by a pluggable `EmbeddingIndex`.
///
/// `SemanticCache::new(threshold)` uses the O(n) `LinearIndex` (backwards compatible).
/// `SemanticCache::with_index(index, threshold)` accepts any index backend.
pub struct SemanticCache {
    index: Box<dyn EmbeddingIndex>,
    threshold: f32,
}

impl SemanticCache {
    /// Create with `LinearIndex` (default; O(n) linear scan). Backwards compatible.
    pub fn new(threshold: f32) -> Self {
        Self {
            index: Box::new(LinearIndex::new()),
            threshold,
        }
    }

    /// Create with a pre-built index backend (e.g. `HnswIndex`).
    pub fn with_index(index: Box<dyn EmbeddingIndex>, threshold: f32) -> Self {
        Self { index, threshold }
    }

    /// Return the hash and similarity score of the most similar cached entry,
    /// or `None` if no entry exceeds the configured threshold.
    pub fn lookup(&self, query: &[f32]) -> Option<(String, f32)> {
        self.index.lookup(query, self.threshold)
    }

    /// Add a new entry. Does not deduplicate — exact-cache hash uniqueness is assumed.
    pub fn insert(&self, embedding: Vec<f32>, prompt_hash: String) {
        self.index.insert(embedding, prompt_hash);
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn clear(&self) {
        self.index.clear();
    }
}

// ── Byte serialization helpers (used for PostgreSQL BYTEA storage) ────────────

pub fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn bytes_to_f32_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
