use super::{ChatCompletionRequest, ChatCompletionResponse, Provider, ProviderError};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
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

        // Extract error message from OpenAI error body if possible
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
