// src/mcp/mod.rs
// MCP (Model Context Protocol) server implementation.
//
// Implements JSON-RPC 2.0 framing over two transports:
//   - HTTP POST /mcp/rpc  (stateless request/response)
//   - HTTP GET  /mcp/sse  (SSE stream)
//   - Stdio               (janus --mcp-stdio)
//
// Auth: admin JWT via Authorization: Bearer <token> header,
//       or params.token in the initialize message.

use crate::{middleware::jwt, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub mod tools;
pub mod transport;

// ── JSON-RPC 2.0 error codes ──────────────────────────────────────────────────

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;
pub const UNAUTHORIZED: i32 = -32001;

// ── JSON-RPC 2.0 wire types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    /// Must be "2.0".
    pub jsonrpc: String,
    /// May be null/missing for notifications.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error_response(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

// ── MCP server ────────────────────────────────────────────────────────────────

pub struct McpServer {
    state: Arc<AppState>,
}

impl McpServer {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Process a single JSON-RPC request.
    ///
    /// Returns `None` for notifications (no response should be sent).
    /// `token` is the JWT extracted from the HTTP Authorization header; if
    /// missing, some methods also accept it inside `params.token`.
    pub async fn handle(
        &self,
        req: JsonRpcRequest,
        token: Option<&str>,
    ) -> Option<JsonRpcResponse> {
        if req.jsonrpc != "2.0" {
            return Some(JsonRpcResponse::error_response(
                req.id,
                INVALID_REQUEST,
                "jsonrpc must be \"2.0\"",
            ));
        }

        let id = req.id.clone();

        match req.method.as_str() {
            // ── initialize ────────────────────────────────────────────────────
            "initialize" => {
                // Accept token from header OR from params.token (MCP spec allows both).
                let param_token = req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("token"))
                    .and_then(Value::as_str);
                let effective = token.or(param_token);

                match effective {
                    Some(tok) if self.validate_jwt(tok) => {}
                    Some(_) => {
                        return Some(JsonRpcResponse::error_response(
                            id,
                            UNAUTHORIZED,
                            "Invalid or expired token",
                        ))
                    }
                    None => {
                        return Some(JsonRpcResponse::error_response(
                            id,
                            UNAUTHORIZED,
                            "Authentication required",
                        ))
                    }
                }

                Some(JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "janus", "version": "0.1.0" }
                    }),
                ))
            }

            // ── notifications/initialized ─────────────────────────────────────
            "notifications/initialized" => None, // notification: no response

            // ── tools/list ────────────────────────────────────────────────────
            "tools/list" => {
                if !self.check_auth(token) {
                    return Some(JsonRpcResponse::error_response(
                        id,
                        UNAUTHORIZED,
                        "Authentication required",
                    ));
                }
                Some(JsonRpcResponse::success(
                    id,
                    serde_json::json!({ "tools": tools::tool_list() }),
                ))
            }

            // ── tools/call ────────────────────────────────────────────────────
            "tools/call" => {
                if !self.check_auth(token) {
                    return Some(JsonRpcResponse::error_response(
                        id,
                        UNAUTHORIZED,
                        "Authentication required",
                    ));
                }

                let params = match req.params {
                    Some(p) => p,
                    None => {
                        return Some(JsonRpcResponse::error_response(
                            id,
                            INVALID_PARAMS,
                            "Missing params",
                        ))
                    }
                };

                let tool_name = match params.get("name").and_then(Value::as_str) {
                    Some(n) => n.to_string(),
                    None => {
                        return Some(JsonRpcResponse::error_response(
                            id,
                            INVALID_PARAMS,
                            "Missing tool name",
                        ))
                    }
                };

                // Validate tool name is known before dispatching
                if tools::tool_list().iter().all(|t| t.name != tool_name) {
                    return Some(JsonRpcResponse::error_response(
                        id,
                        METHOD_NOT_FOUND,
                        format!("Unknown tool: {tool_name}"),
                    ));
                }

                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

                match tools::call_tool(&self.state, &tool_name, arguments).await {
                    Ok(result) => Some(JsonRpcResponse::success(id, result)),
                    Err(msg) => Some(JsonRpcResponse::error_response(id, INTERNAL_ERROR, msg)),
                }
            }

            // ── unknown method ────────────────────────────────────────────────
            method => Some(JsonRpcResponse::error_response(
                id,
                METHOD_NOT_FOUND,
                format!("Method not found: {method}"),
            )),
        }
    }

    fn check_auth(&self, token: Option<&str>) -> bool {
        token.map(|t| self.validate_jwt(t)).unwrap_or(false)
    }

    fn validate_jwt(&self, token: &str) -> bool {
        use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
        decode::<jwt::Claims>(
            token,
            &DecodingKey::from_secret(self.state.config.jwt_secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .is_ok()
    }
}
