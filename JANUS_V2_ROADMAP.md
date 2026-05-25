# JANUS V2 — Engineering Roadmap
> Built on v0.1.0 (commit 97b4c2a). All phases are feature-complete.
> This document is the single source of truth for all v2 work.
> **If you are Claude: read CLAUDE.md first, then this file.**

---

## Table of Contents

1. [V1 Gap Analysis](#1-v1-gap-analysis)
2. [V2 Testing Philosophy](#2-v2-testing-philosophy)
3. [Phase V2-0: Spec Completion](#3-phase-v2-0-spec-completion)
4. [Phase V2-1: SQLite Support](#4-phase-v2-1-sqlite-support)
5. [Phase V2-2: Webhook Alerts](#5-phase-v2-2-webhook-alerts)
6. [Phase V2-3: Extended API Compatibility](#6-phase-v2-3-extended-api-compatibility)
7. [Phase V2-4: Intelligent Routing](#7-phase-v2-4-intelligent-routing)
8. [Phase V2-5: Prompt Management](#8-phase-v2-5-prompt-management)
9. [Phase V2-6: Multi-Node Clustering](#9-phase-v2-6-multi-node-clustering)
10. [Phase V2-7: MCP Server](#10-phase-v2-7-mcp-server)
11. [Migration Plan](#11-migration-plan)
12. [Dependency Plan](#12-dependency-plan)
13. [V2 Phase Status Tracker](#13-v2-phase-status-tracker)

---

## 1. V1 Gap Analysis

Before v2 features, the following gaps exist between the v1 specification (JANUS_ROADMAP.md)
and what was actually built. These are closed in **Phase V2-0** first.

| Gap | Spec Location | Status |
|---|---|---|
| `daily_costs` table never written to by pipeline | §5 DB Schema | ❌ Missing |
| `alerts` table has no evaluation logic anywhere | §6 API Design | ❌ Missing |
| No circuit breaker per provider | Phase 3 tasks | ❌ Missing |
| No background provider health check task | Phase 3 tasks | ❌ Missing |
| Token-per-minute (TPM) rate limiting not implemented | §7 Pipeline step 3 | ❌ Missing |
| No `GET /v1/models` endpoint | §6 API Design | ❌ Missing |
| No `GET /admin/requests/export` (CSV) | §6 API Design | ❌ Missing |
| No `PATCH /admin/config` | §6 API Design | ❌ Missing |
| No `DELETE /admin/cache/entries/:id` | §6 API Design | ❌ Missing |

> **Key insight**: `allowed_models` enforcement IS correctly implemented (gateway.rs:72).
> The above gaps are what actually need to be built in V2-0.

---

## 2. V2 Testing Philosophy

### The Regression Contract

Every phase begins and ends with the same ritual:
```bash
# Gate-in: all existing tests must be green BEFORE touching any code
cargo test
cargo clippy -- -D warnings

# Gate-out: all tests (old + new) must be green AFTER the phase is complete
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

**No phase is complete if any pre-existing test is broken.**

### Test File Layout

```
tests/
├── common/mod.rs              ← Shared helpers (existing)
├── phase0_foundation.rs       ← V1 tests (existing, must stay green)
├── ...
├── phase5_semantic_cache.rs   ← V1 tests (existing, must stay green)
│
└── v2/
    ├── common.rs              ← V2-specific spawn helpers (SQLite, alerts, etc.)
    ├── v2_0_spec_completion.rs
    ├── v2_1_sqlite.rs
    ├── v2_2_webhooks.rs
    ├── v2_3_api_compat.rs
    ├── v2_4_routing.rs
    ├── v2_5_prompts.rs
    ├── v2_6_clustering.rs
    └── v2_7_mcp.rs
```

### Test Naming Convention

```
v2p{phase}_{feature_under_test}_{expected_outcome}

Examples:
  v2p0_daily_costs_written_after_successful_request
  v2p1_sqlite_exact_cache_hit_returns_without_db_call
  v2p2_alert_webhook_posts_correct_payload_to_slack
```

### What Each Test File Must Contain

1. **Happy path** — the feature works as expected
2. **Error cases** — bad input, missing config, downstream failure
3. **Regression check** — at least one test that verifies a key V1 behavior
   still works (gateway proxies, auth enforced, cache hits)

---

## 3. Phase V2-0: Spec Completion

**Goal**: Close all gaps between the v1 specification and the actual implementation.
No new user-visible features — only fill what was promised in v1 but not built.

### What to Build

#### 3.1 Pipeline writes `daily_costs`

After every successful proxy call, upsert a row in `daily_costs`:
```sql
INSERT INTO daily_costs (date, workspace_id, api_key_id, provider, model,
    request_count, error_count, cache_hits, prompt_tokens, completion_tokens, total_cost_usd)
VALUES (CURRENT_DATE, $1, $2, $3, $4, 1, 0, $5, $6, $7, $8)
ON CONFLICT (date, provider, model, COALESCE(api_key_id, '00000000-0000-0000-0000-000000000000'))
DO UPDATE SET
    request_count     = daily_costs.request_count + 1,
    cache_hits        = daily_costs.cache_hits + EXCLUDED.cache_hits,
    prompt_tokens     = daily_costs.prompt_tokens + EXCLUDED.prompt_tokens,
    completion_tokens = daily_costs.completion_tokens + EXCLUDED.completion_tokens,
    total_cost_usd    = daily_costs.total_cost_usd + EXCLUDED.total_cost_usd;
```

Fire this as `tokio::spawn` (async, non-blocking) at the end of `pipeline::run()` and
`pipeline::run_streaming()`, identical to the existing request log pattern.

**Files to modify:**
- `src/db/analytics.rs` — add `upsert_daily_cost()` function
- `src/gateway/pipeline.rs` — call `upsert_daily_cost` after successful responses

#### 3.2 Alert Threshold Engine

The `alerts` table has been sitting empty. Build the evaluation engine.

**New file: `src/alerts/mod.rs`**
```rust
pub struct AlertEngine { pool: PgPool }

impl AlertEngine {
    /// Evaluate all active alerts. Called every 60 seconds by a background task.
    pub async fn evaluate(&self) -> anyhow::Result<()>;

    /// Fire a single alert: update last_triggered, send webhook (Phase V2-2 extends this).
    async fn fire(&self, alert_id: Uuid) -> anyhow::Result<()>;
}
```

Alert types to implement now:
- `spend_threshold`: if `SUM(cost_usd) WHERE created_at >= NOW() - window` exceeds threshold
- `error_rate`: if `COUNT(*) FILTER (status='error') / COUNT(*)` exceeds threshold
- `latency_spike`: if `AVG(latency_ms)` over last N minutes exceeds threshold

Background task in `main.rs`:
```rust
let alert_engine = Arc::new(alerts::AlertEngine::new(pool.clone()));
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let _ = alert_engine.evaluate().await;
    }
});
```

**Files to create:** `src/alerts/mod.rs`
**Files to modify:** `src/main.rs` (start background task)

#### 3.3 Circuit Breaker per Provider

**New file: `src/gateway/circuit_breaker.rs`**

States: `Closed` (normal) → `Open` (failing) → `HalfOpen` (testing recovery)

```rust
pub struct CircuitBreaker {
    state: Arc<Mutex<BreakerState>>,
    failure_threshold: u32,   // open after N consecutive failures
    recovery_timeout: Duration, // wait before trying half-open
}

impl CircuitBreaker {
    pub fn is_open(&self) -> bool;
    pub fn record_success(&self);
    pub fn record_failure(&self);
}
```

One `CircuitBreaker` per provider, stored in `ProviderRegistry`.
The pipeline checks `is_open()` before selecting a provider, skipping open breakers.

**Files to create:** `src/gateway/circuit_breaker.rs`
**Files to modify:**
- `src/gateway/mod.rs` — add `circuit_breakers: DashMap<&'static str, CircuitBreaker>`
- `src/gateway/pipeline.rs` — check breaker state before provider call, record outcome

#### 3.4 Background Provider Health Checks

Background task pings each enabled provider every 60 seconds.
Uses the existing `health_check()` method on the `Provider` trait.
Updates the `providers` table `health_status` column via `db::providers::update_health_status()`.

**Files to modify:** `src/main.rs` (add health-check background task), `src/db/providers.rs`

#### 3.5 Token-per-Minute Rate Limiting

Extend `RateLimiter` with a second sliding window for estimated tokens.

**Logic:**
```rust
// Estimate tokens from request before sending to provider
// (input: count messages, rough 4 chars/token estimate)
// Compare sum of tokens in window against key.rate_limit_tpm
```

The estimate doesn't need to be perfect — it's a guard, not billing.

**Files to modify:** `src/middleware/rate_limit.rs`, `src/handlers/gateway.rs`

#### 3.6 Missing Admin Endpoints

| Endpoint | Handler File | Notes |
|---|---|---|
| `GET /v1/models` | `src/handlers/gateway.rs` | Aggregate from all enabled providers |
| `GET /admin/requests/export` | `src/handlers/admin/requests.rs` | CSV, `Content-Disposition: attachment` |
| `PATCH /admin/config` | `src/handlers/admin/janus_config.rs` | Update runtime-safe config fields |
| `DELETE /admin/cache/entries/:id` | `src/handlers/admin/cache.rs` | Remove single cache entry |

### New Migrations

```
migrations/0012_add_alerts_webhook_url.sql   ← Add webhook_url, webhook_secret columns to alerts
```

### Test Contract

**File:** `tests/v2/v2_0_spec_completion.rs`

```rust
// Daily costs
async fn v2p0_daily_costs_written_after_successful_request()
async fn v2p0_daily_costs_aggregates_multiple_requests_same_day()
async fn v2p0_daily_costs_cache_hits_counted_separately()

// Alert engine
async fn v2p0_spend_threshold_alert_fires_when_exceeded()
async fn v2p0_error_rate_alert_evaluates_over_window()
async fn v2p0_latency_spike_alert_fires_on_high_p95()
async fn v2p0_alert_last_triggered_updated_when_fired()
async fn v2p0_inactive_alert_does_not_fire()

// Circuit breaker
async fn v2p0_circuit_opens_after_consecutive_provider_failures()
async fn v2p0_circuit_skips_open_provider_and_fails_over()
async fn v2p0_circuit_transitions_to_half_open_after_recovery_timeout()
async fn v2p0_circuit_closes_on_successful_half_open_probe()

// TPM rate limiting
async fn v2p0_tpm_rate_limit_enforced_when_token_budget_exhausted()
async fn v2p0_tpm_window_resets_after_one_minute()

// New endpoints
async fn v2p0_models_endpoint_lists_enabled_providers()
async fn v2p0_export_requests_returns_valid_csv()
async fn v2p0_patch_config_updates_log_request_bodies_flag()
async fn v2p0_delete_cache_entry_removes_from_hot_and_db()

// Regression: existing behavior unchanged
async fn v2p0_regression_gateway_still_proxies_correctly()
async fn v2p0_regression_exact_cache_still_hits()
async fn v2p0_regression_auth_still_enforced()
```

### Definition of Done

```bash
cargo test v2_0        # all V2-0 tests pass
cargo test             # ALL tests (V1 + V2-0) pass
cargo clippy -- -D warnings
```

---

## 4. Phase V2-1: SQLite Support

**Goal**: Run Janus with zero external dependencies.
`./janus` on a fresh machine with no PostgreSQL works out of the box.
This closes **Decision D-002** which explicitly deferred SQLite to v2.

### Architecture Decision

Use **feature flags** to compile for either backend:
```bash
cargo build --features postgres   # default (production, compile-time checked queries)
cargo build --features sqlite     # single-binary (runtime-checked queries)
```

This preserves compile-time query checking for the primary deployment target.

### What to Build

#### 4.1 Abstract Pool Type

```rust
// src/db/pool.rs
#[cfg(feature = "postgres")]
pub type DbPool = sqlx::PgPool;

#[cfg(feature = "sqlite")]
pub type DbPool = sqlx::SqlitePool;
```

All handler and pipeline signatures change from `&PgPool` to `&DbPool`.

#### 4.2 SQLite-Compatible Migrations

New parallel migration set under `migrations/sqlite/`:
```
migrations/sqlite/0001_create_users.sql
migrations/sqlite/0002_create_workspaces.sql
...
migrations/sqlite/0011_seed_additional_providers.sql
```

Key SQLite adaptations:
- `UUID PRIMARY KEY DEFAULT gen_random_uuid()` → `TEXT PRIMARY KEY` (UUID generated in Rust)
- `TIMESTAMPTZ` → `TEXT` (ISO 8601)
- `DECIMAL(12,8)` → `TEXT` (with `rust_decimal` parse/serialize)
- `TEXT[]` (PostgreSQL arrays) → `TEXT` (JSON array string)
- `BYTEA` → `BLOB`
- `COALESCE(x, y::UUID)` composite PK in `daily_costs` → single `TEXT` key column
- `ON CONFLICT ... DO UPDATE` → supported in SQLite 3.24+

**Migration runner in `main.rs` selects the correct set based on `database_url` prefix.**

#### 4.3 Query Compatibility

All `sqlx::query!` compile-time macros that use PostgreSQL-specific syntax must be
converted to `sqlx::query` (runtime) when building with the `sqlite` feature.

Strategy: wrap affected queries in `#[cfg(feature = "postgres")]` / `#[cfg(feature = "sqlite")]`.

#### 4.4 Config Changes

```toml
# janus.toml — SQLite usage (auto-detected from URL prefix)
database_url = "sqlite:janus.db"

# PostgreSQL (existing)
database_url = "postgres://user:pass@localhost/janus"
```

No other config change needed.

#### 4.5 Startup Auto-Creation

When `database_url` is `sqlite:`, create the file if it doesn't exist.
SQLite migrations use `migrations/sqlite/` directory.

### New Files

- `src/db/pool.rs` — `DbPool` type alias, `connect()` function, migration runner
- `migrations/sqlite/*.sql` — SQLite-compatible migration set

### Test Contract

**File:** `tests/v2/v2_1_sqlite.rs`

```rust
// Core behavior works on SQLite
async fn v2p1_sqlite_migrations_apply_cleanly()
async fn v2p1_sqlite_gateway_proxies_and_logs_request()
async fn v2p1_sqlite_api_key_created_and_validated()
async fn v2p1_sqlite_exact_cache_hit_and_miss()
async fn v2p1_sqlite_rate_limit_enforced()
async fn v2p1_sqlite_budget_limit_blocks_request()
async fn v2p1_sqlite_daily_costs_written()
async fn v2p1_sqlite_analytics_overview_returns_correct_counts()

// UUID stored/retrieved correctly
async fn v2p1_sqlite_uuids_round_trip_correctly()
// Decimal precision maintained
async fn v2p1_sqlite_cost_decimal_precision_preserved()
// Timestamps round-trip
async fn v2p1_sqlite_timestamps_round_trip_as_utc()

// Regression: PostgreSQL path unchanged
async fn v2p1_regression_postgres_tests_still_pass_unchanged()
```

**Helper added to `tests/v2/common.rs`:**
```rust
pub async fn spawn_app_sqlite(tmp: &tempfile::TempDir) -> String
```

### Definition of Done

```bash
# PostgreSQL (must still pass)
cargo test

# SQLite (new target)
cargo test --features sqlite v2_1

cargo clippy --features sqlite -- -D warnings
```

---

## 5. Phase V2-2: Webhook Alerts

**Goal**: When a configured threshold is breached, Janus POSTs a notification to a
Slack, Discord, or generic HTTP webhook. Zero manual monitoring required.

This extends the `alerts` table (exists since Phase 0) and the `AlertEngine`
skeleton built in V2-0.

### What to Build

#### 5.1 Webhook Delivery

**New file: `src/alerts/webhook.rs`**

```rust
pub enum WebhookFormat { Slack, Discord, Generic }

pub async fn deliver(url: &str, format: WebhookFormat, alert: &Alert) -> anyhow::Result<()>;
```

Payload formats:

**Slack:**
```json
{ "text": "🚨 Janus Alert: spend_threshold exceeded. Cost: $45.20 / limit: $40.00" }
```

**Discord:**
```json
{ "content": "🚨 Janus Alert: spend_threshold exceeded. Cost: $45.20 / limit: $40.00" }
```

**Generic:**
```json
{
  "alert_id": "uuid",
  "type": "spend_threshold",
  "message": "...",
  "value": 45.20,
  "threshold": 40.00,
  "triggered_at": "2026-05-24T10:00:00Z"
}
```

#### 5.2 Alert Cooldown

After an alert fires, do not fire again until `window_minutes` has elapsed.
Check: `last_triggered IS NULL OR last_triggered < NOW() - INTERVAL '{window_minutes} minutes'`

#### 5.3 New Admin Endpoints

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/admin/alerts` | Create new alert |
| `GET` | `/admin/alerts` | List all alerts |
| `GET` | `/admin/alerts/:id` | Get alert details + history |
| `PATCH` | `/admin/alerts/:id` | Update alert |
| `DELETE` | `/admin/alerts/:id` | Delete alert |
| `POST` | `/admin/alerts/:id/test` | Send test webhook delivery |

#### 5.4 Alert History

Track each firing in a new table.

**New migration: `migrations/0013_alert_history.sql`**
```sql
ALTER TABLE alerts
    ADD COLUMN webhook_url     TEXT,
    ADD COLUMN webhook_format  VARCHAR(20) NOT NULL DEFAULT 'generic',
    ADD COLUMN webhook_secret  TEXT;       -- HMAC-SHA256 signature header

CREATE TABLE alert_history (
    id          UUID PRIMARY KEY,
    alert_id    UUID REFERENCES alerts(id) ON DELETE CASCADE,
    triggered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    value        DECIMAL(12,8),
    message      TEXT,
    delivered    BOOLEAN NOT NULL DEFAULT FALSE,
    error        TEXT
);
CREATE INDEX idx_alert_history_alert ON alert_history(alert_id, triggered_at DESC);
```

### New Files

- `src/alerts/webhook.rs` — webhook delivery logic
- `src/handlers/admin/alerts.rs` — CRUD + test endpoint

### Test Contract

**File:** `tests/v2/v2_2_webhooks.rs`

```rust
// Core webhook delivery
async fn v2p2_spend_threshold_fires_when_budget_exceeded()
async fn v2p2_webhook_post_reaches_configured_url()
async fn v2p2_slack_payload_format_is_valid_json()
async fn v2p2_discord_payload_format_is_valid_json()
async fn v2p2_generic_payload_contains_all_required_fields()
async fn v2p2_webhook_secret_hmac_header_added_when_configured()

// Cooldown
async fn v2p2_alert_does_not_fire_twice_within_cooldown_window()
async fn v2p2_alert_fires_again_after_cooldown_expires()

// CRUD
async fn v2p2_create_alert_returns_id()
async fn v2p2_update_alert_changes_threshold()
async fn v2p2_delete_alert_removes_it()
async fn v2p2_inactive_alert_does_not_fire()

// Test endpoint
async fn v2p2_test_endpoint_delivers_webhook_regardless_of_threshold()

// History
async fn v2p2_alert_history_recorded_after_firing()
async fn v2p2_failed_delivery_recorded_with_error()

// Regression
async fn v2p2_regression_gateway_proxy_unaffected()
```

### Definition of Done

```bash
cargo test v2_2
cargo test
cargo clippy -- -D warnings
```

---

## 6. Phase V2-3: Extended API Compatibility

**Goal**: Expand OpenAI-compatible surface area. Users can point their embedding
workflows, tool-use apps, and vision pipelines at Janus without code changes.

### What to Build

#### 6.1 `/v1/embeddings`

```
POST /v1/embeddings
{
  "model": "text-embedding-3-small",
  "input": "The sky is blue",  // string or array of strings
  "encoding_format": "float"   // optional
}
```

Response: OpenAI `EmbeddingResponse` format.

Cost tracking: log to `requests` table with `request_type = 'embedding'`.
Cache: skip semantic cache (embeddings are not completions), but exact cache applies.

**New Rust types in `src/providers/mod.rs`:**
```rust
pub struct EmbeddingRequest { pub model: String, pub input: Value }
pub struct EmbeddingResponse { /* OpenAI-identical */ }
```

Extend `Provider` trait:
```rust
async fn embeddings(
    &self,
    request: &EmbeddingRequest,
) -> Result<EmbeddingResponse, ProviderError>;
```

#### 6.2 `/v1/models`

```
GET /v1/models
```

Returns the union of models from all enabled providers, formatted as OpenAI's
`/v1/models` response. Models are read from the `model_pricing` table.

#### 6.3 Tool Use / Function Calling Pass-Through

Extend `ChatCompletionRequest` with:
```rust
pub tools: Option<serde_json::Value>,     // OpenAI tools array
pub tool_choice: Option<serde_json::Value>,
pub parallel_tool_calls: Option<bool>,
pub response_format: Option<serde_json::Value>,
```

These fields are forwarded verbatim to providers that support them.
The exact cache key must include these fields (they change the semantic of the request).

**Files to modify:** `src/providers/mod.rs`, `src/cache/exact.rs`

#### 6.4 Vision / Multi-Modal Pass-Through

`ChatMessage.content` is already `serde_json::Value`, so image URLs and base64
content arrays already pass through unchanged. Add an explicit test to document this.

#### 6.5 `/v1/completions` (Legacy)

Low priority. Accept the request, convert to chat format internally, return in legacy format.

### New Migration

```
migrations/0014_add_request_type.sql  ← Add request_type column to requests
```

```sql
ALTER TABLE requests ADD COLUMN request_type VARCHAR(20) NOT NULL DEFAULT 'chat';
CREATE INDEX idx_requests_type ON requests(request_type);
```

### Test Contract

**File:** `tests/v2/v2_3_api_compat.rs`

```rust
// Embeddings
async fn v2p3_embeddings_endpoint_returns_openai_format()
async fn v2p3_embeddings_request_logged_with_cost()
async fn v2p3_embeddings_exact_cache_hit_on_identical_input()
async fn v2p3_embeddings_string_and_array_inputs_both_work()

// Models list
async fn v2p3_models_endpoint_returns_active_models()
async fn v2p3_models_endpoint_requires_no_auth()   // OpenAI /v1/models is public
async fn v2p3_models_list_contains_all_enabled_providers()

// Tool use
async fn v2p3_tool_call_fields_passed_through_to_provider()
async fn v2p3_tool_call_included_in_exact_cache_key()
async fn v2p3_identical_tool_call_request_returns_cache_hit()

// Vision
async fn v2p3_image_url_content_passes_through_unchanged()
async fn v2p3_base64_image_content_passes_through_unchanged()

// Legacy completions
async fn v2p3_completions_endpoint_accepts_legacy_format()

// Regression
async fn v2p3_regression_chat_completions_still_work()
async fn v2p3_regression_streaming_still_works()
```

### Definition of Done

```bash
cargo test v2_3
cargo test
cargo clippy -- -D warnings
```

---

## 7. Phase V2-4: Intelligent Routing

**Goal**: Route requests to the best provider based on cost, latency, or load —
not just a static priority list. Zero code changes for existing users (priority
routing remains the default).

### What to Build

#### 7.1 Routing Strategy Enum

```rust
// src/gateway/router.rs
pub enum RoutingStrategy {
    Priority,        // current behavior — unchanged default
    CostOptimized,   // pick cheapest capable provider
    LatencyOptimized,// pick provider with lowest recent p95
    RoundRobin,      // distribute evenly across capable providers
}
```

#### 7.2 Strategy-Aware `select_provider()`

Each API key gains a `routing_strategy` field (migration 0015).
The pipeline passes `api_key.routing_strategy` to `select_provider()`.

**Cost router:**
- Load `model_pricing` for the requested model from each provider
- Pick the provider with the lowest `input_per_1m_tokens + output_per_1m_tokens`

**Latency router:**
- Read rolling 15-minute p95 from `requests` table per provider
- Pick the provider with the lowest p95 (fallback to priority if no data)

**Round-robin:**
- Atomic counter in `ProviderRegistry`, incremented on each call
- `counter % len(enabled_providers)` selects the provider

#### 7.3 Model Fallback Chains

New config section in `janus.toml`:
```toml
[routing.fallbacks]
"gpt-4o"     = ["claude-3-5-sonnet-20241022", "gpt-4o-mini"]
"claude-3-5-sonnet-20241022" = ["gpt-4o", "claude-3-5-haiku-20241022"]
```

When a provider fails for a model, try the fallback models on the remaining providers.
This is a more specific form of failover than the current "try all providers" loop.

#### 7.4 New Migration

```
migrations/0015_add_routing_strategy.sql
```

```sql
ALTER TABLE api_keys
    ADD COLUMN routing_strategy VARCHAR(20) NOT NULL DEFAULT 'priority';

CREATE TABLE routing_fallbacks (
    id              UUID PRIMARY KEY,
    model_id        VARCHAR(100) NOT NULL,
    fallback_model  VARCHAR(100) NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 1,
    UNIQUE(model_id, fallback_model)
);
```

### New Files

- `src/gateway/strategies/cost.rs`
- `src/gateway/strategies/latency.rs`
- `src/gateway/strategies/round_robin.rs`

### Test Contract

**File:** `tests/v2/v2_4_routing.rs`

```rust
// Cost routing
async fn v2p4_cost_router_picks_cheapest_provider_for_model()
async fn v2p4_cost_router_falls_back_to_priority_when_pricing_unknown()

// Latency routing
async fn v2p4_latency_router_picks_provider_with_lowest_p95()
async fn v2p4_latency_router_falls_back_to_priority_on_no_data()

// Round-robin
async fn v2p4_round_robin_distributes_across_three_providers()
async fn v2p4_round_robin_skips_disabled_providers()

// Fallback chains
async fn v2p4_model_fallback_chain_activated_on_provider_error()
async fn v2p4_fallback_chain_respects_priority_order()

// Default unchanged
async fn v2p4_priority_routing_unchanged_for_existing_keys()
async fn v2p4_strategy_stored_per_api_key()

// Regression
async fn v2p4_regression_proxy_still_reaches_correct_provider()
async fn v2p4_regression_failover_still_works()
```

### Definition of Done

```bash
cargo test v2_4
cargo test
cargo clippy -- -D warnings
```

---

## 8. Phase V2-5: Prompt Management

**Goal**: Save, version, and A/B test prompts from the admin dashboard.
Gateway consumers can reference a prompt by ID instead of embedding it in every call.

This is a standalone subsystem that does NOT modify the gateway pipeline for users
who don't opt in.

### What to Build

#### 8.1 Data Model

**New migration: `migrations/0016_create_prompts.sql`**
```sql
CREATE TABLE prompts (
    id          UUID PRIMARY KEY,
    name        VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE prompt_versions (
    id              UUID PRIMARY KEY,
    prompt_id       UUID REFERENCES prompts(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    content         TEXT NOT NULL,        -- template with {{variable}} placeholders
    system_prompt   TEXT,                 -- optional system message
    is_active       BOOLEAN NOT NULL DEFAULT FALSE,
    ab_weight       INTEGER NOT NULL DEFAULT 100, -- 0–100, for A/B splitting
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(prompt_id, version)
);
CREATE INDEX idx_prompt_versions_prompt ON prompt_versions(prompt_id, version DESC);
```

#### 8.2 Template Engine

**New file: `src/prompts/template.rs`**

```rust
/// Interpolate {{variable}} placeholders in a template string.
/// Variables come from the `X-Janus-Variables: {"key": "value"}` header or
/// from an explicit `variables` field in the request body extension.
pub fn render(template: &str, variables: &HashMap<String, String>) -> String;
```

#### 8.3 Gateway Integration

When a request includes `X-Janus-Prompt: {prompt_id}`:
1. Load the active `prompt_version` for the prompt
2. For A/B: select version by weighted random draw
3. Render the template with variables from `X-Janus-Variables` header
4. Inject the rendered content as the user message (or prepend to messages array)
5. Record `prompt_version_id` in the `requests` row

**New column:** `requests.prompt_version_id UUID REFERENCES prompt_versions(id)`

This is added in the same migration 0016.

#### 8.4 Admin API

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/admin/prompts` | Create prompt |
| `GET` | `/admin/prompts` | List prompts (paginated) |
| `GET` | `/admin/prompts/:id` | Get prompt + all versions |
| `POST` | `/admin/prompts/:id/versions` | Create new version |
| `PATCH` | `/admin/prompts/:id/versions/:version` | Update version (is_active, ab_weight) |
| `DELETE` | `/admin/prompts/:id` | Delete prompt + all versions |

#### 8.5 A/B Analytics

Extend `GET /admin/analytics/overview` and `GET /admin/analytics/costs` to group by
`prompt_version_id` when available. This lets users see which prompt version costs less
or has higher cache hit rates.

### New Files

- `src/prompts/mod.rs`
- `src/prompts/template.rs`
- `src/db/prompts.rs`
- `src/handlers/admin/prompts.rs`

### Test Contract

**File:** `tests/v2/v2_5_prompts.rs`

```rust
// Template engine (unit tests, no DB)
fn v2p5_template_single_variable_interpolated()
fn v2p5_template_multiple_variables_interpolated()
fn v2p5_template_missing_variable_leaves_placeholder()
fn v2p5_template_extra_variables_ignored()

// CRUD
async fn v2p5_create_prompt_returns_id()
async fn v2p5_create_version_increments_version_number()
async fn v2p5_activate_version_deactivates_previous()
async fn v2p5_delete_prompt_cascades_to_versions()

// Gateway integration
async fn v2p5_x_janus_prompt_header_loads_active_version()
async fn v2p5_template_variables_rendered_before_send_to_provider()
async fn v2p5_prompt_version_id_recorded_in_request_log()
async fn v2p5_unknown_prompt_id_returns_404()

// A/B testing
async fn v2p5_ab_test_distributes_traffic_by_weight()
async fn v2p5_ab_test_both_versions_appear_in_request_logs()
async fn v2p5_weight_zero_version_never_selected()

// Regression
async fn v2p5_regression_requests_without_prompt_header_work_unchanged()
async fn v2p5_regression_cache_still_works_for_non_prompt_requests()
```

### Definition of Done

```bash
cargo test v2_5
cargo test
cargo clippy -- -D warnings
```

---

## 9. Phase V2-6: Multi-Node Clustering

**Goal**: Run multiple Janus instances behind a load balancer with shared state.
Rate limits and budgets are enforced globally, not per-node.

This is the most architecturally significant v2 phase. It changes fundamental
assumptions from v1. Only attempt after V2-0 through V2-5 are complete and stable.

### Architecture Decision

Rate limiting moves from `DashMap<Uuid, VecDeque<Instant>>` (per-node memory) to
PostgreSQL-backed sliding windows (globally consistent). This adds ~5ms to rate-limited
requests — acceptable because rate limit checks only run before the first provider call,
not on every cached hit.

### What to Build

#### 9.1 Distributed Rate Limiter

Replace `src/middleware/rate_limit.rs` `DashMap` logic with a PostgreSQL strategy:

**New migration: `migrations/0017_distributed_rate_limit.sql`**
```sql
CREATE TABLE rate_limit_windows (
    api_key_id   UUID NOT NULL,
    request_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tokens       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_rate_limit_windows_key ON rate_limit_windows(api_key_id, request_at DESC);
```

Check:
```sql
SELECT COUNT(*), COALESCE(SUM(tokens), 0)
FROM rate_limit_windows
WHERE api_key_id = $1
  AND request_at > NOW() - INTERVAL '1 minute';
```

Insert on every request (non-blocking, fire-and-forget).
Cleanup: background task deletes rows older than 2 minutes every 60 seconds.

Keep the in-memory `DashMap` path as the default for single-node deployments.
Enable distributed mode via config:
```toml
[cluster]
enabled = false              # default: false (single-node, in-memory rate limiting)
node_id = "node-1"
```

#### 9.2 Distributed Budget Enforcement

Budget enforcement already uses `UPDATE api_keys SET budget_used = budget_used + $1`
which is atomic per-row in PostgreSQL. No change needed.

Add `SELECT ... FOR UPDATE SKIP LOCKED` to the budget check to prevent double-spend
race conditions under high concurrency.

#### 9.3 Key Cache Invalidation

When an API key is revoked via `DELETE /admin/keys/:id`, currently only the local
`DashMap` is updated. Other nodes don't know.

**New migration adds a `PostgreSQL LISTEN/NOTIFY` pattern:**
```sql
-- In the DELETE handler, after DB delete:
NOTIFY api_key_invalidated, '{key_sha256_hex}';
```

Each node subscribes to `api_key_invalidated` on startup and removes the key from
its local `DashMap` when a notification arrives.

**New file: `src/cluster/key_sync.rs`** — background task for pg_notify subscription.

#### 9.4 Semantic Cache Index (Per-Node Acceptable)

The in-memory HNSW/linear-scan semantic index remains per-node in v2.
Semantic cache hits on other nodes are cache misses — this is acceptable.
The exact cache (PostgreSQL-backed) is already shared.

#### 9.5 Config Changes

```toml
[cluster]
enabled = false
node_id = "node-1"        # for log correlation
```

### New Files

- `src/cluster/mod.rs`
- `src/cluster/key_sync.rs`
- `src/cluster/rate_limit.rs`

### Test Contract

**File:** `tests/v2/v2_6_clustering.rs`

```rust
// Distributed rate limiting
async fn v2p6_rate_limit_enforced_globally_across_two_nodes()
async fn v2p6_cleanup_task_removes_old_rate_limit_rows()

// Budget enforcement
async fn v2p6_budget_atomic_under_concurrent_requests()

// Key invalidation
async fn v2p6_key_revocation_propagates_via_notify()
async fn v2p6_revoked_key_rejected_on_second_node_after_propagation()

// Exact cache shared
async fn v2p6_exact_cache_hit_on_second_node_after_first_node_populates()

// Single-node mode unchanged
async fn v2p6_single_node_mode_uses_in_memory_rate_limit()
async fn v2p6_cluster_disabled_by_default()

// Regression
async fn v2p6_regression_gateway_still_fast_in_single_node_mode()
async fn v2p6_regression_auth_still_enforced()
```

### Definition of Done

```bash
cargo test v2_6
cargo test
cargo clippy -- -D warnings
```

---

## 10. Phase V2-7: MCP Server

**Goal**: Expose Janus as an MCP (Model Context Protocol) server so LLMs like Claude
can call Janus tools directly — proxy requests, query analytics, manage API keys.

### What to Build

#### 10.1 MCP Transport

Two transports:
- **Stdio** — for local LLM integration (Claude Desktop, etc.)
- **HTTP/SSE** — for remote integration at `GET /mcp/sse`

Both follow the MCP spec: JSON-RPC 2.0 framing over the transport.

#### 10.2 Tools

| Tool | Description | Input | Output |
|---|---|---|---|
| `proxy_llm_request` | Send a chat completion via Janus | `{model, messages, stream?}` | completion or stream |
| `get_usage_stats` | Summary of requests + cost | `{period: "today"\|"7d"\|"30d"}` | stats object |
| `list_api_keys` | List all API keys | none | keys array |
| `create_api_key` | Create a new API key | `{name, budget_limit?}` | `{key, id}` |
| `get_cache_stats` | Cache hit rate + savings | none | cache stats |
| `flush_cache` | Clear all cache entries | none | `{flushed: N}` |

#### 10.3 New Route

```
GET /mcp/sse     → SSE transport for MCP (HTTP-based clients)
```

Stdio transport is started as a separate binary mode:
```bash
janus --mcp-stdio     # reads JSON-RPC from stdin, writes to stdout
```

#### 10.4 Auth

MCP clients authenticate with the same admin JWT token:
```json
{ "method": "initialize", "params": { "token": "eyJ..." } }
```

### New Files

- `src/mcp/mod.rs`
- `src/mcp/tools.rs`
- `src/mcp/transport/stdio.rs`
- `src/mcp/transport/sse.rs`
- `src/handlers/mcp.rs`

### New Dependencies

```toml
# Cargo.toml — Phase V2-7
serde_json = "1"   # already present
# No new heavy deps — implement JSON-RPC 2.0 directly (it's simple enough)
```

### Test Contract

**File:** `tests/v2/v2_7_mcp.rs`

```rust
// Tool schema validation
fn v2p7_all_tools_have_valid_json_schema()
fn v2p7_tool_inputs_correctly_validated()

// Tool execution
async fn v2p7_proxy_llm_request_returns_completion()
async fn v2p7_get_usage_stats_returns_valid_data()
async fn v2p7_list_api_keys_returns_array()
async fn v2p7_create_api_key_returns_new_key()
async fn v2p7_get_cache_stats_returns_valid_data()
async fn v2p7_flush_cache_clears_entries()

// Transport
async fn v2p7_sse_transport_sends_json_rpc_events()
async fn v2p7_invalid_method_returns_error_response()
async fn v2p7_unauthenticated_request_rejected()

// Regression
async fn v2p7_regression_gateway_unaffected_by_mcp_server()
```

### Definition of Done

```bash
cargo test v2_7
cargo test
cargo clippy -- -D warnings
```

---

## 11. Migration Plan

| Migration | Phase | Description |
|---|---|---|
| 0001–0011 | V1 (done) | Existing schema |
| 0012 | V2-0 | Add `webhook_url`, `webhook_secret`, `webhook_format` to `alerts` |
| 0013 | V2-2 | Create `alert_history` table |
| 0014 | V2-3 | Add `request_type` column to `requests` |
| 0015 | V2-4 | Add `routing_strategy` to `api_keys`; create `routing_fallbacks` table |
| 0016 | V2-5 | Create `prompts`, `prompt_versions`; add `prompt_version_id` to `requests` |
| 0017 | V2-6 | Create `rate_limit_windows` table |

> **Rule (from DECISIONS.md D-003)**: Never modify existing migrations. Each change is a new file.

SQLite migrations live in `migrations/sqlite/` — parallel set, maintained alongside PostgreSQL.

---

## 12. Dependency Plan

> Add dependencies ONLY in the phase listed.

### Phase V2-0
```toml
# No new dependencies — all needed pieces are already present
```

### Phase V2-1
```toml
sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "postgres", "sqlite",
    "any", "uuid", "chrono", "macros", "rust_decimal"] }
# NOTE: Add "sqlite" and "any" to the existing sqlx feature list
```

### Phase V2-2
```toml
# No new dependencies — reqwest (for webhooks) already present
hmac = "0.12"     # HMAC-SHA256 webhook signature
```

### Phase V2-3
```toml
# No new dependencies
```

### Phase V2-4
```toml
# No new dependencies
```

### Phase V2-5
```toml
# No new dependencies — template engine is ~30 lines of regex
```

### Phase V2-6
```toml
# No new dependencies — pg_notify via sqlx::PgListener
```

### Phase V2-7
```toml
# No new dependencies — JSON-RPC 2.0 is implemented directly
```

---

## 13. V2 Phase Status Tracker

| Phase | Description | Status | Migration |
|---|---|---|---|
| V2-0 | Spec Completion | ✅ Complete (2026-05-23) | 0012 |
| V2-1 | SQLite Support | ✅ Complete (2026-05-23) | (sqlite dir) |
| V2-2 | Webhook Alerts | ✅ Complete (2026-05-23) | 0013 |
| V2-3 | Extended API Compat | ✅ Complete (2026-05-23) | 0014 |
| V2-4 | Intelligent Routing | ✅ Complete (2026-05-23) | 0015 |
| V2-5 | Prompt Management | ✅ Complete (2026-05-23) | 0016 |
| V2-6 | Multi-Node Clustering | ✅ Complete (2026-05-23) | 0017 |
| V2-7 | MCP Server | ✅ Complete (2026-05-23) | — |

---

## Session Start Ritual for V2 Work

At the start of every V2 session, run:

```bash
# 1. Confirm V1 is still green
cargo test 2>&1 | tail -20

# 2. Check current V2 phase status (this file, §13)

# 3. Run only the phase tests you're currently working on
cargo test v2_0   # (or v2_1, v2_2, etc.)

# 4. Confirm which phase we're in and what's left
# Tell the user: "We are on Phase V2-X. Last completed test: Y. Ready to continue."
```

**Do NOT write any code until you have done all 4 steps.**

---

*Created: 2026-05-23 — based on v0.1.0 (commit 97b4c2a)*
*Update the Phase Status Tracker (§13) at the end of every session.*
