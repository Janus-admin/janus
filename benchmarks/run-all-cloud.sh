#!/usr/bin/env bash
# run-all-cloud.sh — runs the full benchmark suite against a freshly-built Janus.
#
# Assumes ec2-setup.sh has already:
#   • installed Rust + Node.js + Docker + jq + oha
#   • built target/release/janus
#   • built benchmarks/mock-llm/target/release/mock-llm
#   • started the `db` Docker container
#   • generated JWT_SECRET / ENCRYPTION_KEY / DATABASE_URL in .env
#
# Safe to re-run: kills stale janus/mock processes on entry, idempotent
# seed, restores router/cache state on exit.

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
cd "$REPO_DIR"
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true

log()  { printf '\n[bench] %s\n' "$*"; }
fail() { printf '\n[bench] ERROR: %s\n' "$*" >&2; exit 1; }

# sudo wrapper: empty when running as root, otherwise prefixes `sudo`.
if [ "$(id -u)" -eq 0 ]; then SUDO=""; else SUDO="sudo"; fi

JANUS_PID=""
MOCK_PID=""

# ── Cleanup on exit (success OR failure) ──────────────────────────────────────
cleanup() {
    local exit_code=$?
    if [ -n "$JANUS_PID" ]; then kill "$JANUS_PID" 2>/dev/null || true; fi
    if [ -n "$MOCK_PID" ];  then kill "$MOCK_PID"  2>/dev/null || true; fi
    # Belt-and-braces: anything by name we still own.
    pkill -f "target/release/janus serve" 2>/dev/null || true
    pkill -f "target/release/mock-llm"    2>/dev/null || true
    if [ $exit_code -ne 0 ]; then
        log "FAILED (exit $exit_code). Last 30 lines of /tmp/janus.log:"
        tail -30 /tmp/janus.log 2>/dev/null || true
    fi
}
trap cleanup EXIT

# ── Pre-flight: artefacts must exist ──────────────────────────────────────────
[ -x ./target/release/janus ]                                || fail "release binary missing — run 'SQLX_OFFLINE=true cargo build --release' first"
[ -x ./benchmarks/mock-llm/target/release/mock-llm ]         || fail "mock-llm not built — run 'cd benchmarks/mock-llm && SQLX_OFFLINE=true cargo build --release'"
[ -f .env ]                                                  || fail ".env missing — run ec2-setup.sh"
command -v jq  >/dev/null 2>&1                               || fail "jq missing — apt-get install -y jq"
command -v oha >/dev/null 2>&1                               || fail "oha missing — cargo install oha"

# ── Normalise .env so dotenvy reads the right OPENAI_API_KEY ──────────────────
# Previous run-all-cloud invocations appended `OPENAI_API_KEY=...` without
# checking for an existing empty `OPENAI_API_KEY=` line. dotenvy reads the
# FIRST occurrence, so the empty line wins and the OpenAI provider doesn't
# register at startup. Strip empty placeholders before deciding whether to
# append a dummy key.
sed -i.bak '/^OPENAI_API_KEY=$/d' .env && rm -f .env.bak
if ! grep -q "^OPENAI_API_KEY=." .env 2>/dev/null; then
    echo "OPENAI_API_KEY=sk-bench-mock-not-a-real-key" >> .env
    log "Added dummy OPENAI_API_KEY to .env (mock backend doesn't validate keys)."
fi

# ── Health-gate helpers ───────────────────────────────────────────────────────
wait_for_janus() {
    log "Waiting for Janus on :8080 ..."
    for _ in $(seq 1 45); do
        if curl -sf http://localhost:8080/health >/dev/null 2>&1; then
            # Health 200 alone isn't enough — providers are initialised async.
            # Spin until at least one provider shows up in the response.
            if curl -s http://localhost:8080/health | jq -e '.providers | length > 0' >/dev/null 2>&1; then
                return 0
            fi
        fi
        sleep 1
    done
    fail "Janus did not become healthy within 45s. See /tmp/janus.log"
}

wait_for_mock() {
    log "Waiting for mock-llm on :9999 ..."
    for _ in $(seq 1 15); do
        curl -sf http://localhost:9999/healthz >/dev/null 2>&1 && return 0
        sleep 1
    done
    fail "mock-llm did not become healthy within 15s. See /tmp/mock-llm.log"
}

start_mock() {
    pkill -f "target/release/mock-llm" 2>/dev/null || true
    ./benchmarks/mock-llm/target/release/mock-llm "$@" > /tmp/mock-llm.log 2>&1 &
    MOCK_PID=$!
    wait_for_mock
}

start_janus() {
    pkill -f "target/release/janus serve" 2>/dev/null || true
    sleep 1
    ./target/release/janus serve > /tmp/janus.log 2>&1 &
    JANUS_PID=$!
    wait_for_janus
}

login_jwt() {
    curl -s -X POST http://localhost:8080/api/v1/auth/login \
        -H 'Content-Type: application/json' \
        -d '{"email":"bench@janus.local","password":"bench-only-do-not-use-elsewhere"}' \
      | jq -r '.token // empty'
}

# ── Boot mock + Janus ─────────────────────────────────────────────────────────
log "Starting mock-llm (default profile: 250ms TTFT, 20ms TPOT, 50 tokens)..."
start_mock --port 9999

log "Starting Janus (initial boot)..."
start_janus

# ── Seed providers + benchmark API key ────────────────────────────────────────
log "Seeding (creates bench user, points 'openai' provider at the mock, creates API key)..."
./benchmarks/seed.sh

# ── Disable every non-OpenAI provider directly in the DB ──────────────────────
# Bedrock without AWS creds is already skipped at startup by main.rs, but Gemini
# / Anthropic / Groq / DeepSeek still get loaded if env vars are present and
# they'd add latency to the failover loop. Force them off so the gateway only
# tries the local mock.
log "Disabling non-OpenAI providers in DB..."
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE providers SET is_enabled = false WHERE id != 'openai';" > /dev/null

log "Restarting Janus to pick up provider base_url + disabled flags..."
start_janus

# ── Verify the gateway actually serves a 200 before launching oha ─────────────
API_KEY=$(cat benchmarks/.janus_api_key)
log "Smoke-testing /v1/chat/completions ..."
SMOKE=$(curl -s -o /tmp/smoke.out -w '%{http_code}' \
    -X POST http://localhost:8080/v1/chat/completions \
    -H "Authorization: Bearer $API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{"model":"gpt-4o-mini","stream":true,"messages":[{"role":"user","content":"hi"}],"max_tokens":10}')
if [ "$SMOKE" != "200" ]; then
    log "Smoke test failed (HTTP $SMOKE). Response body:"
    cat /tmp/smoke.out >&2
    fail "gateway not responding 200 — aborting before benchmarks"
fi
log "Smoke test passed."

# ── Standard profiles ─────────────────────────────────────────────────────────
log "Running benchmarks..."
./benchmarks/run.sh chat-short 60s 50
./benchmarks/run.sh chat-long  60s 50
./benchmarks/run.sh tools      60s 50
./benchmarks/run.sh cache-warm 60s 50

# ── Pure-overhead profile: mock at 1ms TTFB, cache disabled ───────────────────
log "Running pure-overhead (mock at 1ms TTFT, cache disabled)..."
start_mock --port 9999 --ttft-ms 1 --tpot-ms 0 --output-tokens 1

JWT=$(login_jwt)
[ -n "$JWT" ] || fail "could not log in as bench user"
curl -sf -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":false}' > /dev/null || fail "cache disable PATCH failed"

./benchmarks/run.sh pure-overhead 60s 50

# Re-enable cache for subsequent runs.
JWT=$(login_jwt)
curl -sf -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":true}' > /dev/null || fail "cache re-enable PATCH failed"

# ── Smart-routing profile: model="" forces V5-L6 router to pick a target ──────
# smart_routing_config has a singleton NULL-workspace row used as global default.
# We flip enabled=true with default_model=gpt-4o-mini so the router picks the
# only model the mock recognises. The router lookup happens in-memory; the
# probe measures decision overhead, not DB latency.
log "Enabling smart routing globally (default_model=gpt-4o-mini)..."
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE smart_routing_config SET enabled = true, default_model = 'gpt-4o-mini'
     WHERE workspace_id IS NULL;" > /dev/null

# Restart mock back to realistic-latency profile for this run.
start_mock --port 9999

./benchmarks/run.sh smart-routing 60s 50

# Restore router to disabled so subsequent runs don't drift behaviour.
$SUDO docker compose exec -T db psql -U janus -d janus -c \
    "UPDATE smart_routing_config SET enabled = false WHERE workspace_id IS NULL;" > /dev/null

# ── Mixed workload: 60% cache-hit / 40% cache-miss in parallel ────────────────
log "Running mixed workload (60/40 cache hit/miss, 50 total conn)..."
bash benchmarks/mixed-workload.sh 60s

# ── Summary ───────────────────────────────────────────────────────────────────
log "All done. Per-profile manifest summary (timestamp / profile / rps / p99 / errors):"
jq -r '[.timestamp, .profile, .throughput_rps, .latency.p99, .errors] | @tsv' \
    benchmarks/results/*/manifest.json 2>/dev/null | sort
