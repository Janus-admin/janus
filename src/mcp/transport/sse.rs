// src/mcp/transport/sse.rs
// SSE transport helpers for the MCP server.
//
// The SSE endpoint (GET /mcp/sse):
//   1. Validates the admin JWT from Authorization: Bearer or ?token= query param.
//   2. Sends an initial `endpoint` SSE event pointing to POST /mcp/rpc.
//   3. Sends a `message` SSE event with server capabilities.
//   4. Keeps the connection open with periodic keep-alive pings.

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;

/// Build the SSE stream sent when a client connects to GET /mcp/sse.
///
/// `base_url` is the scheme+host used to construct the endpoint URL that
/// clients should POST to (e.g. "http://127.0.0.1:8080").
pub fn build_sse_stream(
    base_url: String,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let endpoint_event = Event::default()
        .event("endpoint")
        .data(format!("{base_url}/mcp/rpc"));

    let caps_event = Event::default().event("message").data(
        serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {
                "serverInfo": { "name": "velox", "version": "0.1.0" },
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} }
            }
        }))
        .unwrap_or_default(),
    );

    let events = stream::iter(vec![
        Ok::<Event, Infallible>(endpoint_event),
        Ok::<Event, Infallible>(caps_event),
    ]);

    // After the initial burst, keep the connection alive with 30-second pings.
    Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    )
}
