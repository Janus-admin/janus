// src/handlers/mcp.rs
// Axum handlers for the MCP (Model Context Protocol) server.
//
// Endpoints:
//   POST /mcp/rpc  — stateless JSON-RPC 2.0 (JWT in Authorization header)
//   GET  /mcp/sse  — SSE transport (JWT in Authorization header or ?token= param)

use crate::{
    mcp::{transport::sse, JsonRpcRequest, McpServer},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── POST /mcp/rpc ─────────────────────────────────────────────────────────────

/// POST /mcp/rpc
///
/// Accepts a single JSON-RPC 2.0 request and returns the response synchronously.
/// Authentication: `Authorization: Bearer <admin-jwt>`.
///
/// A valid `initialize` call also accepts `params.token` for clients that
/// cannot set HTTP headers.
pub async fn rpc_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let token = extract_bearer(&headers);
    let server = McpServer::new(state);

    match server.handle(request, token).await {
        Some(resp) => Json(resp).into_response(),
        // Notification — 204 No Content.
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

// ── GET /mcp/sse ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// Allow passing the JWT as a query parameter for clients that cannot set
    /// the Authorization header on an EventSource connection.
    pub token: Option<String>,
}

/// GET /mcp/sse
///
/// Opens an SSE stream.  Validates the admin JWT, then sends:
///   1. An `endpoint` event pointing to POST /mcp/rpc.
///   2. A `message` event with server capabilities.
///   3. Periodic keep-alive pings every 30 seconds.
pub async fn sse_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SseQuery>,
) -> impl IntoResponse {
    // Accept token from header or query param.
    let token: Option<&str> = extract_bearer(&headers).or(query.token.as_deref());

    // Validate before opening the SSE stream.
    let authed = token
        .map(|tok| validate_jwt(tok, &state.config.jwt_secret))
        .unwrap_or(false);

    if !authed {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "code": "UNAUTHORIZED",
                    "message": "Valid admin JWT required"
                }
            })),
        )
            .into_response();
    }

    // Derive the base URL from the Host header so the endpoint event is
    // correct regardless of which port the server is running on.
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
        "http"
    } else {
        "https"
    };
    let base_url = format!("{scheme}://{host}");

    sse::build_sse_stream(base_url).into_response()
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

fn validate_jwt(token: &str, secret: &str) -> bool {
    use crate::middleware::jwt::Claims;
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .is_ok()
}
