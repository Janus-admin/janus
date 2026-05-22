use crate::providers::ChatCompletionRequest;
use sha2::{Digest, Sha256};

/// Compute a stable SHA-256 cache key for a chat completion request.
///
/// The `stream` field is excluded so that identical prompts with stream=true/false
/// resolve to the same cache entry.
pub fn compute_hash(request: &ChatCompletionRequest) -> String {
    let normalized = ChatCompletionRequest {
        stream: None,
        ..request.clone()
    };
    let json = serde_json::to_string(&normalized).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(json.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}
