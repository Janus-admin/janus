use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

pub mod anthropic;
pub mod bedrock;
pub mod deepseek;
pub mod gemini;
pub mod groq;
pub mod openai;

// ── OpenAI-compatible request/response types ──────────────────────────────────
// These are the canonical types used throughout Velox. Provider adapters
// translate to/from their own wire formats internally.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// String for plain text; JSON array for multi-modal (images etc.).
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Set on assistant responses when the model is calling a function/tool.
    /// Captured for the V5-0 audit log; passed through unchanged on requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    /// Set on a follow-up `role: "tool"` message to identify the call being answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<serde_json::Value>,
    // ── Tool use / function calling (V2-3) ───────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    /// OpenAI `metadata` field — Velox reads tag keys from here for cost attribution.
    /// Passed through to providers unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: UsageData,
}

// ── Streaming types (Phase 2) ─────────────────────────────────────────────────
// Normalized OpenAI SSE chunk format. All provider adapters produce this.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    /// Only present on the final chunk when the provider sends usage data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageData>,
}

/// Type alias for the boxed stream returned by `chat_completion_stream`.
pub type ProviderStream =
    Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, ProviderError>> + Send>>;

// ── Embeddings types (V2-3) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    /// String or array of strings (OpenAI-compatible).
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub object: String,
    pub embedding: Vec<f64>,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

// ── V5-0 modality types ───────────────────────────────────────────────────────

/// OpenAI-compatible `GET /v1/models` row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default = "default_model_object")]
    pub object: String,
    #[serde(default)]
    pub created: u64,
    pub owned_by: String,
}

fn default_model_object() -> String {
    "model".to_string()
}

/// OpenAI-compatible `POST /v1/images/generations` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagesRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagesResponse {
    pub created: u64,
    pub data: Vec<ImageData>,
}

/// OpenAI-compatible `POST /v1/audio/transcriptions` request.
/// The audio file payload arrives as raw bytes from the multipart body.
#[derive(Debug, Clone)]
pub struct TranscribeRequest {
    pub model: String,
    pub file_bytes: Bytes,
    pub filename: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub response_format: Option<String>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResponse {
    pub text: String,
    /// Duration in seconds, when the provider returns it. Used for cost calc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// OpenAI-compatible `POST /v1/audio/speech` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    pub voice: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
}

/// Streaming response for `POST /v1/audio/speech` — caller proxies bytes through.
/// `content_type` is the negotiated MIME (e.g. `audio/mpeg`).
pub struct SpeechStream {
    pub content_type: String,
    pub bytes: Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>,
}

// ── Provider error type ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Authentication failed")]
    Unauthorized,
    #[error("Provider unavailable: {0}")]
    Unavailable(String),
    #[error("Request timeout")]
    Timeout,
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Response parse error: {0}")]
    ParseError(String),
    #[error("Modality not supported by provider '{0}'")]
    Unsupported(&'static str),
}

// ── Provider trait ────────────────────────────────────────────────────────────

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> u8;
    fn is_enabled(&self) -> bool;

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError>;

    /// Stream chat completion tokens as they arrive from the provider.
    /// Returns a stream of normalized OpenAI-format chunks.
    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError>;

    /// Generate embeddings. Default returns an error; providers override as needed.
    async fn embeddings(
        &self,
        _request: &EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        Err(ProviderError::Unsupported(self.name()))
    }

    /// List models the provider offers, in OpenAI `/v1/models` shape.
    /// Default returns an empty list; providers that expose a discovery endpoint override.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }

    /// Generate images. Default returns Unsupported.
    async fn images_generate(
        &self,
        _request: &ImagesRequest,
    ) -> Result<ImagesResponse, ProviderError> {
        Err(ProviderError::Unsupported(self.name()))
    }

    /// Transcribe audio (speech-to-text). Default returns Unsupported.
    async fn audio_transcribe(
        &self,
        _request: TranscribeRequest,
    ) -> Result<TranscribeResponse, ProviderError> {
        Err(ProviderError::Unsupported(self.name()))
    }

    /// Synthesize speech (text-to-speech). Default returns Unsupported.
    async fn audio_speech(&self, _request: &SpeechRequest) -> Result<SpeechStream, ProviderError> {
        Err(ProviderError::Unsupported(self.name()))
    }

    async fn health_check(&self) -> HealthStatus;
}
