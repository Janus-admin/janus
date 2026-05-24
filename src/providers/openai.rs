use super::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, EmbeddingRequest,
    EmbeddingResponse, ImagesRequest, ImagesResponse, ModelInfo, Provider, ProviderError,
    ProviderStream, SpeechRequest, SpeechStream, TranscribeRequest, TranscribeResponse,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{multipart, Client, StatusCode};
use std::time::Duration;

pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
    priority: u8,
}

impl OpenAIProvider {
    pub fn new(api_key: String, priority: u8) -> Self {
        Self::with_base_url(api_key, "https://api.openai.com/v1".to_string(), priority)
    }

    pub fn with_base_url(api_key: String, base_url: String, priority: u8) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
            api_key,
            base_url,
            priority,
        }
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn priority(&self) -> u8 {
        self.priority
    }

    fn is_enabled(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let resp = crate::telemetry::inject_trace_headers(
            self.client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(request),
        )
        .send()
        .await?;

        let status = resp.status();

        if status.is_success() {
            let body = resp
                .json::<ChatCompletionResponse>()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;
            return Ok(body);
        }

        let error_text = resp.text().await.unwrap_or_default();

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ProviderError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::RateLimit),
            StatusCode::BAD_REQUEST => Err(ProviderError::BadRequest(error_text)),
            _ => Err(ProviderError::Unavailable(format!(
                "OpenAI returned HTTP {}: {}",
                status, error_text
            ))),
        }
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        // Build body with stream:true and stream_options to get usage in the final chunk.
        let mut body =
            serde_json::to_value(request).map_err(|e| ProviderError::ParseError(e.to_string()))?;
        body["stream"] = serde_json::Value::Bool(true);
        body["stream_options"] = serde_json::json!({ "include_usage": true });

        let resp = crate::telemetry::inject_trace_headers(
            self.client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body),
        )
        .send()
        .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(error_text),
                _ => ProviderError::Unavailable(format!(
                    "OpenAI returned HTTP {}: {}",
                    status, error_text
                )),
            });
        }

        // Parse the SSE byte stream into ChatCompletionChunk items.
        let event_stream = resp.bytes_stream().eventsource();

        let stream = event_stream.filter_map(|event_result| async move {
            match event_result {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        return None;
                    }
                    match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                        Ok(chunk) => Some(Ok(chunk)),
                        Err(e) => {
                            tracing::warn!("OpenAI chunk parse error: {}. Data: {}", e, event.data);
                            None
                        }
                    }
                }
                Err(e) => Some(Err(ProviderError::ParseError(e.to_string()))),
            }
        });

        Ok(Box::pin(stream))
    }

    async fn embeddings(
        &self,
        request: &EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        let url = format!("{}/embeddings", self.base_url);
        let resp = crate::telemetry::inject_trace_headers(
            self.client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(request),
        )
        .send()
        .await
        .map_err(ProviderError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::BadRequest(format!(
                "embeddings HTTP {status}: {text}"
            )));
        }

        resp.json::<EmbeddingResponse>()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        #[derive(serde::Deserialize)]
        struct Wire {
            data: Vec<ModelInfo>,
        }
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(ProviderError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable(format!(
                "list_models HTTP {status}: {text}"
            )));
        }
        let wire: Wire = resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
        Ok(wire.data)
    }

    async fn images_generate(
        &self,
        request: &ImagesRequest,
    ) -> Result<ImagesResponse, ProviderError> {
        let url = format!("{}/images/generations", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(ProviderError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(text),
                _ => ProviderError::Unavailable(format!("images HTTP {status}: {text}")),
            });
        }
        resp.json::<ImagesResponse>()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))
    }

    async fn audio_transcribe(
        &self,
        request: TranscribeRequest,
    ) -> Result<TranscribeResponse, ProviderError> {
        let url = format!("{}/audio/transcriptions", self.base_url);

        let file_part = multipart::Part::bytes(request.file_bytes.to_vec())
            .file_name(request.filename)
            .mime_str("application/octet-stream")
            .map_err(|e| ProviderError::BadRequest(e.to_string()))?;

        let mut form = multipart::Form::new()
            .text("model", request.model)
            .part("file", file_part);
        if let Some(lang) = request.language {
            form = form.text("language", lang);
        }
        if let Some(prompt) = request.prompt {
            form = form.text("prompt", prompt);
        }
        if let Some(fmt) = request.response_format {
            form = form.text("response_format", fmt);
        }
        if let Some(t) = request.temperature {
            form = form.text("temperature", t.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(ProviderError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(text),
                _ => ProviderError::Unavailable(format!("transcribe HTTP {status}: {text}")),
            });
        }

        // OpenAI returns `application/json` for json/verbose_json; plain text otherwise.
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.contains("application/json") {
            resp.json::<TranscribeResponse>()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))
        } else {
            let text = resp
                .text()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;
            Ok(TranscribeResponse {
                text,
                duration: None,
            })
        }
    }

    async fn audio_speech(&self, request: &SpeechRequest) -> Result<SpeechStream, ProviderError> {
        let url = format!("{}/audio/speech", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(ProviderError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(text),
                _ => ProviderError::Unavailable(format!("speech HTTP {status}: {text}")),
            });
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();

        let byte_stream = resp
            .bytes_stream()
            .map(|chunk| chunk.map_err(ProviderError::Http));

        Ok(SpeechStream {
            content_type,
            bytes: Box::pin(byte_stream),
        })
    }

    async fn health_check(&self) -> HealthStatus {
        if !self.is_enabled() {
            return HealthStatus::Down;
        }

        let url = format!("{}/models", self.base_url);
        match self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => HealthStatus::Healthy,
            Ok(resp) if resp.status() == StatusCode::TOO_MANY_REQUESTS => HealthStatus::Degraded,
            _ => HealthStatus::Down,
        }
    }
}
