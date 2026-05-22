use std::sync::RwLock;

struct SemanticEntry {
    embedding: Vec<f32>,
    prompt_hash: String,
}

/// In-memory semantic cache backed by a linear scan over L2-normalized embeddings.
///
/// Cosine similarity between two unit vectors equals their dot product, so no
/// explicit normalization step is needed at lookup time.
pub struct SemanticCache {
    entries: RwLock<Vec<SemanticEntry>>,
    threshold: f32,
}

impl SemanticCache {
    pub fn new(threshold: f32) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            threshold,
        }
    }

    /// Linear scan: return the hash of the most similar entry if it exceeds threshold.
    /// Returns `(prompt_hash, similarity_score)`.
    pub fn lookup(&self, query: &[f32]) -> Option<(String, f32)> {
        let entries = self.entries.read().ok()?;
        let mut best_score = self.threshold;
        let mut best_hash: Option<String> = None;

        for entry in entries.iter() {
            let score = dot_product(query, &entry.embedding);
            if score > best_score {
                best_score = score;
                best_hash = Some(entry.prompt_hash.clone());
            }
        }

        best_hash.map(|h| (h, best_score))
    }

    /// Add a new entry. Does not deduplicate — exact-cache hash uniqueness is assumed.
    pub fn insert(&self, embedding: Vec<f32>, prompt_hash: String) {
        if let Ok(mut entries) = self.entries.write() {
            entries.push(SemanticEntry {
                embedding,
                prompt_hash,
            });
        }
    }

    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Dot product of two slices. For L2-normalized vectors this equals cosine similarity.
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
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
