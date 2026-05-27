#!/usr/bin/env bash
# run-all-cloud.sh — starts Janus + mock and runs all benchmark profiles.
# Run this after ec2-setup.sh completes.

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
cd "$REPO_DIR"
source "$HOME/.cargo/env" 2>/dev/null || true

log() { printf '\n[bench] %s\n' "$*"; }

# ── Start mock-llm ─────────────────────────────────────────────────────────────
log "Starting mock-llm on port 9999..."
pkill -f mock-llm 2>/dev/null || true
./benchmarks/mock-llm/target/release/mock-llm --port 9999 &
MOCK_PID=$!
sleep 2

# ── Start Janus ────────────────────────────────────────────────────────────────
log "Starting Janus on port 8080..."
pkill -f "janus serve" 2>/dev/null || true
./target/release/janus serve &
JANUS_PID=$!

log "Waiting for Janus to be ready..."
for i in $(seq 1 30); do
    curl -sf http://localhost:8080/health &>/dev/null && break || sleep 2
done

# ── Seed ───────────────────────────────────────────────────────────────────────
log "Seeding..."
./benchmarks/seed.sh
log "Restarting Janus to pick up provider base_url..."
kill $JANUS_PID
sleep 2
./target/release/janus serve &
JANUS_PID=$!
for i in $(seq 1 30); do
    curl -sf http://localhost:8080/health &>/dev/null && break || sleep 2
done

# ── Run benchmarks ─────────────────────────────────────────────────────────────
log "Running benchmarks..."
./benchmarks/run.sh chat-short 60s 50
./benchmarks/run.sh chat-long  60s 50
./benchmarks/run.sh tools      60s 50
./benchmarks/run.sh cache-warm 60s 50

# pure overhead: restart mock with 1ms latency
log "Running pure-overhead (mock at 1ms)..."
kill $MOCK_PID 2>/dev/null || true
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 1 --tpot-ms 0 --output-tokens 1 &
MOCK_PID=$!
sleep 1

JWT=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"bench@janus.local","password":"bench-only-do-not-use-elsewhere"}' \
    | jq -r '.token')
curl -s -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":false}' > /dev/null

./benchmarks/run.sh pure-overhead 60s 50

curl -s -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":true}' > /dev/null

# ── Summary ────────────────────────────────────────────────────────────────────
log "All done. Results:"
jq -r '[.timestamp, .profile, .throughput_rps, .latency.p99, .errors] | @tsv' \
    benchmarks/results/*/manifest.json 2>/dev/null | sort

# cleanup
kill $MOCK_PID $JANUS_PID 2>/dev/null || true
