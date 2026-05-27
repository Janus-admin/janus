# Chapter 5 — Reading the Results

> You've run `./benchmarks/run.sh chat-short 60s 50`. There's a folder of files. Now what?

---

## 5.1 The TL;DR view: `REPORT.md`

Open `benchmarks/results/<timestamp>-<profile>/REPORT.md`. The top table is the only thing you usually need:

```
| Metric              | Value         |
|---|---:|
| Throughput          | 192.31 req/s  |
| Latency p50         | 0.260 secs    |
| Latency p95         | 0.290 secs    |
| Latency p99         | 0.320 secs    |
| Total requests      | 11,540        |
| Cache hits          | 0             |
| Cache hit ratio     | 0.000         |
| Errors              | 0             |
| CPU% (median)       | 47.2          |
| RSS peak (kB)       | 184,512       |
```

Read the eight rules below to know whether to celebrate this number or panic.

---

## 5.2 The eight things to check, every time

### 5.2.1 Errors must be zero

```
| Errors | 0 |
```

If this is not zero, **stop**. The run is invalid. Reasons it might be non-zero:

- Janus's rate limiter is throttling you (the seed creates a key with no limits — re-check it wasn't tampered with).
- The mock crashed mid-run.
- You hit the OS's file-descriptor limit. (`ulimit -n 8192` on macOS.)
- Concurrency is too high for this machine.

A 99 %-success run with a few errors is **not** "basically a success". Tail latency and throughput are contaminated by the slow failing requests. Re-run.

### 5.2.2 p99 should be within ~30 % of p50 (for chat-short)

For `chat-short`, expect p99 ≈ 1.1× to 1.3× p50. Why? Because the mock is constant-latency, so any spread you see comes from Janus's own jitter or the OS scheduler. A p99 that's 3× p50 means *something is occasionally stalling* — possibly:

- Garbage from a previous run (a leftover mock, an open DB pool that's reconnecting).
- A noisy neighbour on the machine.
- Janus's audit logger flushing to disk synchronously.

For `chat-long` and `cache-warm`, larger spreads are expected (more variance per request).

### 5.2.3 Throughput should make sense relative to concurrency

Heuristic: `RPS ≈ concurrency / latency_seconds`. For `chat-short` with 50 concurrent and 250 ms TTFT + 50 tokens × 20 ms = 1 250 ms latency:

```
expected_RPS ≈ 50 / 1.25 = 40 req/s
```

If your measured RPS is much higher than this, you're probably not getting the latency you think (cached, dropped, etc.). Much lower means Janus is the bottleneck.

For `cache-warm` (no upstream call), expected RPS is bounded only by Janus's cache-path latency, so it can be in the thousands.

### 5.2.4 Cache hit ratio matches the profile

- `chat-short`, `chat-long`, `tools`: should be **0.00** for the first run (cold cache). May creep up on subsequent runs if the same content was sent before — but those profiles have distinct messages, so hits should stay rare.
- `cache-warm`: should be **> 0.99**. The first request misses; everything after should hit. If `cache-warm` is reporting < 0.95, exact-match caching is broken — file a bug.

### 5.2.5 CPU% should be in the saturation band

For a healthy benchmark on a modern multi-core laptop:

| Profile | Expected steady CPU% (median) |
|---|---|
| `chat-short` | 30–70 % |
| `chat-long`  | 25–60 % |
| `cache-warm` | 60–90 % (no I/O wait, so more CPU-bound) |
| `tools`      | 30–70 % |

- **< 20 %**: Janus is idle most of the time, meaning concurrency is too low. Crank it up.
- **> 95 %**: Janus is saturated, queue depth is exploding, tail latency is suspect. Crank concurrency down.

CPU% is normalised per single core — on an 8-core machine, 800 % means all cores at 100 %.

### 5.2.6 RSS shouldn't grow appreciably during the run

Compare RSS at start of `proc.csv` to RSS at end. Growth budget:

| Growth over 60 s | Verdict |
|---|---|
| < 5 MiB | Normal — internal buffers settling |
| 5–20 MiB | Worth investigating if it reproduces |
| > 20 MiB | Likely a leak; run `cargo run --release -- serve` with `RUST_LOG=debug` and look for cache misses with growth |

This is a coarse signal. Janus's caches grow with traffic by design, so some growth is expected — especially during `cache-warm` where you're filling the exact-cache.

### 5.2.7 Compare to the previous run on the same machine

If you have a `manifest.json` from yesterday, run:

```bash
jq -r '[.timestamp, .profile, .throughput_rps, .latency.p99] | @tsv' \
   benchmarks/results/*/manifest.json | sort
```

You should see a stable trend, not a sawtooth. A 10 % drop in one run is noise. A 30 % drop is a regression — investigate.

### 5.2.8 `git status` is clean

Look at the `GIT_DIRTY=` line in `env.txt`. If it says `yes`, the benchmark ran with uncommitted changes. That's fine for exploration but **never publish a number from a dirty tree**. You can't reproduce a build that doesn't have a commit hash.

---

## 5.3 What "Janus overhead" actually is

The mock's configured TTFT is your floor. Janus's contribution is everything above:

```
overhead_ttft = measured_TTFT − mock.ttft_ms
```

For `chat-short` with the default mock (`--ttft-ms 250`), if your `p50` is `0.260 secs` (260 ms), then Janus added **10 ms** of overhead on the median. That's the headline number for "how fast is Janus".

For streaming profiles, also subtract the per-token cost:

```
overhead_e2e = measured_E2E − mock.ttft_ms − (output_tokens × mock.tpot_ms)
```

A healthy Janus on commodity hardware adds **single-digit milliseconds** of `overhead_ttft`. Tens of milliseconds suggests something is wrong. Hundreds means a serious regression.

For a cache hit, there's no upstream call at all, so:

```
overhead_ttft (cache hit) ≈ measured_TTFT
```

That number is the raw cache-path latency. Should be < 2 ms on a warm cache.

---

## 5.4 Reading `oha.txt` (the raw output)

`REPORT.md` summarises five numbers from oha. The full `oha.txt` has more:

```
Response time histogram:
  0.250 [1   ] |
  0.255 [89  ] |■■■
  0.260 [902 ] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.265 [3214] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.270 [1102] |■■■■■■■■■■■■■■■■■■■■■■■
  0.275 [88  ] |■■■
  ...
```

This bar chart tells you whether your latency is a tight distribution (good — tall, narrow) or a long-tailed distribution (suspicious — wide, with isolated bars far to the right). The latter is the visual fingerprint of a noisy run.

The "Latency distribution" section has p10/p25/p50/p75/p90/p95/p99 — `REPORT.md` only pulls p50/p95/p99 but the others are sometimes useful (e.g. p10 tells you how fast the *fastest* 10 % of requests were).

The "Error distribution" section will be empty on a healthy run. If it lists status codes, your auth probe in step 2 of `run.sh` should have caught the configuration issue — but it can also indicate runtime errors (502 from Janus, 429 from rate limiting).

---

## 5.5 Reading `proc.csv`

Open it in your favourite plot tool, or just `cat`. You'll see something like:

```
timestamp_iso,cpu_pct,rss_kb
2026-05-27T13:42:01Z,0.0,184320
2026-05-27T13:42:02Z,210.4,184528  # warm-up kicks in here
2026-05-27T13:42:03Z,287.1,185040
2026-05-27T13:42:04Z,294.8,185280
...
2026-05-27T13:43:02Z,302.5,186112  # steady state
2026-05-27T13:43:03Z,1.3,186112    # measurement done, idle
```

Look for two things:

1. **A flat CPU% line during the measurement window** — means steady state was reached.
2. **A monotonic RSS line** — means memory is not leaking. Some growth is expected (caches fill), but it should stabilise.

If CPU% looks ragged (200% one second, 50% the next), something on your machine is contending. Re-run quieter.

---

## 5.6 Reading `metrics-before.txt` vs `metrics-after.txt`

These are Prometheus text exports. They look like:

```
# HELP janus_requests_total Total /v1/chat/completions requests
# TYPE janus_requests_total counter
janus_requests_total 0
# HELP janus_cache_hits_total Cache hits across all layers
# TYPE janus_cache_hits_total counter
janus_cache_hits_total 0
```

A `diff` of the two files lets you spot any metric whose value changed during the run. Example useful insights:

- `janus_cache_hits_total` jumped by 11 539 out of 11 540 requests → cache hit ratio 0.999 (excellent for `cache-warm`).
- `janus_provider_errors_total` non-zero → the mock returned an error; the run is suspect.
- `janus_db_pool_active` stayed at e.g. 8 → DB pool wasn't saturated, no contention there.

You won't read these files most of the time — `REPORT.md` extracts the headline cache numbers. But they're there if you suspect something the headline doesn't show.

---

## 5.7 Two end-to-end examples

### Example A — healthy run

```
| Metric              | Value         |
|---|---:|
| Throughput          | 195.4 req/s   |
| Latency p50         | 256 ms        |
| Latency p95         | 271 ms        |
| Latency p99         | 289 ms        |
| Errors              | 0             |
| Cache hit ratio     | 0.000         |
| CPU% (median)       | 52.1          |
```

Reading this:

- ✓ Throughput close to theoretical (50 conn / 0.25 s ≈ 200 RPS).
- ✓ p99 only 13 % over p50 — tight distribution.
- ✓ Overhead vs mock 250 ms TTFT: ~6 ms on median. Healthy.
- ✓ No errors.
- ✓ CPU mid-band — room to grow if needed.

**Verdict: publishable.**

### Example B — suspicious run

```
| Metric              | Value         |
|---|---:|
| Throughput          | 142.7 req/s   |
| Latency p50         | 277 ms        |
| Latency p95         | 412 ms        |
| Latency p99         | 1.124 secs    |
| Errors              | 3             |
| Cache hit ratio     | 0.000         |
| CPU% (median)       | 98.3          |
```

Reading this:

- ✗ Errors > 0 → run is invalid full stop.
- ✗ p99 is 4× p50 → long-tail. Something is stalling.
- ✗ CPU at 98 % → machine is saturated; queueing dominates.
- ✗ Throughput well under theoretical → bottleneck.

**Verdict: throw it out.** Reduce concurrency, kill background processes, re-run. If it reproduces, something in Janus changed — bisect.

---

## 5.8 When to publish, when not to

Publish a run if and only if:

- 3 independent runs of the same profile produce similar numbers (CV < 0.10).
- All 3 have zero errors.
- All 3 were on a clean commit (`GIT_DIRTY=no`).
- All 3 used the same machine, OS, and Rust version.
- All 3 archived under `benchmarks/history/<branch>/<timestamp>/` (so they're permanent).

Anything less is exploratory. Use it to inform yourself; don't put it on Hacker News.

---

## 5.9 Building intuition: keep a log

The first month of benchmarking, you won't have a feel for what's normal. Keep a `benchmarks/history/NOTES.md` that's just:

```
2026-05-27  chat-short p99 = 289 ms RPS 195   no changes; baseline
2026-05-29  chat-short p99 = 295 ms RPS 192   after PR #422 (audit log rewrite); within noise
2026-06-02  chat-short p99 = 410 ms RPS 178   after PR #428 (new middleware) — regression, investigate
```

Three months later you'll be able to glance at a new number and know whether to worry. That's the whole goal of this folder.

---

*That's it. You now know how to build, run, and read a Janus benchmark. The only remaining step is **doing it three times**, so the patterns become muscle memory.*

*If you ever want to extend the harness — new profile, new metric, new load tool — read CHAPTER1 through CHAPTER4 first to make sure your addition follows the same shape as everything else.*
