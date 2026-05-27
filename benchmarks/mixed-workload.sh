#!/usr/bin/env bash
# mixed-workload.sh — simulates a 70% cache-hit / 30% cache-miss workload.
#
# Runs two oha instances in parallel:
#   - HIT  process: 35 concurrent connections → chat-short (already in cache)
#   - MISS process: 15 concurrent connections → mixed-miss  (unique prompt, goes to provider)
#
# Combined: 50 total concurrent connections, ~70/30 split.
#
# Usage: ./benchmarks/mixed-workload.sh [duration] [output-dir]
#   duration   default 60s
#   output-dir default benchmarks/results/<timestamp>-mixed

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DURATION="${1:-60s}"
OUTDIR="${2:-}"

JANUS_BASE="${JANUS_BASE:-http://127.0.0.1:8080}"
KEY_FILE="$HERE/.janus_api_key"

JANUS_API_KEY="${JANUS_API_KEY:-}"
if [ -z "$JANUS_API_KEY" ]; then
    [ -f "$KEY_FILE" ] || { echo "[mixed] ERROR: .janus_api_key not found"; exit 1; }
    JANUS_API_KEY=$(cat "$KEY_FILE")
fi

TS=$(date -u '+%Y-%m-%dT%H-%M-%SZ')
[ -z "$OUTDIR" ] && OUTDIR="$HERE/results/${TS}-mixed"
mkdir -p "$OUTDIR"

HIT_FILE="$HERE/profiles/chat-short.json"
MISS_FILE="$HERE/profiles/mixed-miss.json"

echo "[mixed] Warming up cache (10s) ..."
oha -z 10s -c 35 -m POST \
    -H "Authorization: Bearer $JANUS_API_KEY" \
    -H "Content-Type: application/json" \
    -D "$HIT_FILE" \
    --disable-color \
    "$JANUS_BASE/v1/chat/completions" > /dev/null 2>&1

echo "[mixed] Running mixed workload: duration=$DURATION 35 hit + 15 miss connections"

oha -z "$DURATION" -c 35 -m POST \
    -H "Authorization: Bearer $JANUS_API_KEY" \
    -H "Content-Type: application/json" \
    -D "$HIT_FILE" \
    --disable-color \
    "$JANUS_BASE/v1/chat/completions" > "$OUTDIR/oha-hit.txt" 2>&1 &
PID_HIT=$!

oha -z "$DURATION" -c 15 -m POST \
    -H "Authorization: Bearer $JANUS_API_KEY" \
    -H "Content-Type: application/json" \
    -D "$MISS_FILE" \
    --disable-color \
    "$JANUS_BASE/v1/chat/completions" > "$OUTDIR/oha-miss.txt" 2>&1 &
PID_MISS=$!

wait $PID_HIT $PID_MISS

# ── Parse results ─────────────────────────────────────────────────────────────
parse_rps() { grep "Requests/sec" "$1" 2>/dev/null | awk '{print $2}'; }
parse_p50() { grep "50.00%" "$1" 2>/dev/null | awk '{print $3, $4}'; }
parse_p99() { grep "99.00%" "$1" 2>/dev/null | awk '{print $3, $4}'; }
parse_errors() {
    local f="$1"
    local total
    total=$(grep "^  Total:" "$f" 2>/dev/null | awk '{print $2}' || echo 0)
    local success
    success=$(grep "Success rate" "$f" 2>/dev/null | awk '{print $3}' | tr -d '%' || echo 100)
    # rough error count from success rate
    echo "$total $success" | awk '{printf "%d", $1 * (1 - $2/100)}'
}

RPS_HIT=$(parse_rps "$OUTDIR/oha-hit.txt")
RPS_MISS=$(parse_rps "$OUTDIR/oha-miss.txt")
RPS_TOTAL=$(echo "$RPS_HIT $RPS_MISS" | awk '{printf "%.1f", $1+$2}')

P50_HIT=$(parse_p50 "$OUTDIR/oha-hit.txt")
P50_MISS=$(parse_p50 "$OUTDIR/oha-miss.txt")
P99_HIT=$(parse_p99 "$OUTDIR/oha-hit.txt")
P99_MISS=$(parse_p99 "$OUTDIR/oha-miss.txt")

cat > "$OUTDIR/REPORT.md" <<EOF
# Mixed Workload Benchmark — $TS

**Duration:** $DURATION  |  **Total concurrency:** 50 (35 hit + 15 miss)  |  **Split:** ~70% cache hit / 30% cache miss

## Results

| Stream | Concurrency | Throughput | p50 | p99 |
|---|---|---|---|---|
| Cache HIT (chat-short) | 35 | ${RPS_HIT} req/s | ${P50_HIT} | ${P99_HIT} |
| Cache MISS (mixed-miss) | 15 | ${RPS_MISS} req/s | ${P50_MISS} | ${P99_MISS} |
| **Combined** | **50** | **${RPS_TOTAL} req/s** | — | — |

## Artefacts
- \`oha-hit.txt\`  — full oha output for the cache-hit stream
- \`oha-miss.txt\` — full oha output for the cache-miss stream
EOF

echo ""
echo "────────────────────────────────────────────────────────────────────────────────"
echo "Mixed workload complete."
echo ""
echo "  Cache HIT  (35 conn):  ${RPS_HIT} req/s   p50=${P50_HIT}   p99=${P99_HIT}"
echo "  Cache MISS (15 conn):  ${RPS_MISS} req/s  p50=${P50_MISS}  p99=${P99_MISS}"
echo "  Combined total:        ${RPS_TOTAL} req/s"
echo ""
echo "  Full report: $OUTDIR/REPORT.md"
echo "────────────────────────────────────────────────────────────────────────────────"
