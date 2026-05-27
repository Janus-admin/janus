//! POST /v1beta/models/:model_action — Google GenAI inbound shim.
//!
//! Accepts requests in Google's native generateContent format, translates them
//! to the internal OpenAI-compatible format, runs through the full Janus
//! pipeline, then translates the response back to Gemini format.
//!
//! Supported actions (encoded in the path segment after the model name):
//!   - `gemini-2.0-flash:generateContent`  → non-streaming
//!   - `gemini-2.0-flash:streamGenerateContent` → streaming (SSE, Gemini format)
//!
//! Streaming: the pipeline runs non-streaming and the completed response is
//! re-emitted as a Gemini SSE event. True chunk-level streaming can be added
//! later by consuming the OpenAI SSE stream from the pipeline.

use crate::{
    errors::AppError,
    gateway::{pipeline, strategies::RoutingStrategy},
    handlers::gateway::{extract_tags, ValidatedJson},
    middleware::{api_key_auth::GatewayAuth, budget::check_budget},
    providers::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage},
    state::AppState,
};
use axum::{
    extract::{Path, State},
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
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateRequest {
    pub contents: Vec<GeminiContent>,
    /// System prompt passed as `systemInstruction`.
    pub system_instruction: Option<GeminiSystemInstruction>,
    pub generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiContent {
    /// "user" or "model"
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiPart {
    pub text: Option<String>,
    /// Inline image data (base64).
    pub inline_data: Option<GeminiInlineData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Deserialize)]
pub struct GeminiSystemInstruction {
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    pub temperature: Option<f64>,
    pub max_output_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
}

// ── Outbound response types ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateResponse {
    pub candidates: Vec<GeminiCandidate>,
    pub usage_metadata: GeminiUsageMetadata,
    pub model_version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: GeminiResponseContent,
    pub finish_reason: String,
    pub index: u32,
}

#[derive(Debug, Serialize)]
pub struct GeminiResponseContent {
    pub role: &'static str,
    pub parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Serialize)]
pub struct GeminiResponsePart {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    pub prompt_token_count: u32,
    pub candidates_token_count: u32,
    pub total_token_count: u32,
}

// ── Translation: inbound → internal ──────────────────────────────────────────

fn parts_to_text(parts: &[GeminiPart]) -> String {
    parts
        .iter()
        .filter_map(|p| p.text.as_deref())
        .collect::<Vec<_>>()
        .join("")
}

fn parts_to_openai_content(parts: &[GeminiPart]) -> Value {
    let has_image = parts.iter().any(|p| p.inline_data.is_some());
    if !has_image {
        return Value::String(parts_to_text(parts));
    }

    // Build a multi-modal content array.
    let blocks: Vec<Value> = parts
        .iter()
        .filter_map(|p| {
            if let Some(ref data) = p.inline_data {
                Some(json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", data.mime_type, data.data)
                    }
                }))
            } else {
                p.text.as_ref().map(|text| json!({"type": "text", "text": text}))
            }
        })
        .collect();
    Value::Array(blocks)
}

fn to_openai_request(model: &str, req: GeminiGenerateRequest) -> ChatCompletionRequest {
    let mut messages: Vec<ChatMessage> = Vec::new();

    // System instruction → system message.
    if let Some(sys) = req.system_instruction {
        let text = parts_to_text(&sys.parts);
        if !text.is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Value::String(text),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    for content in req.contents {
        // Gemini uses "model" for assistant turns; OpenAI uses "assistant".
        let role = if content.role == "model" {
            "assistant"
        } else {
            "user"
        };
        messages.push(ChatMessage {
            role: role.to_string(),
            content: parts_to_openai_content(&content.parts),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let gc = req.generation_config.unwrap_or_default();
    let stop = gc
        .stop_sequences
        .map(|seqs| Value::Array(seqs.into_iter().map(Value::String).collect()));

    ChatCompletionRequest {
        model: model.to_string(),
        messages,
        max_tokens: gc.max_output_tokens,
        temperature: gc.temperature,
        top_p: gc.top_p,
        stream: None,
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
        metadata: None,
    }
}

// ── Translation: internal → Gemini response ───────────────────────────────────

fn finish_reason_to_gemini(reason: Option<&str>) -> String {
    match reason {
        Some("stop") => "STOP".to_string(),
        Some("length") => "MAX_TOKENS".to_string(),
        Some("content_filter") => "SAFETY".to_string(),
        _ => "STOP".to_string(),
    }
}

fn to_gemini_response(model: &str, resp: ChatCompletionResponse) -> GeminiGenerateResponse {
    let choice = resp.choices.into_iter().next();
    let text = choice
        .as_ref()
        .and_then(|c| c.message.content.as_str())
        .unwrap_or("")
        .to_string();
    let finish_reason = choice
        .as_ref()
        .and_then(|c| c.finish_reason.as_deref())
        .map(|r| finish_reason_to_gemini(Some(r)))
        .unwrap_or_else(|| "STOP".to_string());

    GeminiGenerateResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiResponseContent {
                role: "model",
                parts: vec![GeminiResponsePart { text }],
            },
            finish_reason,
            index: 0,
        }],
        usage_metadata: GeminiUsageMetadata {
            prompt_token_count: resp.usage.prompt_tokens,
            candidates_token_count: resp.usage.completion_tokens,
            total_token_count: resp.usage.total_tokens,
        },
        model_version: model.to_string(),
    }
}

/// Emit a Gemini SSE stream from a completed (non-streaming) response.
/// Sends the full response as a single `data:` event, matching the shape
/// of Gemini's `streamGenerateContent` partial responses.
fn gemini_sse_from_response(model: String, resp: ChatCompletionResponse) -> impl IntoResponse {
    let gemini_resp = to_gemini_response(&model, resp);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(4);
    tokio::spawn(async move {
        if let Ok(data) = serde_json::to_string(&gemini_resp) {
            let _ = tx.send(Ok(Event::default().data(data))).await;
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// POST /v1beta/models/:model_action
///
/// Google GenAI inbound shim. The path segment encodes both model name and
/// action, e.g. `gemini-2.0-flash:generateContent` or
/// `gemini-2.0-flash:streamGenerateContent`.
pub async fn generate_content_handler(
    State(state): State<Arc<AppState>>,
    GatewayAuth(api_key): GatewayAuth,
    Path(model_action): Path<String>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<GeminiGenerateRequest>,
) -> impl IntoResponse {
    // Split "gemini-2.0-flash:generateContent" → ("gemini-2.0-flash", "generateContent")
    let (model, action) = model_action
        .split_once(':')
        .unwrap_or((&model_action, "generateContent"));
    let is_stream = action == "streamGenerateContent";
    let model = model.to_string();

    let mut openai_req = to_openai_request(&model, req);

    // Budget gate.
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
        || !state.semantic_policy.allows(
            &openai_req.model,
            "/v1beta/models",
            &api_key.name,
        );

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

    // Always run non-streaming; for streamGenerateContent we re-emit as
    // Gemini SSE events after the response is complete.
    openai_req.stream = None;

    let endpoint = if is_stream {
        "/v1beta/models:streamGenerateContent"
    } else {
        "/v1beta/models:generateContent"
    };

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
        endpoint,
        &state.audit,
    )
    .await
    {
        Ok((resp, _cache_hit)) => {
            if is_stream {
                gemini_sse_from_response(model, resp).into_response()
            } else {
                Json(to_gemini_response(&model, resp)).into_response()
            }
        }
        Err(e) => e.into_response(),
    }
}
