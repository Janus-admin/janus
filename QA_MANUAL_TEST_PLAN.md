# Janus — Manual QA Test Plan

**For:** QA Engineer  
**What this tests:** The complete Janus product as a single working system  
**How to use:** Run tests top to bottom within each section. Every test has exact steps and a clear pass/fail criterion. If the result does not match "Expected Result" exactly — it is a bug. Write it down.

---

## Test Environment Setup

Start Janus before running any tests:

```bash
# Standard (PostgreSQL)
DATABASE_URL=postgres://postgres:password@localhost/janus \
JWT_SECRET=a-very-long-secret-at-least-32-characters-long \
ENCRYPTION_KEY=0000000000000000000000000000000000000000000000000000000000000000 \
cargo run

# OR: Demo mode (no PostgreSQL needed — good for a first run)
cargo run -- demo
# Default login: admin@janus.local / demo-password
```

**Conventions used throughout this document:**

| Placeholder | Meaning |
|---|---|
| `<jwt>` | Admin JWT token obtained from the login endpoint |
| `<gw-key>` | Gateway API key (`jn-sk-...`) created via the keys endpoint |
| `<uuid>` | Any valid UUID from prior test output |
| `BASE` | `http://localhost:8080` |

---

## Section 1 — Health & System Readiness

### TC-SYS-001: Health endpoint returns system status
**Priority:** P0

```bash
curl -s BASE/health | jq .
```

**Expected:**
- HTTP 200
- Body contains `status` (healthy value), `version` string, database connectivity result, list of configured providers

---

### TC-SYS-002: System readiness checks all pass on a healthy instance
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/system/readiness | jq .
```

**Expected:**
- HTTP 200 (all checks pass) or HTTP 503 (any check fails)
- Response lists individual results for: database connection, migrations status, JWT secret length, encryption key presence, providers enabled, embedding model files, disk space
- Each check has a pass/fail/warning indicator

---

### TC-SYS-003: `janus doctor` prints a human-readable check report
**Priority:** P1

```bash
./target/debug/janus doctor
```

**Expected:**
- Output shows `[✓]`, `[✗]`, or `[!]` next to each check
- Exit code is 0 if all pass, non-zero if any fail
- Failure messages are descriptive (e.g., "JWT secret too short — 12 bytes, minimum 32")

---

### TC-SYS-004: Demo mode starts without any external database
**Priority:** P1

```bash
./target/debug/janus demo
# In a new terminal:
curl -s -X POST BASE/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@janus.local","password":"demo-password"}' | jq .token
```

**Expected:**
- Server starts with no `DATABASE_URL` set
- Login succeeds and returns a JWT
- Pre-seeded API keys and request history are visible in the dashboard

---

## Section 2 — Admin Authentication

### TC-AUTH-001: Register a new admin user
**Priority:** P1

```bash
curl -s -X POST BASE/api/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"qa@example.com","password":"StrongPass123!","name":"QA Tester"}' | jq .
```

**Expected:**
- HTTP 201
- Response body: `{ "data": { "id": "<uuid>", "email": "qa@example.com", "name": "QA Tester" } }`
- No `password` or `password_hash` field anywhere in the response

---

### TC-AUTH-002: Login with valid credentials returns a JWT
**Priority:** P0

```bash
curl -s -X POST BASE/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@janus.local","password":"demo-password"}' | jq .
```

**Expected:**
- HTTP 200
- Body: `{ "data": { "token": "<jwt>", "user": { "id": "...", "email": "admin@janus.local" } } }`
- The token is a valid JWT (three base64url segments separated by dots)
- **Save this token** — it is needed for all subsequent admin tests

---

### TC-AUTH-003: Login with wrong password is rejected
**Priority:** P0

```bash
curl -s -X POST BASE/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@janus.local","password":"WRONG"}' | jq .
```

**Expected:**
- HTTP 401
- Error body present, no token returned

---

### TC-AUTH-004: Protected endpoint rejects missing or invalid token
**Priority:** P0

```bash
# No token
curl -s BASE/api/v1/auth/me

# Invalid token
curl -s -H "Authorization: Bearer GARBAGE" BASE/api/v1/auth/me
```

**Expected:** Both return HTTP 401

---

### TC-AUTH-005: Get current user with valid JWT
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/api/v1/auth/me | jq .
```

**Expected:**
- HTTP 200
- Body contains the authenticated user's `id`, `email`, `name`
- No password hash in response

---

## Section 3 — User Management

### TC-USERS-001: List all users
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/api/v1/users | jq .
```

**Expected:**
- HTTP 200
- Body: `{ "data": [...], "meta": { "page": 1, "per_page": ..., "total": ... } }`
- At least one user (the admin) is present
- No password fields exposed

---

### TC-USERS-002: Get a specific user by ID
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/api/v1/users/<uuid> | jq .
```

**Expected:**
- HTTP 200 for a valid UUID
- HTTP 404 for a non-existent UUID

---

### TC-USERS-003: Update a user's name
**Priority:** P1

```bash
curl -s -X PUT \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"Updated Name"}' \
  BASE/api/v1/users/<uuid> | jq .data.name
```

**Expected:**
- HTTP 200, response shows `"Updated Name"`

---

### TC-USERS-004: Delete a user
**Priority:** P1

```bash
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/api/v1/users/<uuid>
# Verify gone:
curl -s -H "Authorization: Bearer <jwt>" BASE/api/v1/users/<uuid> | jq .
```

**Expected:**
- Delete returns HTTP 200 or 204
- Subsequent GET for the same UUID returns HTTP 404

---

## Section 4 — API Key Management

### TC-KEYS-001: Create an API key — full key shown exactly once
**Priority:** P0

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"Test Key","budget_limit":100.00,"rate_limit_rpm":60}' \
  BASE/admin/keys | jq .
```

**Expected:**
- HTTP 201
- Response contains `"key": "jn-sk-<48 chars>"` — the full key, starting with `jn-sk-`
- **Save this key now** — it will never be shown again

---

### TC-KEYS-002: Key list shows prefix only, never the full key
**Priority:** P0

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/keys | jq '.data[0]'
```

**Expected:**
- HTTP 200
- Each key object has `key_prefix` (e.g., `"jn-sk-a8Kd..."`) — partial only
- No `key` field with the full value in any list item

---

### TC-KEYS-003: Create key with all optional fields
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Advanced Key",
    "budget_limit": 50.00,
    "rate_limit_rpm": 30,
    "rate_limit_tpm": 10000,
    "routing_strategy": "cost_optimized",
    "downgrade_at_percent": 80,
    "downgrade_strategy": "cost_optimized"
  }' \
  BASE/admin/keys | jq .data
```

**Expected:**
- HTTP 201
- All fields returned as submitted: `routing_strategy`, `downgrade_at_percent`, `downgrade_strategy`
- Accepted values for `routing_strategy`: `priority`, `cost_optimized`, `latency_optimized`, `round_robin`

---

### TC-KEYS-004: Rotate a key — old key stays valid during grace period
**Priority:** P1

```bash
# Rotate
curl -s -X POST -H "Authorization: Bearer <jwt>" \
  BASE/admin/keys/<key_id>/rotate | jq .data.key

# Use OLD key immediately after rotation (should still work)
curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer <old_jn-sk-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions
```

**Expected:**
- Rotate returns HTTP 200 and a new full `jn-sk-...` key
- Old key is still accepted during the grace period (default 300 seconds) — returns HTTP 200

---

### TC-KEYS-005: Revoke (delete) a key
**Priority:** P1

```bash
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/admin/keys/<key_id>

# Verify the key no longer works
curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer <that_jn-sk-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions
```

**Expected:**
- Delete: HTTP 200 or 204
- Gateway call with deleted key: HTTP 401

---

## Section 5 — Provider Management

### TC-PROV-001: List all providers
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/providers | jq '.data[] | {name, enabled, health_status, quality_score, priority}'
```

**Expected:**
- HTTP 200
- Each provider shows: `name`, `enabled`, `health_status`, `priority`, `quality_score` (0.0–1.0)
- No API keys shown in plaintext

---

### TC-PROV-002: Test a provider connection
**Priority:** P1

```bash
curl -s -X POST -H "Authorization: Bearer <jwt>" \
  BASE/admin/providers/<provider_id>/test | jq .
```

**Expected:**
- HTTP 200
- Body: `{ "data": { "healthy": true, "latency_ms": 234 } }` if key is valid
- Body: `{ "data": { "healthy": false, "error": "..." } }` if key is invalid or provider is down

---

### TC-PROV-003: Update provider — set a custom base URL
**Priority:** P2

```bash
curl -s -X PATCH \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"base_url":"http://localhost:11434/v1"}' \
  BASE/admin/providers/<openai_provider_id> | jq .data.base_url
```

**Expected:**
- HTTP 200
- Response shows `"base_url": "http://localhost:11434/v1"`
- Gateway calls now route through this URL for this provider

---

### TC-PROV-004: Disable a provider and verify it is skipped
**Priority:** P1

```bash
# Disable provider A
curl -s -X PATCH \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"enabled":false}' \
  BASE/admin/providers/<provider_a_id>

# Make a request (should route to next enabled provider)
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions | jq .

# Check which provider handled it
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0].provider'
```

**Expected:**
- Gateway request returns HTTP 200
- Audit log shows provider B's name (not disabled provider A)

---

### TC-PROV-005: Provider quality score reflects performance
**Priority:** P2

After making some requests, check quality scores:

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/providers | \
  jq '.data[] | {name, quality_score, quality_updated_at}'
```

**Expected:**
- Each provider has `quality_score` between 0.0 and 1.0
- `quality_updated_at` is a recent timestamp (within the last 15 minutes)
- Default is 1.0 when there is no data yet

---

## Section 6 — Gateway: Chat Completions

### TC-GW-001: Non-streaming request returns OpenAI-compatible response
**Priority:** P0

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "Say exactly: Hello from Janus"}
    ]
  }' \
  BASE/v1/chat/completions | jq .
```

**Expected:**
- HTTP 200
- Response matches OpenAI format exactly:
  ```json
  {
    "id": "chatcmpl-...",
    "object": "chat.completion",
    "created": 1234567890,
    "model": "gpt-4o-mini",
    "choices": [{
      "index": 0,
      "message": {"role": "assistant", "content": "Hello from Janus"},
      "finish_reason": "stop"
    }],
    "usage": {"prompt_tokens": ..., "completion_tokens": ..., "total_tokens": ...}
  }
  ```
- Response headers include `X-Janus-Cache-Hit` and `X-Janus-Request-Id`

---

### TC-GW-002: Gateway rejects requests with no key or wrong key
**Priority:** P0

```bash
# No key
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions

# Invalid key
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer INVALID_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions
```

**Expected:** Both return `401`

---

### TC-GW-003: Admin JWT is NOT accepted on the gateway endpoint
**Priority:** P0

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions
```

**Expected:** `401` — admin JWTs must never work as gateway API keys

---

### TC-GW-004: Gateway API key is NOT accepted on admin endpoints
**Priority:** P0

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer <gw-key>" \
  BASE/admin/keys
```

**Expected:** `401` — gateway keys must never work on admin endpoints

---

### TC-GW-005: Streaming request returns Server-Sent Events
**Priority:** P0

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -N \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Count 1 2 3"}],"stream":true}' \
  BASE/v1/chat/completions
```

**Expected:**
- HTTP 200 with `Content-Type: text/event-stream`
- Multiple `data: {...}` lines arrive progressively (not all at once)
- Each chunk has `"object": "chat.completion.chunk"` with a `"delta"` field
- Stream ends with `data: [DONE]`

---

### TC-GW-006: Tool calls pass through and are stored in audit log
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role":"user","content":"What is the weather in London?"}],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get weather",
        "parameters": {
          "type": "object",
          "properties": {"city": {"type": "string"}},
          "required": ["city"]
        }
      }
    }],
    "tool_choice": "auto"
  }' \
  BASE/v1/chat/completions | jq '.choices[0].message.tool_calls'
```

Then verify audit:
```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0].tool_calls'
```

**Expected:**
- Response contains `tool_calls` array with `get_weather` call
- Audit log record has non-null `tool_calls` field containing the same data

---

### TC-GW-007: Every request is written to the audit log
**Priority:** P0

After making a gateway request, check the audit log:

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0] | {provider, model, status, prompt_tokens, cost_usd, latency_ms, cache_type}'
```

**Expected:**
- Record contains all of: `provider`, `model`, `status` ("success"), `prompt_tokens`, `completion_tokens`, `cost_usd` (non-zero decimal), `latency_ms` (integer), `cache_type` (null on a live call)

---

## Section 7 — Caching

### TC-CACHE-001: Second identical request is an exact cache hit
**Priority:** P0

```bash
# First request (cache miss)
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is 2+2?"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-cache"

# Second identical request (must be cache hit)
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is 2+2?"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-cache"
```

**Expected:**
- First request: `X-Janus-Cache-Hit: false` (or absent)
- Second request: `X-Janus-Cache-Hit: exact`
- Second request is noticeably faster (< 5 ms vs hundreds of ms)
- Second response body is identical to the first

---

### TC-CACHE-002: Cache bypass header forces a live provider call
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -H "X-Janus-Cache: false" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is 2+2?"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-cache"
```

**Expected:**
- `X-Janus-Cache-Hit: false` even though this prompt exists in the cache
- Response takes provider latency (not < 5 ms)

---

### TC-CACHE-003: Semantically similar request returns a semantic cache hit
**Priority:** P1

*Requires: embedding model loaded and semantic cache enabled*

```bash
# Seed the cache
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is the capital city of France?"}]}' \
  BASE/v1/chat/completions

# Similar phrasing
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Tell me the capital of France"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-cache"
```

**Expected:**
- Second request: `X-Janus-Cache-Hit: semantic`
- Second request: `X-Janus-Cache-Similarity: 0.9XXX` (value between 0.90 and 1.00)
- Second request arrives in under 10 ms

---

### TC-CACHE-004: Time-sensitive queries are never cached
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is the current price of Bitcoin today?"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-cache"
```

**Expected:**
- Header: `X-Janus-Cache-Skip: time_sensitive`
- No cache hit, even if an identical request was made before
- Live provider call every time

---

### TC-CACHE-005: Cache TTL — entry expires after the configured duration
**Priority:** P2

*Requires: `cache_ttl_secs = 5` in config*

```bash
# Make the request to populate cache
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"TTL expiry test"}]}' \
  BASE/v1/chat/completions

# Immediately repeat — should hit cache
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"TTL expiry test"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "cache-hit"

# Wait for TTL to expire
sleep 6

# Repeat — must be a cache miss now
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"TTL expiry test"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "cache-hit"
```

**Expected:**
- Immediate repeat: `X-Janus-Cache-Hit: exact`
- After 6 seconds: `X-Janus-Cache-Hit: false` — TTL expired, live call made

---

### TC-CACHE-006: Cache stats endpoint returns correct numbers
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/cache/stats | jq .
```

**Expected:**
- HTTP 200
- Response contains: `entries` (integer ≥ 0), `hits` (integer ≥ 0), `tokens_saved` (integer), `cost_saved_usd` (decimal)
- After TC-CACHE-001, `hits` should be at least 1

---

### TC-CACHE-007: Flush cache clears all entries including semantic
**Priority:** P1

```bash
# Flush
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/admin/cache | jq .

# Stats should now be zero
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/cache/stats | jq '.data.entries'

# A previously cached request must be a miss now
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is 2+2?"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "cache-hit"
```

**Expected:**
- Flush returns HTTP 200 with count of flushed entries
- Stats show 0 entries
- Repeat of a formerly-cached request is a miss (live call)
- Semantic cache is also cleared: a semantically similar query to a pre-flush entry is also a miss

---

### TC-CACHE-008: Delete a single cache entry by ID
**Priority:** P2

```bash
# Get entry ID from stats or request log
ENTRY_ID="<cache_entry_uuid>"

curl -s -X DELETE \
  -H "Authorization: Bearer <jwt>" \
  BASE/admin/cache/entries/$ENTRY_ID | jq .

# The specific request should now miss
```

**Expected:**
- HTTP 200
- That specific cached request is now a miss on next call

---

### TC-CACHE-009: Concurrent identical requests result in one provider call
**Priority:** P2

*Flush cache first. This tests in-flight deduplication.*

```bash
# Fire 5 identical requests in parallel
for i in {1..5}; do
  curl -s -X POST -H "Authorization: Bearer <gw-key>" \
    -H "Content-Type: application/json" \
    -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"UNIQUE DEDUP PHRASE 99XYZ"}]}' \
    BASE/v1/chat/completions &
done
wait

# Count how many request records were logged
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=10" | \
  jq '[.data[] | select(.request_body // "" | contains("UNIQUE DEDUP PHRASE 99XYZ"))] | length'
```

**Expected:**
- All 5 HTTP responses are 200 with identical content
- Audit log has 1 or 2 records, not 5 — the other 4 were deduplicated waiters that received the primary's response

---

## Section 8 — Rate Limiting

### TC-RATE-001: RPM rate limit blocks at the configured threshold
**Priority:** P0

*Create a key with `rate_limit_rpm: 2` for this test*

```bash
for i in 1 2 3; do
  HTTP=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST -H "Authorization: Bearer <rate_limited_gw_key>" \
    -H "Content-Type: application/json" \
    -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
    BASE/v1/chat/completions)
  echo "Request $i: $HTTP"
done
```

**Expected:**
- Requests 1 and 2: `200`
- Request 3: `429`

---

### TC-RATE-002: 429 response includes Retry-After header
**Priority:** P0

```bash
curl -s -X POST -H "Authorization: Bearer <rate_limited_gw_key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "retry-after"
```

*Run this after the key is already rate-limited from TC-RATE-001*

**Expected:**
- `Retry-After: <N>` header is present in the 429 response
- N is a positive integer (seconds until the window resets)
- Response body: `{ "error": { "code": "RATE_LIMIT_EXCEEDED", "message": "..." } }`

---

### TC-RATE-003: TPM (tokens per minute) limit is enforced
**Priority:** P2

*Create a key with `rate_limit_tpm: 50`*

Send a large prompt that uses more than 50 tokens.

**Expected:**
- HTTP 429 once the token budget is exhausted
- Error mentions token-per-minute limit

---

## Section 9 — Budget Enforcement

### TC-BUDGET-001: Requests are blocked when budget is fully spent
**Priority:** P0

*Create a key with `budget_limit: 0.000001`*

```bash
curl -s -X POST -H "Authorization: Bearer <budget_exhausted_key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions | jq .
```

**Expected:**
- HTTP 429
- Body: `{ "error": { "code": "BUDGET_EXCEEDED", "message": "..." } }`
- No provider call is made (check audit log — no new record)

---

### TC-BUDGET-002: Budget downgrade triggers at the configured threshold
**Priority:** P2

*Create a key with `budget_limit: 1.00`, `downgrade_at_percent: 80`, `downgrade_strategy: "cost_optimized"`. Spend $0.80+ with this key first.*

```bash
curl -s -X POST -H "Authorization: Bearer <downgrade_key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions -D - 2>&1 | grep -i "x-janus-downgraded"
```

**Expected:**
- HTTP 200 (not blocked)
- Response header: `X-Janus-Downgraded: cost_optimized`
- Audit log shows a cheaper provider or model was actually used (not `gpt-4o`)

---

## Section 10 — Provider Routing & Failover

### TC-ROUTING-001: Priority routing uses the highest-priority enabled provider
**Priority:** P1

```bash
# Make a request and check which provider handled it
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"routing test"}]}' \
  BASE/v1/chat/completions > /dev/null

curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0].provider'
```

**Expected:**
- The provider with the lowest `priority` number (highest priority) handled the request

---

### TC-ROUTING-002: Cost-optimized routing picks the cheaper provider
**Priority:** P2

*Create a key with `routing_strategy: "cost_optimized"`. Multiple providers must be configured with different pricing for the same model.*

Make 5 requests and check the audit log.

**Expected:**
- All 5 requests use the provider with the lower per-token cost for the requested model

---

### TC-ROUTING-003: Round-robin distributes requests across providers
**Priority:** P2

*Create a key with `routing_strategy: "round_robin"`. At least 2 providers enabled.*

Make 10 requests and check the audit log.

**Expected:**
- Requests are distributed across all enabled providers
- No single provider handles all 10 requests

---

### TC-ROUTING-004: Model fallback chain is used when primary provider fails
**Priority:** P2

*Configure a fallback chain: `"gpt-4o" = ["gpt-4o-mini"]`. Set up so that the primary provider for gpt-4o is unavailable.*

```bash
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"fallback test"}]}' \
  BASE/v1/chat/completions | jq .

curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0].model'
```

**Expected:**
- HTTP 200 — request succeeds
- Audit log shows `gpt-4o-mini` was actually used (fallback model)

---

### TC-ROUTING-005: Circuit breaker skips a failing provider after threshold
**Priority:** P2

*Configure a provider with an invalid API key to force repeated failures.*

Make 5+ requests that all fail on that provider. Then check if subsequent requests skip it.

**Expected:**
- After the failure threshold, the failing provider is skipped
- Requests route to the next available provider automatically
- No 503 returned as long as another provider is available

---

## Section 11 — Analytics & Cost Tracking

### TC-ANALYTICS-001: Analytics overview returns summary data
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/analytics/overview | jq .
```

**Expected:**
- HTTP 200
- Response includes: `total_requests`, `total_cost_usd`, `cache_hit_rate`, `error_rate`
- Values are non-zero if requests have been made

---

### TC-ANALYTICS-002: Daily costs aggregate correctly
**Priority:** P0

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/analytics/costs?period=today" | jq '.data'
```

**Expected:**
- HTTP 200
- Response shows today's date with `request_count` ≥ number of requests made today
- `total_cost_usd` matches the sum of individual request costs (within rounding)

---

### TC-ANALYTICS-003: Export requests as CSV
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  BASE/admin/requests/export -o requests.csv
head -3 requests.csv
```

**Expected:**
- HTTP 200
- `Content-Disposition: attachment` header
- `Content-Type: text/csv`
- First row is a header row with column names (`id`, `model`, `provider`, `cost_usd`, etc.)
- Data rows follow, one per request

---

### TC-ANALYTICS-004: Cost simulator shows savings estimate
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/analytics/simulate?strategy=cost_optimized&period=30d" | jq .
```

**Expected:**
- HTTP 200
- Response contains: `original_cost_usd`, `simulated_cost_usd`, `savings_usd`, `savings_percent`, `request_count`
- Per-model breakdown present

---

### TC-ANALYTICS-005: Cost is attributed by tag dimension
**Priority:** P2

```bash
# Send a tagged request via metadata field
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"tag test"}],"metadata":{"team":"backend"}}' \
  BASE/v1/chat/completions > /dev/null

# Send a tagged request via header
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -H "X-Janus-Tags: team=ml" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ml test"}]}' \
  BASE/v1/chat/completions > /dev/null

# Query breakdown
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/analytics/cost?group_by=tag.team&period=7d" | jq .data.groups
```

**Expected:**
- Response shows groups with keys `"backend"` and `"ml"`, each with their own `cost_usd` and `request_count`
- Tags from `X-Janus-Tags` header work the same as `metadata` field
- Both tags are stored in the audit log `tags` field

---

## Section 12 — Audit Log & Request Filtering

### TC-AUDIT-001: Paginated request list with metadata
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?page=1&per_page=10" | jq '.meta'
```

**Expected:**
- HTTP 200
- `meta` contains: `page`, `per_page`, `total`
- `total` matches total number of requests

---

### TC-AUDIT-002: Filter by status
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?status=error" | jq '[.data[].status] | unique'
```

**Expected:**
- Only `"error"` status requests returned

---

### TC-AUDIT-003: Filter by date range
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?start_time=2026-01-01T00:00:00Z&end_time=2026-12-31T23:59:59Z" | \
  jq '.meta.total'
```

**Expected:**
- Only requests within the given date range returned
- Requests outside the range excluded

---

### TC-AUDIT-004: Filter by API key
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?api_key_id=<key_uuid>" | jq '.data | length'
```

**Expected:**
- Only requests made with that specific API key are returned

---

### TC-AUDIT-005: Response includes tamper-evident hash header
**Priority:** P2

```bash
curl -s -I -H "Authorization: Bearer <jwt>" BASE/admin/requests | grep -i "x-janus-audit-hash"
```

**Expected:**
- Header `X-Janus-Audit-Hash: <sha256hex>` is present
- The hex value can be independently verified: `echo -n "<response_body>" | sha256sum`

---

### TC-AUDIT-006: Complete request record contains all expected fields
**Priority:** P0

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0] | keys | sort'
```

**Expected:**
All of these fields are present in every request record:
`id`, `created_at`, `provider`, `model`, `status`, `request_type`, `endpoint`,
`prompt_tokens`, `completion_tokens`, `total_tokens`,
`cost_usd`, `latency_ms`, `ttfb_ms`,
`cache_type`, `cache_similarity`,
`tool_calls`, `tags`,
`is_playground`, `replay_of_request_id`, `prompt_version_id`

---

## Section 13 — Request Replay & Admin Playground

### TC-REPLAY-001: Replay a past request creates a new audit record
**Priority:** P1

```bash
# Get a past request ID
REQUEST_ID=$(curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq -r '.data[0].id')

# Replay it
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{}' \
  BASE/admin/requests/$REQUEST_ID/replay | jq .
```

**Expected:**
- HTTP 200
- Response contains `new_request_id` (different UUID from the original), provider used, latency, cost
- Original request record is unchanged (check it)
- New request record has `replay_of_request_id` = original ID

---

### TC-REPLAY-002: Replay with skip_cache forces a live call
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"skip_cache": true}' \
  BASE/admin/requests/$REQUEST_ID/replay -D - 2>&1 | grep -i "cache-hit"
```

**Expected:**
- `X-Janus-Cache-Hit: false` — cache was bypassed

---

### TC-REPLAY-003: Replay with provider override uses the specified provider
**Priority:** P2

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"provider_id": "<other_provider_uuid>"}' \
  BASE/admin/requests/$REQUEST_ID/replay | jq .data.provider
```

**Expected:**
- Response shows the overridden provider name (not the original)

---

### TC-REPLAY-004: Replay of non-existent request returns 404
**Priority:** P1

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer <jwt>" \
  -d '{}' -H "Content-Type: application/json" \
  BASE/admin/requests/00000000-0000-0000-0000-000000000000/replay
```

**Expected:** `404`

---

### TC-PLAYGROUND-001: Admin playground responds without budget or rate limit checks
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Playground test"}]}' \
  BASE/admin/playground | jq .
```

**Expected:**
- HTTP 200 with a valid completion response
- Response includes extended metadata headers: provider, latency, cost, cache hit status
- Audit log shows this request with `is_playground: true`

---

### TC-PLAYGROUND-002: Playground is inaccessible with a gateway key
**Priority:** P0

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/admin/playground
```

**Expected:** `401`

---

## Section 14 — Alerts & Notifications

### TC-ALERTS-001: Create a spend threshold alert
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "High Spend Alert",
    "alert_type": "spend_threshold",
    "threshold": 10.00,
    "window_minutes": 1440,
    "webhook_url": "https://httpbin.org/post",
    "webhook_format": "generic",
    "enabled": true
  }' \
  BASE/admin/alerts | jq .data
```

**Expected:**
- HTTP 201
- Response echoes all submitted fields, plus `id` and `created_at`

---

### TC-ALERTS-002: Alert types accepted — error rate and latency spike
**Priority:** P1

```bash
# Error rate alert
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"Error Alert","alert_type":"error_rate","threshold":0.05,"window_minutes":60,"enabled":true}' \
  BASE/admin/alerts | jq '.data.alert_type'

# Latency spike alert
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"Latency Alert","alert_type":"latency_spike","threshold":2000,"window_minutes":15,"enabled":true}' \
  BASE/admin/alerts | jq '.data.alert_type'
```

**Expected:**
- Both return HTTP 201
- `alert_type` fields match what was submitted

---

### TC-ALERTS-003: Test webhook fires a delivery immediately
**Priority:** P1

```bash
ALERT_ID=$(curl -s -H "Authorization: Bearer <jwt>" BASE/admin/alerts | jq -r '.data[0].id')

curl -s -X POST -H "Authorization: Bearer <jwt>" \
  BASE/admin/alerts/$ALERT_ID/test | jq .
```

**Expected:**
- HTTP 200
- If using `https://httpbin.org/post`, the test payload was delivered
- Alert history now shows one entry with `triggered_at` set

---

### TC-ALERTS-004: Alert fires when threshold is breached
**Priority:** P1

*Create an alert with very low threshold (e.g., `spend_threshold: 0.001`). Make requests to exceed it. Wait up to 60 seconds for the background task.*

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  BASE/admin/alerts/$ALERT_ID | jq '.data.last_triggered'
```

**Expected:**
- `last_triggered` is not null after the background task runs
- Alert history shows the firing with `delivered: true` (if webhook is reachable)

---

### TC-ALERTS-005: Alert cooldown prevents repeated firings
**Priority:** P2

After an alert fires, verify it does not fire again within the same `window_minutes` period.

**Expected:**
- Alert history shows only one entry per cooldown window, not multiple entries for the same continuous breach

---

### TC-ALERTS-006: Disabled alert does not fire
**Priority:** P1

```bash
# Disable the alert
curl -s -X PATCH \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}' \
  BASE/admin/alerts/$ALERT_ID

# Breach the threshold — then check history
```

**Expected:**
- `last_triggered` remains unchanged after the threshold is breached while the alert is disabled

---

### TC-ALERTS-007: Slack native format alert delivers a Block Kit message
**Priority:** P2

*Configure alert with `slack_webhook_url` pointing to a real Slack incoming webhook.*

```bash
curl -s -X POST -H "Authorization: Bearer <jwt>" \
  BASE/admin/alerts/$ALERT_ID/test
```

**Expected:**
- A Slack message appears in the configured channel
- Message uses Block Kit format: header block + section block with fields (key name, spend, workspace, timestamp)

---

### TC-ALERTS-008: Email alert is delivered via SMTP
**Priority:** P2

*Set SMTP config in `janus.toml`. Configure alert with `email_to` field.*

Trigger the alert (or use test button). Check the inbox.

**Expected:**
- Email received with subject containing "Janus Alert"
- Body contains: alert name, condition triggered, current value, link to dashboard
- Both plain text and HTML parts present

---

### TC-ALERTS-009: Delete alert removes it
**Priority:** P1

```bash
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/admin/alerts/$ALERT_ID
curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer <jwt>" BASE/admin/alerts/$ALERT_ID
```

**Expected:**
- Delete: HTTP 200 or 204
- Subsequent GET: `404`

---

## Section 15 — Prompt Management

### TC-PROMPTS-001: Create a prompt and first version
**Priority:** P1

```bash
# Create prompt
PROMPT_ID=$(curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"greeting","description":"A greeting template"}' \
  BASE/admin/prompts | jq -r '.data.id')

# Create active version
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"content":"Say hello to {{name}} in a friendly way.","system_prompt":"You are a friendly greeter.","is_active":true,"ab_weight":100}' \
  BASE/admin/prompts/$PROMPT_ID/versions | jq .data
```

**Expected:**
- Prompt creation: HTTP 201, returns `id`
- Version creation: HTTP 201, returns `version: 1`, `is_active: true`

---

### TC-PROMPTS-002: Use prompt in gateway request via header with variable substitution
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -H "X-Janus-Prompt: $PROMPT_ID" \
  -H 'X-Janus-Variables: {"name": "Alice"}' \
  -d '{"model":"gpt-4o-mini","messages":[]}' \
  BASE/v1/chat/completions | jq '.choices[0].message.content'
```

**Expected:**
- HTTP 200
- Provider received the rendered prompt "Say hello to Alice in a friendly way."
- Response is a greeting addressed to Alice
- Audit log shows `prompt_version_id` is not null

---

### TC-PROMPTS-003: Activating a new version deactivates the previous one
**Priority:** P1

```bash
# Create a second version
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"content":"Greet {{name}} warmly.","is_active":true,"ab_weight":100}' \
  BASE/admin/prompts/$PROMPT_ID/versions

# Check that version 1 is now inactive
curl -s -H "Authorization: Bearer <jwt>" \
  BASE/admin/prompts/$PROMPT_ID | jq '.data.versions[] | {version, is_active}'
```

**Expected:**
- Version 1: `is_active: false`
- Version 2: `is_active: true`

---

### TC-PROMPTS-004: A/B testing distributes traffic by weight
**Priority:** P2

*Set two versions to 50/50 weight. Both `is_active: true`.*

Make 10 requests using the prompt. Check which version was selected each time.

```bash
curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=10" | \
  jq '[.data[].prompt_version_id] | group_by(.) | map({version: .[0], count: length})'
```

**Expected:**
- Both version UUIDs appear in the results
- Distribution is roughly 50/50 (±4 requests tolerance due to randomness)

---

### TC-PROMPTS-005: Unknown prompt ID returns 404
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -H "X-Janus-Prompt: 00000000-0000-0000-0000-000000000000" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions | jq .error.code
```

**Expected:** `404` response with a clear error message

---

### TC-PROMPTS-006: Delete prompt cascades to all versions
**Priority:** P1

```bash
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/admin/prompts/$PROMPT_ID
curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer <jwt>" BASE/admin/prompts/$PROMPT_ID
```

**Expected:**
- Delete: HTTP 200 or 204
- Subsequent GET: `404`

---

## Section 16 — RBAC & Workspaces

### TC-RBAC-001: Admin role can access every endpoint
**Priority:** P0

Test `GET /admin/analytics/overview`, `POST /admin/keys`, `DELETE /admin/cache`, `PATCH /admin/config`, `POST /admin/providers/:id/test` — all must succeed.

**Expected:** All return HTTP 200 or 201 (no 403)

---

### TC-RBAC-002: BillingViewer can read analytics, cannot mutate
**Priority:** P1

*Add a user as `billing_viewer` in a workspace. Login as that user.*

```bash
# Must succeed (read)
curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer <billing_viewer_jwt>" \
  BASE/admin/analytics/overview

# Must fail (write)
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer <billing_viewer_jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"key"}' BASE/admin/keys
```

**Expected:**
- Analytics read: `200`
- Key create: `403`

---

### TC-RBAC-003: ApiManager can create keys, cannot delete cache
**Priority:** P1

```bash
# Must succeed
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer <api_manager_jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name":"mgr key"}' BASE/admin/keys

# Must fail
curl -s -o /dev/null -w "%{http_code}" \
  -X DELETE -H "Authorization: Bearer <api_manager_jwt>" \
  BASE/admin/cache
```

**Expected:**
- Key create: `201`
- Cache delete: `403`

---

### TC-RBAC-004: ReadOnly role cannot mutate anything
**Priority:** P1

*User with `read_only` role.*

Try: `POST /admin/keys`, `PATCH /admin/config`, `DELETE /admin/cache`, `POST /admin/providers/:id/test`

**Expected:** All return `403`

---

### TC-RBAC-005: Cross-workspace access is denied
**Priority:** P1

*User is admin of workspace A only. Try accessing workspace B's resources.*

**Expected:** `403` on any attempt to access or modify workspace B

---

### TC-RBAC-006: Add, change role, and remove a workspace member
**Priority:** P1

```bash
WS_ID="<workspace_uuid>"
USER_ID="<target_user_uuid>"

# Add member
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"user_id":"'$USER_ID'","role":"billing_viewer"}' \
  BASE/admin/workspaces/$WS_ID/members | jq '.data.role'

# Update role
curl -s -X PATCH \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"role":"api_manager"}' \
  BASE/admin/workspaces/$WS_ID/members/$USER_ID | jq '.data.role'

# Remove
curl -s -X DELETE -H "Authorization: Bearer <jwt>" \
  BASE/admin/workspaces/$WS_ID/members/$USER_ID

# Verify removed member has no access
curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer <that_users_jwt>" \
  BASE/admin/analytics/overview
```

**Expected:**
- Add: HTTP 201, `"billing_viewer"`
- Update: HTTP 200, `"api_manager"`
- Remove: HTTP 200 or 204
- Access after removal: `403`

---

### TC-RBAC-007: Bootstrap rule — no members means every authenticated user is admin
**Priority:** P2

*On a fresh installation where `workspace_members` is empty.*

Login and try any admin-only endpoint.

**Expected:** HTTP 200 — all authenticated users are admin when no memberships exist

---

## Section 17 — Configuration Management

### TC-CONFIG-001: Get current runtime configuration
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/config | jq .data
```

**Expected:**
- HTTP 200
- Response contains all configurable fields:
  `log_request_bodies`, `log_response_bodies`, `cache_enabled`, `semantic_cache_threshold`, `rate_limit_window_secs`, `max_retries`, `prometheus_enabled`

---

### TC-CONFIG-002: Patch a config value — takes effect immediately
**Priority:** P1

```bash
# Change threshold
curl -s -X PATCH \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"semantic_cache_threshold": 0.85}' \
  BASE/admin/config | jq '.data.semantic_cache_threshold'

# Verify persisted
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/config | jq '.data.semantic_cache_threshold'
```

**Expected:**
- Both calls return `0.85`
- No server restart required for the change to apply

---

### TC-CONFIG-003: Config follows the override hierarchy
**Priority:** P2

| Source | Priority |
|---|---|
| Environment variable | Highest |
| `janus.toml` file | Middle |
| Code defaults | Lowest |

Set `SEMANTIC_CACHE_THRESHOLD=0.95` in the environment AND `semantic_cache_threshold = 0.80` in `janus.toml`.

**Expected:** `GET /admin/config` shows `0.95` (env wins)

---

## Section 18 — Extended Gateway APIs

### TC-MODELS-001: Models endpoint returns an aggregated list
**Priority:** P1

```bash
curl -s -H "Authorization: Bearer <gw-key>" BASE/v1/models | jq '.data | length'
```

**Expected:**
- HTTP 200
- Response matches OpenAI format: `{ "object": "list", "data": [ {"id": "gpt-4o-mini", "object": "model", ...}, ... ] }`
- Models from all enabled providers are included
- Results are cached for 5 seconds (two rapid calls return the same data without hitting providers twice)

---

### TC-MODELS-002: Embeddings return vectors in OpenAI format
**Priority:** P1

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"text-embedding-3-small","input":"Hello world"}' \
  BASE/v1/embeddings | jq '{object, data_length: (.data | length), vector_length: (.data[0].embedding | length)}'
```

**Expected:**
- HTTP 200
- `"object": "list"`, `data_length: 1`, `vector_length: 1536`
- Response contains `usage.prompt_tokens`
- Cost is logged in the audit record

---

### TC-MODELS-003: Image generation passes through to provider
**Priority:** P2

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"dall-e-3","prompt":"A red circle","n":1,"size":"1024x1024"}' \
  BASE/v1/images/generations | jq '.data[0].url'
```

**Expected:**
- HTTP 200
- Response contains a URL to the generated image
- Per-image cost is logged in the audit record (not zero)

---

### TC-MODELS-004: Audio transcription accepts a multipart file upload
**Priority:** P2

*Requires a sample audio file.*

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -F "model=whisper-1" \
  -F "file=@sample.mp3" \
  BASE/v1/audio/transcriptions | jq .text
```

**Expected:**
- HTTP 200
- `text` field contains the spoken content of the audio

---

### TC-MODELS-005: Audio speech returns binary audio content
**Priority:** P2

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"tts-1","input":"Hello from Janus","voice":"alloy"}' \
  BASE/v1/audio/speech --output speech.mp3

file speech.mp3
```

**Expected:**
- HTTP 200
- Binary file returned (not JSON)
- `file speech.mp3` identifies it as MPEG audio or similar

---

## Section 19 — PII Scrubbing & Security

### TC-PII-001: Credit card numbers are redacted before storage
**Priority:** P1

*Requires `log_request_bodies: true` in config*

```bash
curl -s -X POST \
  -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"My card is 4111-1111-1111-1111"}]}' \
  BASE/v1/chat/completions > /dev/null

curl -s -H "Authorization: Bearer <jwt>" \
  "BASE/admin/requests?limit=1" | jq '.data[0].request_body'
```

**Expected:**
- The stored `request_body` does NOT contain `4111-1111-1111-1111`
- The value is replaced with `[REDACTED]` or similar
- The actual response from the provider was still generated (scrubbing does not block the request)

---

### TC-PII-002: Email addresses are redacted before storage
**Priority:** P2

Send a message containing `contact john@example.com`. Check the stored request body.

**Expected:** `john@example.com` is replaced with `[REDACTED]`

---

### TC-PII-003: Bearer tokens in message content are redacted
**Priority:** P2

Send a message like `my token is sk-abcdef1234567890`. Check stored request body.

**Expected:** The token value is redacted, not stored in plaintext

---

### TC-PII-004: Provider API keys are encrypted at rest in the database
**Priority:** P0

Connect directly to PostgreSQL and inspect:

```sql
SELECT api_key FROM providers WHERE name = 'openai';
```

**Expected:**
- The value is NOT the plaintext OpenAI API key
- It is encrypted ciphertext (AES-256-GCM)
- The plaintext key is never stored anywhere in the database

---

### TC-SEC-001: Webhook signature can be independently verified
**Priority:** P2

*Configure an alert with `webhook_secret` set. Receive the webhook.*

Verify: `HMAC-SHA256(secret, body) == value in X-Janus-Signature header`

**Expected:** Signatures match — the webhook body has not been tampered with

---

### TC-SEC-002: mTLS startup validation rejects invalid cert paths
**Priority:** P2

Set `provider_tls.ca_cert_path = "/nonexistent/cert.pem"` in config. Attempt to start Janus.

**Expected:**
- Startup fails with a clear error message about the missing cert file
- Server does NOT start silently

---

### TC-SEC-003: API key rotation grace period works correctly
**Priority:** P1

See TC-KEYS-004. After the grace period expires (default 300s), the old key must be rejected.

```bash
sleep 310  # wait for grace period
curl -s -o /dev/null -w "%{http_code}" \
  -X POST -H "Authorization: Bearer <old_key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"test"}]}' \
  BASE/v1/chat/completions
```

**Expected:** `401`

---

## Section 20 — Prometheus Metrics

### TC-METRICS-001: Metrics endpoint returns Prometheus format
**Priority:** P1

```bash
curl -s BASE/metrics | head -20
```

**Expected:**
- HTTP 200
- `Content-Type: text/plain; version=0.0.4`
- Output contains lines like:
  ```
  # HELP janus_exact_cache_size Number of exact cache entries
  janus_exact_cache_size 42
  # HELP janus_cache_hit_ratio Current cache hit ratio
  janus_cache_hit_ratio 0.73
  ```

---

### TC-METRICS-002: Gauge values update after activity
**Priority:** P2

```bash
BEFORE=$(curl -s BASE/metrics | grep janus_exact_cache_size | awk '{print $2}')

# Create a new cache entry
curl -s -X POST -H "Authorization: Bearer <gw-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"METRICS-UPDATE-TEST"}]}' \
  BASE/v1/chat/completions > /dev/null

AFTER=$(curl -s BASE/metrics | grep janus_exact_cache_size | awk '{print $2}')
echo "Before: $BEFORE, After: $AFTER"
```

**Expected:**
- `AFTER` is greater than `BEFORE` by exactly 1

---

## Section 21 — OpenAPI & Swagger UI

### TC-OPENAPI-001: OpenAPI spec is accessible without authentication
**Priority:** P1

```bash
curl -s -o /dev/null -w "%{http_code}" BASE/admin/openapi.json
curl -s BASE/admin/openapi.json | jq .openapi
```

**Expected:**
- HTTP 200 (no auth required)
- `"openapi": "3.1.0"` (or 3.0.x)

---

### TC-OPENAPI-002: Spec contains all major endpoint paths
**Priority:** P1

```bash
curl -s BASE/admin/openapi.json | jq '.paths | keys | .[]' | sort
```

**Expected:**
All of the following paths are present (at minimum):
`/v1/chat/completions`, `/v1/embeddings`, `/v1/models`, `/v1/images/generations`, `/v1/audio/transcriptions`, `/v1/audio/speech`,
`/admin/keys`, `/admin/providers`, `/admin/requests`, `/admin/analytics/overview`,
`/admin/alerts`, `/admin/cache`, `/admin/prompts`, `/admin/config`, `/admin/workspaces`

---

### TC-OPENAPI-003: Swagger UI loads and is interactive
**Priority:** P1

Open `BASE/admin/docs` in a web browser.

**Expected:**
- Page loads with the Swagger UI interface (not a 404 or blank page)
- All API endpoint groups are listed
- "Try it out" button works on individual endpoints
- "Authorize" button shows `Bearer JWT` and `API Key` security schemes

---

## Section 22 — CLI

### TC-CLI-001: Help text lists all subcommands
**Priority:** P1

```bash
./target/debug/janus --help
```

**Expected:**
Output lists all subcommands: `serve`, `doctor`, `demo`, `keys`, `migrate`, `config`, `import`, `backup`

---

### TC-CLI-002: `keys list` shows all API keys
**Priority:** P1

```bash
JANUS_URL=http://localhost:8080 \
JANUS_ADMIN_TOKEN=<jwt> \
./target/debug/janus keys list
```

**Expected:**
- Table output with: key name, ID, prefix, budget, rate limits
- Same data as `GET /admin/keys`

---

### TC-CLI-003: `keys create` creates a key and shows it once
**Priority:** P1

```bash
JANUS_URL=http://localhost:8080 \
JANUS_ADMIN_TOKEN=<jwt> \
./target/debug/janus keys create --name "CLI Key" --budget 25.00
```

**Expected:**
- Full `jn-sk-...` key printed in terminal
- Warning that the key is shown only once
- Key appears in subsequent `janus keys list` output

---

### TC-CLI-004: `migrate status` shows all migrations and their state
**Priority:** P1

```bash
JANUS_URL=http://localhost:8080 \
JANUS_ADMIN_TOKEN=<jwt> \
./target/debug/janus migrate status
```

**Expected:**
- Table showing migration filename, applied yes/no, applied timestamp
- On a running production instance, all migrations are shown as applied

---

### TC-CLI-005: `config get` and `config set` work correctly
**Priority:** P1

```bash
JANUS_URL=http://localhost:8080 JANUS_ADMIN_TOKEN=<jwt> \
./target/debug/janus config get semantic_cache_threshold
# prints current value, e.g. 0.90

JANUS_URL=http://localhost:8080 JANUS_ADMIN_TOKEN=<jwt> \
./target/debug/janus config set semantic_cache_threshold=0.92
# prints confirmation

# Verify via API
curl -s -H "Authorization: Bearer <jwt>" BASE/admin/config | jq .data.semantic_cache_threshold
```

**Expected:**
- `get` prints `0.90`
- `set` confirms the change
- API shows `0.92`

---

### TC-CLI-006: `import litellm --dry-run` parses and shows migration plan without applying
**Priority:** P2

```bash
./target/debug/janus import litellm \
  --file tests/fixtures/v5_2/litellm-sample.yaml \
  --dry-run
```

**Expected:**
- Output shows a readable migration plan: providers to configure, keys to create, cache settings
- **No changes made** to the running instance

---

### TC-CLI-007: `backup create` produces a valid archive
**Priority:** P1

```bash
./target/debug/janus backup create --output janus_backup.tar.gz
ls -lh janus_backup.tar.gz
```

**Expected:**
- `.tar.gz` file is created
- File is non-empty

---

### TC-CLI-008: `backup inspect` shows archive contents
**Priority:** P1

```bash
./target/debug/janus backup inspect janus_backup.tar.gz
```

**Expected:**
- Output shows: `janus_version`, `schema_version`, `created_at`, `has_db_sql: true`

---

### TC-CLI-009: `backup restore` round-trips data
**Priority:** P2

```bash
./target/debug/janus backup restore \
  --file janus_backup.tar.gz \
  --database-url postgres://postgres:password@localhost/janus_restore_test
```

**Expected:**
- Restore completes without errors
- Target database contains the tables and data from the backup

---

### TC-CLI-010: Version-incompatible archive is rejected on restore
**Priority:** P2

*Modify a backup's `VERSION` file to claim a newer schema version than the binary supports.*

```bash
./target/debug/janus backup restore --file tampered_backup.tar.gz
```

**Expected:**
- Restore is refused with a clear error: "archive was created with a newer schema version"

---

## Section 23 — OIDC / SSO

### TC-OIDC-001: Configure an OIDC identity provider
**Priority:** P2

```bash
curl -s -X POST \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Google SSO",
    "kind": "oidc",
    "config": {
      "discovery_url": "https://accounts.google.com",
      "client_id": "YOUR_CLIENT_ID",
      "client_secret": "YOUR_CLIENT_SECRET"
    },
    "enabled": true
  }' \
  BASE/admin/idp | jq .data.id
```

**Expected:**
- HTTP 201
- Returns the new IdP `id`

---

### TC-OIDC-002: OIDC login JIT-creates a user on first login
**Priority:** P2

1. Navigate to `BASE/auth/oidc/<idp_id>/start` in a browser
2. Complete the Google OAuth flow
3. Confirm you land on the Janus dashboard

```bash
curl -s -H "Authorization: Bearer <jwt>" BASE/api/v1/users | jq '.data | length'
```

**Expected:**
- A new user row exists for the Google account's email
- Dashboard session is active (JWT issued)
- No duplicate user on second login with the same Google account

---

### TC-OIDC-003: Disabled IdP returns 404
**Priority:** P2

```bash
curl -s -X DELETE -H "Authorization: Bearer <jwt>" BASE/admin/idp/<idp_id>
curl -s -o /dev/null -w "%{http_code}" BASE/auth/oidc/<idp_id>/start
```

**Expected:** `404`

---

## Section 24 — Dashboard UI

For each UI test, open the page in a browser. Confirm it renders correctly with real data, no blank panels, and no browser console errors.

---

### TC-UI-001: Login page — authenticates and redirects
**Priority:** P0

Navigate to `BASE/`. Enter correct credentials. Click Login.

**Expected:**
- Login form renders
- Correct credentials redirect to the dashboard overview
- Wrong credentials show an inline error, no redirect

---

### TC-UI-002: Overview page shows current stats
**Priority:** P0

**Expected:**
- Displays: today's request count, today's cost, cache hit rate
- Shows comparison to previous period
- Top API keys by cost
- Provider health summary
- No broken charts or empty panels

---

### TC-UI-003: Requests page — table with filters
**Priority:** P1

**Expected:**
- Paginated table: time, model, provider, status, latency, cost, cache
- Click a row → full detail (all audit fields)
- Filter by status (success/error) changes the list
- Date range picker works

---

### TC-UI-004: API Keys page — full lifecycle
**Priority:** P1

1. Create a key (fill in name, budget, rate limit, routing strategy)
2. Observe the full `jn-sk-...` key shown once
3. Close the dialog — key is shown as prefix only
4. Click Rotate — confirm and see the new key
5. Revoke a key

**Expected:**
- Full key shown only during creation and rotation
- Key list always shows prefix only
- Rotation shows a countdown for the grace period
- Revoked key disappears or shows revoked status

---

### TC-UI-005: Providers page — health, quality score, test button
**Priority:** P1

**Expected:**
- List with health indicator (green/yellow/red) per provider
- Quality score badge (0–100)
- "Test Connection" button returns latency in UI
- Editing a provider (API key, base_url, priority) saves without page reload

---

### TC-UI-006: Cache page — stats and flush
**Priority:** P1

**Expected:**
- Shows entry count, hit count, hit rate, tokens saved, cost saved
- Flush button opens a confirmation dialog
- After confirming, all stats reset to zero

---

### TC-UI-007: Analytics page — charts and filters
**Priority:** P1

**Expected:**
- Daily cost bar chart (30 days)
- Cost by model breakdown
- Cache hit rate over time
- Date range filter changes the chart data
- Cost by tag card: dimension selector (team, project, env), bar chart

---

### TC-UI-008: Alerts page — full alert lifecycle
**Priority:** P1

**Expected:**
- Create alert: form has name, type, threshold, window, webhook URL, Slack webhook URL, email recipients
- Test button sends a sample payload
- Alert history shows firings
- Toggle on/off works
- Delete removes alert

---

### TC-UI-009: Prompts page — version management and A/B weights
**Priority:** P1

**Expected:**
- Create prompt with a template (e.g., `Hello {{name}}`)
- Add second version
- Set 50/50 A/B weight sliders
- Template preview with sample variables renders correctly
- Activating a version deactivates the current one

---

### TC-UI-010: Settings page — all fields editable and save correctly
**Priority:** P1

**Expected:**
- All config fields are editable (not read-only)
- Save button calls `PATCH /admin/config`
- Refresh of the page shows the saved values

---

### TC-UI-011: Workspaces page — member management
**Priority:** P1

**Expected:**
- Workspace cards are expandable
- Member table shows name, email, role, date added
- Add member dialog has email input + role dropdown
- Role change works instantly in the table
- Remove member disappears from the table

---

### TC-UI-012: System Health page — readiness check dashboard
**Priority:** P1

**Expected:**
- Visual checklist of all checks (green/yellow/red per check)
- Auto-refresh every 30 seconds (or manual refresh button)
- Warning banner visible if any check fails

---

### TC-UI-013: Playground page — send a test request with full metadata
**Priority:** P1

**Expected:**
- Prompt composer with model selector, system prompt, user messages
- Stream toggle and skip cache toggle
- Submit → response appears + metadata: provider, latency, tokens, cost, cache hit
- Request appears in Requests log with `is_playground: true`

---

### TC-UI-014: Cost Simulator page — shows what-if savings
**Priority:** P1

**Expected:**
- Strategy selector and period selector
- Submit → bar chart: original cost vs simulated cost
- Per-model breakdown table with delta column

---

### TC-UI-015: Onboarding tour appears for new users and is dismissable
**Priority:** P2

*On a fresh user account that has not seen the tour.*

**Expected:**
- 4-step overlay tour appears on first login
- Steps: Create API key → Playground → Requests → Set alert
- Tour can be dismissed
- After dismissal or completion, tour does not appear again on next login

---

### TC-UI-016: SSO settings page — configure and test an OIDC provider
**Priority:** P2

Navigate to Settings → SSO.

**Expected:**
- Form: name, discovery URL, client ID, client secret
- "Test connection" button verifies discovery URL and shows result
- Table of configured IdPs with enable/disable toggle

---

## Section 25 — Deployment

### TC-DEPLOY-001: Helm chart lints without errors
**Priority:** P2

```bash
helm lint charts/janus
```

**Expected:** `1 chart(s) linted, 0 chart(s) failed`

---

### TC-DEPLOY-002: Helm template renders valid manifests
**Priority:** P2

```bash
helm template janus charts/janus \
  --set postgresql.url="postgres://pg:pw@host/janus" \
  --set secrets.jwtSecret="test-secret-at-least-32-chars" | head -50
```

**Expected:**
- YAML output, no errors
- Contains: Deployment, Service, ConfigMap, HPA, Ingress, ServiceMonitor

---

### TC-DEPLOY-003: Docker image builds and the binary starts
**Priority:** P1

```bash
docker build -t janus:test .
docker run --rm \
  -e DATABASE_URL=postgres://pg:pw@host/janus \
  -e JWT_SECRET=a-secret-at-least-32-characters-long \
  -e ENCRYPTION_KEY=0000000000000000000000000000000000000000000000000000000000000000 \
  janus:test janus doctor
```

**Expected:**
- Build completes without errors
- `janus doctor` inside the container runs and prints check results

---

## Quick Smoke Test — Run After Any Change

| # | Test | Pass condition |
|---|---|---|
| 1 | `GET BASE/health` | HTTP 200, `status: ok` |
| 2 | `POST /api/v1/auth/login` | HTTP 200, JWT returned |
| 3 | `POST /v1/chat/completions` with gateway key | HTTP 200, OpenAI format |
| 4 | Same request again | `X-Janus-Cache-Hit: exact` |
| 5 | Request with `rpm=2` key, 3rd call | HTTP 429 |
| 6 | Request with `budget_limit=0.000001` key | HTTP 429, `BUDGET_EXCEEDED` |
| 7 | Admin JWT on gateway endpoint | HTTP 401 |
| 8 | Gateway key on admin endpoint | HTTP 401 |
| 9 | `GET BASE/metrics` | HTTP 200, Prometheus text |
| 10 | `GET BASE/admin/openapi.json` | HTTP 200, `"openapi": "3.x.x"` |

---

## Bug Report Template

```
TC-ID:        e.g. TC-GW-001
Date:
Environment:  PostgreSQL / SQLite / Demo
Janus build:  git commit or binary version
Steps:        exact commands run
Expected:     from this document
Actual:       HTTP status, headers, response body — copy exactly
Severity:     P0 / P1 / P2
Notes:        anything unusual
```

---

*Total test cases: 110+  
Organized by feature domain, version-independent.  
Covers the complete Janus product as a single working system.*
