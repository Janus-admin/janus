use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use std::sync::Arc;
use tokio::sync::broadcast;

/// GET /admin/stream (WebSocket upgrade)
///
/// Streams every completed gateway request as a JSON message to connected clients.
/// Each message is a JSON object with request summary fields.
/// The connection is kept alive as long as the client holds it open.
/// If the broadcast buffer overflows (very high traffic), lagged events are skipped.
pub async fn stream_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.event_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, rx))
}

async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<serde_json::Value>) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let text = event.to_string();
                        if socket.send(Message::Text(text)).await.is_err() {
                            // Client disconnected.
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Skip lagged messages; optionally notify the client.
                        tracing::debug!("WebSocket stream lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            // Handle incoming client messages (ping/close frames handled by axum automatically).
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // ignore text/binary from client
                }
            }
        }
    }
}
