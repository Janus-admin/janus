# Benchmarking Janus

> A reproducible benchmark harness for the Janus AI gateway.
>
> **This document describes how to measure Janus's performance — not how Janus compares to other gateways.** Comparative claims require a separate, peer-reviewed exercise.

---

## Philosophy

Self-published benchmarks are inherently suspect. The only way to make them credible is to be more transparent about *how* the numbers were produced than about the numbers themselves. Three rules govern everything in this document:

1. **Methodology is public.** Every harness, seed script, mock provider, and load profile lives in this repo. If you cannot reproduce a number on your own hardware, the number is meaningless.
2. **No comparative claims without rival sign-off.** We do not say "Janus is faster than X." We say "on hardware H, with workload W, Janus measured Y." If you want to compare two gateways, run both with the same harness and publish the harness alongside the result.
3. **Raw data over summaries.** Median, p95, p99 — never mean. Raw CSV/JSON committed alongside the chart.

If a benchmark you publish about Janus does not follow these rules, it is not a Janus benchmark.

---

## What we measure

| Metric | Definition | Source |
|---|---|---|
| **TTFT** — Time To First Token | Wall-clock ms from request send to first SSE chunk received | `requests.ttfb_ms` column |
| **E2E latency** | Wall-clock ms from request send to last byte of response | load tool (oha / k6 / vegeta) |
| **TPOT** — Time Per Output Token | (E2E − TTFT) / output_tokens | derived |
| **Throughput** | Successful requests per second sustained over the measurement window | load tool |
| **Cache hit ratio** | `cache_hits / total_requests` for the run | `/metrics` (Prometheus) |
| **Cold-start latency** | First-request latency after a fresh boot, separately reported | dedicated probe |
| **Error rate** | Non-2xx responses / total responses | load tool |
| **CPU & RSS** | Steady-state CPU% and resident memory of the `janus` process | `/proc` or `ps` sampler |

We do **not** report mean latency. Means hide the tail; tail latency is what users feel.

---

## Workload profiles

A benchmark without a defined workload is just a vibe. We standardise four profiles, each described as a single TOML file in `benchmarks/profiles/`:

| Profile | Prompt size | Stream | Cache-friendly | Purpose |
|---|---|---|---|---|
| `chat-short` | ~200 tokens in / ~150 out | yes | low (random prompts) | typical chatbot |
| `chat-long` | ~2 000 tokens in / ~500 out | yes | low | RAG, long context |
| `cache-warm` | ~200 tokens in / ~150 out | no | high (10 hot prompts repeated) | cache hit-rate ceiling |
| `tools` | ~400 tokens in, function-calling, 2-turn | no | low | tool-use overhead |

Anyone publishing a Janus number must state which profile they ran. "Janus does 5 000 RPS" without a profile is unfalsifiable.

---

## Hardware disclosure

Every published number must include:

- CPU model + core count + clock
- RAM (size + type)
- OS + kernel version
- Storage type (NVMe / SSD / SAN)
- Concurrent connections used by the load tool
- Janus version: `git rev-parse HEAD` (commit hash, not a tag)
- `rustc --version`
- PostgreSQL version
- Whether semantic cache backend is `linear` or `qdrant`
- Whether `models/all-MiniLM-L6-v2.onnx` was loaded
- Whether `janus.toml` deviated from defaults (and how)

A benchmark that omits any of these is unverifiable and we will not link to it.

---

## Mock upstream

**Do not benchmark against real OpenAI or Anthropic.** Network jitter, model load on the upstream, and rate limits will dominate the signal. Janus's job is to add minimal overhead on top of the upstream — that overhead is what we measure.

The harness ships a `mock-llm` binary (built from `benchmarks/mock-llm/`) that:

- speaks the OpenAI `/v1/chat/completions` schema (streaming + non-streaming)
- emits a configurable number of output tokens
- pauses a configurable inter-token delay (default 20 ms, matching a real frontier model)
- pauses a configurable TTFT delay (default 250 ms)
- never sleeps; uses `tokio::time::sleep` so the runtime stays awake

Janus is configured to talk to this mock as a `provider`. The mock's latency is *constant*, so any variance in measured Janus latency comes from Janus, not the upstream. This is the single most important property of the harness.

---

## Tooling

We use three load tools for three jobs:

| Tool | Used for | Why |
|---|---|---|
| `criterion` | Micro-benchmarks (hashing, cosine similarity, PII scrubbing) | Already present in `benches/`. Statistical rigor, regression detection. |
| `oha` | HTTP load, RPS + tail latency | Single static binary, p99 reporting, simple CLI. Default choice. |
| `k6` | Scenario-based load (multi-step user flows, conditional logic) | JS scripting when a flat RPS profile is not enough. |

We do **not** use `wrk` (no p99 in default output) or `ab` (single-threaded, obsolete).

---

## Running a benchmark

```bash
# 1. Build Janus in release mode (debug builds are not benchmarks)
cargo build --release

# 2. Start a clean PostgreSQL
docker compose up -d postgres
sqlx migrate run

# 3. Start the mock upstream
cargo run --release --bin mock-llm -- --port 9999 --ttft-ms 250 --tpot-ms 20 &

# 4. Start Janus pointed at the mock
JANUS_PROVIDER_OPENAI_BASE_URL=http://localhost:9999 \
  cargo run --release -- serve &

# 5. Seed an API key + provider config
./benchmarks/seed.sh > /tmp/key.txt

# 6. Run the chosen profile
./benchmarks/run.sh chat-short 60s 50  # profile, duration, concurrency

# 7. Collect raw data
#    - load tool output: benchmarks/results/<timestamp>/oha.txt
#    - Prometheus snapshot: benchmarks/results/<timestamp>/metrics.txt
#    - process samples: benchmarks/results/<timestamp>/proc.csv
```

Every artefact ends up under `benchmarks/results/<ISO-timestamp>/`. Nothing is overwritten. Old runs stay in git history.

---

## Reporting

A Janus benchmark report has the following shape, no exceptions:

```markdown
## Run <ISO-timestamp> — <profile>

**Hardware:** Apple M2 Pro, 32 GB, macOS 14.5, NVMe internal
**Janus:** commit abc1234, rustc 1.83.0, PostgreSQL 16.4
**Config:** defaults except `semantic_cache_threshold=0.92`
**Load:** oha, 60s, 50 concurrent, profile `chat-short`

| Metric | Value |
|---|---|
| Throughput | 4 217 req/s |
| TTFT p50 | 252 ms |
| TTFT p95 | 271 ms |
| TTFT p99 | 304 ms |
| E2E p50 | 3 218 ms |
| E2E p99 | 3 380 ms |
| Cache hit ratio | 0.00 |
| Error rate | 0 / 252 920 |
| Janus CPU (steady) | 47 % |
| Janus RSS (steady) | 184 MiB |

Raw data: `benchmarks/results/2026-05-27T14-22-00Z/`
```

Note: TTFT and E2E in a mock-upstream run are dominated by the mock's configured delays. The interesting question is **Janus's added overhead** — `(measured_TTFT − mock_TTFT)`. Always report both the mock's configured delay and the measured value.

---

## What "Janus overhead" actually means

This is the headline number. Define it precisely:

```
overhead_ttft = measured_TTFT - mock.ttft_ms
overhead_e2e  = measured_E2E  - mock.ttft_ms - (output_tokens × mock.tpot_ms)
```

A non-cache request through Janus should add a few milliseconds, not hundreds. If `overhead_ttft` is more than ~10 ms on commodity hardware with a local mock, something is wrong — investigate before publishing.

A cache hit should report `overhead_ttft ≈ measured_TTFT` (no upstream call), and that number is the real cache-path latency.

---

## Statistical rigor

- Minimum **3 runs** per data point. Report median across runs, with min/max as error bars.
- **Warm-up window**: first 10 seconds of each run discarded. The harness does this automatically.
- **Coefficient of variation** (`stddev / mean`) must be reported. CV > 0.10 means the run was noisy — re-run on a quieter machine.
- **One process per host** during measurement. No browser, no IDE, no Slack. Use a dedicated VM if necessary.

---

## Cold-start

Cold-start latency is reported separately from steady-state, because it answers a different question (provisioning) than steady-state (serving).

```bash
./benchmarks/cold-start.sh
# Boots Janus, sends one request, records TTFT, kills the process. Repeats 20 times.
```

Reported as: `cold_start p50`, `cold_start p95`. Do **not** average cold-start into steady-state numbers — it makes both meaningless.

---

## Comparing Janus to other gateways

Don't, unless you are doing it with their team.

If you must:

1. Use the **exact same** mock upstream, profile, hardware, and load tool for both.
2. Open an issue on the rival project linking to your harness *before* publishing.
3. Wait at least 14 days for their methodology objections. Fix the harness if the objections are valid.
4. Publish the response and your reply alongside the numbers.

This is what TechEmpower does. It is the only model that survives adversarial scrutiny. Anything else is marketing.

---

## CI

The micro-benchmarks in `benches/` run on a fixed GitHub Actions runner via `cargo criterion` on every commit to `master`. Results are committed back into `benchmarks/history/criterion/` so regressions are visible in git history. Full HTTP load tests are too noisy on GitHub runners and are run manually on a dedicated machine.

---

## Sharing results

When publishing a Janus benchmark publicly (blog, HN, Twitter):

- Link to the commit hash, not "main"
- Link to `benchmarks/results/<timestamp>/` raw artefacts
- Link to this document
- State the profile name explicitly in the headline
- Do not crop axes on charts. Y-axis starts at zero.

If you publish a number that violates any of these rules, expect to be ignored or downvoted. That is correct. The rules exist because the community has been burned too many times.

---

## Out of scope (for now)

These are deliberately not in the v1 harness — they are easy to fake and add complexity without adding signal:

- Token-counting accuracy (covered by unit tests, not benchmarks)
- Cost-calculation accuracy (covered by unit tests)
- Real-upstream end-to-end runs (too noisy, varies by time of day)
- Multi-region latency (depends on the deployer, not Janus)
- "Apples-to-apples vs Cloudflare AI Gateway" (closed-source rival, cannot be reproduced)

If you need these, run them yourself and publish your own harness with the same standards.

---

*Reproducibility over impressiveness.*
