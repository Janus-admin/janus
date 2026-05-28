use super::EmbeddingIndex;
use async_trait::async_trait;
use hnsw_rs::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

// hnsw_rs 0.3 parameterizes Hnsw with a lifetime 'b for internal data references.
// Using 'static here because:
//  - f32: 'static (primitive type, no borrowed references)
//  - DistCosine: 'static (zero-size struct, no borrowed references)
//  - HNSW clones T internally on insert, so the 'static bound is satisfied trivially.
struct HnswInner {
    hnsw: Hnsw<'static, f32, DistCosine>,
    id_to_hash: HashMap<usize, String>,
    count: usize,
}

impl HnswInner {
    fn new(max_nb_connection: usize, ef_construction: usize) -> Self {
        Self {
            // Parameters: (M, max_elements, max_layer, ef_construction, distance)
            // max_layer=16 is the standard HNSW default.
            hnsw: Hnsw::new(
                max_nb_connection,
                100_000,
                16,
                ef_construction,
                DistCosine {},
            ),
            id_to_hash: HashMap::new(),
            count: 0,
        }
    }
}

/// HNSW approximate nearest-neighbor index.
///
/// Uses cosine distance (DistCosine) which computes `1 - cosine_similarity` for
/// unit vectors. Threshold conversion: `max_distance = 1.0 - threshold`.
///
/// Internally serialized via `Mutex` so `clear()` can atomically swap the
/// underlying HNSW structure (HNSW does not support deletion in-place).
pub struct HnswIndex {
    inner: Mutex<HnswInner>,
    max_nb_connection: usize,
    ef_construction: usize,
    /// Search precision: higher = more accurate but slower.  Defaults to ef_construction.
    ef_search: usize,
}

impl HnswIndex {
    pub fn new(max_nb_connection: usize, ef_construction: usize) -> Self {
        Self {
            inner: Mutex::new(HnswInner::new(max_nb_connection, ef_construction)),
            max_nb_connection,
            ef_construction,
            ef_search: ef_construction.max(50),
        }
    }
}

#[async_trait]
impl EmbeddingIndex for HnswIndex {
    // The HNSW search itself is CPU-bound but bounded in time (log N on a
    // graph of ~100k nodes); running it inline inside `async fn` is fine and
    // avoids the cost of `spawn_blocking`.  The Mutex is held only for the
    // duration of the operation — never across an `.await`.
    async fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)> {
        let inner = self.inner.lock().expect("HNSW index mutex poisoned");
        if inner.count == 0 {
            return None;
        }
        // hnsw_rs DistCosine returns 1 - cos(θ) for unit vectors.
        // similarity = 1.0 - distance, so distance ≤ (1 - threshold) → eligible.
        let results = inner.hnsw.search(query, 1, self.ef_search);
        let best = results.into_iter().next()?;
        let similarity = 1.0_f32 - best.distance;
        if similarity >= threshold {
            inner
                .id_to_hash
                .get(&best.d_id)
                .map(|h| (h.clone(), similarity))
        } else {
            None
        }
    }

    async fn insert(&self, embedding: Vec<f32>, hash: String) {
        let mut inner = self.inner.lock().expect("HNSW index mutex poisoned");
        let id = inner.count;
        inner.hnsw.insert((&embedding, id));
        inner.id_to_hash.insert(id, hash);
        inner.count += 1;
    }

    async fn clear(&self) {
        let mut inner = self.inner.lock().expect("HNSW index mutex poisoned");
        *inner = HnswInner::new(self.max_nb_connection, self.ef_construction);
    }

    async fn len(&self) -> usize {
        self.inner.lock().expect("HNSW index mutex poisoned").count
    }
}
