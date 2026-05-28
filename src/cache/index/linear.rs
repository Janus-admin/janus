use super::EmbeddingIndex;
use async_trait::async_trait;
use std::sync::RwLock;

struct Entry {
    embedding: Vec<f32>,
    hash: String,
}

/// O(n) linear scan over L2-normalized embeddings.
///
/// Cosine similarity between two unit vectors equals their dot product, so
/// no explicit normalization is needed at lookup time.
///
/// The async trait methods run their (synchronous, in-memory) bodies inline.
/// At realistic cache sizes (< 10k entries) the scan completes in microseconds
/// to low single-digit milliseconds; offloading to `spawn_blocking` would add
/// thread-hop overhead without benefit.  If your cache ever grows past ~100k
/// entries, switch to `HnswIndex` instead.
pub struct LinearIndex {
    entries: RwLock<Vec<Entry>>,
}

impl LinearIndex {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }
}

impl Default for LinearIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingIndex for LinearIndex {
    async fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)> {
        let entries = self.entries.read().ok()?;
        let mut best_score = threshold;
        let mut best_hash: Option<String> = None;

        for entry in entries.iter() {
            let score = dot_product(query, &entry.embedding);
            if score > best_score {
                best_score = score;
                best_hash = Some(entry.hash.clone());
            }
        }

        best_hash.map(|h| (h, best_score))
    }

    async fn insert(&self, embedding: Vec<f32>, hash: String) {
        if let Ok(mut entries) = self.entries.write() {
            entries.push(Entry { embedding, hash });
        }
    }

    async fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }

    async fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
