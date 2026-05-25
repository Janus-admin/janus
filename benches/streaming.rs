use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use janus::providers::{ChatCompletionChunk, ChunkChoice, ChunkDelta, UsageData};

// ── Fixtures ──────────────────────────────────────────────────────────────────

fn make_chunk(content: &str, finish: bool) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: "chatcmpl-bench".to_string(),
        object: "chat.completion.chunk".to_string(),
        created: 1_700_000_000,
        model: "gpt-4o".to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: if finish {
                    None
                } else {
                    Some(content.to_string())
                },
            },
            finish_reason: if finish {
                Some("stop".to_string())
            } else {
                None
            },
        }],
        usage: if finish {
            Some(UsageData {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            })
        } else {
            None
        },
    }
}

/// Build a simulated token stream: N content chunks followed by a final stop chunk.
fn make_stream(n_chunks: usize) -> Vec<ChatCompletionChunk> {
    let mut chunks: Vec<ChatCompletionChunk> =
        (0..n_chunks).map(|_| make_chunk("token ", false)).collect();
    chunks.push(make_chunk("", true));
    chunks
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

/// Measures the cost of serializing one SSE chunk to JSON.
/// This runs once per token on the hot streaming path.
fn bench_chunk_serialization(c: &mut Criterion) {
    let chunk = make_chunk("Hello", false);
    let final_chunk = make_chunk("", true);

    c.bench_function("chunk_serialize_content", |b| {
        b.iter(|| serde_json::to_string(black_box(&chunk)).unwrap())
    });

    c.bench_function("chunk_serialize_final_with_usage", |b| {
        b.iter(|| serde_json::to_string(black_box(&final_chunk)).unwrap())
    });
}

/// Measures the cost of building the SSE wire frame ("data: {...}\n\n").
/// Every chunk sent to the client goes through this formatting step.
fn bench_sse_framing(c: &mut Criterion) {
    let chunk = make_chunk("Hello, world!", false);
    let json = serde_json::to_string(&chunk).unwrap();

    c.bench_function("sse_frame_format", |b| {
        b.iter(|| {
            let mut frame = String::with_capacity(json.len() + 8);
            frame.push_str("data: ");
            frame.push_str(black_box(&json));
            frame.push_str("\n\n");
            frame
        })
    });
}

/// Measures throughput of accumulating tokens across a complete streaming response.
/// This models the work done inside `pipeline::run_streaming()` as chunks arrive.
fn bench_stream_accumulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_token_accumulation");

    for &n in &[10usize, 100, 500, 1_000] {
        let stream = make_stream(n);
        group.throughput(Throughput::Elements(n as u64 + 1));

        group.bench_with_input(BenchmarkId::from_parameter(n), &stream, |b, stream| {
            b.iter(|| {
                let mut prompt_tokens = 0u32;
                let mut completion_tokens = 0u32;
                let mut full_text = String::new();
                let mut ttfb_recorded = false;
                let mut ttfb_chunk_idx = 0usize;

                for (idx, chunk) in black_box(stream).iter().enumerate() {
                    // Record TTFB index on first non-empty delta.
                    if !ttfb_recorded {
                        if chunk.choices[0].delta.content.is_some() {
                            ttfb_chunk_idx = idx;
                            ttfb_recorded = true;
                        }
                    }

                    // Accumulate text.
                    if let Some(text) = &chunk.choices[0].delta.content {
                        full_text.push_str(text);
                    }

                    // Collect usage from the final chunk.
                    if let Some(usage) = &chunk.usage {
                        prompt_tokens = usage.prompt_tokens;
                        completion_tokens = usage.completion_tokens;
                    }
                }

                (full_text, prompt_tokens, completion_tokens, ttfb_chunk_idx)
            })
        });
    }
    group.finish();
}

/// Measures the full serialize-and-frame cost for a complete streaming response.
/// Combines chunk serialization + SSE framing across all N chunks.
fn bench_stream_serialize_and_frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_full_serialize");

    for &n in &[10usize, 100, 500] {
        let stream = make_stream(n);
        group.throughput(Throughput::Elements(n as u64 + 1));

        group.bench_with_input(BenchmarkId::from_parameter(n), &stream, |b, stream| {
            b.iter(|| {
                let mut total_bytes = 0usize;
                for chunk in black_box(stream) {
                    let json = serde_json::to_string(chunk).unwrap();
                    // Format as SSE frame
                    let frame = format!("data: {json}\n\n");
                    total_bytes += frame.len();
                }
                // Final [DONE] sentinel
                total_bytes += "data: [DONE]\n\n".len();
                total_bytes
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_chunk_serialization,
    bench_sse_framing,
    bench_stream_accumulation,
    bench_stream_serialize_and_frame,
);
criterion_main!(benches);
