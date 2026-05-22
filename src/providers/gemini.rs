use super::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Provider,
    ProviderError, ProviderStream, UsageData,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::time::Duration;

/// Google Gemini provider adapter.
/// Converts OpenAI format requests to Gemini API format and responses back.
pub struct GeminiProvider {
    api_key: String,
    priority: u8,
    client: Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, priority: u8) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            api_key,
            priority,
            client,
        }
    }

    fn build_gemini_request(&self, request: &ChatCompletionRequest) -> serde_json::Value {
        let contents: Vec<_> = request
            .messages
            .iter()
            .map(|msg| {
                let role = if msg.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                let content = msg.content.as_str().unwrap_or("").to_string();
                serde_json::json!({
                    "role": role,
                    "parts": [{ "text": content }]
                })
            })
            .collect();

        serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": request.temperature.unwrap_or(1.0),
                "maxOutputTokens": request.max_tokens.unwrap_or(2048),
                "topP": request.top_p.unwrap_or(1.0),
            }
        })
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
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
                "Gemini API key not configured".to_string(),
            ));
        }

        let gemini_request = self.build_gemini_request(request);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            request.model, self.api_key
        );

        let response = self.client.post(&url).json(&gemini_request).send().await?;

        let status = response.status();

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Unauthorized,
                StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimit,
                StatusCode::BAD_REQUEST => ProviderError::BadRequest(body),
                StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => {
                    ProviderError::Unavailable("Gemini service unavailable".to_string())
                }
                _ => ProviderError::Unavailable(format!("Gemini returned HTTP {}", status)),
            });
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(format!("Invalid response: {}", e)))?;

        // Parse Gemini response and convert to OpenAI format
        let content = json["candidates"]
            .get(0)
            .and_then(|c| c["content"]["parts"].get(0))
            .and_then(|p| p["text"].as_str())
            .unwrap_or("")
            .to_string();

        let completion_tokens = json["usageMetadata"]
            .get("candidatesTokenCount")
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;

        let prompt_tokens = json["usageMetadata"]
            .get("promptTokenCount")
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;

        Ok(ChatCompletionResponse {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: request.model.clone(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: serde_json::Value::String(content),
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: UsageData {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    async fn chat_completion_stream(
        &self,
        _request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::Unavailable(
            "Gemini streaming not yet implemented".to_string(),
        ))
    }

    async fn health_check(&self) -> HealthStatus {
        if !self.is_enabled() {
            return HealthStatus::Down;
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            self.api_key
        );

        match self
            .client
            .get(&url)
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
