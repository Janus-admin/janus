use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use janus::{
    cache::{exact::compute_hash, CacheEngine},
    providers::{
        ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, UsageData,
    },
};
use serde_json::json;
use std::sync::Arc;

// ── Fixtures ──────────────────────────────────────────────────────────────────

fn make_request(n_messages: usize) -> ChatCompletionRequest {
    let messages: Vec<serde_json::Value> = (0..n_messages)
        .map(|i| {
            if i % 2 == 0 {
                json!({ "role": "user", "content": format!("Turn {i}: what is the meaning of life?") })
            } else {
                json!({ "role": "assistant", "content": "The answer is 42." })
            }
        })
        .collect();
    serde_json::from_value(json!({ "model": "gpt-4o", "messages": messages })).unwrap()
}

fn make_response() -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: "chatcmpl-bench".to_string(),
        object: "chat.completion".to_string(),
        created: 1_700_000_000,
        model: "gpt-4o".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String("The answer is 42.".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 50,
            completion_tokens: 10,
            total_tokens: 60,
        },
    }
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_request_deserialization(c: &mut Criterion) {
    let raw_2msg = serde_json::to_string(&json!({
        "model": "gpt-4o",
        "messages": [
            { "role": "system", "content": "You are a helpful assistant." },
            { "role": "user",   "content": "What is the capital of France?" }
        ]
    }))
    .unwrap();

    let raw_16msg = serde_json::to_string(&make_request(16)).unwrap();

    c.bench_function("request_deserialize_2_messages", |b| {
        b.iter(|| {
            let _: ChatCompletionRequest = serde_json::from_str(black_box(&raw_2msg)).unwrap();
        })
    });

    c.bench_function("request_deserialize_16_messages", |b| {
        b.iter(|| {
            let _: ChatCompletionRequest = serde_json::from_str(black_box(&raw_16msg)).unwrap();
        })
    });
}

fn bench_response_serialization(c: &mut Criterion) {
    let response = make_response();

    c.bench_function("response_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&response)).unwrap())
    });
}

fn bench_hot_cache(c: &mut Criterion) {
    let cache = CacheEngine::new();
    let req = make_request(2);
    let hash = compute_hash(&req);
    cache.insert(hash.clone(), Arc::new(make_response()));

    c.bench_function("hot_cache_hit", |b| {
        b.iter(|| cache.lookup(black_box(&hash)))
    });

    c.bench_function("hot_cache_miss", |b| {
        b.iter(|| cache.lookup(black_box("deadbeefdeadbeefdeadbeefdeadbeef")))
    });

    c.bench_function("hot_cache_insert", |b| {
        let resp = Arc::new(make_response());
        b.iter(|| cache.insert(black_box(hash.clone()), Arc::clone(&resp)))
    });
}

/// End-to-end in-memory pipeline: deserialize → hash → lookup (miss) → insert.
/// This models the overhead Janus adds to every request that misses the cache,
/// excluding the actual provider call and all I/O.
fn bench_pipeline_overhead(c: &mut Criterion) {
    let cache = CacheEngine::new();
    let resp = Arc::new(make_response());

    let mut group = c.benchmark_group("pipeline_in_memory_overhead");
    for &n in &[1usize, 4, 16] {
        let raw = serde_json::to_string(&make_request(n)).unwrap();
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(n), &raw, |b, raw| {
            b.iter(|| {
                // 1. Parse
                let req: ChatCompletionRequest = serde_json::from_str(black_box(raw)).unwrap();
                // 2. Hash
                let hash = compute_hash(&req);
                // 3. Lookup (miss on first call, hit on subsequent — mirrors real traffic)
                if cache.lookup(&hash).is_none() {
                    // 4. Store result
                    cache.insert(hash, Arc::clone(&resp));
                }
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_request_deserialization,
    bench_response_serialization,
    bench_hot_cache,
    bench_pipeline_overhead,
);
criterion_main!(benches);
