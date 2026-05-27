#!/usr/bin/env bash
# run-all-cloud.sh — runs the full benchmark suite against a freshly-built Janus.
# Designed for: a cloud VM that already finished `bash benchmarks/ec2-setup.sh`.

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
cd "$REPO_DIR"
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true

log() { printf '\n[bench] %s\n' "$*"; }
have_sudo() { [ "$(id -u)" -eq 0 ] && SUDO="" || SUDO="sudo"; }
have_sudo

# ── Ensure a dummy OPENAI_API_KEY so the OpenAI provider is registered ────────
# (otherwise the registry only carries Bedrock and every request 503s).
if ! grep -q "^OPENAI_API_KEY=." .env 2>/dev/null; then
    echo "OPENAI_API_KEY=sk-bench-mock-not-a-real-key" >> .env
    log "Added dummy OPENAI_API_KEY to .env so OpenAI provider registers."
fi

# ── Start mock-llm ─────────────────────────────────────────────────────────────
log "Starting mock-llm on port 9999..."
pkill -f mock-llm 2>/dev/null || true
./benchmarks/mock-llm/target/release/mock-llm --port 9999 > /tmp/mock-llm.log 2>&1 &
MOCK_PID=$!
sleep 2

# ── Start Janus ────────────────────────────────────────────────────────────────
log "Starting Janus on port 8080..."
pkill -f "janus serve" 2>/dev/null || true
./target/release/janus serve > /tmp/janus.log 2>&1 &
JANUS_PID=$!

log "Waiting for Janus to be ready..."
for _ in $(seq 1 30); do
    curl -sf http://localhost:8080/health &>/dev/null && break || sleep 2
done

# ── Seed providers + create benchmark API key ─────────────────────────────────
log "Seeding..."
./benchmarks/seed.sh

# ── Disable every non-OpenAI provider directly in the DB ──────────────────────
# We avoid PATCHing through the admin API because every provider in the DB ends
# up loaded into the in-memory registry at startup; disabling them here ensures
# the failover loop never tries Bedrock without credentials (which would add
# 50ms+ per request and dwarf the gateway-overhead numbers we're measuring).
log "Disabling non-OpenAI providers in DB..."
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE providers SET is_enabled = false WHERE id != 'openai';" > /dev/null

log "Restarting Janus so it picks up provider base_url + disable flags..."
kill "$JANUS_PID" 2>/dev/null || true
sleep 2
./target/release/janus serve > /tmp/janus.log 2>&1 &
JANUS_PID=$!
for _ in $(seq 1 30); do
    curl -sf http://localhost:8080/health &>/dev/null && break || sleep 2
done

# ── Run all benchmark profiles ─────────────────────────────────────────────────
log "Running benchmarks..."
./benchmarks/run.sh chat-short 60s 50
./benchmarks/run.sh chat-long  60s 50
./benchmarks/run.sh tools      60s 50
./benchmarks/run.sh cache-warm 60s 50

# ── Pure-overhead profile: mock at 1ms TTFB, cache disabled ───────────────────
log "Running pure-overhead (mock at 1ms, cache disabled)..."
kill "$MOCK_PID" 2>/dev/null || true
sleep 1
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 1 --tpot-ms 0 --output-tokens 1 \
    > /tmp/mock-llm.log 2>&1 &
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

# Re-enable cache for any further runs.
JWT=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"bench@janus.local","password":"bench-only-do-not-use-elsewhere"}' \
    | jq -r '.token')
curl -s -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":true}' > /dev/null

# ── Smart-routing profile: model="" forces the router to pick a target ────────
# Flip the global smart_routing_config row to enabled with gpt-4o-mini as the
# default so empty-model requests are routed through the V5-L6 router engine.
log "Enabling smart routing with default model gpt-4o-mini..."
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE smart_routing_config SET enabled = true, default_model = 'gpt-4o-mini'
     WHERE workspace_id IS NULL;" > /dev/null

# Restart the mock back to the realistic-latency profile (was 1ms during
# pure-overhead). The smart-routing probe runs against the same workload so
# its numbers compare directly with chat-short.
kill "$MOCK_PID" 2>/dev/null || true
sleep 1
./benchmarks/mock-llm/target/release/mock-llm --port 9999 > /tmp/mock-llm.log 2>&1 &
MOCK_PID=$!
sleep 1

./benchmarks/run.sh smart-routing 60s 50

# Restore router to disabled so subsequent runs don't drift behaviour.
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE smart_routing_config SET enabled = false WHERE workspace_id IS NULL;" > /dev/null

# ── Mixed workload: 60% cache-hit / 40% cache-miss in parallel ────────────────
# Realistic traffic shape: most prompts repeat, a minority are unique.
# Runs two oha processes side-by-side (30 hit + 20 miss = 50 conn) so the
# numbers compare directly with the single-profile runs above.
log "Running mixed workload (60/40 cache hit/miss)..."
bash benchmarks/mixed-workload.sh 60s

# ── Summary ────────────────────────────────────────────────────────────────────
log "All done. Per-profile manifest summary (timestamp / profile / rps / p99 / errors):"
jq -r '[.timestamp, .profile, .throughput_rps, .latency.p99, .errors] | @tsv' \
    benchmarks/results/*/manifest.json 2>/dev/null | sort

# ── Cleanup ────────────────────────────────────────────────────────────────────
kill "$MOCK_PID" "$JANUS_PID" 2>/dev/null || true
