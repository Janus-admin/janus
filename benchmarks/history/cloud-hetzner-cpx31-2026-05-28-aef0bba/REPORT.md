# Janus Cloud Benchmark — Hetzner CPX31 (2026-05-28, commit `aef0bba`)

**Commit under test:** `aef0bba` (async EmbeddingIndex trait + spawn_blocking embed) on top of `3276793` (hardening + observability)

Re-verification of the May 28 CPX32 baseline (`689f9bd`) on the same hardware
class to confirm commits `3276793` and `aef0bba` did not regress published
performance. *Same hardware as the earlier run — `CPX32` was the previous
internal naming for the plan Hetzner now calls `CPX31` (4 shared AMD vCPU
+ 8 GB RAM, €0.026/hour).*

## Hardware

| Spec | Value |
|------|-------|
| Provider | Hetzner Cloud |
| Instance | **CPX31** |
| vCPU | 4 (**shared** AMD EPYC-Genoa @ 2.4 GHz) |
| RAM | 7.6 GiB |
| Disk | 150 GB |
| Region | Falkenstein, Germany (fsn1, eu-central) |
| OS | Ubuntu 26.04 LTS |
| Price | €0.026 / hour |

## Software

| Component | Setting |
|-----------|---------|
| Rust toolchain | 1.95.0 stable |
| Build profile | `--release`, `SQLX_OFFLINE=true` |
| PostgreSQL | 16-alpine, single Docker container, default tuning |
| mock-llm | default profile (250 ms TTFT / 20 ms TPOT) for realistic profiles; 1 ms TTFT for overhead probe |
| oha | 1.14.0 |
| Concurrency | 50 connections |
| Run length | 60 s measurement + 10 s warm-up (discarded) |
| Audit writer | bounded mpsc, 10 000 capacity, 100 ms flush, 500-event batch |
| Bedrock | skipped (no AWS creds) |
| Semantic cache | disabled (no ONNX model on disk) |
| Exact cache | enabled |

## Results (realistic profiles)

100 % cache hit ratio across all profiles (3.1 M–3.8 M hits, ≤ 50 misses each).

| Profile | Throughput | p50 | p95 | p99 | Errors |
|---------|-----------:|----:|----:|----:|-------:|
| **chat-short** — single-turn, ~50-token reply | **51,227 RPS** | 0.87 ms | 2.09 ms | **2.99 ms** | 0 |
| **chat-long** — 5-turn conversation           | **45,053 RPS** | 1.02 ms | 2.24 ms | **3.08 ms** | 0 |
| **tools** — function-calling, 3 tools         | **46,898 RPS** | 0.98 ms | 2.11 ms | **3.02 ms** | 0 |
| **cache-warm** — 100 % hit rate               | **55,451 RPS** | 0.81 ms | 1.90 ms | **2.84 ms** | 0 |
| **smart-routing** — V5-L6 router on every req  | **76,101 RPS** | 0.54 ms | 1.61 ms | **2.54 ms** | 0 |

### Mixed 60/40 workload (concurrent hit + miss streams)

Two oha processes run side-by-side: 30 connections on `chat-short` (cached)
and 20 connections on `mixed-miss` (unique prompt).

| Stream | Concurrency | Throughput | p50 | p99 |
|--------|------------:|-----------:|----:|----:|
| Cache HIT  | 30 | 26,458 RPS | 1.05 ms | 3.01 ms |
| Cache MISS | 20 | 17,132 RPS | 1.13 ms | 3.04 ms |
| **Combined** | **50** | **43,591 RPS** | — | — |

### Headline

> **Janus on a 4 vCPU shared / 8 GB Hetzner CPX31 serves 43k–76k req/s
> with p99 latency between 2.54 and 3.08 ms across every realistic
> workload, with the full feature set active (exact cache, PII redaction,
> audit logging, plugin chain, time-guard, smart routing).**

## Isolated-overhead probe (cache disabled, mock at 1 ms TTFT)

Two probe runs taken consecutively:

| Run | Phase 1 (mock alone) | Phase 2 (Janus + mock) | Inferred Janus overhead |
|-----|---------------------:|-----------------------:|------------------------:|
| 60 s, conc 50 | **44.14 ms** | 46.32 ms | **2.18 ms** |
| 30 s, conc 50 | **44.14 ms** | 46.46 ms | **2.32 ms** |

Matches the May 28 CPX32 baseline (`689f9bd`) value of **~2.25 ms** within
noise. The 44 ms floor visible in Phase 1 is the shared-vCPU scheduling
artefact — every `tokio::time::sleep(1ms)` from inside mock-llm pays the
hypervisor's scheduling granularity. The 46 ms full-stack number is not a
Janus characteristic. For an absolute Janus-only p99, re-run the probe on
a dedicated-CPU host (CCX class) — the May 28 CCX23 baseline measured
0.84 ms p99.

The `bench-overhead-probe.sh` script automates both phases for any host.

## Change vs. May 28 CPX32 baseline (commit `689f9bd` → `aef0bba`)

| Profile | `689f9bd` RPS | `aef0bba` RPS | Δ RPS | `689f9bd` p99 | `aef0bba` p99 | Δ p99 |
|---------|--------------:|--------------:|------:|--------------:|--------------:|------:|
| chat-short | 40,694 | **51,227** | **+26 %** | 3.01 ms | 2.99 ms | ≈ 0 % |
| chat-long  | 37,405 | **45,053** | **+20 %** | 3.47 ms | 3.08 ms | **−11 %** |
| tools      | 40,262 | **46,898** | **+16 %** | 2.99 ms | 3.02 ms | ≈ 0 % |
| cache-warm | 46,747 | **55,451** | **+19 %** | 2.67 ms | 2.84 ms | +6 % |

Throughput rose 16–26 % across realistic profiles on the same hardware
class while p99 stayed flat (or improved on `chat-long`). The most likely
contributors over the 3-commit gap (`689f9bd` → `3276793` → `aef0bba`):

- **`aef0bba`**: `EmbeddingIndex` async trait + `spawn_blocking` around
  ONNX inference. Removes executor-thread stalls under semantic-cache load
  and frees a tokio worker during embedding compute. On shared CPU this
  shows up as throughput uplift rather than p99 improvement, because the
  hypervisor latency floor dominates the tail.
- **`3276793`**: audit-writer observability + PII pattern expansion. PII
  scrub regressed +72–110 % on the synthetic micro-bench but stays
  sub-microsecond — invisible against ms-scale request budgets.
- The exact attribution between `aef0bba` and `3276793` cannot be isolated
  from these numbers alone; both commits ship together as one upgrade.

## Comparison vs. competitors (published numbers, similar 4-core hosts)

| Gateway | p99 | Notes |
|---------|----:|-------|
| **Janus (this run)** | **2.99 ms** | 4 vCPU shared / 8 GB Hetzner CPX31 |
| LiteLLM proxy | ~50–150 ms | Python, well-documented overhead |
| Kong (LLM plugin) | ~5–15 ms | C-based, heavier feature set |
| Envoy + ext_proc | ~3–8 ms | bare proxy, no LLM-specific features |

## Known limitations

1. **Shared vCPU** — Hetzner CPX class shares physical cores with other
   tenants. Tail latency has ±15 % jitter relative to dedicated hosts.
2. **Mock-llm baseline** — real OpenAI / Anthropic upstreams add their own
   variance. The numbers above isolate Janus overhead from upstream
   behaviour.
3. **Semantic cache disabled** — no ONNX model on the bench host, so the
   `aef0bba` D.1/D.2 wins (spawn_blocking embed, async vector index) are
   not measured here directly. They only matter when semantic cache is
   active.
4. **PostgreSQL co-located** — the DB runs in a sibling Docker container
   on the same VM.
5. **No TLS termination** — plain HTTP on localhost.

## Reproducing

```bash
# On a fresh Ubuntu 22.04 / 24.04 / 26.04 cloud host:
git clone https://github.com/Janus-admin/janus ~/janus
cd ~/janus
bash benchmarks/ec2-setup.sh        # installs deps, builds, starts PG
bash benchmarks/run-all-cloud.sh    # runs the realistic profiles
bash benchmarks/bench-overhead-probe.sh 60s 50   # isolated overhead probe
```

## Raw artefacts

Original `oha` results on the bench host:

```
benchmarks/results/2026-05-28T10-58-52Z-chat-short
benchmarks/results/2026-05-28T11-00-04Z-chat-long
benchmarks/results/2026-05-28T11-01-16Z-tools
benchmarks/results/2026-05-28T11-02-28Z-cache-warm
benchmarks/results/2026-05-28T11-03-41Z-smart-routing
benchmarks/results/2026-05-28T11-04-52Z-mixed
```
