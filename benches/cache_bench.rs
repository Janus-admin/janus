use criterion::{black_box, criterion_group, criterion_main, Criterion};
use janus::{cache::exact::compute_hash, pii::scrub, providers::ChatCompletionRequest};
use serde_json::json;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_request(prompt: &str) -> ChatCompletionRequest {
    serde_json::from_value(json!({
        "model": "gpt-4o",
        "messages": [
            { "role": "system", "content": "You are a helpful assistant." },
            { "role": "user", "content": prompt }
        ]
    }))
    .unwrap()
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_exact_hash(c: &mut Criterion) {
    let req = make_request("What is the capital of France?");

    c.bench_function("exact_cache_hash_short_prompt", |b| {
        b.iter(|| compute_hash(black_box(&req)))
    });

    let long_prompt = "word ".repeat(500);
    let req_long = make_request(&long_prompt);
    c.bench_function("exact_cache_hash_long_prompt", |b| {
        b.iter(|| compute_hash(black_box(&req_long)))
    });
}

fn bench_cosine_similarity(c: &mut Criterion) {
    // Inline cosine similarity — mirrors what SemanticCache does at lookup time.
    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb)
        }
    }

    // 384-dim vectors — same dimensionality as all-MiniLM-L6-v2
    let a: Vec<f32> = (0..384).map(|i| (i as f32).sin()).collect();
    let b: Vec<f32> = (0..384).map(|i| (i as f32).cos()).collect();

    c.bench_function("cosine_similarity_384d", |b_bench| {
        b_bench.iter(|| cosine(black_box(&a), black_box(&b)))
    });

    // Simulated index scan over 1 000 entries
    let index: Vec<Vec<f32>> = (0..1_000)
        .map(|i| (0..384).map(|j| ((i * j) as f32).sin()).collect())
        .collect();

    c.bench_function("cosine_scan_1000_entries", |b_bench| {
        b_bench.iter(|| {
            index
                .iter()
                .map(|entry| cosine(black_box(&a), entry))
                .fold(f32::NEG_INFINITY, f32::max)
        })
    });
}

fn bench_pii_scrubber(c: &mut Criterion) {
    let clean = "What is the capital of France? The answer is Paris.";
    let with_pii = "My email is alice@example.com and my card is 4111 1111 1111 1111. \
                    Please book a flight for me. My SSN is 123-45-6789.";

    c.bench_function("pii_scrub_clean_text", |b| {
        b.iter(|| scrub(black_box(clean)))
    });

    c.bench_function("pii_scrub_text_with_pii", |b| {
        b.iter(|| scrub(black_box(with_pii)))
    });
}

criterion_group!(
    benches,
    bench_exact_hash,
    bench_cosine_similarity,
    bench_pii_scrubber
);
criterion_main!(benches);
