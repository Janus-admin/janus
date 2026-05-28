#!/usr/bin/env bash
# bench-overhead-probe.sh — optional probe of bare gateway overhead.
#
# Why it's NOT in run-all-cloud.sh by default:
#
#   On shared-vCPU cloud instances (Hetzner CPX, AWS t-class) this number is
#   dominated by hypervisor scheduling latency, not by Janus. A May 2026 run
#   on CPX32 measured 46 ms p99 with cache disabled and mock at 1 ms TTFT;
#   isolating mock-llm alone (Janus removed from the path entirely) showed
#   the SAME workload reports 44 ms p99 — so true Janus overhead was ~2 ms,
#   the other 44 ms came from the shared CPU itself. Publishing the 46 ms
#   headline misleads readers who skim past the footnote.
#
#   Run this probe ONLY on a dedicated-CPU host (Hetzner CCX, AWS c-class
#   dedicated, bare metal). Then the number reflects what it claims to.
#
# Pre-reqs:
#   • bash benchmarks/ec2-setup.sh has completed
#   • mock-llm + janus build artefacts are present
#   • the host is dedicated-CPU (you have verified this — script does not)
#
# Usage: bash benchmarks/bench-overhead-probe.sh [duration] [concurrency]

set -euo pipefail

REPO_DIR="${REPO_DIR:-$HOME/janus}"
cd "$REPO_DIR"
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true

DURATION="${1:-60s}"
CONCURRENCY="${2:-50}"

log()  { printf '\n[probe] %s\n' "$*"; }
fail() { printf '\n[probe] ERROR: %s\n' "$*" >&2; exit 1; }

if [ "$(id -u)" -eq 0 ]; then SUDO=""; else SUDO="sudo"; fi

JANUS_PID=""
MOCK_PID=""
cleanup() {
    [ -n "$JANUS_PID" ] && kill "$JANUS_PID" 2>/dev/null || true
    [ -n "$MOCK_PID" ]  && kill "$MOCK_PID"  2>/dev/null || true
    pkill -f "target/release/janus serve" 2>/dev/null || true
    pkill -f "target/release/mock-llm"    2>/dev/null || true
}
trap cleanup EXIT

[ -x ./target/release/janus ]                        || fail "release binary missing"
[ -x ./benchmarks/mock-llm/target/release/mock-llm ] || fail "mock-llm not built"

# ── Baseline 1: mock-llm only, no Janus in the path ───────────────────────────
log "Phase 1: mock-llm alone (baseline — measures VM/scheduler floor)"
pkill -f "target/release/mock-llm" 2>/dev/null || true
./benchmarks/mock-llm/target/release/mock-llm --port 9999 --ttft-ms 1 --tpot-ms 0 --output-tokens 1 \
    > /tmp/mock-llm.log 2>&1 &
MOCK_PID=$!
sleep 2

cat > /tmp/overhead-probe.json <<'JSON'
{"model":"gpt-4o-mini","stream":true,"messages":[{"role":"user","content":"hi"}],"max_tokens":1}
JSON

oha -z "$DURATION" -c "$CONCURRENCY" -m POST \
    -H "Content-Type: application/json" \
    -D /tmp/overhead-probe.json \
    --disable-color \
    http://localhost:9999/v1/chat/completions \
  | tee /tmp/probe-mock-only.txt

MOCK_P99=$(grep "99.00%" /tmp/probe-mock-only.txt | awk '{print $3, $4}')

# ── Baseline 2: Janus on top of the same mock, cache disabled ────────────────
log "Phase 2: Janus + mock (full gateway, cache disabled)"
pkill -f "target/release/janus serve" 2>/dev/null || true
sleep 1
./target/release/janus serve > /tmp/janus.log 2>&1 &
JANUS_PID=$!
for _ in $(seq 1 45); do
    curl -s http://localhost:8080/health | jq -e '.providers | length > 0' >/dev/null 2>&1 && break
    sleep 1
done

JWT=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"bench@janus.local","password":"bench-only-do-not-use-elsewhere"}' \
  | jq -r '.token // empty')
[ -n "$JWT" ] || fail "login failed — has seed.sh been run?"

curl -sf -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":false}' > /dev/null

API_KEY=$(cat benchmarks/.janus_api_key)
oha -z "$DURATION" -c "$CONCURRENCY" -m POST \
    -H "Authorization: Bearer $API_KEY" \
    -H "Content-Type: application/json" \
    -D /tmp/overhead-probe.json \
    --disable-color \
    http://localhost:8080/v1/chat/completions \
  | tee /tmp/probe-janus.txt

# Restore cache for any follow-up runs.
curl -sf -X PATCH http://localhost:8080/admin/config \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d '{"cache_enabled":true}' > /dev/null

JANUS_P99=$(grep "99.00%" /tmp/probe-janus.txt | awk '{print $3, $4}')

# ── Report ────────────────────────────────────────────────────────────────────
cat <<EOF

────────────────────────────────────────────────────────────────────────────────
Overhead probe report (duration=$DURATION, concurrency=$CONCURRENCY)

  Phase 1 — mock-llm alone (VM floor):              p99 = $MOCK_P99
  Phase 2 — Janus + mock (full gateway):            p99 = $JANUS_P99

  >>> Inferred Janus-only overhead = Phase 2 − Phase 1 <<<

If you saw a Phase 1 p99 above ~5 ms on this host, you are on a shared vCPU
and the inferred overhead number is the cleanest signal you can extract. To
get an absolute "Janus only" number, repeat on a dedicated-CPU host where
Phase 1 should fall well under 2 ms.

────────────────────────────────────────────────────────────────────────────────
EOF
