use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::json;
use std::sync::{Arc, Barrier};
use uuid::Uuid;
use velox::{
    cache::{exact::compute_hash, CacheEngine},
    middleware::rate_limit::RateLimiter,
    providers::{
        ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, UsageData,
    },
};

// ── Fixtures ──────────────────────────────────────────────────────────────────

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
                content: serde_json::Value::String("42".to_string()),
                name: None,
            },
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: UsageData {
            prompt_tokens: 20,
            completion_tokens: 5,
            total_tokens: 25,
        },
    }
}

fn make_request(prompt: &str) -> ChatCompletionRequest {
    serde_json::from_value(json!({
        "model": "gpt-4o",
        "messages": [{ "role": "user", "content": prompt }]
    }))
    .unwrap()
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

/// Rate limiter throughput under N concurrent threads, each using a unique key.
///
/// Models N simultaneous users each making their first request of the minute.
/// No key shares a limit → all calls succeed, measuring pure DashMap write overhead.
fn bench_rate_limiter_concurrent_keys(c: &mut Criterion) {
    let limiter = RateLimiter::new(60);
    let mut group = c.benchmark_group("rate_limiter_concurrent_keys");

    for &n_threads in &[1usize, 2, 4, 8, 16] {
        group.throughput(Throughput::Elements(n_threads as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(n_threads),
            &n_threads,
            |b, &n| {
                b.iter_custom(|iters| {
                    let limiter = Arc::clone(&limiter);
                    let barrier = Arc::new(Barrier::new(n + 1));

                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            let limiter = Arc::clone(&limiter);
                            let barrier = Arc::clone(&barrier);
                            std::thread::spawn(move || {
                                barrier.wait();
                                let start = std::time::Instant::now();
                                for _ in 0..iters {
                                    let key_id = black_box(Uuid::new_v4());
                                    let _ = limiter.check_and_record(key_id, 1_000);
                                }
                                start.elapsed()
                            })
                        })
                        .collect();

                    barrier.wait();
                    let total: std::time::Duration =
                        handles.into_iter().map(|h| h.join().unwrap()).sum();
                    total / n as u32
                })
            },
        );
    }
    group.finish();
}

/// Rate limiter throughput when N threads all check the same key.
///
/// Models a single high-traffic API key receiving burst requests concurrently.
/// All threads contend on one DashMap bucket — worst-case scenario.
fn bench_rate_limiter_shared_key(c: &mut Criterion) {
    let limiter = RateLimiter::new(60);
    let shared_key = Uuid::new_v4();
    let mut group = c.benchmark_group("rate_limiter_shared_key");

    for &n_threads in &[1usize, 2, 4, 8] {
        group.throughput(Throughput::Elements(n_threads as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(n_threads),
            &n_threads,
            |b, &n| {
                b.iter_custom(|iters| {
                    let limiter = Arc::clone(&limiter);
                    let barrier = Arc::new(Barrier::new(n + 1));

                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            let limiter = Arc::clone(&limiter);
                            let barrier = Arc::clone(&barrier);
                            std::thread::spawn(move || {
                                barrier.wait();
                                let start = std::time::Instant::now();
                                for _ in 0..iters {
                                    let _ =
                                        limiter.check_and_record(black_box(shared_key), 1_000_000);
                                }
                                start.elapsed()
                            })
                        })
                        .collect();

                    barrier.wait();
                    let total: std::time::Duration =
                        handles.into_iter().map(|h| h.join().unwrap()).sum();
                    total / n as u32
                })
            },
        );
    }
    group.finish();
}

/// Hot cache read throughput under N concurrent threads, all reading the same entry.
///
/// Models the most common production pattern: a popular prompt cached in memory,
/// many concurrent users all getting a cache hit simultaneously.
fn bench_cache_concurrent_reads(c: &mut Criterion) {
    let cache = Arc::new(CacheEngine::new());
    let req = make_request("What is the capital of France?");
    let hash = compute_hash(&req);
    cache.insert(hash.clone(), Arc::new(make_response()));

    let mut group = c.benchmark_group("cache_concurrent_reads");

    for &n_threads in &[1usize, 2, 4, 8, 16] {
        group.throughput(Throughput::Elements(n_threads as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(n_threads),
            &n_threads,
            |b, &n| {
                b.iter_custom(|iters| {
                    let cache = Arc::clone(&cache);
                    let hash = hash.clone();
                    let barrier = Arc::new(Barrier::new(n + 1));

                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            let cache = Arc::clone(&cache);
                            let hash = hash.clone();
                            let barrier = Arc::clone(&barrier);
                            std::thread::spawn(move || {
                                barrier.wait();
                                let start = std::time::Instant::now();
                                for _ in 0..iters {
                                    let _ = cache.lookup(black_box(&hash));
                                }
                                start.elapsed()
                            })
                        })
                        .collect();

                    barrier.wait();
                    let total: std::time::Duration =
                        handles.into_iter().map(|h| h.join().unwrap()).sum();
                    total / n as u32
                })
            },
        );
    }
    group.finish();
}

/// Mixed read/write cache throughput: 80% reads, 20% writes across N threads.
///
/// Models realistic traffic: most requests are cache hits, some are new prompts
/// being written to the cache after a provider response.
fn bench_cache_mixed_rw(c: &mut Criterion) {
    let cache = Arc::new(CacheEngine::new());
    let resp = Arc::new(make_response());

    // Pre-warm with 100 entries so reads have something to hit.
    for i in 0..100usize {
        let req = make_request(&format!("Warm-up prompt number {i}"));
        let hash = compute_hash(&req);
        cache.insert(hash, Arc::clone(&resp));
    }

    // Fixed read key (always hits)
    let read_req = make_request("Warm-up prompt number 0");
    let read_hash = compute_hash(&read_req);

    let mut group = c.benchmark_group("cache_mixed_rw_80_20");

    for &n_threads in &[1usize, 2, 4, 8] {
        group.throughput(Throughput::Elements(n_threads as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(n_threads),
            &n_threads,
            |b, &n| {
                b.iter_custom(|iters| {
                    let cache = Arc::clone(&cache);
                    let resp = Arc::clone(&resp);
                    let read_hash = read_hash.clone();
                    let barrier = Arc::new(Barrier::new(n + 1));

                    let handles: Vec<_> = (0..n)
                        .map(|thread_id| {
                            let cache = Arc::clone(&cache);
                            let resp = Arc::clone(&resp);
                            let read_hash = read_hash.clone();
                            let barrier = Arc::clone(&barrier);
                            std::thread::spawn(move || {
                                barrier.wait();
                                let start = std::time::Instant::now();
                                for i in 0..iters {
                                    if i % 5 == 0 {
                                        // 20%: write a unique key
                                        let write_hash = format!("bench-write-{thread_id}-{i}");
                                        cache.insert(write_hash, Arc::clone(&resp));
                                    } else {
                                        // 80%: read the pre-warmed key
                                        let _ = cache.lookup(black_box(&read_hash));
                                    }
                                }
                                start.elapsed()
                            })
                        })
                        .collect();

                    barrier.wait();
                    let total: std::time::Duration =
                        handles.into_iter().map(|h| h.join().unwrap()).sum();
                    total / n as u32
                })
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_rate_limiter_concurrent_keys,
    bench_rate_limiter_shared_key,
    bench_cache_concurrent_reads,
    bench_cache_mixed_rw,
);
criterion_main!(benches);
