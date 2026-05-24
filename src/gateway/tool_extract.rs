//! V5-0 — function-calling audit extraction.
//!
//! Pulls the `tools` array from the chat request and the `tool_calls` array
//! from the response so they can be persisted on the `requests` row for
//! analytics (most-called tool, error rate per tool, etc.).
//!
//! The extracted shape mirrors the OpenAI spec — no transformation, just
//! filtering. Anything missing is `None`.

use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use serde_json::{json, Value};

/// Extract a compact JSON object describing the request's `tools` declaration
/// and the response's `tool_calls`. Returns `None` when neither is present so
/// callers can persist `NULL` in the DB rather than `{}`.
pub fn extract(
    request: &ChatCompletionRequest,
    response: &ChatCompletionResponse,
) -> Option<Value> {
    let tools = request.tools.clone();

    // Each choice may carry its own tool_calls; flatten across choices.
    let combined: Vec<Value> = response
        .choices
        .iter()
        .filter_map(|choice| choice.message.tool_calls.clone())
        .flat_map(|v| match v {
            Value::Array(arr) => arr,
            other => vec![other],
        })
        .collect();

    if tools.is_none() && combined.is_empty() {
        return None;
    }

    Some(json!({
        "tools":      tools,
        "tool_calls": combined,
    }))
}
