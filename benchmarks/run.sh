#!/usr/bin/env bash
# run.sh — orchestrate a single Janus benchmark run.
#
# Usage:
#   ./run.sh <profile> <duration> <concurrency>
#
# Example:
#   ./run.sh chat-short 60s 50
#
# What it produces:
#   benchmarks/results/<ISO-timestamp>/
#     ├── REPORT.md            human-readable summary
#     ├── oha.txt              raw oha output
#     ├── metrics-before.txt   /metrics snapshot before the run
#     ├── metrics-after.txt    /metrics snapshot after the run
#     ├── proc.csv             CPU% / RSS / time series (1 Hz)
#     ├── profile.json         a frozen copy of the profile body
#     ├── env.txt              every environment variable that could affect timing
#     └── manifest.json        machine-readable summary
#
# Exit codes:
#   0   all good
#   1   bad arguments
#   2   missing dependency
#   3   Janus or mock not reachable
#   4   load tool reported errors

set -euo pipefail

# ── Inputs ────────────────────────────────────────────────────────────────────
PROFILE="${1:-}"
DURATION="${2:-}"
CONCURRENCY="${3:-}"

# ── Configuration ─────────────────────────────────────────────────────────────
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILES_DIR="$HERE/profiles"
RESULTS_DIR="$HERE/results"

# Use 127.0.0.1 explicitly, not localhost.
# On macOS, `localhost` resolves to ::1 (IPv6) first; oha doesn't fall back to
# IPv4, so it fails with "Connection refused" when Janus listens on 0.0.0.0
# (IPv4-only). curl is more forgiving and works either way.
JANUS_BASE="${JANUS_BASE:-http://127.0.0.1:8080}"
JANUS_API_KEY="${JANUS_API_KEY:-}"
WARMUP_SECS="${WARMUP_SECS:-10}"

# Profiles must be on this allow-list so a typo doesn't produce results filed
# under a non-existent name. Add new profiles here when you add files.
ALLOWED_PROFILES=("chat-short" "chat-long" "cache-warm" "tools")

# ── Utilities ─────────────────────────────────────────────────────────────────
log()  { printf '[run] %s\n' "$*" >&2; }
fail() { printf '[run] ERROR: %s\n' "$*" >&2; exit "${2:-1}"; }

require() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1 — install hint: $2" 2
}

# ── Argument validation ───────────────────────────────────────────────────────
if [ -z "$PROFILE" ] || [ -z "$DURATION" ] || [ -z "$CONCURRENCY" ]; then
    cat <<EOF >&2
usage: $(basename "$0") <profile> <duration> <concurrency>

Profiles (file under benchmarks/profiles/<name>.json):
EOF
    for p in "${ALLOWED_PROFILES[@]}"; do echo "    $p" >&2; done
    cat <<EOF >&2

Examples:
    $(basename "$0") chat-short 60s 50
    $(basename "$0") cache-warm 30s 100

Environment:
    JANUS_BASE       default $JANUS_BASE
    JANUS_API_KEY    your jn-sk-... key (or pre-load from .janus_api_key)
    WARMUP_SECS      default 10
EOF
    exit 1
fi

# Allow-list check.
if ! printf '%s\n' "${ALLOWED_PROFILES[@]}" | grep -qx "$PROFILE"; then
    fail "unknown profile: $PROFILE. Allowed: ${ALLOWED_PROFILES[*]}"
fi

PROFILE_FILE="$PROFILES_DIR/$PROFILE.json"
[ -f "$PROFILE_FILE" ] || fail "profile file missing: $PROFILE_FILE"

# ── Dependency check ─────────────────────────────────────────────────────────
require curl "system package manager"
require jq   "brew install jq  /  apt-get install jq"
require oha  "brew install oha /  cargo install oha"

# ── API key resolution ────────────────────────────────────────────────────────
if [ -z "$JANUS_API_KEY" ]; then
    if [ -f "$HERE/.janus_api_key" ]; then
        JANUS_API_KEY=$(cat "$HERE/.janus_api_key")
        log "Using API key from .janus_api_key"
    else
        fail "JANUS_API_KEY not set and .janus_api_key not found. Run seed.sh first."
    fi
fi

# ── Preflight ─────────────────────────────────────────────────────────────────
log "Checking Janus at $JANUS_BASE ..."
curl -s -f -m 5 "$JANUS_BASE/health" >/dev/null \
    || fail "Janus not reachable at $JANUS_BASE" 3

# Verify the API key actually works against the gateway. We do a HEAD-equivalent
# by sending an obviously-tiny body; if Janus accepts the key it'll either 200
# or 400-with-validation-error, both of which prove auth passed.
log "Validating API key ..."
auth_probe=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "$JANUS_BASE/v1/chat/completions" \
    -H "Authorization: Bearer $JANUS_API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}],"max_tokens":1}')

case "$auth_probe" in
    200|400|503) log "  → auth ok (HTTP $auth_probe)" ;;
    401|403)     fail "API key rejected (HTTP $auth_probe) — re-run seed.sh" 3 ;;
    *)           log "  → auth probe returned HTTP $auth_probe (continuing)" ;;
esac

# ── Discover the Janus PID for process sampling ──────────────────────────────
JANUS_PID=""
if command -v pgrep >/dev/null 2>&1; then
    # On macOS pgrep matches against the full command. Match the release binary.
    JANUS_PID=$(pgrep -f 'target/release/janus' | head -1 || true)
fi
if [ -z "$JANUS_PID" ]; then
    log "  → could not locate Janus PID (process sampling will be skipped)"
fi

# ── Set up result folder ─────────────────────────────────────────────────────
TS=$(date -u +"%Y-%m-%dT%H-%M-%SZ")
RUN_DIR="$RESULTS_DIR/$TS-$PROFILE"
mkdir -p "$RUN_DIR"
log "Output → $RUN_DIR"

cp "$PROFILE_FILE" "$RUN_DIR/profile.json"

# Freeze the relevant environment so the report can prove what was configured.
{
    echo "# Frozen environment for run $TS"
    echo "PROFILE=$PROFILE"
    echo "DURATION=$DURATION"
    echo "CONCURRENCY=$CONCURRENCY"
    echo "WARMUP_SECS=$WARMUP_SECS"
    echo "JANUS_BASE=$JANUS_BASE"
    echo "GIT_COMMIT=$(git -C "$HERE/.." rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "GIT_BRANCH=$(git -C "$HERE/.." rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
    echo "GIT_DIRTY=$(git -C "$HERE/.." diff --quiet 2>/dev/null && echo no || echo yes)"
    echo "UNAME=$(uname -a)"
    echo "RUSTC=$(rustc --version 2>/dev/null || echo missing)"
    echo "OHA=$(oha --version 2>/dev/null || echo missing)"
    if [ "$(uname)" = "Darwin" ]; then
        echo "CPU_MODEL=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
        echo "CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo unknown)"
        echo "MEM_BYTES=$(sysctl -n hw.memsize 2>/dev/null || echo unknown)"
    else
        echo "CPU_MODEL=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //' || echo unknown)"
        echo "CPU_CORES=$(nproc 2>/dev/null || echo unknown)"
        echo "MEM_BYTES=$(($(grep MemTotal /proc/meminfo | awk '{print $2}') * 1024))"
    fi
} > "$RUN_DIR/env.txt"

# ── Snapshot Prometheus metrics before ────────────────────────────────────────
log "Snapshotting /metrics (before) ..."
curl -s "$JANUS_BASE/metrics" > "$RUN_DIR/metrics-before.txt" || true

# ── Start process sampler ─────────────────────────────────────────────────────
SAMPLER_PID=""
if [ -n "$JANUS_PID" ]; then
    "$HERE/sample-proc.sh" "$JANUS_PID" "$RUN_DIR/proc.csv" &
    SAMPLER_PID=$!
    log "Process sampler started (PID $SAMPLER_PID) tracking Janus PID $JANUS_PID"
fi

# ── Optional warm-up ─────────────────────────────────────────────────────────
if [ "$WARMUP_SECS" -gt 0 ]; then
    log "Warming up for ${WARMUP_SECS}s (not recorded) ..."
    oha -z "${WARMUP_SECS}s" -c "$CONCURRENCY" \
        -m POST \
        -H "Authorization: Bearer $JANUS_API_KEY" \
        -H 'Content-Type: application/json' \
        -D "$PROFILE_FILE" \
        --no-tui \
        "$JANUS_BASE/v1/chat/completions" >/dev/null 2>&1 || true
fi

# ── Measurement window ───────────────────────────────────────────────────────
log "Running measurement: profile=$PROFILE duration=$DURATION concurrency=$CONCURRENCY"
oha -z "$DURATION" -c "$CONCURRENCY" \
    -m POST \
    -H "Authorization: Bearer $JANUS_API_KEY" \
    -H 'Content-Type: application/json' \
    -D "$PROFILE_FILE" \
    --no-tui \
    "$JANUS_BASE/v1/chat/completions" > "$RUN_DIR/oha.txt" 2>&1

# ── Snapshot Prometheus metrics after ─────────────────────────────────────────
log "Snapshotting /metrics (after) ..."
curl -s "$JANUS_BASE/metrics" > "$RUN_DIR/metrics-after.txt" || true

# ── Stop sampler ──────────────────────────────────────────────────────────────
if [ -n "$SAMPLER_PID" ]; then
    kill "$SAMPLER_PID" 2>/dev/null || true
    wait "$SAMPLER_PID" 2>/dev/null || true
fi

# ── Parse oha output and build REPORT.md ─────────────────────────────────────
log "Parsing results ..."
OHA="$RUN_DIR/oha.txt"

# oha's "Response time distribution" lines look like "  50.00% in 1.3709 sec".
# BSD awk (default on macOS) does NOT support `\s` — use `[ \t]` instead.
success_rate=$(awk '/^[ \t]*Success rate:/   {print $3; exit}' "$OHA")
rps=$(          awk '/^[ \t]*Requests\/sec:/ {print $2; exit}' "$OHA")
p50=$(          awk '/^[ \t]*50\.00%/ {print $3, $4; exit}' "$OHA")
p95=$(          awk '/^[ \t]*95\.00%/ {print $3, $4; exit}' "$OHA")
p99=$(          awk '/^[ \t]*99\.00%/ {print $3, $4; exit}' "$OHA")

# Errors: parse "Error distribution" section, but EXCLUDE "deadline" aborts —
# those are oha cancelling in-flight requests when -z time runs out, expected
# behavior. Real errors are anything else (connection refused, 5xx, etc).
errors=$(awk '
    /^Error distribution:/   {in_section=1; next}
    in_section && /[ \t]*\[[0-9]+\]/ {
        # Skip lines that mention "deadline" (oha shutdown aborts).
        if (index($0, "deadline") == 0) {
            # Extract the count inside [N]
            match($0, /\[[0-9]+\]/)
            n = substr($0, RSTART+1, RLENGTH-2)
            sum += n
        }
    }
    END { print sum+0 }
' "$OHA")

# Metrics: counter series may have multiple label-set instances; sum across them.
# Prometheus text format: `metric_name{labels} value [timestamp]`. The value is
# at $2 because the metric+labels has no internal whitespace.
sum_metric() {
    local file="$1"
    local name="$2"
    awk -v m="^$name" '$0 ~ m {sum += $2} END {print sum+0}' "$file" 2>/dev/null
}

hits_before=$(sum_metric "$RUN_DIR/metrics-before.txt" "janus_cache_hits_total")
hits_after=$( sum_metric "$RUN_DIR/metrics-after.txt"  "janus_cache_hits_total")
reqs_before=$(sum_metric "$RUN_DIR/metrics-before.txt" "janus_requests_total")
reqs_after=$( sum_metric "$RUN_DIR/metrics-after.txt"  "janus_requests_total")

hits_delta=$(awk "BEGIN { print ${hits_after:-0} - ${hits_before:-0} }")
reqs_delta=$(awk "BEGIN { print ${reqs_after:-0} - ${reqs_before:-0} }")

hit_ratio="n/a"
if [ -n "$reqs_delta" ] && [ "$(awk "BEGIN { print ($reqs_delta > 0) }")" = "1" ]; then
    hit_ratio=$(awk "BEGIN { printf \"%.3f\", $hits_delta / $reqs_delta }")
fi

# Process samples — pull steady-state median and peak.
cpu_med="n/a"; rss_peak="n/a"
if [ -f "$RUN_DIR/proc.csv" ] && [ "$(wc -l < "$RUN_DIR/proc.csv")" -gt 1 ]; then
    cpu_med=$(tail -n +2 "$RUN_DIR/proc.csv" | awk -F, '{print $2}' | sort -n | awk 'BEGIN{c=0} {a[c++]=$1} END{print a[int(c/2)]}')
    rss_peak=$(tail -n +2 "$RUN_DIR/proc.csv" | awk -F, '{print $3}' | sort -n | tail -1)
fi

# ── REPORT.md ─────────────────────────────────────────────────────────────────
cat > "$RUN_DIR/REPORT.md" <<EOF
# Benchmark run — $TS

**Profile:** \`$PROFILE\`
**Duration:** $DURATION   **Concurrency:** $CONCURRENCY   **Warm-up:** ${WARMUP_SECS}s

## Headline numbers

| Metric | Value |
|---|---:|
| Throughput | $rps req/s |
| Success rate (oha) | $success_rate |
| Latency p50 | $p50 |
| Latency p95 | $p95 |
| Latency p99 | $p99 |
| Total requests (Janus) | ${reqs_delta:-?} |
| Cache hits (Janus) | ${hits_delta:-?} |
| Cache hit ratio | $hit_ratio |
| Errors (oha-reported) | $errors |
| CPU% (median, steady) | $cpu_med |
| RSS peak (kB) | $rss_peak |

## Provenance

\`\`\`
$(cat "$RUN_DIR/env.txt")
\`\`\`

## How to read this

- **Latency** is end-to-end wall-clock. Subtract the mock's configured TTFT
  to get Janus's overhead. For streaming profiles, also subtract
  \`output_tokens × tpot_ms\`.
- **Cache hit ratio** counts only after the warm-up window.
- **Errors must be zero.** If non-zero, the run is invalid — investigate
  before publishing.

## Artefacts

| File | What it is |
|---|---|
| \`oha.txt\` | Raw load-tool output (full latency histogram inside) |
| \`metrics-before.txt\` / \`metrics-after.txt\` | Prometheus snapshots framing the run |
| \`proc.csv\` | CPU% and RSS, sampled at 1 Hz |
| \`profile.json\` | The exact request body that was sent |
| \`env.txt\` | Frozen environment description |
EOF

# ── Machine-readable manifest ────────────────────────────────────────────────
jq -n \
    --arg profile "$PROFILE" \
    --arg duration "$DURATION" \
    --arg concurrency "$CONCURRENCY" \
    --arg ts "$TS" \
    --arg rps "$rps" \
    --arg p50 "$p50" --arg p95 "$p95" --arg p99 "$p99" \
    --argjson reqs "${reqs_delta:-0}" \
    --argjson hits "${hits_delta:-0}" \
    --arg hit_ratio "$hit_ratio" \
    --arg errors "$errors" \
    --arg cpu_med "$cpu_med" \
    --arg rss_peak "$rss_peak" \
    '{ profile:$profile, duration:$duration, concurrency:$concurrency, timestamp:$ts,
       throughput_rps:$rps,
       latency: { p50:$p50, p95:$p95, p99:$p99 },
       requests:$reqs, cache_hits:$hits, cache_hit_ratio:$hit_ratio,
       errors:$errors,
       resources: { cpu_pct_median:$cpu_med, rss_kb_peak:$rss_peak } }' \
    > "$RUN_DIR/manifest.json"

# ── Final output ──────────────────────────────────────────────────────────────
cat >&2 <<EOF

────────────────────────────────────────────────────────────────────────────────
Run complete.

  Profile:     $PROFILE
  Throughput:  $rps req/s
  Latency:     p50=$p50  p95=$p95  p99=$p99
  Cache hits:  $hits_delta / $reqs_delta (ratio $hit_ratio)
  Errors:      $errors

  Full report: $RUN_DIR/REPORT.md
────────────────────────────────────────────────────────────────────────────────
EOF

# Exit code reflects error count so CI scripts can gate on it.
if [ "$errors" != "0" ] && [ "$errors" != "" ]; then
    exit 4
fi
exit 0
