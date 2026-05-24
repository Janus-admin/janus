use super::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Provider, ProviderError,
    ProviderStream,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use std::time::Duration;

/// Groq API provider adapter.
/// Groq is largely OpenAI-compatible with a different API endpoint.
pub struct GroqProvider {
    api_key: String,
    base_url: String,
    priority: u8,
    client: Client,
}

impl GroqProvider {
    pub fn new(api_key: String, priority: u8) -> Self {
        Self::with_base_url(api_key, "https://api.groq.com/openai/v1".to_string(), priority)
    }

    pub fn with_base_url(api_key: String, base_url: String, priority: u8) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");
        Self { api_key, base_url, priority, client }
    }
}

#[async_trait]
impl Provider for GroqProvider {
    fn name(&self) -> &'static str {
        "groq"
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
        if !self.is_enabled() {
            return Err(ProviderError::Unavailable(
                "Groq API key not configured".to_string(),
            ));
        }

        let mut payload = serde_json::to_value(request)
            .map_err(|_| ProviderError::ParseError("Request serialization failed".to_string()))?;
        payload["stream"] = serde_json::json!(false);

        let url = format!("{}/chat/completions", self.base_url);
        let response = crate::telemetry::inject_trace_headers(
            self.client.post(&url).bearer_auth(&self.api_key).json(&payload),
        )
        .send()
        .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(body),
                StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => {
                    ProviderError::Unavailable("Groq service unavailable".to_string())
                }
                _ => ProviderError::Unavailable(format!("Groq returned HTTP {}", status)),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(format!("Invalid response: {}", e)))
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError> {
        if !self.is_enabled() {
            return Err(ProviderError::Unavailable(
                "Groq API key not configured".to_string(),
            ));
        }

        let mut payload = serde_json::to_value(request)
            .map_err(|_| ProviderError::ParseError("Request serialization failed".to_string()))?;
        payload["stream"] = serde_json::json!(true);
        payload["stream_options"] = serde_json::json!({"include_usage": true});

        let url = format!("{}/chat/completions", self.base_url);
        let response = crate::telemetry::inject_trace_headers(
            self.client.post(&url).bearer_auth(&self.api_key).json(&payload),
        )
        .send()
        .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(error_text),
                StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => {
                    ProviderError::Unavailable("Groq service unavailable".to_string())
                }
                _ => ProviderError::Unavailable(format!("Groq returned HTTP {}", status)),
            });
        }

        let event_stream = response.bytes_stream().eventsource();
        let stream = event_stream.filter_map(|event_result| async move {
            match event_result {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        return None;
                    }
                    match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                        Ok(chunk) => Some(Ok(chunk)),
                        Err(e) => {
                            tracing::warn!("Groq chunk parse error: {}. Data: {}", e, event.data);
                            None
                        }
                    }
                }
                Err(e) => Some(Err(ProviderError::ParseError(e.to_string()))),
            }
        });

        Ok(Box::pin(stream))
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
