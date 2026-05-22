use anyhow::Result;
use ort::{session::Session, value::Tensor};
use std::{path::Path, sync::Mutex};
use tokenizers::Tokenizer;

/// Sentence embedding model backed by ONNX Runtime.
///
/// Loads `all-MiniLM-L6-v2` (or any compatible model) and produces
/// 384-dimensional L2-normalized float vectors for text inputs.
///
/// `Session::run` requires `&mut self`, so the session is behind a `Mutex`
/// to allow shared `&EmbeddingModel` access from multiple threads.
pub struct EmbeddingModel {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl EmbeddingModel {
    /// Load model from disk. Returns `Err` on any load failure.
    pub fn load(model_path: impl AsRef<Path>, tokenizer_path: impl AsRef<Path>) -> Result<Self> {
        let session = Session::builder()?.commit_from_file(model_path)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load error: {e}"))?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Embed `text` into a 384-dimensional L2-normalized unit vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenization error: {e}"))?;

        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();
        let seq_len = ids.len();

        // Build input tensors using the (shape, data) tuple form — avoids a direct
        // ndarray version dependency (ort internally uses ndarray 0.17).
        let ids_tensor = Tensor::<i64>::from_array(([1usize, seq_len], ids))?;
        let mask_tensor = Tensor::<i64>::from_array(([1usize, seq_len], mask.clone()))?;
        let type_ids_tensor = Tensor::<i64>::from_array(([1usize, seq_len], type_ids))?;

        // Run inference inside a scope so the session lock is released promptly.
        let (hidden_size, raw_data): (usize, Vec<f32>) = {
            let mut session = self.session.lock().expect("session lock poisoned");
            let outputs = session.run(ort::inputs! {
                "input_ids"      => ids_tensor,
                "attention_mask" => mask_tensor,
                "token_type_ids" => type_ids_tensor,
            })?;

            // last_hidden_state: shape [1, seq_len, hidden_size]
            let lhs = outputs[0].try_extract_array::<f32>()?;
            let shape = lhs.shape().to_vec();
            let hs = *shape.get(2).unwrap_or(&384);
            let raw = lhs
                .as_slice()
                .ok_or_else(|| anyhow::anyhow!("tensor is not contiguous"))?
                .to_vec();
            (hs, raw)
        };

        // Mean pool: sum(token_hidden * mask) / sum(mask)
        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;
        for (i, &m) in mask.iter().enumerate() {
            let mf = m as f32;
            mask_sum += mf;
            for j in 0..hidden_size {
                pooled[j] += raw_data[i * hidden_size + j] * mf;
            }
        }
        if mask_sum > 1e-9 {
            for v in &mut pooled {
                *v /= mask_sum;
            }
        }

        // L2 normalize → unit vector
        let norm = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for v in &mut pooled {
                *v /= norm;
            }
        }

        Ok(pooled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_produces_384_dims() {
        let model = EmbeddingModel::load("models/all-MiniLM-L6-v2.onnx", "models/tokenizer.json")
            .expect(
                "Failed to load embedding model — \
             ensure models/all-MiniLM-L6-v2.onnx and models/tokenizer.json exist",
            );

        let embedding = model.embed("hello world").expect("Embedding failed");

        assert_eq!(embedding.len(), 384, "Expected 384-dimensional output");

        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "Expected L2-normalized unit vector, got norm={norm}"
        );
    }
}
