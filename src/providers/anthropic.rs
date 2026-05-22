use super::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Provider,
    ProviderError, UsageData,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

// ── Anthropic wire types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    error: AnthropicErrorBody,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

// ── Provider implementation ───────────────────────────────────────────────────

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    priority: u8,
}

impl AnthropicProvider {
    pub fn new(api_key: String, priority: u8) -> Self {
        Self::with_base_url(
            api_key,
            "https://api.anthropic.com/v1".to_string(),
            priority,
        )
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

    /// Extract system prompt and convert remaining messages to Anthropic format.
    fn convert_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<AnthropicMessage>) {
        let mut system_prompt: Option<String> = None;
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            if msg.role == "system" {
                // Anthropic takes system as a top-level field
                let text = extract_text_content(&msg.content);
                system_prompt = Some(text);
            } else {
                anthropic_messages.push(AnthropicMessage {
                    role: msg.role.clone(),
                    content: convert_content(&msg.content),
                });
            }
        }

        (system_prompt, anthropic_messages)
    }
}

/// Extract plain-text string from either a JSON string or content array.
fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if p.get("type")?.as_str()? == "text" {
                    p.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

/// Convert OpenAI content value to Anthropic content format.
/// Anthropic accepts a plain string or an array of content blocks.
fn convert_content(content: &Value) -> Value {
    match content {
        Value::String(s) => Value::String(s.clone()),
        Value::Array(parts) => {
            let blocks: Vec<Value> = parts
                .iter()
                .filter_map(|p| {
                    let t = p.get("type")?.as_str()?;
                    match t {
                        "text" => {
                            let text = p.get("text")?.as_str()?;
                            Some(serde_json::json!({"type": "text", "text": text}))
                        }
                        "image_url" => {
                            // OpenAI: { "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,..." } }
                            let url = p.get("image_url")?.get("url")?.as_str()?;
                            if let Some(rest) = url.strip_prefix("data:") {
                                let mut parts = rest.splitn(2, ',');
                                let media_type = parts.next()?.strip_suffix(";base64")?.to_string();
                                let data = parts.next()?.to_string();
                                Some(serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": data
                                    }
                                }))
                            } else {
                                // URL-based images not supported by Anthropic natively
                                None
                            }
                        }
                        _ => None,
                    }
                })
                .collect();
            Value::Array(blocks)
        }
        other => Value::String(other.to_string()),
    }
}

fn stop_reason_to_finish_reason(stop_reason: Option<&str>) -> Option<String> {
    match stop_reason {
        Some("end_turn") | Some("stop_sequence") => Some("stop".to_string()),
        Some("max_tokens") => Some("length".to_string()),
        Some("tool_use") => Some("tool_calls".to_string()),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
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
        let (system_prompt, anthropic_messages) = Self::convert_messages(&request.messages);

        let anthropic_req = AnthropicRequest {
            model: &request.model,
            messages: anthropic_messages,
            system: system_prompt,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            top_p: request.top_p,
            stop_sequences: request.stop.as_ref().and_then(|s| match s {
                Value::String(v) => Some(vec![v.clone()]),
                Value::Array(arr) => Some(
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                ),
                _ => None,
            }),
        };

        let url = format!("{}/messages", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&anthropic_req)
            .send()
            .await?;

        let status = resp.status();

        if status.is_success() {
            let anthropic_resp = resp
                .json::<AnthropicResponse>()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;

            return Ok(to_openai_response(anthropic_resp));
        }

        // Try to parse error body
        let error_text = resp
            .json::<AnthropicError>()
            .await
            .map(|e| format!("{}: {}", e.error.error_type, e.error.message))
            .unwrap_or_default();

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ProviderError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::RateLimit),
            StatusCode::BAD_REQUEST => Err(ProviderError::BadRequest(error_text)),
            _ => Err(ProviderError::Unavailable(format!(
                "Anthropic returned HTTP {}: {}",
                status, error_text
            ))),
        }
    }

    async fn health_check(&self) -> HealthStatus {
        if !self.is_enabled() {
            return HealthStatus::Down;
        }

        // Anthropic has no free "ping" endpoint, so we send a minimal request
        let req = serde_json::json!({
            "model": "claude-3-haiku-20240307",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        });

        let url = format!("{}/messages", self.base_url);
        match self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .timeout(Duration::from_secs(10))
            .json(&req)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => HealthStatus::Healthy,
            Ok(resp) if resp.status() == StatusCode::TOO_MANY_REQUESTS => HealthStatus::Degraded,
            _ => HealthStatus::Down,
        }
    }
}

fn to_openai_response(resp: AnthropicResponse) -> ChatCompletionResponse {
    let text = resp
        .content
        .iter()
        .filter(|c| c.content_type == "text")
        .filter_map(|c| c.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    let finish_reason = stop_reason_to_finish_reason(resp.stop_reason.as_deref());
    let total_tokens = resp.usage.input_tokens + resp.usage.output_tokens;

    ChatCompletionResponse {
        id: resp.id,
        object: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: resp.model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: Value::String(text),
                name: None,
            },
            finish_reason,
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens,
        },
    }
}
