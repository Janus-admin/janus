pub mod exact;

use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use dashmap::DashMap;
use std::sync::Arc;

/// Two-layer exact cache: DashMap hot layer (sub-millisecond) backed by PostgreSQL.
///
/// Only non-streaming responses are cached. The DashMap is authoritative for
/// in-process lifetime; PostgreSQL stores entries for stats persistence.
pub struct CacheEngine {
    hot: DashMap<String, Arc<ChatCompletionResponse>>,
}

impl CacheEngine {
    pub fn new() -> Self {
        Self {
            hot: DashMap::new(),
        }
    }

    /// Look up a cached response by its pre-computed hash.
    pub fn lookup(&self, hash: &str) -> Option<Arc<ChatCompletionResponse>> {
        self.hot.get(hash).map(|v| Arc::clone(&*v))
    }

    /// Insert a response into the hot cache.
    pub fn insert(&self, hash: String, response: Arc<ChatCompletionResponse>) {
        self.hot.insert(hash, response);
    }

    /// Clear all entries from the hot cache (called on flush).
    pub fn clear(&self) {
        self.hot.clear();
    }

    pub fn len(&self) -> usize {
        self.hot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hot.is_empty()
    }
}

impl Default for CacheEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the cache key for a request and look it up in the hot layer.
/// Returns `(hash, Option<cached_response>)`.
pub fn check(
    engine: &CacheEngine,
    request: &ChatCompletionRequest,
) -> (String, Option<Arc<ChatCompletionResponse>>) {
    let hash = exact::compute_hash(request);
    let hit = engine.lookup(&hash);
    (hash, hit)
}
