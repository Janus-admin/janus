use super::EmbeddingIndex;
use std::sync::RwLock;

struct Entry {
    embedding: Vec<f32>,
    hash: String,
}

/// O(n) linear scan over L2-normalized embeddings.
///
/// Cosine similarity between two unit vectors equals their dot product, so
/// no explicit normalization is needed at lookup time.
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

impl EmbeddingIndex for LinearIndex {
    fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)> {
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

    fn insert(&self, embedding: Vec<f32>, hash: String) {
        if let Ok(mut entries) = self.entries.write() {
            entries.push(Entry { embedding, hash });
        }
    }

    fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }

    fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
