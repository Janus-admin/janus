#!/usr/bin/env bash
# seed.sh — one-time database preparation for the benchmark harness.
#
# What this script does, in order:
#   1. Probes that Janus is up at JANUS_ADMIN_URL.
#   2. Probes that the mock upstream is up at MOCK_URL.
#   3. Registers the benchmark admin user (idempotent).
#   4. Logs in as that user → obtains a JWT.
#   5. PATCHes the `openai` provider's base_url to point at the mock.
#   6. POSTs a fresh API key with no budget cap and no rate limit.
#   7. Prints the key to stdout AND saves it to .janus_api_key in this folder.
#
# IMPORTANT: After running this script you MUST restart Janus once. The provider
# base_url is loaded into the in-memory ProviderRegistry at startup; a running
# Janus won't pick up the new value until it boots again. This is one-time pain.
#
# Re-running this script after the first time is safe — the user is detected and
# only the API key is re-created.

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────
JANUS_ADMIN_URL="${JANUS_ADMIN_URL:-http://localhost:8080}"
MOCK_URL="${MOCK_URL:-http://localhost:9999}"
ADMIN_EMAIL="${ADMIN_EMAIL:-bench@janus.local}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-bench-only-do-not-use-elsewhere}"
ADMIN_NAME="${ADMIN_NAME:-Benchmark Admin}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEY_FILE="$HERE/.janus_api_key"

# ── Utilities ────────────────────────────────────────────────────────────────
log()  { printf '[seed] %s\n' "$*" >&2; }
fail() { printf '[seed] ERROR: %s\n' "$*" >&2; exit 1; }

require() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1"
}

require curl
require jq

# ── Preflight ─────────────────────────────────────────────────────────────────
log "Checking Janus at $JANUS_ADMIN_URL ..."
if ! curl -s -f -m 5 "$JANUS_ADMIN_URL/health" >/dev/null; then
    fail "Janus is not reachable at $JANUS_ADMIN_URL. Start it: 'cargo run --release -- serve'"
fi

log "Checking mock-llm at $MOCK_URL ..."
if ! curl -s -f -m 5 "$MOCK_URL/healthz" >/dev/null; then
    fail "mock-llm is not reachable at $MOCK_URL. Start it: './benchmarks/mock-llm/target/release/mock-llm --port 9999'"
fi

# ── Step 1: register benchmark admin (idempotent) ─────────────────────────────
log "Registering benchmark admin user ($ADMIN_EMAIL) ..."
REG_RESP=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "$JANUS_ADMIN_URL/api/v1/auth/register" \
    -H 'Content-Type: application/json' \
    -d "$(jq -nc --arg e "$ADMIN_EMAIL" --arg p "$ADMIN_PASSWORD" --arg n "$ADMIN_NAME" \
        '{email:$e, password:$p, name:$n}')")

case "$REG_RESP" in
    200|201) log "  → user created" ;;
    409)     log "  → user already exists (idempotent)" ;;
    *)       fail "register returned HTTP $REG_RESP — is the admin API working?" ;;
esac

# ── Step 2: login → JWT ───────────────────────────────────────────────────────
log "Logging in to obtain JWT ..."
LOGIN_BODY=$(jq -nc --arg e "$ADMIN_EMAIL" --arg p "$ADMIN_PASSWORD" '{email:$e, password:$p}')
LOGIN_RESP=$(curl -s -X POST "$JANUS_ADMIN_URL/api/v1/auth/login" \
    -H 'Content-Type: application/json' -d "$LOGIN_BODY")

JWT=$(echo "$LOGIN_RESP" | jq -r '.token // empty')
if [ -z "$JWT" ]; then
    fail "login failed; response was: $LOGIN_RESP"
fi
log "  → got JWT (length ${#JWT})"

# ── Step 3: PATCH openai provider to point at the mock ────────────────────────
# Janus's OpenAI adapter appends `/chat/completions` to base_url, so base_url
# must include the `/v1` path segment — exactly mirroring api.openai.com/v1.
# Strip any trailing slash from $MOCK_URL, ensure it ends with /v1.
MOCK_BASE="${MOCK_URL%/}"
case "$MOCK_BASE" in
    */v1) ;;                  # already has /v1
    *)    MOCK_BASE="$MOCK_BASE/v1" ;;
esac

log "Pointing 'openai' provider at $MOCK_BASE ..."
PATCH_BODY=$(jq -nc --arg url "$MOCK_BASE" '{is_enabled:true, base_url:$url}')
PATCH_STATUS=$(curl -s -o /tmp/janus-seed-patch.json -w '%{http_code}' \
    -X PATCH "$JANUS_ADMIN_URL/admin/providers/openai" \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' \
    -d "$PATCH_BODY")

if [ "$PATCH_STATUS" != "200" ]; then
    cat /tmp/janus-seed-patch.json >&2
    fail "PATCH /admin/providers/openai returned $PATCH_STATUS"
fi
log "  → base_url updated in DB (Janus must restart to load it)"

# ── Step 4: create an API key with no caps ────────────────────────────────────
log "Creating benchmark API key ..."
KEY_BODY=$(jq -nc '{name:"bench-harness", routing_strategy:"priority"}')
KEY_RESP=$(curl -s -X POST "$JANUS_ADMIN_URL/admin/keys" \
    -H "Authorization: Bearer $JWT" \
    -H 'Content-Type: application/json' -d "$KEY_BODY")

API_KEY=$(echo "$KEY_RESP" | jq -r '.data.key // empty')
if [ -z "$API_KEY" ]; then
    fail "key creation failed; response was: $KEY_RESP"
fi

# ── Step 5: persist for run.sh ────────────────────────────────────────────────
umask 077
printf '%s\n' "$API_KEY" > "$KEY_FILE"
log "  → key saved to $KEY_FILE (file mode 600)"

# ── Output ────────────────────────────────────────────────────────────────────
cat >&2 <<EOF

────────────────────────────────────────────────────────────────────────────────
Seed complete.

  Benchmark API key: $API_KEY

NEXT STEPS:

  1. RESTART Janus. The provider base_url change requires it.
       (Stop the running 'cargo run' and start it again.)

  2. Export the API key in your shell so run.sh can find it:
       export JANUS_API_KEY=$API_KEY

  3. Run your first benchmark:
       ./benchmarks/run.sh chat-short 60s 50

The key is also saved to: $KEY_FILE
────────────────────────────────────────────────────────────────────────────────
EOF

# Echo just the key to stdout so scripts can capture it via $(./seed.sh).
printf '%s\n' "$API_KEY"
