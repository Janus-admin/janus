//! POST /v1/messages — Anthropic Messages API inbound shim.
//!
//! Accepts requests in Anthropic's native format, translates them to the
//! internal OpenAI-compatible format, runs through the full Janus pipeline
//! (cache, budget, routing, failover), then translates the response back.
//!
//! Streaming (`"stream": true`): the pipeline runs non-streaming and the
//! completed response is re-emitted as Anthropic SSE events. True chunk-level
//! streaming can be added later by consuming the OpenAI SSE stream.

use crate::{
    errors::AppError,
    gateway::{pipeline, strategies::RoutingStrategy},
    handlers::gateway::{extract_tags, ValidatedJson},
    middleware::{api_key_auth::GatewayAuth, budget::check_budget},
    providers::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage},
    state::AppState,
};
use axum::{
    extract::State,
    http::HeaderMap,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

// ── Inbound request types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    pub messages: Vec<AnthropicInboundMessage>,
    /// System prompt: either a plain string or an array of content blocks.
    pub system: Option<Value>,
    pub max_tokens: u32,
    #[serde(default)]
    pub stream: Option<bool>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicInboundMessage {
    pub role: String,
    /// Either a plain string or an array of Anthropic content blocks.
    pub content: Value,
}

// ── Outbound response types ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AnthropicMessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: &'static str,
    pub role: &'static str,
    pub content: Vec<AnthropicResponseContent>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<Value>,
    pub usage: AnthropicResponseUsage,
}

#[derive(Debug, Serialize)]
pub struct AnthropicResponseContent {
    #[serde(rename = "type")]
    pub content_type: &'static str,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct AnthropicResponseUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ── Translation: inbound → internal ──────────────────────────────────────────

fn system_value_to_string(system: &Value) -> String {
    match system {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type")?.as_str()? == "text" {
                    b.get("text")?.as_str().map(str::to_owned)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

/// Convert an Anthropic content value to OpenAI content format.
/// Anthropic image blocks use `source.type = "base64"` or `"url"`;
/// OpenAI uses `image_url.url` with a data-URI or plain URL.
fn anthropic_content_to_openai(content: &Value) -> Value {
    match content {
        Value::String(s) => Value::String(s.clone()),
        Value::Array(blocks) => {
            let parts: Vec<Value> = blocks
                .iter()
                .filter_map(|b| {
                    match b.get("type")?.as_str()? {
                        "text" => {
                            let text = b.get("text")?.as_str()?;
                            Some(json!({"type": "text", "text": text}))
                        }
                        "image" => {
                            let source = b.get("source")?;
                            match source.get("type")?.as_str()? {
                                "base64" => {
                                    let media_type = source.get("media_type")?.as_str()?;
                                    let data = source.get("data")?.as_str()?;
                                    Some(json!({
                                        "type": "image_url",
                                        "image_url": {
                                            "url": format!("data:{};base64,{}", media_type, data)
                                        }
                                    }))
                                }
                                "url" => {
                                    let url = source.get("url")?.as_str()?;
                                    Some(json!({
                                        "type": "image_url",
                                        "image_url": {"url": url}
                                    }))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                })
                .collect();
            Value::Array(parts)
        }
        other => Value::String(other.to_string()),
    }
}

fn to_openai_request(req: AnthropicMessagesRequest) -> ChatCompletionRequest {
    let mut messages: Vec<ChatMessage> = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Value::String(system_value_to_string(system)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in req.messages {
        messages.push(ChatMessage {
            role: msg.role,
            content: anthropic_content_to_openai(&msg.content),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let stop = req
        .stop_sequences
        .map(|seqs| Value::Array(seqs.into_iter().map(Value::String).collect()));

    ChatCompletionRequest {
        model: req.model,
        messages,
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        stream: req.stream,
        stop,
        n: None,
        presence_penalty: None,
        frequency_penalty: None,
        seed: None,
        user: None,
        logit_bias: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        response_format: None,
        metadata: req.metadata,
    }
}

// ── Translation: internal → Anthropic response ────────────────────────────────

fn finish_reason_to_stop_reason(reason: Option<&str>) -> Option<String> {
    match reason {
        Some("stop") => Some("end_turn".to_string()),
        Some("length") => Some("max_tokens".to_string()),
        Some("tool_calls") => Some("tool_use".to_string()),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

fn to_anthropic_response(resp: ChatCompletionResponse) -> AnthropicMessagesResponse {
    let choice = resp.choices.into_iter().next();
    let text = choice
        .as_ref()
        .and_then(|c| c.message.content.as_str())
        .unwrap_or("")
        .to_string();
    let stop_reason = choice
        .as_ref()
        .and_then(|c| c.finish_reason.as_deref())
        .and_then(|r| finish_reason_to_stop_reason(Some(r)));

    AnthropicMessagesResponse {
        id: resp.id,
        response_type: "message",
        role: "assistant",
        content: vec![AnthropicResponseContent {
            content_type: "text",
            text,
        }],
        model: resp.model,
        stop_reason,
        stop_sequence: None,
        usage: AnthropicResponseUsage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
        },
    }
}

/// Emit an Anthropic SSE stream from a completed (non-streaming) response.
/// Sends the canonical Anthropic event sequence: message_start → content_block_start
/// → ping → content_block_delta → content_block_stop → message_delta → message_stop.
fn anthropic_sse_from_response(resp: ChatCompletionResponse) -> impl IntoResponse {
    let choice = resp.choices.into_iter().next();
    let text = choice
        .as_ref()
        .and_then(|c| c.message.content.as_str())
        .unwrap_or("")
        .to_string();
    let stop_reason = choice
        .as_ref()
        .and_then(|c| c.finish_reason.as_deref())
        .and_then(|r| finish_reason_to_stop_reason(Some(r)))
        .unwrap_or_else(|| "end_turn".to_string());

    let id = resp.id;
    let model = resp.model;
    let input_tokens = resp.usage.prompt_tokens;
    let output_tokens = resp.usage.completion_tokens;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);

    tokio::spawn(async move {
        let events: &[(&str, Value)] = &[
            (
                "message_start",
                json!({
                    "type": "message_start",
                    "message": {
                        "id": id,
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": model,
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {"input_tokens": input_tokens, "output_tokens": 1}
                    }
                }),
            ),
            (
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""}
                }),
            ),
            ("ping", json!({"type": "ping"})),
            (
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": text}
                }),
            ),
            (
                "content_block_stop",
                json!({"type": "content_block_stop", "index": 0}),
            ),
            (
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                    "usage": {"output_tokens": output_tokens}
                }),
            ),
            ("message_stop", json!({"type": "message_stop"})),
        ];

        for (event_type, data) in events {
            let _ = tx
                .send(Ok(Event::default()
                    .event(*event_type)
                    .data(data.to_string())))
                .await;
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// POST /v1/messages
///
/// Anthropic Messages API inbound shim. Accepts native Anthropic format and
/// routes through the Janus pipeline (cache, failover, budget, analytics).
/// Supports both non-streaming and streaming responses (`"stream": true`).
pub async fn messages_handler(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<AnthropicMessagesRequest>,
) -> impl IntoResponse {
    let is_stream = req.stream.unwrap_or(false);
    let mut openai_req = to_openai_request(req);

    // Budget gate — same as the main gateway handler.
    let downgrade = match check_budget(&api_key, &state.config.budget_downgrade) {
        Ok(d) => d,
        Err(e) => return e.into_response(),
    };
    use crate::middleware::budget::DowngradeDecision;
    let downgrade_triggered = !matches!(downgrade, DowngradeDecision::None);
    if let DowngradeDecision::UseModel(ref m) = downgrade {
        openai_req.model = m.clone();
    }

    // Rate limit gate (RPM).
    if let Some(rpm) = api_key.rate_limit_rpm {
        if let Some(ref cluster_rl) = state.cluster_rate_limiter {
            if let Err(retry_after) = cluster_rl.check_and_record(api_key.id, rpm).await {
                return AppError::RateLimitExceeded(Some(retry_after)).into_response();
            }
        } else if let Err(retry_after) = state.rate_limiter.check_and_record(api_key.id, rpm) {
            return AppError::RateLimitExceeded(Some(retry_after)).into_response();
        }
    }

    let rc = state.runtime_config.load();
    let cache_enabled = rc.cache_enabled;
    let max_retries = rc.max_retries;
    drop(rc);

    let bypass_cache = !cache_enabled
        || headers
            .get("x-janus-cache")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("false"))
            .unwrap_or(false);
    let bypass_semantic = bypass_cache
        || !state
            .semantic_policy
            .allows(&openai_req.model, "/v1/messages", &api_key.name);

    let strategy = match &downgrade {
        DowngradeDecision::UseStrategy(s) => RoutingStrategy::from_db_str(s),
        _ => RoutingStrategy::from_db_str(&api_key.routing_strategy),
    };
    let fallback_models = state
        .config
        .routing
        .fallbacks
        .get(&openai_req.model)
        .cloned()
        .unwrap_or_default();
    let tags = extract_tags(&openai_req, &headers);
    let cache_ttl_secs = state
        .config
        .cache_ttl_overrides
        .get(&openai_req.model)
        .copied()
        .unwrap_or(state.config.cache_ttl_secs);

    // Always run non-streaming through the pipeline so we can translate the
    // response back to Anthropic format. For `stream: true` we re-emit the
    // completed response as Anthropic SSE events.
    openai_req.stream = None;

    match pipeline::run(
        &state.pool,
        &state.providers,
        &openai_req,
        &api_key,
        max_retries,
        &state.cache,
        bypass_cache,
        bypass_semantic,
        &strategy,
        &fallback_models,
        None,
        &state.plugins,
        &state.dedup,
        cache_ttl_secs,
        downgrade_triggered,
        None,
        &tags,
        "/v1/messages",
        &state.audit_semaphore,
    )
    .await
    {
        Ok((resp, _cache_hit)) => {
            if is_stream {
                anthropic_sse_from_response(resp).into_response()
            } else {
                Json(to_anthropic_response(resp)).into_response()
            }
        }
        Err(e) => e.into_response(),
    }
}
