# Chapter 1 — The Mock Upstream

> If you skipped CHAPTER0, go back. This chapter assumes you understand *why* we need a mock.

---

## 1.1 What `mock-llm` is

A standalone Rust binary at `benchmarks/mock-llm/`. It speaks the OpenAI HTTP API but, instead of calling a real model, it sleeps for configurable amounts of time and returns canned responses.

You start it once before a benchmark:

```bash
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 250 --tpot-ms 20
```

Janus, configured to use `http://localhost:9999` as its OpenAI base_url, has no idea it's talking to a fake.

---

## 1.2 The two knobs that matter

```text
--ttft-ms 250     time from receiving the request to sending the first byte
--tpot-ms 20      time between subsequent SSE chunks (streaming only)
```

These two numbers control the entire "physics" of the simulated upstream. Realistic defaults:

| Real model | Realistic `--ttft-ms` | Realistic `--tpot-ms` |
|---|---:|---:|
| GPT-4o | 400–600 | 18–25 |
| Claude 3.5 Sonnet | 500–700 | 14–20 |
| GPT-4o-mini | 200–400 | 10–15 |
| Groq Llama-70B | 80–150 | 3–6 |

If you want to benchmark Janus against a Groq-like upstream, pass `--ttft-ms 100 --tpot-ms 4`. The whole point is that *you* control the upstream's profile so it doesn't drift.

---

## 1.3 Walking through the source

The file is [mock-llm/src/main.rs](mock-llm/src/main.rs). About 220 lines. Read it. Below is a tour of the parts that matter.

### Listening

```rust
let listener = tokio::net::TcpListener::bind(&addr).await?;
axum::serve(listener, app).await?;
```

A plain axum HTTP server. Nothing exotic. If port 9999 is busy, the server fails immediately — change the `--port` flag.

### The endpoint

```rust
.route("/v1/chat/completions", post(chat_completions))
```

That's the *only* endpoint Janus actually calls during a chat benchmark. We also implement `/v1/models` because Janus probes it during provider health checks; and `/healthz` for our own pre-flight in `run.sh`.

### The TTFT pause

```rust
sleep(state.ttft).await;
```

This is the first line of the handler. Every response — streaming or not — waits this long before doing anything. That's how we simulate the model "thinking". `tokio::time::sleep` is async, so it yields the worker thread back to other requests; the mock can sustain many thousands of concurrent in-flight requests without its own timing drifting.

### The streaming branch

```rust
let stream = async_stream::stream! {
    yield first_chunk;
    for i in 0..output_tokens {
        sleep(state.tpot).await;
        yield chunk_for_token(i);
    }
    yield stop_chunk;
    yield usage_chunk;
    yield [DONE];
};
```

Each `sleep` happens between yields, so the client genuinely waits `tpot_ms` between SSE events. This matters: the per-token latency Janus measures is the per-token latency *you* configured here. There is no other source of variance.

### What it does NOT validate

- The Authorization header (any non-empty `Bearer` is accepted).
- The prompt content (the `messages` field is parsed into `Value` and ignored).
- The token count of the request (always reports the static `--prompt-tokens-reported` value in the `usage` block).

This is by design. If the mock validated input, you'd be partially benchmarking the validator. We want Janus's overhead to be the only signal.

---

## 1.4 Why a separate Cargo project

Look at `benchmarks/mock-llm/Cargo.toml`. It is **not** a workspace member of Janus. Reasons:

1. **Independence.** When someone clones Janus and runs `cargo build` they should not pay the price of compiling axum twice (Janus already has it, with different features).
2. **Distribution.** The mock could be shipped as a tiny static binary on its own. We may want that later for users who don't have Rust installed.
3. **Determinism.** The mock's compile flags (`lto = false`, single codegen unit) are tuned for *predictable* timing, not throughput. We don't want those settings infecting Janus's release build.

The price you pay is one extra build step the first time:

```bash
cargo build --release --manifest-path benchmarks/mock-llm/Cargo.toml
```

After that, the binary lives at `benchmarks/mock-llm/target/release/mock-llm` and you ignore it.

---

## 1.5 Verifying the mock yourself

You should run these by hand once to feel what the mock does. The numbers will not lie to you.

### Health check

```bash
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 250 --tpot-ms 20 &
sleep 0.5
curl -s http://localhost:9999/healthz
# → ok
```

### Non-streaming timing

```bash
time curl -s -X POST http://localhost:9999/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer anything" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}' >/dev/null
# → real    0m0.253s
```

That's the 250 ms `--ttft-ms` you set, plus a couple of ms for the HTTP round-trip. **This is the floor for any TTFT Janus can measure.** Janus cannot be faster than its upstream. Anything Janus adds on top is overhead.

### Streaming timing

```bash
time curl -s -N -X POST http://localhost:9999/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}' >/dev/null
# With defaults: 250 ms TTFT + 50 tokens × 20 ms = 1 250 ms total
# → real    0m1.260s
```

Predictable. Boring. *Good.* The boredom of these numbers is what makes the mock useful — it never surprises you.

---

## 1.6 Common gotchas

### Port already in use

```text
Error: failed to bind port — Address already in use (os error 48)
```

A previous mock is still running. Find it: `lsof -i :9999`. Kill it: `kill <pid>`.

### Janus connects to real OpenAI by mistake

Symptom: latency is way higher than expected, or you see real OpenAI errors.

Cause: Janus's DB still has `https://api.openai.com/v1` as the openai provider's base_url. The seed script (CHAPTER 3) fixes this. Make sure you ran `seed.sh` **and** restarted Janus after.

### The mock seems to slow down at high concurrency

Symptom: at 200+ concurrent connections, mock TTFT measured by oha drifts upward.

Cause: you're saturating your OS's TCP backlog, the kernel's epoll handling, or the mock's tokio runtime. Drop concurrency, or run mock and Janus on separate machines.

### TTFT looks higher than configured

If `--ttft-ms 250` is producing measured TTFTs of 280 ms, the extra 30 ms is your network stack (loopback isn't free), DNS, TLS handshake (if you accidentally enabled it), or connection establishment. For repeated requests with keep-alive (which `oha` uses), the overhead drops to a few ms.

---

## 1.7 Extending the mock — and when not to

The mock is intentionally minimal. Resist the urge to add:

- ❌ Realistic tokenization (your benchmark would now also measure your tokenizer).
- ❌ Variable latency ("more realistic") (your numbers stop being repeatable).
- ❌ Random failures (your benchmark would now measure error handling, which deserves its own harness).
- ❌ Stateful conversation memory (the upstream is meant to be stateless for measurement).

What *would* be reasonable to add later, if you have a clear motivation:

- ✅ A "spike" mode that sleeps an extra N ms every K requests (to study how Janus handles upstream tail latency).
- ✅ A "rate-limit" mode that returns 429s at a configured rate (to study Janus's retry behaviour).

If you find yourself wanting one of these, write a *new mock* with a clear flag. Don't muddy this one.

---

*Next: [CHAPTER2_profiles.md](CHAPTER2_profiles.md). It explains the four workload profiles we ship and how to write your own.*
