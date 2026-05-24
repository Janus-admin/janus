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
    base_url: String,
    priority: u8,
    client: Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, priority: u8) -> Self {
        Self::with_base_url(
            api_key,
            "https://generativelanguage.googleapis.com".to_string(),
            priority,
        )
    }

    pub fn with_base_url(api_key: String, base_url: String, priority: u8) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");
        Self {
            api_key,
            base_url,
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
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, request.model, self.api_key
        );

        let response =
            crate::telemetry::inject_trace_headers(self.client.post(&url).json(&gemini_request))
                .send()
                .await?;

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

        Ok(parse_gemini_response(&json, &request.model))
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

        let url = format!("{}/v1beta/models?key={}", self.base_url, self.api_key);

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

/// Translate a Gemini `generateContent` JSON response into our OpenAI-shaped
/// `ChatCompletionResponse`. Missing fields fall back to empty/zero — Gemini
/// occasionally omits `usageMetadata` for short replies, and content parts can
/// be absent for safety-filtered responses.
fn parse_gemini_response(json: &serde_json::Value, model: &str) -> ChatCompletionResponse {
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

    ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: model.to_string(),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Golden response captured from a real Gemini `generateContent` call.
    /// Validates the happy path: content extraction + token counts + role mapping.
    #[test]
    fn parses_typical_gemini_response() {
        let body = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{ "text": "Hello! How can I help you today?" }]
                },
                "finishReason": "STOP",
                "index": 0
            }],
            "usageMetadata": {
                "promptTokenCount": 8,
                "candidatesTokenCount": 9,
                "totalTokenCount": 17
            }
        });

        let resp = parse_gemini_response(&body, "gemini-1.5-flash");

        assert_eq!(resp.model, "gemini-1.5-flash");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert_eq!(
            resp.choices[0].message.content.as_str().unwrap(),
            "Hello! How can I help you today?"
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.prompt_tokens, 8);
        assert_eq!(resp.usage.completion_tokens, 9);
        assert_eq!(resp.usage.total_tokens, 17);
    }

    /// Safety-filtered responses arrive with no `parts`. We must not panic;
    /// content becomes empty string, tokens still parse from usageMetadata.
    #[test]
    fn handles_response_with_no_content_parts() {
        let body = json!({
            "candidates": [{
                "content": { "role": "model" },
                "finishReason": "SAFETY"
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 0,
                "totalTokenCount": 5
            }
        });

        let resp = parse_gemini_response(&body, "gemini-1.5-pro");
        assert_eq!(resp.choices[0].message.content.as_str().unwrap(), "");
        assert_eq!(resp.usage.prompt_tokens, 5);
        assert_eq!(resp.usage.completion_tokens, 0);
    }

    /// Short replies sometimes omit `usageMetadata` entirely. Token counts
    /// fall back to 0 — the gateway will still log the request, just without cost.
    #[test]
    fn handles_response_missing_usage_metadata() {
        let body = json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "ok" }] },
                "finishReason": "STOP"
            }]
        });

        let resp = parse_gemini_response(&body, "gemini-1.5-flash");
        assert_eq!(resp.choices[0].message.content.as_str().unwrap(), "ok");
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
        assert_eq!(resp.usage.total_tokens, 0);
    }

    /// Verifies the OpenAI → Gemini request translation: role rewrite
    /// (`assistant` → `model`), content unwrapping, and generationConfig defaults.
    #[test]
    fn builds_gemini_request_with_role_translation() {
        let provider = GeminiProvider::new("test-key".to_string(), 40);
        let request = ChatCompletionRequest {
            model: "gemini-1.5-flash".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: json!("Hi"),
                    name: None,
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: json!("Hello!"),
                    name: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: json!("How are you?"),
                    name: None,
                },
            ],
            max_tokens: Some(512),
            temperature: Some(0.7),
            top_p: None,
            n: None,
            stream: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            seed: None,
            user: None,
            logit_bias: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
        };

        let payload = provider.build_gemini_request(&request);
        let contents = payload["contents"].as_array().unwrap();

        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hi");
        // assistant must be rewritten to "model" — Gemini rejects "assistant"
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "Hello!");
        assert_eq!(contents[2]["role"], "user");

        assert_eq!(payload["generationConfig"]["maxOutputTokens"], 512);
        assert_eq!(payload["generationConfig"]["temperature"], 0.7);
    }
}
