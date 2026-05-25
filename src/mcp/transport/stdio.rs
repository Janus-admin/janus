// src/mcp/transport/stdio.rs
// Stdio transport: read newline-delimited JSON-RPC from stdin, write responses to stdout.
//
// Usage: janus --mcp-stdio
//
// The first message must be an `initialize` request containing the admin JWT in
// `params.token`.  Subsequent messages are processed without re-authentication.

use crate::mcp::{McpServer, PARSE_ERROR};
use crate::state::AppState;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Run the stdio MCP transport until stdin is closed.
///
/// Reads one JSON-RPC request per line, dispatches to `McpServer`, and writes
/// the response (if any) followed by a newline to stdout.
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let server = McpServer::new(state);
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    // Track whether the client has authenticated via `initialize`.
    let mut session_token: Option<String> = None;

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request; write a parse-error response on failure.
        let req = match serde_json::from_str::<crate::mcp::JsonRpcRequest>(&line) {
            Ok(r) => r,
            Err(e) => {
                let err = crate::mcp::JsonRpcResponse::error_response(
                    None,
                    PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                let mut out = serde_json::to_string(&err).unwrap_or_default();
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        // After a successful `initialize` the session_token is set.
        // For subsequent requests pass it as the bearer token.
        let token_ref = session_token.as_deref();

        let response = server.handle(req.clone(), token_ref).await;

        // After a successful initialize, capture the token for the session.
        if req.method == "initialize" {
            if let Some(ref params) = req.params {
                if let Some(tok) = params.get("token").and_then(serde_json::Value::as_str) {
                    // Only store if the server accepted it (response is a result, not error).
                    if let Some(ref resp) = response {
                        if resp.result.is_some() {
                            session_token = Some(tok.to_string());
                        }
                    }
                }
            }
        }

        if let Some(resp) = response {
            let mut out = serde_json::to_string(&resp).unwrap_or_default();
            out.push('\n');
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}
