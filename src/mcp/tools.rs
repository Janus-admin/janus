// src/mcp/tools.rs
// MCP tool definitions (JSON Schema) and implementations.

use crate::{
    db,
    gateway::{pipeline, strategies::RoutingStrategy},
    metrics,
    models::api_key::ApiKey,
    providers::{ChatCompletionRequest, ChatMessage},
    state::AppState,
};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Tool descriptor ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Return the canonical list of tools this MCP server exposes.
pub fn tool_list() -> Vec<Tool> {
    vec![
        Tool {
            name: "proxy_llm_request",
            description: "Send a chat completion request through the Janus gateway",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model": {
                        "type": "string",
                        "description": "Model name (e.g. gpt-4o-mini)"
                    },
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "role":    { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["role", "content"]
                        }
                    }
                },
                "required": ["model", "messages"]
            }),
        },
        Tool {
            name: "get_usage_stats",
            description: "Get a summary of requests and cost over a time period",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "period": {
                        "type": "string",
                        "enum": ["today", "7d", "30d"],
                        "description": "Time period for the stats"
                    }
                },
                "required": ["period"]
            }),
        },
        Tool {
            name: "list_api_keys",
            description: "List all API keys (safe view — prefix only, no secret values)",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "create_api_key",
            description: "Create a new Janus API key",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Display name for the key"
                    },
                    "budget_limit": {
                        "type": "number",
                        "description": "Optional spending cap in USD"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "get_cache_stats",
            description: "Get cache hit rate and estimated savings statistics",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "flush_cache",
            description: "Clear all cache entries (exact + semantic hot layers and database)",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

// ── Tool dispatcher ───────────────────────────────────────────────────────────

pub async fn call_tool(state: &Arc<AppState>, name: &str, args: Value) -> Result<Value, String> {
    match name {
        "proxy_llm_request" => proxy_llm_request(state, args).await,
        "get_usage_stats" => get_usage_stats(state, args).await,
        "list_api_keys" => list_api_keys(state).await,
        "create_api_key" => create_api_key(state, args).await,
        "get_cache_stats" => get_cache_stats(state).await,
        "flush_cache" => flush_cache(state).await,
        _ => Err(format!("Unknown tool: {name}")),
    }
}

// ── Tool implementations ──────────────────────────────────────────────────────

async fn proxy_llm_request(state: &Arc<AppState>, args: Value) -> Result<Value, String> {
    let model = args["model"]
        .as_str()
        .ok_or("missing required field: model")?
        .to_string();

    let messages: Vec<ChatMessage> = serde_json::from_value(args["messages"].clone())
        .map_err(|e| format!("invalid messages: {e}"))?;

    let request = ChatCompletionRequest {
        model,
        messages,
        stream: Some(false),
        max_tokens: None,
        temperature: None,
        top_p: None,
        n: None,
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
        metadata: None,
    };

    // Use an unconstrained internal key for MCP-originated requests.
    // The caller already authenticated via admin JWT.
    let service_key = internal_service_key();
    let strategy = RoutingStrategy::Priority;

    let max_retries = {
        let rc = state.runtime_config.read().await;
        rc.max_retries
    };

    match pipeline::run(
        &state.pool,
        &state.providers,
        &request,
        &service_key,
        max_retries,
        &state.cache,
        false,
        false, // MCP requests always allow semantic cache
        &strategy,
        &[],
        None,
        &state.plugins,
        &state.dedup,
        0,     // no TTL for MCP calls
        false, // MCP calls are internal — no budget downgrade
        None,
        &serde_json::Value::Object(serde_json::Map::new()),
        "/mcp/chat",
    )
    .await
    {
        Ok((response, _)) => {
            let text = serde_json::to_string(&response).unwrap_or_default();
            Ok(json!({
                "content": [{ "type": "text", "text": text }]
            }))
        }
        Err(e) => Err(format!("Gateway error: {e}")),
    }
}

async fn get_usage_stats(state: &Arc<AppState>, args: Value) -> Result<Value, String> {
    let period = args["period"].as_str().unwrap_or("today");

    let days = match period {
        "today" => 1,
        "7d" => 7,
        "30d" => 30,
        other => return Err(format!("unknown period '{other}'; use today, 7d, or 30d")),
    };

    let breakdown = db::analytics::cost_breakdown(&state.pool, days)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&breakdown).unwrap_or_default()
        }]
    }))
}

async fn list_api_keys(state: &Arc<AppState>) -> Result<Value, String> {
    let (keys, total) = db::api_keys::list(&state.pool, 1, 100)
        .await
        .map_err(|e| e.to_string())?;

    let views: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "id": k.id,
                "name": k.name,
                "key_prefix": k.key_prefix,
                "is_active": k.is_active,
                "created_at": k.created_at,
            })
        })
        .collect();

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({ "keys": views, "total": total }))
                .unwrap_or_default()
        }]
    }))
}

async fn create_api_key(state: &Arc<AppState>, args: Value) -> Result<Value, String> {
    let name = args["name"]
        .as_str()
        .ok_or("missing required field: name")?;

    let budget_limit: Option<Decimal> = args
        .get("budget_limit")
        .and_then(Value::as_f64)
        .and_then(|f| Decimal::try_from(f).ok());

    let raw_key = db::api_keys::generate_key();
    let key_hash = bcrypt::hash(&raw_key, bcrypt::DEFAULT_COST).map_err(|e| e.to_string())?;
    let key_sha256 = db::api_keys::sha256_hex(&raw_key);
    let key_prefix: String = raw_key.chars().take(12).collect();
    let id = Uuid::new_v4();

    let key = db::api_keys::create(
        &state.pool,
        id,
        name,
        &key_hash,
        &key_sha256,
        &key_prefix,
        None,
        budget_limit,
        None,
        None,
        None,
        None,
        "priority",
        None,
        None,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Insert into dashmap so the key works immediately.
    let hash_bytes = db::api_keys::sha256_bytes(&raw_key);
    state.key_cache.insert(hash_bytes, key);

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "id": id,
                "key": raw_key,
                "key_prefix": key_prefix
            })).unwrap_or_default()
        }]
    }))
}

async fn get_cache_stats(state: &Arc<AppState>) -> Result<Value, String> {
    let stats = db::cache::get_stats(&state.pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&stats).unwrap_or_default()
        }]
    }))
}

async fn flush_cache(state: &Arc<AppState>) -> Result<Value, String> {
    state.cache.clear();
    metrics::set_exact_cache_size(0);
    metrics::set_semantic_cache_size(0);

    let deleted = db::cache::flush_all(&state.pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({ "flushed": deleted })).unwrap_or_default()
        }]
    }))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// A minimal ApiKey with no budget or rate limits, used for MCP-originated
/// gateway calls where the caller is already authenticated via admin JWT.
fn internal_service_key() -> ApiKey {
    ApiKey {
        id: Uuid::nil(),
        name: "mcp-internal".to_string(),
        key_hash: String::new(),
        key_sha256: None,
        previous_key_sha256: None,
        rotation_expires_at: None,
        key_prefix: "mcp-".to_string(),
        workspace_id: None,
        budget_limit: None,
        budget_used: Decimal::ZERO,
        rate_limit_rpm: None,
        rate_limit_tpm: None,
        allowed_models: None,
        routing_strategy: "priority".to_string(),
        downgrade_at_percent: None,
        downgrade_strategy: None,
        downgrade_to_model: None,
        is_active: true,
        created_at: Utc::now(),
        expires_at: None,
        last_used_at: None,
    }
}
