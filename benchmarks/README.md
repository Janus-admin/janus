# `benchmarks/` — A Guided Tour

> This folder is a **teaching harness** as much as a measurement harness.
> If you have never written a benchmark before, read the chapters in order. The code in this folder will not run correctly until you understand the chapters; that is by design.

Everything here is designed so that, by the end, you can:

1. Explain what the harness does and why each piece exists.
2. Run a measurement on your laptop.
3. Read the resulting numbers and decide whether they are good, bad, or noise.
4. Defend the methodology if someone on Hacker News pushes back.

---

## How to read this folder

| Order | File | What it teaches |
|------:|---|---|
| 1 | [CHAPTER0_concepts.md](CHAPTER0_concepts.md) | What is a benchmark; why means lie; what TTFT really measures |
| 2 | [CHAPTER1_mock_llm.md](CHAPTER1_mock_llm.md) | Why we build a fake OpenAI; how `mock-llm/` works line by line |
| 3 | [CHAPTER2_profiles.md](CHAPTER2_profiles.md) | What a "workload profile" is; the four we ship |
| 4 | [CHAPTER3_seed.md](CHAPTER3_seed.md) | How `seed.sh` prepares the database |
| 5 | [CHAPTER4_running.md](CHAPTER4_running.md) | How `run.sh` orchestrates a single measurement |
| 6 | [CHAPTER5_reading_results.md](CHAPTER5_reading_results.md) | How to read a result file and what a regression looks like |

Each chapter is ~3 pages. **Don't skip CHAPTER0.** Without it the rest will feel like cargo-cult.

---

## What is in this folder

| Path | Purpose |
|---|---|
| `mock-llm/` | A fake OpenAI server written in Rust. Janus talks to this instead of real OpenAI. |
| `profiles/*.json` | Request bodies for the four standard workload profiles. |
| `seed.sh` | One-time setup: creates an admin user, points the OpenAI provider at the mock, creates an API key. |
| `run.sh` | The orchestrator. Runs one measurement and writes results to `results/<timestamp>/`. |
| `sample-proc.sh` | A small sampler that records CPU and RSS once per second while a benchmark is running. |
| `results/` | Where each run's artefacts land. Not committed to git. |

---

## Five-minute quick start

> Don't run this until you've read CHAPTER0, CHAPTER1, and CHAPTER3 at minimum. Otherwise the numbers will mean nothing.

```bash
# 0. Prerequisites (one time, on your machine)
brew install oha jq                                       # macOS
# OR: apt-get install jq && cargo install oha             # Linux

# 1. Build everything in release mode
cargo build --release
cargo build --release --manifest-path benchmarks/mock-llm/Cargo.toml

# 2. Start PostgreSQL + run migrations (separate terminal)
docker compose up -d postgres
cargo run --release -- migrate up

# 3. Start the mock upstream (terminal A)
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 250 --tpot-ms 20

# 4. Start Janus pointed at the mock (terminal B)
OPENAI_API_KEY=mock-key cargo run --release -- serve

# 5. One-time seed (terminal C)
./benchmarks/seed.sh
# Output: API key (jn-sk-...) — save it to $JANUS_API_KEY for run.sh
# Then RESTART Janus so it picks up the new provider base_url

# 6. Run a benchmark (terminal C)
export JANUS_API_KEY=jn-sk-...
./benchmarks/run.sh chat-short 60s 50
# Reads:    profile name, duration, concurrent connections
# Writes:   benchmarks/results/<timestamp>/

# 7. Inspect results
cat benchmarks/results/<timestamp>/REPORT.md
```

If any step is unclear, the relevant chapter explains it.

---

## What this harness does not measure

To save you frustration when reading the output:

- **Real-world LLM latency.** Real OpenAI is sometimes 2× faster, sometimes 5× slower. We deliberately use a mock with constant latency so noise comes from Janus, not the upstream.
- **Multi-region deployments.** Network is the same machine here. Cross-region latency is a different experiment.
- **Janus vs $other-gateway.** That would require running both with the same harness and getting sign-off from the rival project. See [BENCHMARKING.md](../BENCHMARKING.md) in repo root for the rules.
- **Cold-start performance.** Out of scope for v1 of this harness. Run `cold-start.sh` separately (not yet written; tracked in the roadmap).

---

## When to re-run

You should re-run the standard four profiles when:

- You merge a PR that touches the request path, caching, or provider adapters.
- You upgrade Rust or a major dependency (axum, sqlx, tokio).
- You change PostgreSQL major version.
- You change machine.

Commit the result folder under `benchmarks/history/<branch>/<timestamp>/` (separate from `results/`) so regressions are visible in `git log`. The `results/` folder is intentionally `.gitignore`d to keep noise out of the repo.

---

*Next: read [CHAPTER0_concepts.md](CHAPTER0_concepts.md).*
