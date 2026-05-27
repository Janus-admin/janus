use crate::providers::{ChatCompletionRequest, EmbeddingRequest};
use sha2::{Digest, Sha256};

/// Compute a stable SHA-256 cache key for a chat completion request.
///
/// The `stream` field is excluded so that identical prompts with stream=true/false
/// resolve to the same cache entry. We use a proxy struct to avoid cloning the request.
pub fn compute_hash(request: &ChatCompletionRequest) -> String {
    #[derive(serde::Serialize)]
    struct Proxy<'a> {
        model: &'a str,
        messages: &'a [crate::providers::ChatMessage],
        #[serde(skip_serializing_if = "Option::is_none")]
        max_tokens: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        temperature: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        top_p: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        n: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        presence_penalty: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        frequency_penalty: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        seed: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<&'a String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logit_bias: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tools: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_choice: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parallel_tool_calls: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_format: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<&'a serde_json::Value>,
    }

    let proxy = Proxy {
        model: &request.model,
        messages: &request.messages,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        top_p: request.top_p,
        n: request.n,
        stop: request.stop.as_ref(),
        presence_penalty: request.presence_penalty,
        frequency_penalty: request.frequency_penalty,
        seed: request.seed,
        user: request.user.as_ref(),
        logit_bias: request.logit_bias.as_ref(),
        tools: request.tools.as_ref(),
        tool_choice: request.tool_choice.as_ref(),
        parallel_tool_calls: request.parallel_tool_calls,
        response_format: request.response_format.as_ref(),
        metadata: request.metadata.as_ref(),
    };

    let mut h = Sha256::new();
    {
        use std::io::Write;
        let mut writer = std::io::BufWriter::new(&mut h);
        let _ = serde_json::to_writer(&mut writer, &proxy);
        let _ = writer.flush();
    }
    let mut hex = String::with_capacity(64);
    for b in h.finalize().iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    hex
}

/// Compute a stable SHA-256 cache key for an embedding request.
pub fn compute_embedding_hash(request: &EmbeddingRequest) -> String {
    let mut h = Sha256::new();
    {
        use std::io::Write;
        let mut writer = std::io::BufWriter::new(&mut h);
        let _ = serde_json::to_writer(&mut writer, request);
        let _ = writer.flush();
    }
    let mut hex = String::with_capacity(64);
    for b in h.finalize().iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    hex
}
