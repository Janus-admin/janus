pub mod embedding;
pub mod exact;
pub mod semantic;

use crate::{
    cache::{embedding::EmbeddingModel, semantic::SemanticCache},
    providers::{ChatCompletionRequest, ChatCompletionResponse},
};
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
/// Semantic cache: cosine similarity over sentence embeddings → linear scan.
pub struct CacheEngine {
    hot: DashMap<String, Arc<ChatCompletionResponse>>,
    pub semantic: Option<Arc<SemanticCache>>,
    pub model: Option<Arc<EmbeddingModel>>,
}

impl CacheEngine {
    /// Exact-only cache (Phase 4 mode).
    pub fn new() -> Self {
        Self {
            hot: DashMap::new(),
            semantic: None,
            model: None,
        }
    }

    /// Exact + semantic cache (Phase 5 mode).
    pub fn new_with_semantic(model: Arc<EmbeddingModel>, threshold: f32) -> Self {
        Self {
            hot: DashMap::new(),
            semantic: Some(Arc::new(SemanticCache::new(threshold))),
            model: Some(model),
        }
    }

    // ── Exact cache operations ────────────────────────────────────────────────

    pub fn lookup(&self, hash: &str) -> Option<Arc<ChatCompletionResponse>> {
        self.hot.get(hash).map(|v| Arc::clone(&*v))
    }

    pub fn insert(&self, hash: String, response: Arc<ChatCompletionResponse>) {
        self.hot.insert(hash, response);
    }

    pub fn clear(&self) {
        self.hot.clear();
        if let Some(ref sc) = self.semantic {
            // Rebuild empty — drop and recreate via Arc swap isn't possible here,
            // so we rely on the caller to recreate the engine for a full semantic flush.
            let _ = sc; // semantic entries are not cleared; flushing exact cache only
        }
    }

    pub fn len(&self) -> usize {
        self.hot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hot.is_empty()
    }

    // ── Semantic cache operations ─────────────────────────────────────────────

    /// Look up the most similar cached embedding. Returns `(hash, score)` if found.
    pub fn semantic_lookup(&self, embedding: &[f32]) -> Option<(String, f32)> {
        self.semantic.as_ref()?.lookup(embedding)
    }

    /// Insert a new embedding mapping into the in-memory semantic index.
    pub fn semantic_insert(&self, embedding: Vec<f32>, hash: String) {
        if let Some(ref sc) = self.semantic {
            sc.insert(embedding, hash);
        }
    }

    // ── Warm-up from PostgreSQL ───────────────────────────────────────────────

    /// Load all persisted cache entries from the database into the hot layer and
    /// (if the semantic index is active) the embedding index.
    ///
    /// Called once at startup so restarts inherit the full in-memory state.
    pub async fn warm_from_db(&self, pool: &sqlx::PgPool) -> usize {
        match crate::db::cache::load_all_entries(pool).await {
            Ok(entries) => {
                let mut loaded = 0usize;
                for entry in &entries {
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
