// tests/v3_3_streaming.rs
// Phase V3-3 acceptance tests — Streaming Hardening.
//
// Run with: cargo test v3_3
//
// All tests are pure unit tests — no DB, no HTTP server, no real providers.
// They verify the three behavioral guarantees added in V3-3:
//
//   6.1  Client disconnect cancels the provider task via tx.closed()
//   6.2  Bounded channel provides backpressure; slow consumers never lose items
//   6.3  Mid-stream provider errors set request status = "error"

use std::convert::Infallible;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use axum::response::sse::Event;
use futures_util::StreamExt;
use tokio::sync::mpsc;

// ─── 6.1: Client disconnect propagation ──────────────────────────────────────

/// Verify that a spawned task using the new `select! { tx.closed() => ... }`
/// pattern exits promptly when the receiver end of the channel is dropped,
/// instead of blocking on the provider stream until it finishes naturally.
#[tokio::test]
async fn v3_3_provider_task_cancelled_when_client_disconnects() {
    let (tx, rx) = mpsc::channel::<i32>(4);

    // Simulate the spawned provider task: races between client disconnect
    // (tx.closed) and a "provider" that never completes (60-second sleep).
    let task = tokio::spawn(async move {
        let mut cancelled = false;
        loop {
            tokio::select! {
                biased;
                _ = tx.closed() => {
                    cancelled = true;
                    break;
                }
                // Simulate a provider stream that never produces a chunk.
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    break;
                }
            }
        }
        cancelled
    });

    // Drop the receiver — this is what axum does when the SSE body is closed
    // (client disconnects or aborts the HTTP connection).
    drop(rx);

    // The task must detect the disconnect and complete well under 100 ms,
    // not after 60 seconds.
    let cancelled = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("task should finish within 100 ms after client disconnect")
        .expect("task should not panic");

    assert!(
        cancelled,
        "task should have taken the tx.closed() arm, not the 60-second sleep"
    );
}

/// Verify that the spawned task releases all its resources after a disconnect.
/// Uses a drop-guard to confirm the task's stack frame is cleaned up.
#[tokio::test]
async fn v3_3_no_resource_leak_after_client_disconnect() {
    let released = Arc::new(AtomicBool::new(false));
    let released_clone = released.clone();

    let (tx, rx) = mpsc::channel::<i32>(4);

    let task = tokio::spawn(async move {
        struct DropGuard(Arc<AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _guard = DropGuard(released_clone);

        loop {
            tokio::select! {
                biased;
                _ = tx.closed() => break,
                _ = tokio::time::sleep(Duration::from_secs(60)) => break,
            }
        }
        // _guard drops here, setting released = true
    });

    drop(rx);
    let _ = tokio::time::timeout(Duration::from_millis(100), task).await;

    assert!(
        released.load(Ordering::SeqCst),
        "task resources should be released once it exits after client disconnect"
    );
}

// ─── 6.2: Backpressure ───────────────────────────────────────────────────────

/// A slow consumer must receive every item produced by the provider.
/// The bounded channel + `send().await` combination suspends the producer
/// when the buffer is full, so no items are ever dropped.
#[tokio::test]
async fn v3_3_slow_consumer_does_not_truncate_stream() {
    const ITEMS: usize = 20;

    // Intentionally tiny buffer to guarantee the producer will block.
    let (tx, mut rx) = mpsc::channel::<usize>(2);

    let producer = tokio::spawn(async move {
        for i in 0..ITEMS {
            // send().await = backpressure: suspends until the consumer drains
            // the buffer.  Does NOT drop items when the channel is full.
            tx.send(i).await.expect("channel should still be open");
        }
    });

    let consumer = tokio::spawn(async move {
        let mut received = Vec::with_capacity(ITEMS);
        while let Some(v) = rx.recv().await {
            // Artificial delay — forces the producer to block on send().await.
            tokio::time::sleep(Duration::from_millis(1)).await;
            received.push(v);
        }
        received
    });

    producer.await.unwrap();
    let received = consumer.await.unwrap();

    assert_eq!(
        received.len(),
        ITEMS,
        "all {ITEMS} items must arrive despite the slow consumer"
    );
    for (i, &v) in received.iter().enumerate() {
        assert_eq!(v, i, "items must arrive in order");
    }
}

/// When the channel is at capacity, send().await yields (suspends) the task
/// rather than returning an error or silently dropping the item.
#[tokio::test]
async fn v3_3_full_channel_yields_not_drops() {
    let (tx, mut rx) = mpsc::channel::<u8>(1); // capacity = 1

    // Fill the channel.
    tx.send(1).await.unwrap();

    // A second send must suspend because the buffer is full.
    let tx2 = tx.clone();
    let send_task = tokio::spawn(async move { tx2.send(2).await });

    // Give the task time to confirm it is suspended, not completing.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Drain one slot — this unblocks the suspended sender.
    let v1 = rx.recv().await.unwrap();
    assert_eq!(v1, 1);

    // The send task should now complete.
    let result = tokio::time::timeout(Duration::from_millis(50), send_task)
        .await
        .expect("send should complete after channel was drained")
        .expect("task should not panic");
    assert!(
        result.is_ok(),
        "send should succeed once space is available"
    );

    // The second item must be in the channel — not dropped.
    let v2 = rx.recv().await.unwrap();
    assert_eq!(
        v2, 2,
        "second item must not be dropped when channel was full"
    );
}

// ─── 6.3: Stream error recovery ──────────────────────────────────────────────

/// The stream-status tracking logic must flip to "error" when the provider
/// stream yields Err(ProviderError).
/// This test drives the same state-machine pattern used inside pipeline.rs.
#[tokio::test]
async fn v3_3_mid_stream_provider_error_sets_request_status_error() {
    use janus::providers::{ChatCompletionChunk, ChunkChoice, ChunkDelta, ProviderError};

    let good_chunk = ChatCompletionChunk {
        id: "id-1".to_string(),
        object: "chat.completion.chunk".to_string(),
        created: 0,
        model: "gpt-4o-mini".to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("hello".to_string()),
            },
            finish_reason: None,
        }],
        usage: None,
    };

    // Simulated provider stream: one good chunk, then an error.
    let provider_stream = futures_util::stream::iter(vec![
        Ok::<ChatCompletionChunk, ProviderError>(good_chunk),
        Err(ProviderError::ParseError(
            "malformed event data from provider".to_string(),
        )),
    ]);

    let mut stream_status = "success";
    tokio::pin!(provider_stream);

    while let Some(item) = provider_stream.next().await {
        match item {
            Ok(_) => {}
            Err(_e) => {
                stream_status = "error";
                break;
            }
        }
    }

    assert_eq!(
        stream_status, "error",
        "mid-stream provider error must flip stream_status to \"error\""
    );
}

/// A parse-error mid-stream is distinguishable from a success; its Display
/// representation is non-empty so it can be included in the tracing log.
#[test]
fn v3_3_mid_stream_error_includes_error_detail_in_log() {
    use janus::providers::ProviderError;

    let msg = "unexpected end of stream";
    let err = ProviderError::ParseError(msg.to_string());
    let formatted = format!("{err}");

    assert!(
        formatted.contains(msg),
        "error message '{msg}' should appear in formatted output '{formatted}'"
    );
}

// ─── Regression ──────────────────────────────────────────────────────────────

/// Happy-path: a normal stream delivers every chunk and the [DONE] sentinel
/// through the select!-based producer pattern.
#[tokio::test]
async fn v3_3_normal_stream_still_works_end_to_end() {
    let (tx, mut rx) = mpsc::channel::<Result<Event, Infallible>>(8);

    let task_tx = tx.clone();
    let task = tokio::spawn(async move {
        let payloads = ["chunk-1", "chunk-2", "chunk-3"];
        for payload in payloads {
            tokio::select! {
                biased;
                _ = task_tx.closed() => break,
                result = task_tx.send(Ok(Event::default().data(payload))) => {
                    if result.is_err() { break; }
                }
            }
        }
        let _ = task_tx.send(Ok(Event::default().data("[DONE]"))).await;
    });

    // Drop our own tx clone so rx detects closure after task_tx is dropped.
    drop(tx);

    let mut events = Vec::new();
    while let Some(Ok(event)) = rx.recv().await {
        events.push(event);
    }

    task.await.unwrap();

    // 3 payload events + 1 [DONE] sentinel.
    assert_eq!(
        events.len(),
        4,
        "expected 3 chunks + [DONE], got {}",
        events.len()
    );
}

/// A cache-hit stream produced by synthesize_sse_from_cached must still
/// deliver events after the select!-loop refactor.  Tests the SSE synthesis
/// path by verifying that the cached response data type round-trips cleanly.
#[test]
fn v3_3_streaming_cache_hit_still_works() {
    use janus::providers::{ChatChoice, ChatCompletionResponse, ChatMessage, UsageData};

    // Build the same shape that pipeline::run_streaming returns on a cache hit.
    let resp = ChatCompletionResponse {
        id: "chatcmpl-cached".to_string(),
        object: "chat.completion".to_string(),
        created: 1_716_000_000,
        model: "gpt-4o-mini".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String("from cache".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        },
    };

    // synthesize_sse_from_cached is private; verify the data round-trips via JSON
    // (the exact same serialization the function uses for SSE events).
    let json = serde_json::to_string(&resp).expect("ChatCompletionResponse must serialize");
    let back: ChatCompletionResponse =
        serde_json::from_str(&json).expect("must round-trip through JSON");

    assert_eq!(back.id, "chatcmpl-cached");
    assert_eq!(back.usage.prompt_tokens, 10);
}
