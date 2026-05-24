# VELOX V4 — Engineering Roadmap
> Built on V3 (all 6 phases complete).
> **If you are Claude: read CLAUDE.md first, then VELOX_V3_ROADMAP.md §11, then this file.**

---

## V4 Philosophy

V3 made the core correct. V4 makes the product complete.

V3 fixed bugs, hardened streaming, added observability, and locked the plugin model.
What remains is a combination of:

1. **Backend gaps** — features deferred from V2/V3 (cost intelligence, deduplication, RBAC)
2. **Dashboard gaps** — V2 features (Alerts, Prompts) built backend-only, no UI
3. **Operational readiness** — self-diagnosis, debug tooling, demo capability

> *After V4, there should be no feature with a backend API but no dashboard UI,
> no known technical debt, and no "planned for later" items in the backlog.*

**Rules for every V4 phase:**
1. Every phase must have a one-sentence answer to "what problem does this solve?"
2. Dashboard phases ship zero new Rust code. Backend phases ship zero new dashboard code.
3. Exception: V4-6 and V4-8 — backend and UI are tightly coupled and ship together.

---

## Table of Contents

1. [Complete Gap Audit](#1-complete-gap-audit)
2. [V4 Testing Philosophy](#2-v4-testing-philosophy)
3. [Phase V4-0: Foundation & DX](#3-phase-v4-0-foundation--dx)
4. [Phase V4-1: Dashboard Catch-up](#4-phase-v4-1-dashboard-catch-up)
5. [Phase V4-2: In-flight Request Deduplication](#5-phase-v4-2-in-flight-request-deduplication)
6. [Phase V4-3: Cache TTL + Time-sensitive Safety](#6-phase-v4-3-cache-ttl--time-sensitive-safety)
7. [Phase V4-4: Budget-Aware Auto-Downgrade](#7-phase-v4-4-budget-aware-auto-downgrade)
8. [Phase V4-5: Cost Simulator + Provider Quality Scoring](#8-phase-v4-5-cost-simulator--provider-quality-scoring)
9. [Phase V4-6: Request Replay & Debug Console](#9-phase-v4-6-request-replay--debug-console)
10. [Phase V4-7: Dashboard Overhaul](#10-phase-v4-7-dashboard-overhaul)
11. [Phase V4-8: RBAC / True Multi-tenancy](#11-phase-v4-8-rbac--true-multi-tenancy)
12. [Phase V4-9: External Vector Stores](#12-phase-v4-9-external-vector-stores)
13. [What V4 Explicitly Does NOT Include](#13-what-v4-explicitly-does-not-include)
14. [Migration Plan](#14-migration-plan)
15. [Dependency Plan](#15-dependency-plan)
16. [V4 Phase Status Tracker](#16-v4-phase-status-tracker)

---

## 1. Complete Gap Audit

Two types of gaps are tracked here: **backend** (missing Rust code) and **dashboard**
(backend exists, no UI). Both are resolved in V4 — no gap survives to V5.

### Backend Gaps

| Gap | Source | Phase |
|---|---|---|
| Provider `base_url` is hardcoded — Ollama/vLLM unreachable | V4 planning | V4-0 |
| No system readiness check / `velox doctor` | V4 planning | V4-0 |
| N identical concurrent requests hit provider N times | V4 planning | V4-2 |
| Cache entries never expire (no TTL) | V1 design | V4-3 |
| No time-sensitive query detection — "current price?" cached | V4 planning | V4-3 |
| Budget enforcement only blocks at 100% — no graceful downgrade | V2-0 design | V4-4 |
| No cost simulator — can't answer "what would this strategy have saved?" | V4 planning | V4-5 |
| Provider selection has no quality score | V4 planning | V4-5 |
| No request replay or admin playground | V4 planning | V4-6 |
| `workspaces` table exists but no RBAC, roles, or member scoping | V3 roadmap §9 | V4-8 |
| External vector stores deferred until scale demands | V3 roadmap §9 | V4-9 |

### Dashboard Gaps

| Gap | V2/V3 Phase That Built Backend | Dashboard Phase |
|---|---|---|
| Alerts management UI — all 6 endpoints have zero UI | V2-2 | V4-1 |
| Prompts management UI — versioning, A/B testing, all have zero UI | V2-5 | V4-1 |
| Settings page is read-only — `PATCH /admin/config` never called from UI | V2-0 | V4-1 |
| Routing strategy field missing from key create/edit form | V2-4 | V4-1 |
| Key rotation button missing — `POST /admin/keys/:id/rotate` has no UI | V3-5 | V4-1 |
| Playground / test console — backend in V4-6, UI needed | V4-6 | V4-7 |
| Cost simulator page — backend in V4-5, UI needed | V4-5 | V4-7 |
| System health panel — backend in V4-0, UI needed | V4-0 | V4-7 |
| Provider quality score indicators | V4-5 | V4-7 |
| Operational dashboard views for non-technical users | V4 planning | V4-7 |
| Member management UI for workspaces | V4-8 | V4-8 |

---

## 2. V4 Testing Philosophy

Inherits V2/V3 regression contract. Every phase gate-in and gate-out:

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

V4 backend tests live in `tests/v4/`. Dashboard phases have no Rust tests
(they are tested visually and via existing API integration tests).

```
tests/
├── v2/   ← must stay green, never touch
├── v3/   ← must stay green, never touch
└── v4/
    ├── common.rs
    ├── v4_0_foundation.rs
    ├── v4_2_dedup.rs
    ├── v4_3_cache_ttl.rs
    ├── v4_4_budget_downgrade.rs
    ├── v4_5_cost_sim.rs
    ├── v4_6_replay.rs
    ├── v4_8_rbac.rs
    └── v4_9_vector_store.rs
```

Test naming: `v4p{phase}_{feature}_{expected_outcome}`

---

## 3. Phase V4-0: Foundation & DX

**Goal**: Unlock Ollama/vLLM support, add system self-diagnosis, and make Velox
evaluable by anyone in under 5 minutes.

### 3.1 Configurable Provider `base_url`

**Problem**: `src/providers/openai.rs` hardcodes `https://api.openai.com/v1`.
Ollama, vLLM, LM Studio, and any OpenAI-compatible endpoint are blocked.

**Fix**: Add `base_url TEXT` to the `providers` table.
The OpenAI adapter reads it from the provider record, falling back to the
default when `NULL`. This is a config change, not a new adapter.

```sql
-- migrations/0019_provider_base_url.sql
ALTER TABLE providers ADD COLUMN base_url TEXT;
```

```rust
// src/providers/openai.rs
let base = provider_record.base_url
    .as_deref()
    .unwrap_or("https://api.openai.com/v1");
```

**Result**: Pointing Velox at a local Ollama instance requires one database
UPDATE — zero code changes, zero new adapters.

**Files to modify:** `src/providers/openai.rs`, `src/providers/anthropic.rs`

### 3.2 `velox doctor` — System Readiness Checks

**Problem**: Misconfigurations are only discovered at runtime under load.

**Fix**: A `--doctor` CLI flag and `GET /admin/system/readiness` endpoint.

**Checks:**

| Check | Pass Condition |
|---|---|
| Database reachable | `SELECT 1` within 2 seconds |
| Migrations current | Latest applied = latest file in migrations dir |
| JWT secret strength | `JWT_SECRET` ≥ 32 bytes |
| Encryption key set | `ENCRYPTION_KEY` set when any provider key is stored |
| Provider available | ≥ 1 provider enabled |
| Embedding model | Files exist at configured paths (if semantic cache enabled) |
| Disk space | ≥ 100 MB free |

```
velox --doctor

[✓] Database connection
[✓] Migrations up to date (0024)
[✗] JWT secret too short (12 bytes — minimum 32)
[✓] Encryption key present
[✓] 2 providers enabled
[!] Embedding model not found — semantic cache disabled

1 error, 1 warning.
```

`GET /admin/system/readiness` returns `200` when all pass, `503` on any failure.

### 3.3 Demo Mode

**Problem**: Evaluating Velox requires PostgreSQL, real API keys, and real LLM
calls — barrier to "see what this does" is too high.

**Fix**: `velox --demo` starts with SQLite in-memory, a mock provider, 2 pre-made
API keys, and 100 seeded historical requests.

```bash
velox --demo
# Velox demo mode at http://localhost:8080
# Login: admin@velox.local / demo-password
```

### New Files

- `src/doctor.rs` — readiness check implementations
- `src/handlers/admin/system.rs` — `GET /admin/system/readiness` handler
- `src/demo.rs` — mock provider, seed data, demo startup

### Test Contract

**File:** `tests/v4/v4_0_foundation.rs`

```rust
async fn v4p0_provider_uses_custom_base_url_when_set()
async fn v4p0_provider_falls_back_to_default_url_when_base_url_null()
async fn v4p0_doctor_passes_all_checks_on_valid_config()
async fn v4p0_doctor_fails_when_jwt_secret_too_short()
async fn v4p0_doctor_fails_when_no_providers_enabled()
async fn v4p0_doctor_warns_when_embedding_model_missing()
async fn v4p0_readiness_endpoint_returns_200_when_healthy()
async fn v4p0_readiness_endpoint_returns_503_when_check_fails()
async fn v4p0_demo_mode_starts_without_postgres()
async fn v4p0_demo_gateway_responds_to_chat_completions()
async fn v4p0_regression_existing_provider_adapters_unaffected()
```

### Definition of Done

```bash
cargo test v4_0
cargo test
cargo clippy -- -D warnings
velox --doctor   # must pass on a correctly configured local instance
```

---

## 4. Phase V4-1: Dashboard Catch-up

**Goal**: Close all dashboard gaps left by V2 and V3 — every backend feature
that was built without a UI gets its UI in this phase.

**This is a pure frontend phase. Zero Rust changes.**

### 4.1 Alerts Management Page (`/alerts`)

Backend: `POST/GET/PATCH/DELETE /admin/alerts` + `POST /admin/alerts/:id/test` (V2-2)

New page with:
- Table of all alerts (type, threshold, status, last triggered)
- Create alert dialog: type, threshold, window, webhook URL, webhook format, webhook secret
- Edit / delete / toggle active
- "Send test webhook" button
- Alert history drawer: last N firings, delivered/failed, error message

### 4.2 Prompts Management Page (`/prompts`)

Backend: `POST/GET /admin/prompts`, `POST /admin/prompts/:id/versions`,
`PATCH /admin/prompts/:id/versions/:v`, `DELETE /admin/prompts/:id` (V2-5)

New page with:
- Table of all prompts (name, active version, last updated)
- Create prompt + first version
- Version history list per prompt
- Activate version button
- A/B weight editor (0–100 sliders for each version)
- Template preview with variable substitution

### 4.3 Settings Page — Make Fields Editable

Backend: `PATCH /admin/config` (V2-0)

Currently the Settings page calls only `GET /admin/config`. Add:
- Save button that calls `PATCH /admin/config`
- Editable fields: log_request_bodies, log_response_bodies, cache_enabled,
  semantic_cache_threshold, rate_limit_window_secs, max_retries, prometheus_enabled

### 4.4 Keys Page — Routing Strategy Field

Backend: `routing_strategy` column on `api_keys` (V2-4)

Add to the key create and key edit forms:
- Routing strategy dropdown: Priority (default) / Cost Optimized / Latency Optimized / Round Robin

### 4.5 Keys Page — Key Rotation Button

Backend: `POST /admin/keys/:id/rotate` (V3-5)

Add to the key detail view:
- "Rotate Key" button with confirmation dialog
- Shows the new full key in a one-time display dialog (same UX as key creation)
- Grace period countdown shown in the key row while old key is still valid

### Definition of Done

```bash
# No Rust changes — gate is visual verification
cargo test   # must still be green (no regressions from dashboard rebuild)
```

Verify each new page/feature works end-to-end against a running Velox instance.

---

## 5. Phase V4-2: In-flight Request Deduplication

**Goal**: When N identical requests arrive concurrently, make exactly one provider
call and share the result with all waiters.

### 5.1 Problem

If 20 clients send the same request simultaneously, all 20 pass the cache check
(cache is cold), all 20 call the provider, and 19 of those 20 calls are wasted.

### 5.2 Design

**New file: `src/gateway/dedup.rs`**

```rust
pub struct InFlightDeduplicator {
    in_flight: DashMap<String, broadcast::Sender<Arc<DeduplicatedResult>>>,
}

pub enum DeduplicatedResult {
    Response(ChatCompletionResponse),
    Error(String),
}

impl InFlightDeduplicator {
    /// None → caller is primary, must proceed to provider.
    /// Some(rx) → duplicate in-flight, await broadcast.
    pub fn register_or_subscribe(&self, hash: &str)
        -> Option<broadcast::Receiver<Arc<DeduplicatedResult>>>;

    pub fn broadcast_result(&self, hash: &str, result: Arc<DeduplicatedResult>);
    pub fn release(&self, hash: &str);
}
```

**Pipeline position** (after both cache layers, before provider call):

```
exact cache hit   → return
semantic cache hit → return
dedup: in-flight?  → await broadcast
dedup: primary     → register, call provider, broadcast, release
cache insert
return
```

The dedup hash reuses the SHA-256 already computed for exact cache — no extra work.

**Streaming**: dedup is **skipped** for streaming requests — SSE responses cannot
be broadcast across multiple connections.

**Error propagation**: if the primary fails, all waiters receive the error
immediately and are released (not stuck).

### New Files

- `src/gateway/dedup.rs` — `InFlightDeduplicator`

### Files to Modify

- `src/gateway/pipeline.rs` — add dedup step
- `src/state.rs` — add `dedup: Arc<InFlightDeduplicator>`

### Test Contract

**File:** `tests/v4/v4_2_dedup.rs`

```rust
async fn v4p2_second_identical_concurrent_request_awaits_first()
async fn v4p2_all_waiters_receive_same_response()
async fn v4p2_only_one_provider_call_made_for_n_concurrent_identical_requests()
async fn v4p2_different_requests_not_deduplicated()
async fn v4p2_provider_error_propagates_to_all_waiters()
async fn v4p2_primary_timeout_releases_waiters_with_error()
async fn v4p2_exact_cache_hit_bypasses_dedup()
async fn v4p2_dedup_slot_cleared_after_completion()
async fn v4p2_streaming_request_not_deduplicated()
async fn v4p2_regression_sequential_requests_unaffected()
```

### Definition of Done

```bash
cargo test v4_2
cargo test
cargo clippy -- -D warnings
```

---

## 6. Phase V4-3: Cache TTL + Time-sensitive Safety

**Goal**: Prevent stale cache responses and skip caching for prompts that are
inherently time-bound.

### 6.1 Cache TTL

```sql
-- migrations/0020_cache_ttl.sql
ALTER TABLE cache_entries
    ADD COLUMN ttl_secs   INTEGER,
    ADD COLUMN expires_at TIMESTAMPTZ;
CREATE INDEX idx_cache_entries_expires ON cache_entries(expires_at)
    WHERE expires_at IS NOT NULL;
```

**Config:**
```toml
cache_ttl_secs = 0   # 0 = no expiry (backward compatible)

[cache_ttl_overrides]
"gpt-4o-mini" = 3600
```

- Cache lookup filters: `expires_at IS NULL OR expires_at > NOW()`
- Cache insert sets `expires_at = NOW() + ttl_secs` when TTL > 0
- Background task prunes expired entries every 5 minutes

### 6.2 Time-sensitive Query Detection

**New file: `src/cache/time_guard.rs`**

Checks all message content against a configurable pattern list before cache lookup.
If matched: skip both lookup AND write. Sets `X-Velox-Cache-Skip: time_sensitive` header.

**Default patterns (configurable, extensible):**
```toml
[cache]
time_sensitive_patterns = [
    # English
    "\\btoday\\b", "\\bright now\\b", "\\bcurrently\\b", "\\blatest\\b",
    "\\bcurrent price\\b", "\\bthis week\\b", "\\bat this moment\\b",
    # Persian
    "امروز", "الان", "هم‌اکنون", "قیمت فعلی", "این هفته", "اخبار",
    # Arabic
    "اليوم", "الآن", "السعر الحالي",
]
```

### New Files

- `src/cache/time_guard.rs`

### Files to Modify

- `src/gateway/pipeline.rs` — TTL check on lookup, TTL write on insert, time-guard check
- `src/db/cache.rs` — extend queries for TTL fields
- `src/config.rs` — `cache_ttl_secs`, `cache_ttl_overrides`, `time_sensitive_patterns`
- `src/main.rs` — start TTL prune background task

### Test Contract

**File:** `tests/v4/v4_3_cache_ttl.rs`

```rust
async fn v4p3_entry_not_returned_after_ttl_expires()
async fn v4p3_entry_returned_before_ttl_expires()
async fn v4p3_zero_ttl_means_no_expiry()
async fn v4p3_per_model_ttl_override_takes_precedence()
async fn v4p3_prune_task_removes_expired_entries()
async fn v4p3_time_sensitive_prompt_bypasses_cache_lookup()
async fn v4p3_time_sensitive_prompt_not_written_to_cache()
async fn v4p3_skip_header_set_on_time_sensitive_request()
async fn v4p3_persian_time_pattern_detected()
async fn v4p3_custom_pattern_added_via_config()
async fn v4p3_non_time_sensitive_prompt_uses_cache_normally()
async fn v4p3_regression_exact_cache_hit_still_works_without_ttl()
```

### Definition of Done

```bash
cargo test v4_3
cargo test
cargo clippy -- -D warnings
```

---

## 7. Phase V4-4: Budget-Aware Auto-Downgrade

**Goal**: When an API key approaches its budget limit, automatically switch to
cheaper providers/models rather than blocking cold at 100%.

### 7.1 Design

```sql
-- migrations/0021_budget_downgrade.sql
ALTER TABLE api_keys
    ADD COLUMN downgrade_at_percent INTEGER,
    ADD COLUMN downgrade_strategy   VARCHAR(20),
    ADD COLUMN downgrade_to_model   VARCHAR(100);
```

**Config (global defaults, per-key config overrides):**
```toml
[budget_downgrade]
enabled           = false
threshold_percent = 80
strategy          = "cost_optimized"   # or "specific_model"
fallback_model    = ""
```

**Pipeline behavior:**
1. Existing budget check — block at 100% (unchanged)
2. New: if `spend / budget_limit >= downgrade_at_percent / 100`:
   - Override routing strategy to `downgrade_strategy`
   - OR override model to `downgrade_to_model`
   - Set `X-Velox-Downgraded: cost_optimized` response header
   - Log `downgrade_triggered = true` in request record

### Files to Modify

- `src/middleware/budget.rs` — return `DowngradeDecision` alongside block decision
- `src/gateway/pipeline.rs` — apply downgrade decision to routing
- `src/handlers/admin/keys.rs` — expose downgrade fields in CRUD
- `src/config.rs` — `[budget_downgrade]` section
- `src/models/api_key.rs` — add downgrade fields

### Test Contract

**File:** `tests/v4/v4_4_budget_downgrade.rs`

```rust
async fn v4p4_request_uses_cost_optimized_when_threshold_reached()
async fn v4p4_request_uses_premium_model_when_under_threshold()
async fn v4p4_specific_model_downgrade_overrides_requested_model()
async fn v4p4_downgrade_header_set_when_triggered()
async fn v4p4_downgrade_disabled_by_default()
async fn v4p4_budget_block_still_fires_at_100_percent()
async fn v4p4_downgrade_logged_in_request_record()
async fn v4p4_regression_keys_without_downgrade_config_unaffected()
```

### Definition of Done

```bash
cargo test v4_4
cargo test
cargo clippy -- -D warnings
```

---

## 8. Phase V4-5: Cost Simulator + Provider Quality Scoring

**Goal**: Answer "what would last month's costs have been under a different
strategy?" and give each provider a continuously-updated quality score.

### 8.1 Cost Simulator

**New endpoint: `GET /admin/analytics/simulate`**

| Param | Type | Description |
|---|---|---|
| `strategy` | string | `cost_optimized` \| `round_robin` \| `priority` |
| `period` | string | `7d` \| `30d` \| `90d` |
| `model_overrides` | JSON | e.g. `{"gpt-4o":"gpt-4o-mini"}` |

Recalculates all requests in the period under the given strategy using `model_pricing`
table. Returns original cost, simulated cost, savings, and per-model breakdown.

**Response:**
```json
{
  "data": {
    "strategy": "cost_optimized",
    "period": "30d",
    "original_cost_usd": 142.50,
    "simulated_cost_usd": 89.20,
    "savings_usd": 53.30,
    "savings_percent": 37.4,
    "request_count": 15420,
    "by_model": [...]
  }
}
```

### 8.2 Provider Quality Score

Background task (every 15 minutes) recalculates a `0.0–1.0` quality score per provider:

```
quality_score = 0.40 × availability_score   (success_count / total in last hour)
              + 0.35 × latency_score         (1 - p95_ms / 10_000, clamped)
              + 0.25 × reliability_score     (1 - error_rate)
```

```sql
-- migrations/0022_provider_quality_score.sql
ALTER TABLE providers
    ADD COLUMN quality_score       DECIMAL(5,4) DEFAULT 1.0,
    ADD COLUMN quality_updated_at  TIMESTAMPTZ;
```

Score is exposed in `GET /admin/providers` response.

### New Files

- `src/analytics/mod.rs`
- `src/analytics/quality_score.rs` — scoring background task

### Files to Modify

- `src/handlers/admin/analytics.rs` — add simulate endpoint
- `src/main.rs` — start quality score task
- `src/db/providers.rs` — `update_quality_score()`

### Test Contract

**File:** `tests/v4/v4_5_cost_sim.rs`

```rust
async fn v4p5_simulate_cost_optimized_returns_lower_cost()
async fn v4p5_simulate_priority_returns_same_cost_as_actual()
async fn v4p5_simulate_model_override_applies_new_pricing()
async fn v4p5_simulate_empty_period_returns_zero()
async fn v4p5_simulate_includes_per_model_breakdown()
fn v4p5_quality_score_decreases_on_high_error_rate()
fn v4p5_quality_score_decreases_on_high_latency()
fn v4p5_quality_score_defaults_to_one_with_no_data()
async fn v4p5_quality_score_visible_in_provider_list()
async fn v4p5_regression_analytics_overview_unaffected()
```

### Definition of Done

```bash
cargo test v4_5
cargo test
cargo clippy -- -D warnings
```

---

## 9. Phase V4-6: Request Replay & Debug Console

**Goal**: Allow replaying any past request with modified parameters, and expose
an admin playground endpoint for interactive testing.

### 9.1 Request Replay API

**New endpoint: `POST /admin/requests/:id/replay`**

```json
{
  "provider_id": "uuid",   // optional override
  "skip_cache": true,      // optional
  "stream": false,         // optional
  "model": "gpt-4o-mini"  // optional
}
```

- Loads original `request_body` from `requests` table
- Applies overrides, runs through full pipeline
- Creates new request record with `replay_of_request_id = :id`
- Returns response + provider, latency_ms, cost_usd, cache_hit, new request_id
- Original record is never modified

### 9.2 Admin Playground

**New endpoint: `POST /admin/playground`**

Same shape as `POST /v1/chat/completions`, authenticated via admin JWT.
Returns extended metadata headers. Logged with `is_playground = true`.
No budget or rate limit checks applied.

```sql
-- migrations/0023_request_replay.sql
ALTER TABLE requests
    ADD COLUMN replay_of_request_id UUID REFERENCES requests(id),
    ADD COLUMN is_playground        BOOLEAN NOT NULL DEFAULT FALSE;
```

### New Files

- `src/handlers/admin/replay.rs`

### Test Contract

**File:** `tests/v4/v4_6_replay.rs`

```rust
async fn v4p6_replay_creates_new_request_record()
async fn v4p6_replay_records_replay_of_request_id()
async fn v4p6_replay_with_provider_override_uses_specified_provider()
async fn v4p6_replay_with_skip_cache_bypasses_cache()
async fn v4p6_replay_of_nonexistent_request_returns_404()
async fn v4p6_original_request_record_not_modified()
async fn v4p6_playground_accessible_with_admin_jwt()
async fn v4p6_playground_returns_extended_metadata_headers()
async fn v4p6_playground_flagged_in_request_log()
async fn v4p6_playground_not_accessible_with_gateway_key()
async fn v4p6_regression_normal_gateway_unaffected()
```

### Definition of Done

```bash
cargo test v4_6
cargo test
cargo clippy -- -D warnings
```

---

## 10. Phase V4-7: Dashboard Overhaul

**Goal**: Add UI for all new V4 backend features and elevate the dashboard from
a monitoring tool to an operational product usable by non-engineers.

**This is a pure frontend phase. Zero Rust changes.**

### 10.1 System Health Page (`/health`)

Calls `GET /admin/system/readiness` (V4-0).

- Visual checklist of all readiness checks
- Green / yellow / red indicators per check
- Auto-refreshes every 30 seconds
- Prominent warning banner on any failing check

### 10.2 Playground Page (`/playground`)

Calls `POST /admin/playground` (V4-6).

- Prompt composer: model selector, system prompt, user messages
- Options: stream toggle, skip cache toggle, provider selector
- Submit → shows response with all extended metadata:
  - Provider used, latency, tokens, cost, cache hit status, request ID
- "Replay as different provider" shortcut
- History of last 10 playground requests

### 10.3 Cost Simulator Page (`/analytics/simulate`)

Calls `GET /admin/analytics/simulate` (V4-5).

- Strategy selector, period selector, optional model override table
- Submit → bar chart: original vs simulated cost
- Per-model breakdown table with delta column
- "Apply this strategy to all new keys" shortcut

### 10.4 Provider Quality Indicators (`/providers`)

Uses `quality_score` field from `GET /admin/providers` (V4-5).

- Quality score badge (0–100) next to each provider
- Color: green ≥ 0.90, yellow ≥ 0.70, red < 0.70
- Tooltip showing component scores: availability, latency, reliability
- "Last updated" timestamp

### 10.5 Overview Page — Operational Questions

Improve `GET /admin/analytics/overview` presentation to answer:

- "Today's spend and how it compares to yesterday"
- "Top 5 API keys by cost this month"
- "Cache savings this month in dollars"
- "Provider with most errors in last 24h"
- "Is the system healthy right now?" (velox doctor summary)

These are answered using APIs that already exist — this is a presentation improvement.

### Definition of Done

```bash
cargo test   # must still be green (no regressions from dashboard rebuild)
```

Visual verification of all 5 sections against a running Velox instance with data.

---

## 11. Phase V4-8: RBAC / True Multi-tenancy

**Goal**: Add role-based access control so team members can be given scoped access
without sharing admin credentials.

This is the largest phase in V4. It is placed last intentionally.
It is broken into three independently-shippable sub-phases.

### Sub-phase V4-8a: Permission Model + Schema

```sql
-- migrations/0024_rbac.sql
CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    name        VARCHAR(50) NOT NULL UNIQUE
);

INSERT INTO roles (id, name) VALUES
    (gen_random_uuid(), 'admin'),
    (gen_random_uuid(), 'billing_viewer'),
    (gen_random_uuid(), 'api_manager'),
    (gen_random_uuid(), 'read_only');

CREATE TABLE workspace_members (
    id           UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id      UUID NOT NULL REFERENCES roles(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(workspace_id, user_id)
);
CREATE INDEX idx_workspace_members_user ON workspace_members(user_id);
```

**Role permissions:**

| Role | Can Do |
|---|---|
| `admin` | Everything |
| `api_manager` | Create/revoke keys in their workspace; view requests |
| `billing_viewer` | View costs and analytics; no mutations |
| `read_only` | View requests and providers; nothing else |

Existing users in `users` table get `admin` role on all workspaces automatically.

### Sub-phase V4-8b: API Enforcement

New middleware `src/middleware/rbac.rs` — extracts user's role for the target
workspace and enforces it per endpoint.

Admin endpoint permission requirements:

| Minimum Role | Endpoints |
|---|---|
| `billing_viewer` | `GET /admin/analytics/*`, `GET /admin/requests` |
| `api_manager` | `POST/PATCH/DELETE /admin/keys` |
| `admin` | `DELETE /admin/cache`, `PATCH /admin/config`, `POST /admin/providers/:id/test` |

### Sub-phase V4-8c: Member Management API + UI

**New backend endpoints:**

| Method | Path | Role |
|---|---|---|
| `GET` | `/admin/workspaces/:id/members` | `admin` |
| `POST` | `/admin/workspaces/:id/members` | `admin` |
| `PATCH` | `/admin/workspaces/:id/members/:uid` | `admin` |
| `DELETE` | `/admin/workspaces/:id/members/:uid` | `admin` |

**New dashboard page: `/workspaces`**

- List workspaces with member count
- Member list per workspace with role badge
- Invite member (email + role selector)
- Change role / remove member

### New Files

- `src/middleware/rbac.rs`
- `src/handlers/admin/members.rs`
- `src/db/rbac.rs`

### Test Contract

**File:** `tests/v4/v4_8_rbac.rs`

```rust
async fn v4p8_admin_can_access_all_endpoints()
async fn v4p8_billing_viewer_can_read_analytics()
async fn v4p8_billing_viewer_cannot_create_api_keys()
async fn v4p8_api_manager_can_create_keys_in_own_workspace()
async fn v4p8_api_manager_cannot_delete_cache()
async fn v4p8_read_only_cannot_mutate_anything()
async fn v4p8_cross_workspace_access_denied()
async fn v4p8_admin_can_add_member()
async fn v4p8_removed_member_loses_access_immediately()
async fn v4p8_existing_users_get_admin_role_on_migration()
async fn v4p8_regression_gateway_api_key_auth_unaffected()
async fn v4p8_regression_existing_admin_jwt_unaffected()
```

### Definition of Done

```bash
cargo test v4_8
cargo test
cargo clippy -- -D warnings
cargo build --release
```

---

## 12. Phase V4-9: External Vector Stores

**Goal**: Plug Qdrant into the `EmbeddingIndex` trait from V3-1.

**Start condition**: Only when deployment has > 500,000 semantic cache entries,
OR a specific customer requests it. HNSW handles everything below that scale.

### Design

**New file: `src/cache/index/qdrant.rs`** — implements `EmbeddingIndex` trait.

```toml
# velox.toml
[semantic_cache]
backend          = "qdrant"
qdrant_url       = "http://localhost:6334"
qdrant_collection = "velox_cache"
```

### New Dependencies

```toml
# Cargo.toml — V4-9 only
qdrant-client = "1.9"
```

### Test Contract

**File:** `tests/v4/v4_9_vector_store.rs`

```rust
fn v4p9_qdrant_index_implements_embedding_index_trait()
async fn v4p9_qdrant_lookup_returns_above_threshold_match()
async fn v4p9_qdrant_insert_and_find()
async fn v4p9_qdrant_clear_empties_collection()
async fn v4p9_qdrant_unavailable_at_startup_returns_error()
async fn v4p9_regression_hnsw_backend_unchanged()
```

### Definition of Done

```bash
cargo test v4_9
cargo test
cargo clippy -- -D warnings
```

---

## 13. What V4 Explicitly Does NOT Include

| Item | Why |
|---|---|
| Prompt compression / summarization | Transparent proxy contract violated; possible as opt-in V3-4 plugin only |
| Quality evaluation / LLM-as-judge | Evals platform — out of scope for a gateway |
| Batch chat completions (OpenAI Batch API) | Requires async job queue; architectural overhaul |
| Backup/restore CLI | `pg_dump` covers this; Velox-specific tooling adds maintenance cost |
| WASM/Lua dynamic plugins | V3-4 Rust plugins cover real needs |
| SSO / SAML | Requires identity provider layer; V5 territory |
| Budget forecasting ML | Time-series model; V5 territory |

---

## 14. Migration Plan

| Migration | Phase | Description |
|---|---|---|
| 0001–0018 | V1/V2/V3 | Existing schema |
| 0019 | V4-0 | Add `base_url` to `providers` |
| 0020 | V4-3 | Add `ttl_secs`, `expires_at` to `cache_entries` |
| 0021 | V4-4 | Add `downgrade_at_percent`, `downgrade_strategy`, `downgrade_to_model` to `api_keys` |
| 0022 | V4-5 | Add `quality_score`, `quality_updated_at` to `providers` |
| 0023 | V4-6 | Add `replay_of_request_id`, `is_playground` to `requests` |
| 0024 | V4-8 | Create `roles`, `workspace_members` tables |

SQLite migrations: maintained in `migrations/sqlite/` per V2-1 convention.

> **Rule**: Never modify existing migrations. Each change is a new file.

---

## 15. Dependency Plan

### V4-0 through V4-7
```toml
# No new dependencies
# tokio broadcast (dedup) is in tokio core — already present
# regex (time_guard) — already present
```

### V4-8
```toml
# No new dependencies — RBAC uses existing sqlx + axum extractors
```

### V4-9
```toml
qdrant-client = "1.9"
```

---

## 16. V4 Phase Status Tracker

| Phase | Type | Description | Status | Migration |
|---|---|---|---|---|
| V4-0 | Backend | Foundation & DX | ✅ Complete | 0019 |
| V4-1 | Frontend | Dashboard Catch-up (Alerts, Prompts, Settings) | ✅ Complete | — |
| V4-2 | Backend | In-flight Request Deduplication | ✅ Complete | — |
| V4-3 | Backend | Cache TTL + Time-sensitive Safety | ✅ Complete | 0020 |
| V4-4 | Backend | Budget-Aware Auto-Downgrade | ✅ Complete | 0021 |
| V4-5 | Backend | Cost Simulator + Provider Quality Scoring | ✅ Complete | 0022 |
| V4-6 | Backend | Request Replay & Debug Console | ✅ Complete (2026-05-24) | 0023 |
| V4-7 | Frontend | Dashboard Overhaul | ✅ Complete (2026-05-24) | — |
| V4-8 | Both | RBAC / True Multi-tenancy | ✅ Complete (2026-05-24) | 0024 |
| V4-9 | Backend | External Vector Stores (demand-driven) | ✅ Complete (2026-05-24) | — |

---

## Session Start Ritual for V4 Work

```bash
# 1. Confirm V3 is still green
cargo test 2>&1 | tail -20

# 2. Check V4 phase status (this file, §16)

# 3. Run the specific phase tests if work is in progress
cargo test v4_0   # (or v4_2, v4_3, etc.)

# 4. Tell the user: "We are on Phase V4-X. Ready to continue."
```

**Do NOT write any code until you have done all 4 steps.**

---

*Created: 2026-05-24 — based on V3 complete (all 6 phases) and full gap audit*
*Update the Phase Status Tracker (§16) at the end of every session.*
