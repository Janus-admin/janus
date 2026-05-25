# JANUS V3 — Engineering Roadmap
> Built on V2 (all 8 phases complete, 2026-05-23).
> **If you are Claude: read CLAUDE.md first, then JANUS_V2_ROADMAP.md §13, then this file.**

---

## V3 Philosophy

V2 was about **surface area** — new capabilities, new integrations, wider API coverage.

V3 is about **depth** — fixing what is wrong, hardening what is fragile, making existing
features enterprise-grade. No new product ideas. No speculative features.

> *The difference between an interesting open-source repo and an infrastructure standard
> is not features — it is reliability, observability, and trust.*

**Rules for every V3 phase:**
1. If it doesn't fix a confirmed bug or a confirmed production gap, it doesn't go in.
2. No new user-visible API surface unless it directly supports the phase goal.
3. Every phase must improve the production experience of the features already built.

---

## Table of Contents

1. [V2 Technical Debt Audit](#1-v2-technical-debt-audit)
2. [V3 Testing Philosophy](#2-v3-testing-philosophy)
3. [Phase V3-0: Foundation Fixes](#3-phase-v3-0-foundation-fixes)
4. [Phase V3-1: Semantic Cache Redesign](#4-phase-v3-1-semantic-cache-redesign)
5. [Phase V3-2: OpenTelemetry](#5-phase-v3-2-opentelemetry)
6. [Phase V3-3: Streaming Hardening](#6-phase-v3-3-streaming-hardening)
7. [Phase V3-4: Plugin Middleware](#7-phase-v3-4-plugin-middleware)
8. [Phase V3-5: Security Hardening](#8-phase-v3-5-security-hardening)
9. [What V3 Explicitly Does NOT Include](#9-what-v3-explicitly-does-not-include)
10. [Dependency Plan](#10-dependency-plan)
11. [V3 Phase Status Tracker](#11-v3-phase-status-tracker)

---

## 1. V2 Technical Debt Audit

These are confirmed bugs and doc/code mismatches found by code review, not
speculation. All are fixed in V3-0 before any new work begins.

| Issue | Location | Severity |
|---|---|---|
| CLAUDE.md claims HNSW index; code is O(n) linear scan | `src/cache/semantic.rs:8`, `CLAUDE.md:267` | High |
| CLAUDE.md says semantic threshold default is 0.95; code default is 0.90 | `src/config.rs:182`, `CLAUDE.md:271` | Medium |
| CLAUDE.md architecture diagram says `semantic.rs ← HNSW vector similarity` | `CLAUDE.md:421` | Medium |
| `DELETE /admin/cache` does not flush semantic index entries | `src/cache/mod.rs:84` | Medium |
| `benches/` directory is untracked; benchmarks don't compile as part of CI | `benches/*.rs` | Low |

---

## 2. V3 Testing Philosophy

Inherits V2's regression contract. Every phase gate-in and gate-out:

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

V3 tests live in `tests/v3/`:
```
tests/
├── v2/   ← must stay green, never touch
└── v3/
    ├── common.rs
    ├── v3_0_fixes.rs
    ├── v3_1_semantic_cache.rs
    ├── v3_2_otel.rs
    ├── v3_3_streaming.rs
    ├── v3_4_plugins.rs
    └── v3_5_security.rs
```

Test naming: `v3_{phase}_{feature}_{expected_outcome}` (e.g. `v3_1_hnsw_lookup_returns_above_threshold_entry`)

---

## 3. Phase V3-0: Foundation Fixes

**Goal**: Close all confirmed bugs and doc/code mismatches before any V3 features.
This phase contains zero new features — only corrections.

### 3.1 Fix CLAUDE.md Documentation

The locked architectural decisions section (§11) and the architecture diagram (§13)
both claim HNSW. The actual implementation is a linear scan. Fix in-place:

**`CLAUDE.md §11` change:**
```
Layer 2 — Semantic match:
  Key:    Cosine similarity over sentence embeddings (linear scan; HNSW in V3-1)
  Store:  Vec<(embedding, hash)> in memory + embeddings in PostgreSQL (BYTEA)
  Speed:  < 10ms (degrades linearly with entry count)
  Threshold: 0.90 cosine similarity (configurable via semantic_cache_threshold)
```

**`CLAUDE.md §13` change:**
```
└── semantic.rs      ← Linear cosine scan (HNSW planned for V3-1)
```

**`CLAUDE.md Common Mistakes #10` change:**
```
10. **Semantic index is in-memory only (linear scan over Vec).** It is rebuilt from
    PostgreSQL embeddings at startup via `warm_from_db()`. No snapshot file needed —
    the source of truth is the DB. HNSW indexing is planned for V3-1.
```

**Files to modify:** `CLAUDE.md` only.

### 3.2 Fix Semantic Cache Flush

`DELETE /admin/cache` calls `cache.clear()` which clears the DashMap hot layer
but explicitly skips the semantic `Vec`. This is a bug: a user who flushes the cache
still gets semantic hits from entries that no longer exist in the hot layer.

The fix: add a `clear_semantic()` method to `SemanticCache` that replaces the inner
`Vec` with an empty one, and call it from `CacheEngine::clear()`.

```rust
// src/cache/semantic.rs — add:
pub fn clear(&self) {
    if let Ok(mut entries) = self.entries.write() {
        entries.clear();
    }
}
```

```rust
// src/cache/mod.rs — fix CacheEngine::clear():
pub fn clear(&self) {
    self.hot.clear();
    self.embedding_hot.clear();
    if let Some(ref sc) = self.semantic {
        sc.clear();
    }
}
```

**Files to modify:** `src/cache/semantic.rs`, `src/cache/mod.rs`

### 3.3 Fix Benchmarks

The `benches/` directory has 4 files that are untracked (not in git). They reference
`janus::` types and must compile cleanly. Add them to git and verify they pass:

```bash
cargo bench --no-run   # must compile
```

Add `[[bench]]` entries to `Cargo.toml` if missing and confirm `criterion` dependency
is declared.

**Files to modify:** `Cargo.toml` (bench declarations if missing), commit `benches/`

### Test Contract

**File:** `tests/v3/v3_0_fixes.rs`

```rust
// Semantic flush is complete
async fn v3p0_flush_cache_also_clears_semantic_entries()
async fn v3p0_semantic_hit_does_not_occur_after_flush()

// Regression
async fn v3p0_regression_exact_cache_flush_still_works()
async fn v3p0_regression_gateway_proxy_unaffected()
```

### Definition of Done

```bash
cargo test v3_0
cargo test
cargo bench --no-run   # benches compile
cargo clippy -- -D warnings
```

---

## 4. Phase V3-1: Semantic Cache Redesign

**Goal**: Replace the O(n) linear scan with an HNSW approximate nearest-neighbor index.
Add a `trait EmbeddingIndex` so the backend is swappable without changing callers.
Add per-model and per-route exclusion policies.

This is the highest-priority engineering item in V3. The current O(n) scan is the
single biggest performance cliff in the codebase.

### 4.1 EmbeddingIndex Trait

**New file: `src/cache/index.rs`**

```rust
/// Pluggable vector index backend.
/// All implementations must be `Send + Sync`.
pub trait EmbeddingIndex: Send + Sync {
    /// Find the most similar entry. Returns `(hash, similarity)` if above threshold.
    fn lookup(&self, query: &[f32], threshold: f32) -> Option<(String, f32)>;

    /// Insert a new entry into the index.
    fn insert(&self, embedding: Vec<f32>, hash: String);

    /// Remove all entries. Called on cache flush.
    fn clear(&self);

    /// Number of entries in the index.
    fn len(&self) -> usize;
}
```

Two implementations shipped with V3-1:

| Implementation | File | When to Use |
|---|---|---|
| `LinearIndex` | `src/cache/index/linear.rs` | Default; wraps existing `SemanticCache` logic |
| `HnswIndex` | `src/cache/index/hnsw.rs` | Enabled via `semantic_cache_backend = "hnsw"` config |

`SemanticCache` is refactored to hold `Box<dyn EmbeddingIndex>` internally.
Existing `semantic.rs` linear logic moves to `index/linear.rs`.

### 4.2 HNSW Implementation

Use the `hnsw_rs` crate (pure Rust, no native deps, no `unsafe` for the caller).

```toml
# Cargo.toml — V3-1
hnsw_rs = "0.3"
```

**`src/cache/index/hnsw.rs`:**
```rust
use hnsw_rs::prelude::*;
use parking_lot::RwLock;
use std::collections::HashMap;

pub struct HnswIndex {
    hnsw: RwLock<Hnsw<f32, DistCosine>>,
    hash_map: RwLock<HashMap<usize, String>>,  // data-point id → prompt_hash
    next_id: std::sync::atomic::AtomicUsize,
}
```

Parameters (configurable via `janus.toml`):
```toml
[semantic_cache]
backend        = "hnsw"     # "linear" (default) or "hnsw"
hnsw_ef_construction = 200
hnsw_max_nb_connection = 16
```

### 4.3 Exclusion Policies

Add a `SemanticCachePolicy` section to config:

```toml
[semantic_cache]
enabled = true

# Only cache for these models (empty = all models)
models = ["gpt-4o-mini", "claude-3-5-haiku-20241022"]

# Never cache routes matching these prefixes
exclude_routes = []

# Disable semantic cache for specific API keys by name
exclude_keys = []
```

The gateway handler checks policy before calling `semantic_lookup()`.

**New file: `src/cache/policy.rs`**
```rust
pub struct SemanticCachePolicy { /* fields mirror config */ }

impl SemanticCachePolicy {
    pub fn allows(&self, model: &str, route: &str, api_key_name: &str) -> bool;
}
```

### 4.4 Config Changes

```rust
// src/config.rs additions
pub semantic_cache_backend: String,       // "linear" (default) or "hnsw"
pub semantic_cache_hnsw_ef: usize,        // default 200
pub semantic_cache_hnsw_connections: usize, // default 16
pub semantic_cache_models: Vec<String>,   // default empty (all)
pub semantic_cache_exclude_routes: Vec<String>, // default empty
```

### New Files

- `src/cache/index.rs` — `EmbeddingIndex` trait
- `src/cache/index/linear.rs` — existing linear scan, refactored
- `src/cache/index/hnsw.rs` — HNSW backend
- `src/cache/policy.rs` — exclusion policy checker

### Test Contract

**File:** `tests/v3/v3_1_semantic_cache.rs`

```rust
// HNSW backend
fn v3p1_hnsw_lookup_returns_above_threshold_entry()
fn v3p1_hnsw_lookup_returns_none_below_threshold()
fn v3p1_hnsw_clear_empties_index()
fn v3p1_hnsw_insert_then_lookup_finds_entry()

// Trait abstraction
fn v3p1_linear_index_implements_embedding_index_trait()
fn v3p1_hnsw_index_implements_embedding_index_trait()

// Policy
fn v3p1_policy_allows_all_when_models_list_empty()
fn v3p1_policy_denies_unlisted_model()
fn v3p1_policy_denies_excluded_route_prefix()

// Integration
async fn v3p1_hnsw_backend_hits_on_similar_prompt()
async fn v3p1_linear_backend_still_works_unchanged()
async fn v3p1_policy_blocks_semantic_cache_for_excluded_model()

// Scale: verify O(log n) vs O(n) difference exists
fn v3p1_hnsw_lookup_faster_than_linear_at_1000_entries()

// Regression
async fn v3p1_regression_exact_cache_unaffected()
async fn v3p1_regression_semantic_flush_clears_hnsw_index()
```

### Definition of Done

```bash
cargo test v3_1
cargo test
cargo clippy -- -D warnings
```

---

## 5. Phase V3-2: OpenTelemetry

**Goal**: Add distributed tracing to the request pipeline.
Every proxied request produces a trace with spans for each major stage.
Traces export via OTLP to any compatible backend (Jaeger, Grafana Tempo, Honeycomb).

This closes the biggest enterprise observability gap.

### 5.1 What Gets a Span

Every call to `POST /v1/chat/completions` produces a root span with child spans:

```
janus.request [root]
├── janus.cache.exact_lookup
├── janus.cache.semantic_lookup     (if semantic enabled)
├── janus.rate_limit.check
├── janus.budget.check
├── janus.provider.call [provider=openai, model=gpt-4o]
│   └── janus.provider.http         (raw HTTP roundtrip)
└── janus.cache.insert              (on miss + successful response)
```

Span attributes (OpenTelemetry semantic conventions):
```
janus.provider          = "openai"
janus.model             = "gpt-4o"
janus.cache_hit         = "exact" | "semantic" | "none"
janus.cache_similarity  = 0.9312   (semantic only)
janus.prompt_tokens     = 142
janus.completion_tokens = 88
janus.cost_usd          = 0.00043
janus.api_key_id        = "uuid..."
http.status_code        = 200
```

### 5.2 Trace Context Propagation

Incoming requests with `traceparent` header (W3C Trace Context) are linked to the
upstream trace. This enables end-to-end tracing when Janus sits behind another
instrumented service.

Outgoing requests to providers carry the `traceparent` header so provider-side
tracing (where available) is correlated.

### 5.3 OTLP Export

Configuration:

```toml
[tracing]
enabled          = false           # default off — zero overhead when disabled
otlp_endpoint    = "http://localhost:4317"   # gRPC OTLP
service_name     = "janus"
sample_rate      = 1.0             # 1.0 = 100%, 0.1 = 10%
```

When `tracing.enabled = false`, no spans are created and no overhead is incurred.
This is enforced via a `NoopTracer` compile path, not a runtime boolean check.

### 5.4 Dependencies

```toml
# Cargo.toml — V3-2
opentelemetry          = { version = "0.23", features = ["trace"] }
opentelemetry_sdk      = { version = "0.23", features = ["rt-tokio"] }
opentelemetry-otlp     = { version = "0.16", features = ["grpc-tonic"] }
tracing-opentelemetry  = "0.24"
```

### New Files

- `src/telemetry.rs` — tracer init, shutdown, span helper macros

### Test Contract

**File:** `tests/v3/v3_2_otel.rs`

```rust
// Span creation
async fn v3p2_request_produces_root_span()
async fn v3p2_cache_hit_span_has_correct_attributes()
async fn v3p2_provider_call_span_has_model_attribute()
async fn v3p2_span_includes_token_counts_on_success()
async fn v3p2_span_includes_error_on_provider_failure()

// Propagation
async fn v3p2_incoming_traceparent_header_linked_to_root_span()
async fn v3p2_outgoing_request_to_provider_carries_traceparent()

// Config
async fn v3p2_tracing_disabled_produces_no_spans()
async fn v3p2_tracing_enabled_exports_to_configured_endpoint()

// Regression
async fn v3p2_regression_gateway_latency_unchanged_when_tracing_disabled()
```

### Definition of Done

```bash
cargo test v3_2
cargo test
cargo clippy -- -D warnings
```

---

## 6. Phase V3-3: Streaming Hardening

**Goal**: Fix three confirmed fragile behaviors in the streaming path.

The current implementation works under happy-path conditions. Under adversarial
or real-world conditions (client disconnects mid-stream, slow consumers, provider
returns partial data then errors), the behavior is undefined or resource-leaking.

### 6.1 Client Disconnect Propagation

**Problem**: When the client disconnects during an SSE stream, the Tokio task
driving the provider HTTP connection continues until the provider finishes. On
long-running streams with expensive models this wastes money and ties up connections.

**Fix**: Use axum's `on_upgrade` / connection close signal to cancel the provider task.

```rust
// src/gateway/pipeline.rs — run_streaming() change:
// Pass a CancellationToken into the spawn task.
// Axum closes the channel when the client disconnects.
// The provider task checks the token and aborts the HTTP stream.
```

Uses the `tokio-util` `CancellationToken` — already a transitive dependency.

### 6.2 Backpressure in SSE Channel

**Problem**: `tokio::sync::mpsc` channel between the stream-reading task and the SSE
response is currently unbounded (`mpsc::channel(100)` — actually bounded, but no
slowdown path). A slow consumer causes the channel to fill, then the `send()` fails
silently and the stream is truncated without error.

**Fix**: Convert to a bounded channel and handle `TrySendError::Full` by yielding
with backpressure rather than dropping.

### 6.3 Stream Error Recovery

**Problem**: If a provider returns an HTTP 200 then sends an error chunk mid-stream
(e.g., OpenAI's `{"error": {...}}` as a data event), the pipeline currently forwards
it as-is. The `requests` table records `status = "success"` even though the response
was partial.

**Fix**: Parse error events in the stream reader; set `status = "error"` in the
request log and include the provider error message.

### New Files

None — all changes are in `src/gateway/pipeline.rs` and `src/handlers/gateway.rs`.

### Test Contract

**File:** `tests/v3/v3_3_streaming.rs`

```rust
// Client disconnect
async fn v3p3_provider_task_cancelled_when_client_disconnects()
async fn v3p3_no_resource_leak_after_client_disconnect()

// Backpressure
async fn v3p3_slow_consumer_does_not_truncate_stream()
async fn v3p3_full_channel_yields_not_drops()

// Error recovery
async fn v3p3_mid_stream_provider_error_sets_request_status_error()
async fn v3p3_mid_stream_error_includes_error_detail_in_log()

// Regression
async fn v3p3_normal_stream_still_works_end_to_end()
async fn v3p3_streaming_cache_hit_still_works()
```

### Definition of Done

```bash
cargo test v3_3
cargo test
cargo clippy -- -D warnings
```

---

## 7. Phase V3-4: Plugin Middleware

**Goal**: Allow custom logic to be injected into the request pipeline without
modifying core code. First-party use case: move PII scrubbing from a hardcoded
call to a plugin, so operators can disable it or extend it.

This does NOT include WASM plugins. Plugins are Rust trait implementations,
compiled into the binary. Dynamic loading is out of scope for V3.

### 7.1 Plugin Trait

**New file: `src/plugins/mod.rs`**

```rust
use crate::providers::{ChatCompletionRequest, ChatCompletionResponse};
use crate::models::api_key::ApiKey;

#[async_trait::async_trait]
pub trait RequestPlugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Called after auth/rate-limit, before cache lookup.
    /// May modify the request or return an error to abort the pipeline.
    async fn before_request(
        &self,
        request: &mut ChatCompletionRequest,
        api_key: &ApiKey,
    ) -> Result<(), PluginError>;

    /// Called after a successful provider response (or cache hit).
    /// May modify the response or record side effects.
    async fn after_response(
        &self,
        request: &ChatCompletionRequest,
        response: &mut ChatCompletionResponse,
        api_key: &ApiKey,
    ) -> Result<(), PluginError>;
}

pub enum PluginError {
    /// Abort the request with HTTP 400 and this message.
    BadRequest(String),
    /// Abort the request with HTTP 403 and this message.
    Forbidden(String),
    /// Log the error and continue (non-fatal).
    Warning(String),
}
```

### 7.2 Plugin Chain

`AppState` gains a `plugins: Arc<Vec<Box<dyn RequestPlugin>>>` field.

The pipeline calls `before_request` on each plugin in order, then calls
`after_response` in reverse order (like HTTP middleware stacks).

### 7.3 Built-in Plugins (shipped with V3-4)

| Plugin | Config Key | Default |
|---|---|---|
| `PiiRedactionPlugin` | `plugins.pii_redaction.enabled` | `true` |
| `ContentLengthPlugin` | `plugins.max_prompt_chars` | `0` (disabled) |

`PiiRedactionPlugin` wraps the existing `src/pii.rs` scrubber. Moving it to a
plugin means operators can disable PII scrubbing via config (at their own risk).

### 7.4 Config

```toml
[plugins]
# PII redaction is on by default; set false only if you control all traffic
pii_redaction = true

# Reject requests where total message content exceeds this many characters.
# 0 = no limit.
max_prompt_chars = 0
```

Plugins are instantiated in `main.rs` based on config, before `AppState` is built.

### New Files

- `src/plugins/mod.rs` — trait + `PluginError`
- `src/plugins/pii.rs` — wraps existing `src/pii.rs`
- `src/plugins/content_length.rs` — max prompt size guard

### Test Contract

**File:** `tests/v3/v3_4_plugins.rs`

```rust
// Trait behavior
fn v3p4_before_request_bad_request_aborts_pipeline()
fn v3p4_before_request_forbidden_returns_403()
fn v3p4_before_request_warning_continues_pipeline()
fn v3p4_after_response_can_modify_response()

// Plugin chain ordering
fn v3p4_plugins_called_in_order_before()
fn v3p4_plugins_called_in_reverse_order_after()
fn v3p4_second_plugin_not_called_when_first_returns_error()

// Built-in plugins
fn v3p4_pii_plugin_redacts_credit_card_in_request()
async fn v3p4_pii_plugin_disabled_by_config_skips_redaction()
async fn v3p4_content_length_plugin_rejects_oversized_prompt()
async fn v3p4_content_length_zero_means_no_limit()

// Regression
async fn v3p4_no_plugins_configured_pipeline_works_unchanged()
async fn v3p4_regression_gateway_proxy_unaffected()
```

### Definition of Done

```bash
cargo test v3_4
cargo test
cargo clippy -- -D warnings
```

---

## 8. Phase V3-5: Security Hardening

**Goal**: Three concrete security improvements with clear threat models.
No enterprise SSO or SCIM — those are V4 territory. Only what is
provably missing and provably needed for production use.

### 8.1 mTLS for Provider Connections

**Problem**: All provider connections use standard TLS. There is no way for an
operator to pin the provider certificate or present a client certificate.

**Fix**: Add optional mTLS config per provider. The reqwest client is built with
a custom `TlsConnector` when certs are configured.

```toml
[provider_tls]
# Path to PEM-encoded CA cert for pinning (optional)
ca_cert_path = ""

# Client cert + key for mTLS (optional, both required together)
client_cert_path = ""
client_key_path  = ""
```

### 8.2 API Key Rotation Endpoint

**Problem**: There is no way to rotate an API key (generate a new secret while
keeping the same ID, budget, and rate limit settings) without deleting and
recreating it. Rotation requires a short window where two keys are valid.

**New endpoint:**
```
POST /admin/keys/:id/rotate
```

Returns a new full key. The old key remains valid for `rotation_grace_period_secs`
(configurable, default 300 seconds). After the grace period, the old hash is zeroed
in the DB and removed from the `key_cache` DashMap.

### 8.3 Audit Log API

**Problem**: The `requests` table contains a full audit log but there is no
filtering API for compliance use cases. `GET /admin/requests` exists but has
limited filter support.

**Extend `GET /admin/requests`** with:

| Filter | Type | Description |
|---|---|---|
| `api_key_id` | UUID | Filter by key |
| `provider` | string | Filter by provider name |
| `status` | string | `success` / `error` |
| `start_time` | RFC3339 | Inclusive lower bound |
| `end_time` | RFC3339 | Inclusive upper bound |
| `has_cache_hit` | bool | Only cached or only live responses |

Response adds `X-Janus-Audit-Hash` header: SHA-256 of the response body, so
the caller can verify the log hasn't been tampered with since export.

### New Migration

```
migrations/0018_add_key_rotation.sql
```
```sql
ALTER TABLE api_keys
    ADD COLUMN previous_key_sha256   BYTEA,
    ADD COLUMN rotation_expires_at   TIMESTAMPTZ;
```

The auth middleware accepts `key_sha256` OR `previous_key_sha256` when
`rotation_expires_at` is in the future.

### Test Contract

**File:** `tests/v3/v3_5_security.rs`

```rust
// mTLS config
async fn v3p5_invalid_ca_cert_path_fails_at_startup()
async fn v3p5_missing_client_key_with_cert_fails_at_startup()

// Key rotation
async fn v3p5_rotate_returns_new_key()
async fn v3p5_old_key_still_valid_within_grace_period()
async fn v3p5_old_key_rejected_after_grace_period_expires()
async fn v3p5_new_key_valid_immediately_after_rotation()
async fn v3p5_rotate_nonexistent_key_returns_404()

// Audit log
async fn v3p5_audit_log_filters_by_api_key_id()
async fn v3p5_audit_log_filters_by_date_range()
async fn v3p5_audit_log_filters_by_status()
async fn v3p5_audit_log_response_includes_hash_header()
async fn v3p5_audit_log_hash_matches_body_sha256()

// Regression
async fn v3p5_regression_existing_key_auth_unaffected()
async fn v3p5_regression_gateway_proxy_unaffected()
```

### Definition of Done

```bash
cargo test v3_5
cargo test
cargo clippy -- -D warnings
cargo build --release
```

---

## 9. What V3 Explicitly Does NOT Include

These were evaluated and rejected for V3. They may be revisited in V4.

| Item | Why Rejected |
|---|---|
| WASM / Lua plugins | High maintenance, low adoption, V3-4 Rust plugins cover real needs |
| SCIM / SSO / SAML | Requires an identity provider integration layer; V4 work |
| CEL policy engine | V3-4 plugin system handles the same use cases more cleanly |
| Managed cloud plane | Product strategy, not engineering; out of scope |
| Python / JS SDKs | Separate repositories; don't block core |
| Qdrant / Pinecone integration | V3-1 `EmbeddingIndex` trait enables it; the integration itself is V4 |
| Budget forecasting | Requires ML model on usage patterns; V4 |
| Multi-tenancy hardening (RBAC, billing isolation) | `workspaces` table exists; full RBAC is V4 |

---

## 10. Dependency Plan

> Add dependencies ONLY in the phase listed.

### Phase V3-0
```toml
# No new dependencies
```

### Phase V3-1
```toml
hnsw_rs = "0.3"
```

### Phase V3-2
```toml
opentelemetry          = { version = "0.23", features = ["trace"] }
opentelemetry_sdk      = { version = "0.23", features = ["rt-tokio"] }
opentelemetry-otlp     = { version = "0.16", features = ["grpc-tonic"] }
tracing-opentelemetry  = "0.24"
```

### Phase V3-3
```toml
# No new dependencies — tokio-util (CancellationToken) already present transitively
# Confirm it is in [dependencies] explicitly if not already
```

### Phase V3-4
```toml
async-trait = "0.1"   # if not already present
```

### Phase V3-5
```toml
# No new dependencies — reqwest TLS config uses existing feature flags
```

---

## 11. V3 Phase Status Tracker

| Phase | Description | Status | Key Change |
|---|---|---|---|
| V3-0 | Foundation Fixes | ✅ Complete (2026-05-24) | CLAUDE.md + semantic flush + benches |
| V3-1 | Semantic Cache Redesign | ✅ Complete (2026-05-24) | O(n) → HNSW, EmbeddingIndex trait, SemanticCachePolicy |
| V3-2 | OpenTelemetry | ✅ Complete (2026-05-24) | `src/telemetry.rs`, span tree, OTLP gRPC, W3C propagation, 16 tests |
| V3-3 | Streaming Hardening | ✅ Complete (2026-05-24) | select!+tx.closed() disconnect, bounded-channel backpressure, mid-stream error status |
| V3-4 | Plugin Middleware | ✅ Complete (2026-05-24) | RequestPlugin trait, plugin chain, PiiRedactionPlugin, ContentLengthPlugin, 13 tests |
| V3-5 | Security Hardening | ✅ Complete (2026-05-24) | mTLS validation, key rotation with grace period, extended audit log + X-Janus-Audit-Hash, 16 tests |

---

## Session Start Ritual for V3 Work

```bash
# 1. Confirm V2 is still green
cargo test 2>&1 | tail -20

# 2. Check V3 phase status (this file, §11)

# 3. Run the specific phase tests if work is in progress
cargo test v3_0   # (or v3_1, etc.)

# 4. Tell the user: "We are on Phase V3-X. Ready to continue."
```

**Do NOT write any code until you have done all 4 steps.**

---

*Created: 2026-05-23 — based on V2 complete (all 8 phases, 2026-05-23)*
*Update the Phase Status Tracker (§11) at the end of every session.*
