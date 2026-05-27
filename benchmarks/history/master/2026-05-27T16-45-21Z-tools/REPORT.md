# Benchmark run — 2026-05-27T16-45-21Z

**Profile:** `tools`
**Duration:** 60s   **Concurrency:** 50   **Warm-up:** 10s

## Headline numbers

| Metric | Value |
|---|---:|
| Throughput | 19235.3993 req/s |
| Success rate (oha) | 100.00% |
| Latency p50 | 2.3743 ms |
| Latency p95 | 4.8220 ms |
| Latency p99 | 6.1359 ms |
| Total requests (Janus) | 1355292 |
| Cache hits (total) | 1355291 |
| Cache hits (exact) | 1355284 |
| Cache hits (semantic) | 7 |
| Cache misses (to provider) | 1 |
| Cache hit ratio | 1.000 |
| Errors (oha-reported) | 0 |
| CPU% (median, steady) | 218.4 |
| RSS peak (kB) | 1460800 |

## Provenance

```
# Frozen environment for run 2026-05-27T16-45-21Z
PROFILE=tools
DURATION=60s
CONCURRENCY=50
WARMUP_SECS=10
JANUS_BASE=http://127.0.0.1:8080
GIT_COMMIT=fb05f4471a2d75cbc19845095fd584cce4f839eb
GIT_BRANCH=master
GIT_DIRTY=no
UNAME=Darwin wallexs-MacBook-Air-70.local 25.5.0 Darwin Kernel Version 25.5.0: Mon Apr 27 20:38:00 PDT 2026; root:xnu-12377.121.6~2/RELEASE_ARM64_T8103 arm64
RUSTC=rustc 1.95.0 (59807616e 2026-04-14)
OHA=oha 1.14.0
CPU_MODEL=Apple M1
CPU_CORES=8
MEM_BYTES=8589934592
```

## How to read this

- **Latency** is end-to-end wall-clock. Subtract the mock's configured TTFT
  to get Janus's overhead. For streaming profiles, also subtract
  `output_tokens × tpot_ms`.
- **Cache hit ratio** counts only after the warm-up window.
- **Errors must be zero.** If non-zero, the run is invalid — investigate
  before publishing.

## Artefacts

| File | What it is |
|---|---|
| `oha.txt` | Raw load-tool output (full latency histogram inside) |
| `metrics-before.txt` / `metrics-after.txt` | Prometheus snapshots framing the run |
| `proc.csv` | CPU% and RSS, sampled at 1 Hz |
| `profile.json` | The exact request body that was sent |
| `env.txt` | Frozen environment description |
