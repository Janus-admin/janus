# Chapter 0 — Concepts

> Read this once, slowly. Everything else in this folder builds on it.

---

## 0.1 What a benchmark is, in one sentence

A benchmark answers **one specific question** about a system, with **a method anyone can repeat**, producing **numbers that can be compared over time**.

That's it. Three properties: specific, repeatable, comparable. If you skip any of them, you don't have a benchmark — you have a vibe.

### A bad benchmark

> "Janus is fast."

There is no question, no method, and no number. You can't argue with this and you can't disprove it. Useless.

### A good benchmark

> "On Apple M2 Pro, 32 GB RAM, with the `chat-short` workload profile sent at 50 concurrent connections for 60 seconds against a mock upstream that emits 50 tokens with 250 ms TTFT and 20 ms per token, Janus 0.1.0 (commit `abc1234`) handles **4 217 requests per second** with **p99 end-to-end latency of 3 380 ms** and zero errors."

Notice what changed:

- The **machine** is named.
- The **workload** is named and defined elsewhere.
- The **upstream** is mocked with explicit constants.
- The **build** is identified by commit hash, not "main".
- The **metric** is RPS *and* p99, not "fast".

This is the bar. Every claim in this folder will meet it or won't be made.

---

## 0.2 The two reasons to benchmark

There are really only two:

1. **Capacity planning.** "If I run this on a 4-core VM, how many requests per second before it cracks?" Answers a sizing question.
2. **Regression detection.** "Did the PR I merged yesterday make Janus 30 % slower?" Answers a quality question.

These two goals want different harnesses. Capacity planning wants the **biggest hardware** and the **longest runs**. Regression detection wants **consistent hardware** and **fast runs**. We design for both, prioritising regression detection because that's the daily-driver use case.

---

## 0.3 What we measure — and why each thing matters

These are the metrics this harness produces. For each one, ask yourself: "if this number doubled overnight, which user-facing thing would suffer?"

### TTFT — Time To First Token

The wall-clock time from when the client sends the request to when the first byte of the model's response arrives.

**Why it matters:** in chat UIs, TTFT *is* the perceived latency. A long output that starts streaming in 200 ms feels snappy. A 100-token output that takes 2 seconds to *start* feels broken.

**Janus's contribution to TTFT:** Janus adds overhead on top of the upstream's TTFT. The upstream's TTFT in our harness is a constant (we configure it). So:

```
janus_overhead_ttft = measured_TTFT − mock.ttft_ms
```

That single number is the headline of any Janus benchmark.

### E2E latency — end-to-end

Wall-clock time from request send to last byte of response. Dominated by output token count × per-token latency, not by Janus.

**Why it matters:** for non-streaming clients (cron jobs, batch processing), this is the only latency that exists.

### TPOT — Time Per Output Token

`(E2E − TTFT) / output_token_count`. The steady-state token rate after streaming starts.

**Why it matters:** if TPOT degrades, streaming chat feels jittery even when TTFT is fast.

### RPS — requests per second

How many concurrent successful requests Janus handles per second under load.

**Why it matters:** this is what determines your bill. If Janus does 4 000 RPS per core and you have 100 000 daily users, you need different machines than if it does 400 RPS.

### Cache hit ratio

`cache_hits / total_requests` for the measurement window.

**Why it matters:** Janus's entire value proposition is that cached responses are 100× faster than calling an LLM. If the ratio is high, the latency numbers improve dramatically. We report this alongside latency because they are tied.

### Error rate

`(non_2xx + connection_failures) / total_requests`.

**Why it matters:** A benchmark with errors is not a benchmark. If error rate is non-zero, throw out the run. **Always.** A "fast" system that drops 5 % of requests is not fast — it is broken.

### Process metrics — CPU% and RSS

Steady-state CPU utilisation and resident memory of the Janus process during the measurement window.

**Why it matters:** the same RPS achieved at 30 % CPU vs 95 % CPU mean different things. The former has headroom; the latter is at the edge of falling over. Memory growth over the run hints at leaks.

---

## 0.4 Why mean (average) lies

This is the single most important statistical concept in benchmarking. Internalise it.

Imagine 1 000 requests with these latencies (ms):

```
990 requests:   10 ms each
 10 requests:  500 ms each
```

The **mean** is `(990 × 10 + 10 × 500) / 1 000 = 14.9 ms`. Looks great.

The **p99** is `500 ms`. The slowest 1 % of users have a *fifty-times* worse experience than the average.

If you have 1 million daily requests, that 1 % is **10 000 angry users per day** who experience 500 ms latency. They are the ones writing your support tickets.

The mean hides them. The tail (p95, p99) reveals them.

**Rule:** never report mean latency. Always report `p50` (median), `p95`, and `p99`. The harness will refuse to print a mean by default.

---

## 0.5 The single most important word: "reproducible"

If someone else cannot run your benchmark and get a similar number on similar hardware, your benchmark is not useful — it's a story.

Reproducibility requires:

- **The code:** the harness lives in the same repo as the system being measured. Versioned together.
- **The build:** identified by commit hash, not by "today's main".
- **The hardware:** named specifically. "MacBook Pro" is not specific; "Apple M2 Pro, 12-core, 32 GB unified memory, macOS 14.5" is.
- **The environment:** version of OS, Rust, PostgreSQL. Everything that could affect timing.
- **The workload:** every parameter (concurrency, duration, body, headers) committed as a file.
- **The mock:** all latencies the harness depends on are configured *by us* and committed as flags.

When you publish a Janus number, anyone in the world should be able to clone the repo, check out the commit, run six commands, and see ~the same number. If they cannot, the number does not exist.

---

## 0.6 Workload profile — what it is, why we need many

A **profile** is a frozen description of "what kind of traffic we are sending."

Real production traffic is not a single shape. It is:

- short chat messages (~150 tokens out)
- long RAG generations (~1 000 tokens out)
- function-calling requests (extra schema in body)
- repeated identical requests (cache-friendly)
- one-off questions (cache-busting)

A single profile can answer only one question. So we ship four (see CHAPTER2). When you say "Janus did X RPS," you must also say "on which profile." Otherwise the claim is unfalsifiable.

---

## 0.7 Why we mock the upstream

Real OpenAI:
- varies in latency by time of day
- enforces rate limits we'd hit instantly
- costs money — a full benchmark could be $50–$200
- changes behaviour over time (models get faster, then they don't)

If we benchmark against real OpenAI and get 3 217 RPS today and 2 891 RPS tomorrow, **we cannot tell whether Janus got slower or OpenAI got slower**. The signal is buried in the upstream's noise.

Mock has none of these problems. Every TTFT is exactly 250 ms (or whatever we set). Every per-token delay is exactly 20 ms. Every response is identical. So when our numbers move, **Janus is the only thing that could have changed**.

This is the same trick a chemist uses with a "control" sample. The whole experiment is designed around isolating the variable we care about.

---

## 0.8 Statistical rigor in plain English

Three rules, no exceptions:

1. **At least 3 runs per data point.** Anything can happen on a single run — your laptop's thermal sensor kicks in, Spotify decides to sync, the kernel pauses a thread. Three runs lets you see whether the result is stable.

2. **Discard a warm-up window.** The first ~10 seconds of every run are noisy: JIT-equivalent code paths haven't been taken, connections aren't pooled, OS schedulers are still figuring out who's who. Throw it out.

3. **Report coefficient of variation** (`stddev / mean` — yes, we compute mean for *this* purpose only, never to report latency itself). If CV > 0.10, the runs are too noisy — close other applications, rerun.

The `run.sh` script handles 1 and 2 automatically. You do 3 by eyeballing the report.

---

## 0.9 What "good" looks like — order of magnitude

You won't have intuition for the numbers yet. Here is a rough sense:

| Metric | Healthy | Suspicious | Bad |
|---|---|---|---|
| `overhead_ttft` (no cache) | 1–5 ms | 10–30 ms | > 50 ms |
| `overhead_ttft` (cache hit) | < 2 ms | 5–10 ms | > 20 ms |
| RPS (chat-short, 50 conn) | 3 000–6 000 | 1 500–3 000 | < 1 500 |
| Error rate | 0 | > 0 | > 0 |
| CPU at steady state | 40–70 % | 80–90 % | > 95 % |
| RSS growth over 60 s | < 5 MiB | 5–20 MiB | > 20 MiB |

These ranges assume a modern laptop or small VM, single-machine, mock upstream. They are not predictions for production with real LLMs.

---

## 0.10 What you'll feel as you do this

The first time you run the harness, you'll see numbers and have no idea if they are good. That is normal and expected. After three or four runs you'll start to recognise the shape of "healthy Janus" on your specific machine. From that point on, regressions will stand out the same way a slightly off-key note stands out in a familiar song.

The whole point of this folder is to build that intuition. The code is secondary.

---

*Next: [CHAPTER1_mock_llm.md](CHAPTER1_mock_llm.md). It explains the mock upstream — the foundation of every measurement.*
