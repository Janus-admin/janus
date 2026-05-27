# Chapter 4 — The Orchestrator

> `run.sh` ties everything together. It's a long shell script, but every line is doing something. This chapter walks you through it.

---

## 4.1 What `run.sh` does, top to bottom

1. **Validates arguments.** Profile must exist; duration and concurrency must be supplied.
2. **Checks dependencies.** `curl`, `jq`, `oha` must all be installed.
3. **Resolves the API key** from `$JANUS_API_KEY` or `.janus_api_key`.
4. **Pre-flight probes** — Janus health, then auth probe to make sure the key works.
5. **Locates Janus's PID** (so we can sample CPU/RSS).
6. **Creates a timestamped result directory** under `results/`.
7. **Freezes the environment** into `env.txt` (CPU model, RAM, OS, git commit, etc).
8. **Snapshots `/metrics` before the run** so we can diff counters at the end.
9. **Spawns the CPU/RSS sampler** in the background.
10. **Optional warm-up** (default 10 seconds, not recorded).
11. **The actual measurement** with `oha`.
12. **Snapshots `/metrics` after the run.**
13. **Stops the sampler.**
14. **Parses `oha`'s output** and the metrics diff.
15. **Writes `REPORT.md` and `manifest.json`** to the result directory.

Each step has a `log` line so you can follow along in real time.

---

## 4.2 Why we use `oha`

`oha` is a single-binary Rust load tool with a sane default output. We chose it over alternatives for these reasons:

- **`wrk`** — no p99 in default output, hard to read structured.
- **`ab`** (Apache Bench) — single-threaded, obsolete, doesn't do keep-alive properly.
- **`k6`** — fantastic, but requires writing JS for each profile and is a heavyweight install.
- **`vegeta`** — great for rate-controlled attacks ("exactly 1000 rps for 60s"). We may add it later.
- **`hey`** — similar to `oha`, but `oha` has a nicer TUI and slightly more readable output.

If you prefer one of the others, swap the `oha` invocation in [run.sh](run.sh) for it. The rest of the orchestrator doesn't care which tool produced the output.

---

## 4.3 The three numbers you pass

```
./run.sh chat-short 60s 50
            │        │   └── concurrency:  how many requests in flight at once
            │        └────── duration:     how long to run (oha syntax: 60s, 2m, 1h)
            └─────────────── profile:      one of the allowed names
```

### Picking a duration

- **Too short** (under 20s): tail percentiles are unstable; statistics aren't significant.
- **Too long** (over 5m): laptop thermals throttle; results stop being reproducible.
- **Just right**: 60s for routine regression runs, 5m for "we're publishing this" runs.

The harness defaults to a 10-second warm-up *outside* the duration window, so a 60s run actually takes ~70 wall-clock seconds.

### Picking a concurrency

This is the trickiest knob. The principle:

- **You want Janus saturated but not collapsing.**
- Saturated = CPU at 50–80 % steady state.
- Collapsing = errors > 0 or p99 latency exploding (10× p50).

A reasonable starting point on a laptop:
- `chat-short`, mock at 250 ms TTFT: try concurrency 50.
- `cache-warm`, no upstream wait: try concurrency 100.

If errors appear, halve it. If CPU is at 15 %, double it.

Don't blindly crank concurrency to "see what happens" — past saturation, all you measure is queue depth.

---

## 4.4 What `/metrics` snapshots are for

The Prometheus `/metrics` endpoint exposes counters like `janus_cache_hits_total` and `janus_requests_total`. These are monotonically increasing across the lifetime of the process.

To get the cache hit ratio *for this run* (not lifetime), we snapshot before and after, then subtract:

```
hits_during_run = janus_cache_hits_total_after − janus_cache_hits_total_before
reqs_during_run = janus_requests_total_after  − janus_requests_total_before
hit_ratio       = hits_during_run / reqs_during_run
```

That's the only metric in the report that comes from Janus's own internals rather than from `oha`'s observations. Everything else `oha` measures from the outside.

---

## 4.5 The CPU/RSS sampler

`sample-proc.sh` is a tiny script that runs `ps` once a second on the Janus PID and appends to a CSV. While the benchmark is running it generates ~60 lines.

Read it: [sample-proc.sh](sample-proc.sh). 25 lines. It exits when the target PID disappears, or when `run.sh` kills it at the end.

Why no fancy tool? Because `pidstat` isn't on macOS, `top` is interactive, and we want a portable 1 Hz sample. `ps` is in POSIX, so it's everywhere.

The report extracts two summary numbers from the CSV:
- **Median CPU%** during the run — a stable measure of load.
- **Peak RSS** during the run — a watch for runaway memory.

The raw CSV is preserved so you can plot it if you suspect a CPU spike pattern.

---

## 4.6 The warm-up window

```bash
oha -z "${WARMUP_SECS}s" ... >/dev/null 2>&1
```

For 10 seconds before the measurement window we hit Janus with the same load but discard the output. Why:

- Connection pools (DB, HTTP) need a few requests to fill.
- The OS scheduler needs a few seconds to learn the thread pattern.
- Caches that are *supposed* to hit need a chance to be populated.
- JIT-equivalent paths (PGO hints, branch predictors) settle.

Without warm-up, the first 5–10 seconds of measured numbers are biased low (cold caches) or high (cold connections). Throwing them away makes the headline number reflect steady-state.

To disable, set `WARMUP_SECS=0`. Don't.

---

## 4.7 The output folder

After a successful run you'll have a folder like:

```
benchmarks/results/2026-05-27T13-42-00Z-chat-short/
├── REPORT.md            ← read this first
├── oha.txt              ← raw load-tool output, has the full latency histogram
├── metrics-before.txt   ← Prometheus snapshot before the run
├── metrics-after.txt    ← Prometheus snapshot after the run
├── proc.csv             ← CPU and RSS samples (1 Hz)
├── profile.json         ← exact body that was sent
├── env.txt              ← machine + git + tools description
└── manifest.json        ← same numbers as REPORT.md, but machine-parsable
```

`manifest.json` exists so a future script can compare runs:

```bash
jq -r '[.timestamp, .throughput_rps, .latency.p99] | @tsv' \
   benchmarks/results/*/manifest.json | sort
```

That command prints a regression table out of every run you've ever done.

---

## 4.8 Recommended cadence

| When | What to run |
|---|---|
| Sanity check after editing the request path | `chat-short 30s 50` (~50 s) |
| Before committing a perf-sensitive PR | All four profiles, 60s each |
| Releasing a new version | All four profiles, 5m each, three runs of each |
| Comparing two PostgreSQL versions | All four profiles, run on each |

For "I'm publishing this", you want three independent runs per profile and want to report the median across them. That part is currently manual — there's no `repeat=3` flag yet — you just run the script three times and aggregate. A future iteration may add a `--repeats N` flag.

---

## 4.9 Things that ruin a run

If any of these are true when you're running, the numbers are unreliable:

- **Other heavy CPU consumers running** (compilation, browser, IDE indexing, Spotify sync).
- **Battery mode** (most laptops downclock the CPU when unplugged).
- **VPN active** (some VPNs intercept loopback traffic).
- **Antivirus** scanning new files (touches every result you write).
- **Outdated build** (`cargo run` in debug mode is 5–20× slower than `cargo run --release`).

A clean run is: laptop plugged in, IDE closed, only `cargo run --release -- serve` and the mock and the run script.

---

## 4.10 Exit codes

The script exits with a non-zero status when the run is invalid:

| Code | Meaning |
|---|---|
| 0 | Run completed; numbers in `REPORT.md` are usable |
| 1 | Bad arguments (typo, missing positional) |
| 2 | Missing dependency (`oha`, `jq`, or `curl`) |
| 3 | Pre-flight failed (Janus or mock unreachable, or auth rejected) |
| 4 | Errors during the load run (any non-2xx response) |

CI integration: gate on `run.sh ... && process-result.sh`. If `run.sh` reports errors, that's a regression.

---

*Next: [CHAPTER5_reading_results.md](CHAPTER5_reading_results.md). It explains how to read the output you just produced.*
