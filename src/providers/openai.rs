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

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(request)
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

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
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
