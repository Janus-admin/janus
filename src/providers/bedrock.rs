use super::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Provider,
    ProviderError, UsageData,
};
use crate::models::provider::HealthStatus;
use async_trait::async_trait;
use aws_sdk_bedrockruntime::{
    types::{
        ContentBlock, ConversationRole, InferenceConfiguration, Message, StopReason,
        SystemContentBlock,
    },
    Client,
};
use serde_json::Value;
use std::sync::Arc;

pub struct BedrockProvider {
    client: Arc<Client>,
    priority: u8,
    default_model: String,
}

impl BedrockProvider {
    pub async fn new(priority: u8) -> Self {
        let sdk_config = aws_config::load_from_env().await;
        let client = Client::new(&sdk_config);
        Self {
            client: Arc::new(client),
            priority,
            default_model: "anthropic.claude-3-haiku-20240307-v1:0".to_string(),
        }
    }

    /// Resolve the Bedrock model ID. If the requested model already uses a Bedrock
    /// prefix (e.g. "anthropic.claude-3-..."), use it directly; otherwise fall back
    /// to the configured default.
    fn resolve_model_id<'a>(&'a self, requested: &'a str) -> &'a str {
        const PREFIXES: &[&str] = &["anthropic.", "amazon.", "meta.", "mistral.", "cohere."];
        if PREFIXES.iter().any(|p| requested.starts_with(p)) {
            requested
        } else {
            &self.default_model
        }
    }

    fn build_sdk_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<Message>) {
        let mut system_text: Option<String> = None;
        let mut sdk_messages: Vec<Message> = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system_text = Some(extract_text(&msg.content));
                }
                "user" | "assistant" => {
                    let role = if msg.role == "user" {
                        ConversationRole::User
                    } else {
                        ConversationRole::Assistant
                    };
                    let text = extract_text(&msg.content);
                    match Message::builder()
                        .role(role)
                        .content(ContentBlock::Text(text))
                        .build()
                    {
                        Ok(m) => sdk_messages.push(m),
                        Err(e) => {
                            tracing::warn!("Failed to build Bedrock message: {e}");
                        }
                    }
                }
                _ => {}
            }
        }

        (system_text, sdk_messages)
    }
}

fn extract_text(content: &Value) -> String {
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

fn stop_reason_to_finish_reason(stop_reason: &StopReason) -> String {
    match stop_reason {
        StopReason::EndTurn | StopReason::StopSequence => "stop".to_string(),
        StopReason::MaxTokens => "length".to_string(),
        StopReason::ToolUse => "tool_calls".to_string(),
        _ => "stop".to_string(),
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn name(&self) -> &'static str {
        "bedrock"
    }

    fn priority(&self) -> u8 {
        self.priority
    }

    fn is_enabled(&self) -> bool {
        true
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let model_id = self.resolve_model_id(&request.model);
        let (system_text, sdk_messages) = Self::build_sdk_messages(&request.messages);

        let inference = {
            let mut b = InferenceConfiguration::builder()
                .max_tokens(request.max_tokens.unwrap_or(4096) as i32);
            if let Some(temp) = request.temperature {
                b = b.temperature(temp as f32);
            }
            if let Some(tp) = request.top_p {
                b = b.top_p(tp as f32);
            }
            b.build()
        };

        let mut builder = self
            .client
            .converse()
            .model_id(model_id)
            .set_messages(Some(sdk_messages))
            .inference_config(inference);

        if let Some(sys) = system_text {
            builder = builder.system(SystemContentBlock::Text(sys));
        }

        let output = builder.send().await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("throttl") || msg.contains("TooManyRequests") {
                ProviderError::RateLimit
            } else if msg.contains("AccessDenied") || msg.contains("UnauthorizedClient") {
                ProviderError::Unauthorized
            } else {
                ProviderError::Unavailable(msg)
            }
        })?;

        let text = output
            .output()
            .and_then(|o| o.as_message().ok())
            .map(|m| {
                m.content()
                    .iter()
                    .filter_map(|b| b.as_text().ok())
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let finish_reason = stop_reason_to_finish_reason(output.stop_reason());

        let prompt_tokens = output.usage().map(|u| u.input_tokens() as u32).unwrap_or(0);
        let completion_tokens = output
            .usage()
            .map(|u| u.output_tokens() as u32)
            .unwrap_or(0);

        Ok(ChatCompletionResponse {
            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model: model_id.to_string(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: Value::String(text),
                    name: None,
                },
                finish_reason: Some(finish_reason),
                logprobs: None,
            }],
            usage: UsageData {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    async fn health_check(&self) -> HealthStatus {
        let test_msg = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::Text("hi".to_string()))
            .build();

        let msg = match test_msg {
            Ok(m) => m,
            Err(_) => return HealthStatus::Down,
        };

        let result = self
            .client
            .converse()
            .model_id(&self.default_model)
            .messages(msg)
            .inference_config(InferenceConfiguration::builder().max_tokens(1).build())
            .send()
            .await;

        match result {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("throttl") {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Down
                }
            }
        }
    }
}
