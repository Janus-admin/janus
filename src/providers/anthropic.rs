use super::{
    ChatChoice, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChatMessage,
    ChunkChoice, ChunkDelta, Provider, ProviderError, ProviderStream, UsageData,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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

// ── Anthropic SSE event types (used during streaming) ────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicStreamMessage,
    },
    // index and content_block fields exist in JSON but are not needed
    ContentBlockStart {},
    ContentBlockDelta {
        delta: AnthropicDelta,
    },
    ContentBlockStop {},
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: AnthropicStreamUsage,
    },
    MessageStop,
    Ping,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    id: String,
    model: String,
    usage: AnthropicStreamInputUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamInputUsage {
    input_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicDelta {
    TextDelta {
        text: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamUsage {
    output_tokens: u32,
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
            stream: None,
        };

        let url = format!("{}/messages", self.base_url);

        let resp = crate::telemetry::inject_trace_headers(
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&anthropic_req),
        )
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

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError> {
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
            stream: Some(true),
        };

        let url = format!("{}/messages", self.base_url);

        let resp = crate::telemetry::inject_trace_headers(
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&anthropic_req),
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
                    "Anthropic returned HTTP {}: {}",
                    status, error_text
                )),
            });
        }

        // Use a channel + spawned task because the Anthropic SSE format is stateful:
        // message_start carries the ID and prompt token count, content_block_delta
        // carries the text, and message_delta carries the output token count.
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatCompletionChunk, ProviderError>>(32);

        tokio::spawn(async move {
            let event_stream = resp.bytes_stream().eventsource();
            tokio::pin!(event_stream);

            // State accumulated across events for a single Anthropic message
            let mut msg_id = String::new();
            let mut model = String::new();
            let mut prompt_tokens: u32 = 0;
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            while let Some(event_result) = event_stream.next().await {
                match event_result {
                    Err(e) => {
                        let _ = tx.send(Err(ProviderError::ParseError(e.to_string()))).await;
                        return;
                    }
                    Ok(event) => {
                        let parsed: AnthropicStreamEvent = match serde_json::from_str(&event.data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    "Anthropic stream parse error: {}. Data: {}",
                                    e,
                                    event.data
                                );
                                continue;
                            }
                        };

                        match parsed {
                            AnthropicStreamEvent::MessageStart { message } => {
                                msg_id = message.id;
                                model = message.model;
                                prompt_tokens = message.usage.input_tokens;

                                // Emit the role chunk (OpenAI convention)
                                let chunk = ChatCompletionChunk {
                                    id: msg_id.clone(),
                                    object: "chat.completion.chunk".to_string(),
                                    created,
                                    model: model.clone(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta: ChunkDelta {
                                            role: Some("assistant".to_string()),
                                            content: None,
                                        },
                                        finish_reason: None,
                                    }],
                                    usage: None,
                                };
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }

                            AnthropicStreamEvent::ContentBlockDelta {
                                delta: AnthropicDelta::TextDelta { text },
                            } => {
                                let chunk = ChatCompletionChunk {
                                    id: msg_id.clone(),
                                    object: "chat.completion.chunk".to_string(),
                                    created,
                                    model: model.clone(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta: ChunkDelta {
                                            role: None,
                                            content: Some(text),
                                        },
                                        finish_reason: None,
                                    }],
                                    usage: None,
                                };
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }

                            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                                let finish_reason =
                                    stop_reason_to_finish_reason(delta.stop_reason.as_deref());
                                let output_tokens = usage.output_tokens;
                                let total = prompt_tokens + output_tokens;

                                let chunk = ChatCompletionChunk {
                                    id: msg_id.clone(),
                                    object: "chat.completion.chunk".to_string(),
                                    created,
                                    model: model.clone(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta: ChunkDelta {
                                            role: None,
                                            content: None,
                                        },
                                        finish_reason,
                                    }],
                                    usage: Some(UsageData {
                                        prompt_tokens,
                                        completion_tokens: output_tokens,
                                        total_tokens: total,
                                    }),
                                };
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }

                            // Ignored: Ping, ContentBlockStart, ContentBlockStop,
                            // MessageStop, Unknown
                            _ => {}
                        }
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> HealthStatus {
        if !self.is_enabled() {
            return HealthStatus::Down;
        }

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
