//! mock-llm — a constant-latency, OpenAI-compatible mock server.
//!
//! ## Why this exists
//!
//! Janus is an LLM proxy. To measure Janus's *own* overhead we must talk to
//! something downstream of it that has **constant, known timing**. Real OpenAI
//! varies by tens or hundreds of milliseconds between calls; that variance
//! would dominate any signal coming from Janus itself.
//!
//! ## What this server pretends to do
//!
//! It speaks the OpenAI `POST /v1/chat/completions` schema. Janus's `openai`
//! provider adapter cannot tell it apart from `api.openai.com` apart from one
//! thing: its base_url. Janus reads that base_url from its DB at startup.
//!
//! ## How it controls latency
//!
//! Two knobs:
//!   * `--ttft-ms`  — how long to wait before sending the first byte of the
//!                    response (or first SSE chunk). Default 250 ms.
//!   * `--tpot-ms`  — how long to wait between each subsequent SSE chunk in a
//!                    streaming response. Default 20 ms.
//!
//! Both are honoured by `tokio::time::sleep`, which yields to the runtime so
//! we don't block worker threads. That means the mock can sustain very high
//! concurrency without its own latency drifting.
//!
//! ## What it does NOT do
//!
//! - Token counting (returns hard-coded usage numbers).
//! - Validating the prompt content.
//! - Validating the API key (any non-empty Bearer is accepted).
//! - Modelling rate limits or error rates.
//!
//! If you find yourself wanting any of these, you are no longer benchmarking —
//! you are integration-testing. Use a different tool.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{sse::{Event, Sse}, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;
use uuid::Uuid;

/// CLI flags — everything important is configurable from the command line.
/// We deliberately use a flat structure (no config file) so that the exact
/// invocation appears in process listings and shell history.
#[derive(Debug, Parser)]
#[command(name = "mock-llm", about = "Constant-latency OpenAI mock for benchmarks")]
struct Args {
    /// TCP port to listen on. Janus will be configured to talk to localhost:<port>.
    #[arg(long, default_value_t = 9999)]
    port: u16,

    /// Time-to-first-token in milliseconds. The delay between receiving the
    /// request and sending the first byte of the response (or first SSE chunk).
    #[arg(long, default_value_t = 250)]
    ttft_ms: u64,

    /// Time-per-output-token in milliseconds. The pause between subsequent SSE
    /// chunks in a streaming response. Ignored for non-streaming requests.
    #[arg(long, default_value_t = 20)]
    tpot_ms: u64,

    /// Number of output tokens to emit per response. Each "token" in this mock
    /// is the word "tok" — we are not measuring tokenization, just throughput.
    #[arg(long, default_value_t = 50)]
    output_tokens: usize,

    /// Static prompt-tokens count to report in the `usage` block. Set to a
    /// realistic value matching your profile so cost calculations in Janus
    /// match the expected math.
    #[arg(long, default_value_t = 200)]
    prompt_tokens_reported: u32,
}

/// Shared state for handlers. Cloned per request (cheap — it's an Arc).
#[derive(Clone)]
struct AppState {
    ttft: Duration,
    tpot: Duration,
    output_tokens: usize,
    prompt_tokens_reported: u32,
}

/// OpenAI-style request body. We only need the fields Janus will populate; the
/// rest are accepted as `serde_json::Value` so unknown fields don't break us.
#[derive(Debug, Deserialize)]
struct ChatRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    stream: bool,
    // We don't validate or use messages — accept any shape.
    #[allow(dead_code)]
    #[serde(default)]
    messages: Value,
}

/// Non-streaming response shape. Matches OpenAI's spec exactly because Janus's
/// adapter deserialises this into its own internal struct.
#[derive(Debug, Serialize)]
struct ChatResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: u32,
    message: Message,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[tokio::main]
async fn main() {
    // Initialise logging. Default level INFO; tweak with RUST_LOG=mock_llm=debug.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let state = Arc::new(AppState {
        ttft: Duration::from_millis(args.ttft_ms),
        tpot: Duration::from_millis(args.tpot_ms),
        output_tokens: args.output_tokens,
        prompt_tokens_reported: args.prompt_tokens_reported,
    });

    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        // Janus probes `/v1/models` for capability discovery. Return a small,
        // realistic catalogue so the provider self-test passes.
        .route("/v1/models", get(list_models))
        // A health endpoint for the harness to probe before starting a run.
        .route("/healthz", get(healthz))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind port");

    tracing::info!(
        port = args.port,
        ttft_ms = args.ttft_ms,
        tpot_ms = args.tpot_ms,
        output_tokens = args.output_tokens,
        "mock-llm started"
    );

    axum::serve(listener, app).await.expect("server crashed");
}

async fn healthz() -> &'static str {
    "ok\n"
}

async fn list_models(State(_state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            { "id": "gpt-4o", "object": "model", "created": 1700000000, "owned_by": "mock" },
            { "id": "gpt-4o-mini", "object": "model", "created": 1700000000, "owned_by": "mock" },
            { "id": "gpt-3.5-turbo", "object": "model", "created": 1700000000, "owned_by": "mock" },
        ]
    }))
}

/// The actual endpoint Janus calls.
///
/// Flow:
///   1. Sleep `ttft` ms (simulates the upstream's "thinking" time).
///   2. If stream=false: return a single JSON response with hard-coded usage.
///   3. If stream=true:  emit `output_tokens` SSE chunks separated by `tpot` ms,
///      then a final usage chunk, then `[DONE]`.
///
/// We deliberately ignore the Authorization header — any non-empty bearer is
/// accepted. Authentication is Janus's job; the mock is the upstream.
async fn chat_completions(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Response {
    // Honour TTFT before doing anything else.
    sleep(state.ttft).await;

    let model = if req.model.is_empty() {
        "gpt-4o".to_string()
    } else {
        req.model.clone()
    };

    if !req.stream {
        return non_streaming(&state, &model).into_response();
    }

    streaming(state.clone(), model).into_response()
}

/// Non-streaming branch: one JSON blob, no further delays.
fn non_streaming(state: &AppState, model: &str) -> Response {
    let content = (0..state.output_tokens)
        .map(|_| "tok")
        .collect::<Vec<_>>()
        .join(" ");

    let body = ChatResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion",
        created: now_secs(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant",
                content,
            },
            finish_reason: "stop",
        }],
        usage: Usage {
            prompt_tokens: state.prompt_tokens_reported,
            completion_tokens: state.output_tokens as u32,
            total_tokens: state.prompt_tokens_reported + state.output_tokens as u32,
        },
    };

    (StatusCode::OK, Json(body)).into_response()
}

/// Streaming branch: emit `output_tokens + 2` SSE events (chunks + usage + [DONE]).
///
/// Why an explicit `async-stream` style instead of a channel? Because the
/// timing here is the entire point of the mock. Inline `sleep().await` between
/// `yield` points is the most direct way to make the timing visible — anyone
/// reading this function can see the exact protocol.
fn streaming(
    state: Arc<AppState>,
    model: String,
) -> Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = async_stream::stream! {
        let id = format!("chatcmpl-{}", Uuid::new_v4());
        let created = now_secs();

        // First chunk: announce role. OpenAI's convention.
        let first = json!({
            "id": &id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": "" },
                "finish_reason": null
            }]
        });
        yield Ok(Event::default().data(first.to_string()));

        // Body chunks: one per token, with tpot pause between each.
        for i in 0..state.output_tokens {
            sleep(state.tpot).await;
            let token = if i == 0 { "tok".to_string() } else { " tok".to_string() };
            let chunk = json!({
                "id": &id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": { "content": token },
                    "finish_reason": null
                }]
            });
            yield Ok(Event::default().data(chunk.to_string()));
        }

        // Stop chunk.
        let stop = json!({
            "id": &id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        });
        yield Ok(Event::default().data(stop.to_string()));

        // Usage chunk. Sent when the client asks for stream_options.include_usage.
        // We send it unconditionally because Janus expects it.
        let usage = json!({
            "id": &id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [],
            "usage": {
                "prompt_tokens": state.prompt_tokens_reported,
                "completion_tokens": state.output_tokens,
                "total_tokens": state.prompt_tokens_reported as usize + state.output_tokens,
            }
        });
        yield Ok(Event::default().data(usage.to_string()));

        // Sentinel. axum's Sse handler does NOT auto-append [DONE] — we must.
        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(stream)
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
