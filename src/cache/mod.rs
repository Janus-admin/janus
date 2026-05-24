pub mod embedding;
pub mod exact;
pub mod index;
pub mod policy;
pub mod semantic;
pub mod time_guard;

use crate::{
    cache::{
        embedding::EmbeddingModel,
        index::{hnsw::HnswIndex, qdrant::QdrantIndex},
        semantic::SemanticCache,
    },
    providers::{ChatCompletionRequest, ChatCompletionResponse, EmbeddingResponse},
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;

// ── Cache hit discriminant ────────────────────────────────────────────────────

/// Describes how a response was sourced.
#[derive(Debug, Clone)]
pub enum CacheHit {
    /// Response came from a live provider call.
    None,
    /// SHA-256 exact match on the normalized request body.
    Exact,
    /// Nearest-neighbour semantic match; carries the cosine similarity score.
    Semantic(f32),
}

// ── CacheEngine ───────────────────────────────────────────────────────────────

/// Two-layer cache: DashMap hot layer (sub-millisecond) + optional semantic layer.
///
/// Exact cache: SHA-256 of normalized request → DashMap + PostgreSQL.
/// Semantic cache: cosine similarity over sentence embeddings.
///   Backend: `LinearIndex` (O(n), default) or `HnswIndex` (O(log n), via config).
pub struct CacheEngine {
    hot: DashMap<String, Arc<ChatCompletionResponse>>,
    /// Per-entry expiry timestamps. Absence = no expiry. Checked on every lookup.
    hot_expiry: DashMap<String, DateTime<Utc>>,
    embedding_hot: DashMap<String, Arc<EmbeddingResponse>>,
    pub semantic: Option<Arc<SemanticCache>>,
    pub model: Option<Arc<EmbeddingModel>>,
}

impl CacheEngine {
    /// Exact-only cache (no embedding model loaded).
    pub fn new() -> Self {
        Self {
            hot: DashMap::new(),
            hot_expiry: DashMap::new(),
            embedding_hot: DashMap::new(),
            semantic: None,
            model: None,
        }
    }

    /// Exact + semantic cache using the default LinearIndex (O(n) scan).
    ///
    /// Backwards-compatible constructor — used by `tests/common/mod.rs` and the
    /// main binary when `semantic_cache_backend = "linear"` (or not set).
    pub fn new_with_semantic(model: Arc<EmbeddingModel>, threshold: f32) -> Self {
        Self {
            hot: DashMap::new(),
            hot_expiry: DashMap::new(),
            embedding_hot: DashMap::new(),
            semantic: Some(Arc::new(SemanticCache::new(threshold))),
            model: Some(model),
        }
    }

    /// Exact + semantic cache using the HNSW approximate nearest-neighbor index.
    ///
    /// Parameters:
    /// - `ef_construction`: build-time precision (higher = better recall, slower inserts).
    /// - `max_nb_connection`: graph connectivity (M parameter, typical: 16).
    pub fn new_with_hnsw_semantic(
        model: Arc<EmbeddingModel>,
        threshold: f32,
        ef_construction: usize,
        max_nb_connection: usize,
    ) -> Self {
        let index = Box::new(HnswIndex::new(max_nb_connection, ef_construction));
        let sc = SemanticCache::with_index(index, threshold);
        Self {
            hot: DashMap::new(),
            hot_expiry: DashMap::new(),
            embedding_hot: DashMap::new(),
            semantic: Some(Arc::new(sc)),
            model: Some(model),
        }
    }

    /// Exact + semantic cache backed by a remote Qdrant vector store (V4-9).
    ///
    /// The `QdrantIndex` is pre-connected and the collection already exists at
    /// this point — call `QdrantIndex::new()` first and handle any connection
    /// errors before passing it here.
    pub fn new_with_qdrant_semantic(
        model: Arc<EmbeddingModel>,
        threshold: f32,
        qdrant_index: QdrantIndex,
    ) -> Self {
        let index = Box::new(qdrant_index);
        let sc = SemanticCache::with_index(index, threshold);
        Self {
            hot: DashMap::new(),
            hot_expiry: DashMap::new(),
            embedding_hot: DashMap::new(),
            semantic: Some(Arc::new(sc)),
            model: Some(model),
        }
    }

    // ── Exact cache operations ────────────────────────────────────────────────

    /// Look up a cached response. Returns `None` if not found or if the entry has expired.
    /// Expired entries are evicted from the hot layer on access.
    pub fn lookup(&self, hash: &str) -> Option<Arc<ChatCompletionResponse>> {
        // Check expiry before returning.
        if let Some(expires_at) = self.hot_expiry.get(hash) {
            if Utc::now() >= *expires_at {
                // Entry has expired — evict it now.
                drop(expires_at);
                self.hot.remove(hash);
                self.hot_expiry.remove(hash);
                return None;
            }
        }
        self.hot.get(hash).map(|v| Arc::clone(&*v))
    }

    /// Insert without TTL (entry never expires).
    pub fn insert(&self, hash: String, response: Arc<ChatCompletionResponse>) {
        self.hot.insert(hash, response);
    }

    /// Insert with TTL. When `ttl_secs > 0` the entry expires after that many seconds.
    pub fn insert_with_ttl(
        &self,
        hash: String,
        response: Arc<ChatCompletionResponse>,
        ttl_secs: u64,
    ) {
        if ttl_secs > 0 {
            let expires_at = Utc::now() + chrono::Duration::seconds(ttl_secs as i64);
            self.hot_expiry.insert(hash.clone(), expires_at);
        }
        self.hot.insert(hash, response);
    }

    /// Remove all entries whose TTL has elapsed. Returns the number of evictions.
    pub fn evict_expired(&self) -> usize {
        let now = Utc::now();
        let expired_hashes: Vec<String> = self
            .hot_expiry
            .iter()
            .filter(|e| now >= *e.value())
            .map(|e| e.key().clone())
            .collect();
        let count = expired_hashes.len();
        for hash in &expired_hashes {
            self.hot.remove(hash);
            self.hot_expiry.remove(hash);
        }
        count
    }

    // ── Embedding cache operations ────────────────────────────────────────────

    pub fn lookup_embedding(&self, hash: &str) -> Option<Arc<EmbeddingResponse>> {
        self.embedding_hot.get(hash).map(|v| Arc::clone(&*v))
    }

    pub fn insert_embedding(&self, hash: String, response: Arc<EmbeddingResponse>) {
        self.embedding_hot.insert(hash, response);
    }

    pub fn clear(&self) {
        self.hot.clear();
        self.hot_expiry.clear();
        self.embedding_hot.clear();
        if let Some(ref sc) = self.semantic {
            sc.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.hot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hot.is_empty()
    }

    /// Remove a single entry from the hot layer. Returns `true` if it was present.
    pub fn remove(&self, hash: &str) -> bool {
        self.hot_expiry.remove(hash);
        self.hot.remove(hash).is_some()
    }

    // ── Semantic cache operations ─────────────────────────────────────────────

    /// Look up the most similar cached embedding. Returns `(hash, score)` if found.
    pub fn semantic_lookup(&self, embedding: &[f32]) -> Option<(String, f32)> {
        self.semantic.as_ref()?.lookup(embedding)
    }

    /// Insert a new embedding into the in-memory semantic index.
    pub fn semantic_insert(&self, embedding: Vec<f32>, hash: String) {
        if let Some(ref sc) = self.semantic {
            sc.insert(embedding, hash);
        }
    }

    // ── Warm-up from PostgreSQL ───────────────────────────────────────────────

    /// Load all non-expired persisted cache entries from the database into the hot
    /// layer and (if the semantic index is active) the embedding index.
    pub async fn warm_from_db(&self, pool: &crate::db::DbPool) -> usize {
        match crate::db::cache::load_all_entries(pool).await {
            Ok(entries) => {
                let now = Utc::now();
                let mut loaded = 0usize;
                for entry in &entries {
                    // Skip entries that have already expired.
                    if let Some(exp) = entry.expires_at_utc() {
                        if now >= exp {
                            continue;
                        }
                        // Store the expiry in the hot_expiry map.
                        self.hot_expiry.insert(entry.prompt_hash.clone(), exp);
                    }

                    if let Ok(resp) =
                        serde_json::from_str::<ChatCompletionResponse>(&entry.response_body)
                    {
                        self.hot.insert(entry.prompt_hash.clone(), Arc::new(resp));
                        loaded += 1;
                    }

                    if let Some(ref emb_bytes) = entry.embedding {
                        let embedding = semantic::bytes_to_f32_vec(emb_bytes);
                        self.semantic_insert(embedding, entry.prompt_hash.clone());
                    }
                }
                loaded
            }
            Err(e) => {
                tracing::warn!("warm_from_db: {e}");
                0
            }
        }
    }
}

impl Default for CacheEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Convenience helper ────────────────────────────────────────────────────────

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
