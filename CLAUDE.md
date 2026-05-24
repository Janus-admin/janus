# CLAUDE.md — Velox Project Master Context
> This file is the single source of truth for every Claude session.
> **If you are Claude: read this file completely before touching any code.**

---

## ⚠️ MANDATORY SESSION START CHECKLIST

Run through this EVERY time — no exceptions:

```bash
# 1. See what has changed since last session
git log --oneline -10
git status

# 2. Verify current state compiles and tests pass
cargo test 2>&1 | tail -20

# 3. Check which phase we are in (bottom of this file)
# 4. Read "Current Phase Contract" section below
# 5. Tell the user: "We are on Phase X. Last completed: Y. Ready to continue."
```

**Do NOT write any code until you have done all 5 steps above.**

---

## Project Identity

| Key | Value |
|---|---|
| Project name | Velox |
| What it is | Self-hosted AI gateway — proxy for LLM calls with caching, cost tracking, streaming |
| What it is NOT | A Firebase clone, a BaaS, a database replacement |
| Primary language | Rust |
| Web framework | axum 0.7 |
| Database | PostgreSQL (docker-compose) + SQLite (`--no-default-features --features sqlite`) |
| Repository | /Users/wallex/rust-backend |
| Roadmap | VELOX_ROADMAP.md (v1), VELOX_V2_ROADMAP.md (v2), VELOX_V3_ROADMAP.md (v3), VELOX_V4_ROADMAP.md (v4), **VELOX_V5_ROADMAP.md (v5 — current)** |
| Decisions | DECISIONS.md |

---

## Current Phase Status

```
Phase 0: Foundation          → [x] COMPLETE
Phase 1: Core Proxy          → [x] COMPLETE
Phase 2: Streaming           → [x] COMPLETE
Phase 3: Rate Limiting       → [x] COMPLETE
Phase 4: Exact Cache         → [x] COMPLETE
Phase 5: Semantic Cache      → [x] COMPLETE
Phase 6: Web Dashboard       → [x] COMPLETE
Phase 7: Production Hardening → [x] COMPLETE
Phase 8: Open Source Launch  → [SKIPPED] — marketing/launch work, not technical
Phase 9: Mobile App          → [SKIPPED] — out of scope for v1
```

**VELOX v0.1.0 IS FEATURE-COMPLETE. V2 is also complete (all 8 phases, 2026-05-23).**

**V4 is fully complete — all 10 phases done (2026-05-24). No remaining V4 work.**
See **VELOX_V4_ROADMAP.md §16** for the full phase history.

**V5 — Market-Readiness — is IN PROGRESS. See VELOX_V5_ROADMAP.md.**
Read that file's §16 (phase status), §17 (locked decisions), and §18 (session start ritual)
before touching any code on a V5 phase.

---

## What Has Been Built

> This section is updated at the end of every phase. It is the ground truth.
> Never assume something is built. Check here first.

### Database Tables (existing from before Velox)
- [x] `users` — admin user accounts (email, password_hash, name)

### API Endpoints (existing from before Velox)
- [x] `GET  /health` — returns status, version, database ping, providers list, cache config
- [x] `POST /api/v1/auth/register` — create user
- [x] `POST /api/v1/auth/login` — login, get JWT
- [x] `GET  /api/v1/auth/me` — get current user (JWT protected)
- [x] `GET  /api/v1/users` — list users (JWT protected)
- [x] `GET  /api/v1/users/:id` — get user (JWT protected)
- [x] `PUT  /api/v1/users/:id` — update user (JWT protected)
- [x] `DELETE /api/v1/users/:id` — delete user (JWT protected)

### Velox-Specific Tables (Phase 0 — all created via migrations)
- [x] `workspaces` — multi-tenancy (migration 0002)
- [x] `api_keys` — gateway API keys with budget/rate limits (migration 0003)
- [x] `providers` — OpenAI/Anthropic/Bedrock configs + health status (migration 0004, seeded)
- [x] `model_pricing` — per-token costs, updatable without redeployment (migration 0005, seeded)
- [x] `requests` — full audit log for every proxied call (migration 0006)
- [x] `cache_entries` — exact + semantic cache storage (migration 0007)
- [x] `daily_costs` — pre-aggregated daily cost rollups (migration 0008)
- [x] `alerts` — threshold-based spend/error/latency alerts (migration 0009)

### Velox-Specific Rust Modules (Phase 0)
- [x] `src/config.rs` — Config struct via `config` crate (TOML + ENV, with defaults); extended in Phase 1 with `openai_api_key`, `anthropic_api_key`
- [x] `src/models/api_key.rs` — ApiKey, CreateApiKeyRequest/Response, ApiKeyView
- [x] `src/models/provider.rs` — Provider, ProviderView, HealthStatus, UpdateProviderRequest
- [x] `src/models/request.rs` — Request, RequestStatus, CacheType, RequestSummary, RequestFilter
- [x] `src/models/cache_entry.rs` — CacheEntry, CacheStats, FlushCacheRequest

### Velox-Specific Rust Modules (Phase 1)
- [x] `src/crypto.rs` — AES-256-GCM encrypt/decrypt for provider API keys at rest
- [x] `src/providers/mod.rs` — `Provider` trait, `ChatCompletionRequest/Response`, `ProviderError`
- [x] `src/providers/openai.rs` — OpenAI adapter (passthrough via reqwest)
- [x] `src/providers/anthropic.rs` — Anthropic adapter (converts OpenAI↔Anthropic Messages API format)
- [x] `src/providers/bedrock.rs` — AWS Bedrock adapter (Converse API via aws-sdk-bedrockruntime)
- [x] `src/gateway/mod.rs` — `ProviderRegistry` (sorted list of enabled providers + key_cache)
- [x] `src/gateway/router.rs` — `select_provider()` (priority-based provider selection)
- [x] `src/gateway/pipeline.rs` — Non-streaming proxy pipeline (select→call→cost→log→return)
- [x] `src/pricing/mod.rs` — `calculate_cost()` pure function using DECIMAL(12,8) arithmetic
- [x] `src/middleware/api_key_auth.rs` — `GatewayAuth` extractor (SHA-256 dashmap lookup, O(1))
- [x] `src/middleware/budget.rs` — `check_budget()` function for spend limit enforcement
- [x] `src/handlers/gateway.rs` — `POST /v1/chat/completions` handler + `ValidatedJson<T>` extractor
- [x] `src/handlers/admin/keys.rs` — `POST /admin/keys`, `GET /admin/keys` (admin API key management)
- [x] `src/state.rs` — Extended with `providers: Arc<ProviderRegistry>`, `key_cache: Arc<DashMap<...>>`
- [x] `migrations/0010_add_api_key_sha256.sql` — Added `key_sha256` column for fast dashmap lookup

### Velox-Specific Rust Modules (Phase 3)
- [x] `src/config.rs` — Added `rate_limit_window_secs: u64` (default 60) and `max_retries: u32` (default 1)
- [x] `src/middleware/rate_limit.rs` — `RateLimiter` struct: sliding window per API key (`DashMap<Uuid, VecDeque<i64>>`); `check_and_record()` returns `Err(retry_after_secs)` when limit exceeded
- [x] `src/gateway/router.rs` — Added `select_all_providers()` returning all enabled providers sorted by priority (used by failover loop)
- [x] `src/gateway/pipeline.rs` — `run()` and `run_streaming()` iterate all providers; per-provider retry loop on `Unavailable`/`Timeout` up to `max_retries`; exhausted retries → next provider; all fail → 503
- [x] `src/handlers/gateway.rs` — Rate limit check (gate 2, after budget) via `state.rate_limiter.check_and_record()`; passes `max_retries` to pipeline
- [x] `src/errors.rs` — `RateLimitExceeded(Option<u64>)` with `Retry-After` header injected in `into_response()` when payload is `Some`
- [x] `src/state.rs` — Added `rate_limiter: Arc<RateLimiter>` field

### Velox-Specific Rust Modules (Phase 2)
- [x] `src/providers/mod.rs` — Extended with `ChunkDelta`, `ChunkChoice`, `ChatCompletionChunk`, `ProviderStream` type alias; `Provider` trait now has `chat_completion_stream`
- [x] `src/providers/openai.rs` — `chat_completion_stream` via `eventsource-stream` passthrough; adds `stream_options: {include_usage: true}` to get usage in final chunk
- [x] `src/providers/anthropic.rs` — `chat_completion_stream` via channel+task; stateful SSE parsing (`message_start`→id/prompt tokens, `content_block_delta`→text, `message_delta`→output tokens)
- [x] `src/providers/bedrock.rs` — `chat_completion_stream` via channel+task using `converse_stream` SDK; normalises `ContentBlockDelta` events
- [x] `src/gateway/pipeline.rs` — Extended with `run_streaming()`: selects provider, drives stream, records TTFB on first chunk, accumulates tokens, logs to DB after stream closes
- [x] `src/handlers/gateway.rs` — `chat_completions` now branches on `request.stream == Some(true)` → SSE response vs JSON response
- [x] `src/db/requests.rs` — `insert_request` extended with `is_stream: bool` and `ttfb_ms: Option<i32>` parameters; uses existing `ttfb_ms` column in `requests` table

### Velox-Specific Rust Modules (Phase 4)
- [x] `src/cache/exact.rs` — `compute_hash()`: SHA-256 of normalized request (stream field excluded)
- [x] `src/cache/mod.rs` — `CacheEngine`: DashMap hot layer; `lookup`, `insert`, `clear`, `check` helpers
- [x] `src/db/cache.rs` — `upsert_entry`, `record_hit`, `get_stats`, `flush_all` DB queries
- [x] `src/handlers/admin/cache.rs` — `GET /admin/cache/stats` + `DELETE /admin/cache` handlers
- [x] `src/gateway/pipeline.rs` — `run()` and `run_streaming()` extended: cache lookup before provider, cache write after success, SSE synthesis for streaming cache hits; return `(response, cache_hit: bool)`
- [x] `src/handlers/gateway.rs` — `X-Velox-Cache: false` bypass header; `X-Velox-Cache-Hit: exact` response header on hits
- [x] `src/state.rs` — Added `cache: Arc<CacheEngine>` field
- [x] `src/models/api_key.rs` — Fixed `ApiKeyView` Decimal serialization (`serde-float` feature)

### Velox-Specific Endpoints (Phase 1)
- [x] `POST /v1/chat/completions` — OpenAI-compatible gateway proxy (streaming + non-streaming)
- [x] `POST /admin/keys` — Create API key (returns full key once, never again)
- [x] `GET  /admin/keys` — List API keys (safe view: prefixes only, no hashes)

### Velox-Specific Endpoints (Phase 4)
- [x] `GET  /admin/cache/stats` — Aggregate cache stats (entries, hits, tokens saved, cost saved)
- [x] `DELETE /admin/cache` — Flush all cache entries (DashMap + DB)

### Velox-Specific Rust Modules (Phase 5)
- [x] `src/cache/embedding.rs` — `EmbeddingModel`: ONNX Runtime session (Mutex) + HuggingFace tokenizer; `embed()` → mean-pool + L2-normalize → 384-dim unit vector
- [x] `src/cache/semantic.rs` — `SemanticCache`: `RwLock<Vec<SemanticEntry>>` with linear cosine scan; `f32_vec_to_bytes` / `bytes_to_f32_vec` for PostgreSQL BYTEA storage
- [x] `src/cache/mod.rs` — Extended: `CacheHit` enum (`None`, `Exact`, `Semantic(f32)`); `CacheEngine::new_with_semantic(model, threshold)`; `warm_from_db()` loads hot layer + semantic index from DB
- [x] `src/config.rs` — Added `embedding_model_path` (default `models/all-MiniLM-L6-v2.onnx`), `embedding_tokenizer_path` (default `models/tokenizer.json`), `semantic_cache_threshold` (default 0.90)
- [x] `src/db/cache.rs` — Added `save_embedding(pool, hash, bytes)`, `load_all_entries(pool) -> Vec<CacheEntryRow>` for startup warm-up
- [x] `src/gateway/pipeline.rs` — `run()` and `run_streaming()` return `CacheHit` instead of `bool`; semantic lookup before provider call; semantic insert + DB persist after provider success
- [x] `src/handlers/gateway.rs` — `attach_cache_headers()` sets `X-Velox-Cache-Hit: semantic` + `X-Velox-Cache-Similarity: {score:.4}` on semantic hits
- [x] `src/main.rs` — Tries to load `EmbeddingModel` at startup; graceful degradation if model missing; `warm_from_db()` called after pool init

### Velox-Specific Rust Modules (Phase 7 — finalized)
- [x] `src/pii.rs` — PII scrubber: redacts credit cards, SSNs, emails, bearer tokens, API keys using compiled regex patterns; applied to request bodies before `cache_entries` DB storage and before `tracing::debug!` body logs
- [x] `src/handlers/gateway.rs` — Extended: config-gated `tracing::debug!` for request bodies (`log_request_bodies`) and response bodies (`log_response_bodies`); PII-scrubbed before emission
- [x] `src/metrics.rs` — Prometheus endpoint with native atomic gauges (exact cache size, semantic cache size, hit ratio); bypasses `metrics::gauge!()` naming conflict
- [x] `src/handlers/metrics.rs` — `GET /metrics` refreshes gauges from live AppState on every scrape
- [x] `benches/cache_bench.rs` — Criterion benchmarks: SHA-256 hashing, cosine similarity (single + 1 000-entry scan), PII scrubber overhead
- [x] `docs/quickstart.md` — 5-minute getting-started guide
- [x] `docs/configuration.md` — full configuration reference
- [x] `docs/deployment/docker.md` — Docker Compose production setup
- [x] `docs/deployment/systemd.md` — Linux system service (hardened unit file + nginx proxy)
- [x] `docs/deployment/kubernetes.md` — Kubernetes manifests, ServiceMonitor, persistent volume for models

### Velox-Specific Rust Modules (V4-8 — RBAC / True Multi-tenancy)
- [x] `migrations/0024_rbac.sql` — Creates `roles` (4 roles) and `workspace_members` tables; seeds existing users as admin on all workspaces via cross-join INSERT
- [x] `src/db/rbac.rs` — DB queries: `get_user_highest_role`, `get_role_in_workspace`, `list_members`, `add_member`, `update_member_role`, `remove_member`, `find_user_by_email`, `list_workspaces`
- [x] `src/middleware/rbac.rs` — `Role` enum (ReadOnly=1..Admin=4, Ord-comparable); `require_role()` global check; `require_role_in_workspace()` workspace-scoped check; bootstrap rule: no memberships → admin
- [x] `src/handlers/admin/members.rs` — `GET /admin/workspaces`, member CRUD endpoints (`list_members`, `add_member`, `update_member`, `remove_member`)
- [x] `dashboard/src/app/(dashboard)/workspaces/page.tsx` — Workspace management page with expandable workspace cards, member table, add/edit/remove member dialogs
- [x] RBAC enforcement added to all admin handlers: `analytics.*` (BillingViewer+), `keys.*` (ApiManager+), `cache.*` (Admin), `providers.test_provider` (Admin), `velox_config.patch_config` (Admin), `requests.*` (BillingViewer+)
- [x] `tests/v4_8_rbac.rs` — 14 acceptance tests covering role enforcement, bootstrap rule, cross-workspace isolation, migration seeding

### Velox-Specific Rust Modules (V4-9 — External Vector Stores)
- [x] `src/cache/index/qdrant.rs` — `QdrantIndex`: implements `EmbeddingIndex` trait via gRPC calls to Qdrant; `new()` is async (connects + ensures collection exists); sync trait methods bridge via `block_in_place`; point IDs derived from hash prefix; hash stored in payload for retrieval
- [x] `src/cache/index/mod.rs` — Added `pub mod qdrant;` to expose `QdrantIndex`
- [x] `src/cache/mod.rs` — Added `CacheEngine::new_with_qdrant_semantic()` constructor
- [x] `src/config.rs` — Added `qdrant_url` (default `http://localhost:6334`), `qdrant_collection` (default `velox_cache`), `qdrant_vector_size` (default 384)
- [x] `src/main.rs` — Wired `semantic_cache_backend = "qdrant"` branch; graceful fallback to linear on connection error
- [x] `Cargo.toml` — Added `qdrant-client = "1.9"`
- [x] `tests/v4_9_vector_store.rs` — 6 acceptance tests; Qdrant-dependent tests skip gracefully when instance not running (CI-safe); full suite runs with `docker run -p 6334:6334 qdrant/qdrant`

### Velox-Specific Rust Modules (V5-0 — API Surface Expansion)
- [x] `migrations/0027_api_expansion.sql` (+ sqlite mirror): adds `requests.tool_calls` (JSONB) and `requests.endpoint` (VARCHAR(50), default `/v1/chat/completions`); GIN/B-tree indexes; adds `price_per_image`, `price_per_audio_second`, `price_per_character` to `model_pricing`
- [x] `src/providers/mod.rs`: new modality types (`ModelInfo`, `ImagesRequest/Response`, `TranscribeRequest/Response`, `SpeechRequest/Stream`); extends `Provider` trait with `list_models`, `images_generate`, `audio_transcribe`, `audio_speech` (all default to `ProviderError::Unsupported`); extends `ChatMessage` with `tool_calls` + `tool_call_id` so function-calling responses survive round-trip
- [x] `src/providers/openai.rs`: implements all four new trait methods against `/v1/models`, `/v1/images/generations`, `/v1/audio/transcriptions` (multipart), `/v1/audio/speech` (streaming bytes)
- [x] `src/gateway/tool_extract.rs`: pulls `tools` from the request and `tool_calls` from the response into a single JSON value for persistence
- [x] `src/gateway/pipeline.rs`: now takes `endpoint: &str`; carries it through all `insert_request` callsites (success, error, streaming) via an `Arc<str>` cloned per spawn; tool_calls extracted on the non-streaming success path (streaming tool_calls audit deferred)
- [x] `src/db/requests.rs`: `insert_request` now accepts `endpoint` + `tool_calls`; `insert_embedding_request` hardcodes `/v1/embeddings`; new `insert_modality_request` helper for non-token-priced rows; new `find_modality_pricing` returns `(price_per_image, per_second, per_character)`
- [x] `src/pricing/mod.rs`: new `calculate_image_cost`, `calculate_audio_cost`, `calculate_character_cost` helpers
- [x] `src/handlers/gateway.rs`: rewrites `/v1/models` to aggregate from providers behind a 5-second in-memory TTL (`OnceLock<Mutex<…>>`); adds `images_generations`, `audio_transcriptions` (multipart via `axum::extract::Multipart`), `audio_speech` (binary streaming response with provider-negotiated content-type)
- [x] `src/routes/mod.rs`: wires `/v1/images/generations`, `/v1/audio/speech` (1 MB limit), and `/v1/audio/transcriptions` on a separate 25 MB-limit branch for audio uploads
- [x] `Cargo.toml`: adds `axum` feature `multipart` + `reqwest` feature `multipart`
- [x] `tests/v5_0_api_expansion.rs`: 14 acceptance tests — embeddings shape/cost/priority, `/v1/models` aggregation + 5-second TTL, images passthrough + per-image cost, audio multipart + speech streaming, legacy completions, tool-calls extraction into requests row, endpoint-per-route attribution, unsupported modality error path, regression on chat completions

---

## Locked Architectural Decisions

> These are FINAL. Do not change them without an explicit conversation with the user
> and updating DECISIONS.md with a reason. Changing these mid-project causes cascading failures.

### 1. Database
- Primary: PostgreSQL (via docker-compose)
- Connection: `sqlx` with compile-time checked queries
- Migrations: numbered SQL files in `/migrations/`, run on startup
- Rule: **Never modify an existing migration file.** Always add a new one.
- UUIDs: generated in Rust code (`Uuid::new_v4()`), NOT by the database

### 2. API Key Format
```
Format:  vx-sk-[48 alphanumeric chars]
Example: vx-sk-a8Kd92nPqRx4mTvL7wYjBc3hEiZsNfGu5oQpAb1Cy6Xk
Stored:  bcrypt hash (source of truth in DB) + SHA-256 hash (fast in-memory lookup)
Display: prefix only — vx-sk-a8Kd92n... (shown in dashboard)
Revealed: full key shown ONCE at creation, never again
```

### 3. Gateway API Format
- **OpenAI-compatible.** Exactly. No deviations.
- `POST /v1/chat/completions` — identical request/response shape to OpenAI
- Clients change ONLY the `base_url`. Zero other code changes.
- This is non-negotiable. Breaking OpenAI compatibility breaks all users.

### 4. Admin API Format
```json
// Success
{ "data": { ... }, "meta": { "page": 1, "per_page": 50, "total": 100 } }

// Error
{ "error": { "code": "RATE_LIMIT_EXCEEDED", "message": "...", "details": { ... } } }
```

### 5. Provider Abstraction
- Providers implement a `Provider` trait (see DECISIONS.md)
- Three providers in v1: OpenAI, Anthropic, AWS Bedrock
- New providers added by implementing the trait — no changes to core
- Provider selection is priority-based (integer, lower = higher priority)

### 6. Error Handling
- Use `thiserror` for defining error types
- Use `anyhow` for application-level error propagation
- All errors must implement `IntoResponse` for axum
- HTTP errors return JSON with the Admin API error format above

### 7. State Management
```rust
// AppState is passed to all handlers via axum State extractor
// Add fields here — never pass things ad-hoc
pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub cache: Arc<CacheEngine>,       // added in Phase 4
    pub metrics: Arc<MetricsStore>,    // added in Phase 3
    pub providers: Arc<ProviderRegistry>, // added in Phase 1
}
```

### 8. Streaming
- Use `tokio::sync::mpsc` channels internally for streaming data
- SSE responses via axum's `Sse<>` response type
- WebSocket via `tokio-tungstenite`
- All provider stream formats normalized to OpenAI SSE format

### 9. Configuration Loading Order (priority: high to low)
```
1. Environment variables (highest priority)
2. velox.toml file
3. Default values in code (lowest priority)
```

### 10. Cost Calculation
- Costs stored as `DECIMAL(12,8)` — precise to 8 decimal places (sub-cent precision)
- Pricing loaded from `model_pricing` database table (updatable without redeployment)
- Formula: `(prompt_tokens / 1_000_000.0 * input_price) + (completion_tokens / 1_000_000.0 * output_price)`

### 11. Caching (two layers)
```
Layer 1 — Exact match:
  Key:    SHA-256(normalized_request_body)
  Store:  dashmap (hot) + PostgreSQL (persistent)
  Speed:  < 2ms

Layer 2 — Semantic match:
  Key:    Cosine similarity over sentence embeddings (linear scan; HNSW planned V3-1)
  Store:  Vec<(embedding, hash)> in memory + embeddings in PostgreSQL (BYTEA)
  Speed:  < 10ms (degrades linearly with entry count)
  Threshold: 0.90 cosine similarity (configurable via semantic_cache_threshold)
```

### 12. What Lives on Which Port
```
8080: Everything — gateway API (/v1/*), admin API (/admin/*), dashboard (/)
      Admin API should NOT be on a separate port in v1 (adds deployment complexity)
```

### 13. Dashboard Embedding
- Next.js dashboard built at compile time via `build.rs`
- Static files baked into binary via `include_dir!` macro
- Served by Rust at `/` route
- No separate npm process required at runtime

### 14. Authentication
```
Admin dashboard: Username/password → JWT (uses existing users table + JWT system)
Gateway API:     Velox API key (vx-sk-...) → validated against api_keys table
These are SEPARATE auth systems. Gateway keys never work on admin endpoints.
```

---

## File Ownership Per Phase

> This is critical. Only touch files listed for the current phase.
> If you need to touch a file from a different phase, STOP and discuss first.

### Phase 0 — Foundation
**May create:**
- `migrations/0002_*.sql` through `migrations/0009_*.sql`
- `src/models/api_key.rs`, `src/models/provider.rs`, `src/models/request.rs`, `src/models/cache_entry.rs`
- `src/config.rs` (replace existing)
- `src/state.rs` (replace existing)
- `src/errors.rs` (replace existing)

**May modify:**
- `Cargo.toml` (add new dependencies only)
- `src/main.rs` (minor startup changes)

**Must NOT touch:**
- Existing migration `migrations/0001_create_users.sql`
- `src/handlers/` (leave existing handlers intact in Phase 0)
- `src/routes/` (leave intact)

### Phase 1 — Core Proxy
**May create:**
- `src/providers/` (entire directory)
- `src/gateway/` (entire directory)
- `src/handlers/gateway.rs`
- `src/handlers/admin/keys.rs`
- `src/middleware/api_key_auth.rs`
- `src/middleware/budget.rs`
- `src/pricing/mod.rs`
- `src/db/api_keys.rs`
- `src/db/requests.rs`

**Must NOT touch:**
- Any existing migration files
- `src/handlers/auth.rs` (existing admin auth — still needed)

### Phase 2 — Streaming
**May modify:**
- `src/handlers/gateway.rs` (add stream=true support)
- `src/providers/*.rs` (add stream method)

**Must NOT touch:**
- Database schema (no new migrations)
- API key validation logic
- Cost calculation logic (only extend it)

### Phase 3 — Rate Limiting & Reliability
**May create:**
- `src/middleware/rate_limit.rs`
- `src/routing/mod.rs`

**May modify:**
- `src/state.rs` (add MetricsStore)
- `src/providers/*.rs` (add retry/circuit breaker)

**Must NOT touch:**
- Gateway handler request/response format
- Database schema

### Phase 4 — Exact Cache
**May create:**
- `src/cache/mod.rs`
- `src/cache/exact.rs`
- `src/db/cache.rs`
- `src/handlers/admin/cache.rs`

**May modify:**
- `src/state.rs` (add CacheEngine)
- `src/gateway/pipeline.rs` (add cache lookup steps)

**Must NOT touch:**
- Provider adapters
- Rate limiting logic
- Database schema (except adding cache_type/cache_similarity to requests table — plan this in Phase 0 migration)

### Phase 5 — Semantic Cache
**May create:**
- `src/cache/semantic.rs`

**May modify:**
- `src/cache/mod.rs`
- `src/cache/exact.rs` (if needed)

**Must NOT touch:**
- Exact cache logic (extend, don't modify)
- Provider adapters
- API format

### Phase 6 — Dashboard
**May create:**
- `dashboard/` (entire directory)
- `build.rs`
- `src/dashboard.rs`

**May modify:**
- `src/routes/mod.rs` (add dashboard routes)
- `Cargo.toml` (add include_dir)

**Must NOT touch:**
- Any src/ files except routes and dashboard.rs
- Any existing tests

---

## Architecture Quick Reference

```
velox/
├── src/
│   ├── main.rs              ← startup: load config, connect db, run migrations, start server
│   ├── config.rs            ← Config struct (TOML + env)
│   ├── state.rs             ← AppState (shared across all handlers)
│   ├── errors.rs            ← AppError enum → HTTP responses
│   ├── gateway/             ← Core proxy logic
│   │   ├── mod.rs
│   │   ├── router.rs        ← Select which provider to use
│   │   └── pipeline.rs      ← The 15-step request pipeline
│   ├── providers/           ← LLM provider adapters
│   │   ├── mod.rs           ← Provider trait
│   │   ├── openai.rs
│   │   ├── anthropic.rs
│   │   └── bedrock.rs
│   ├── cache/               ← Caching subsystem
│   │   ├── mod.rs           ← CacheEngine (combines exact + semantic)
│   │   ├── exact.rs         ← SHA-256 based exact match
│   │   └── semantic.rs      ← Linear cosine scan (HNSW planned V3-1)
│   ├── middleware/          ← axum middleware
│   │   ├── api_key_auth.rs  ← Validate vx-sk-... keys
│   │   ├── rate_limit.rs    ← Sliding window limiter
│   │   └── budget.rs        ← Spend limit enforcement
│   ├── handlers/            ← HTTP handlers
│   │   ├── gateway.rs       ← POST /v1/chat/completions etc.
│   │   ├── health.rs        ← GET /health (existing)
│   │   ├── auth.rs          ← Admin auth (existing, keep)
│   │   └── admin/           ← Admin API handlers
│   │       ├── keys.rs
│   │       ├── requests.rs
│   │       ├── analytics.rs
│   │       ├── cache.rs
│   │       ├── providers.rs
│   │       └── stream.rs    ← WebSocket live feed
│   ├── models/              ← Data structs
│   ├── db/                  ← Database queries (sqlx)
│   ├── pricing/             ← Cost calculation
│   ├── metrics/             ← In-memory metrics + Prometheus
│   └── routing/             ← Provider selection logic
```

---

## Common Mistakes to Avoid

> This section grows over time as mistakes are discovered.
> Read every entry before starting work.

1. **Never modify existing migration files.** Once applied, they are immutable.
   If you need to change a table, add a new migration.

2. **Never break OpenAI API compatibility.** The `/v1/chat/completions` response
   format must match exactly. Users' apps will break silently if this drifts.

3. **Always run `cargo test` before ending a session.** Never leave broken tests.

4. **Never add a dependency without adding it to both `[dependencies]` and the
   "Dependencies per Phase" section in DECISIONS.md.**

5. **AppState fields must be `Clone` or `Arc<T>`.** axum requires State to be Clone.
   Wrap large or non-Clone types in `Arc<>`.

6. **The `api_key_auth` middleware and `jwt` middleware are DIFFERENT.**
   - `jwt`: for admin dashboard users
   - `api_key_auth`: for gateway API consumers
   Do not mix them up in route definitions.

7. **Cost calculation uses `DECIMAL(12,8)` not floats.** Use `sqlx::types::Decimal`
   or `rust_decimal::Decimal`, not `f64`, when storing/reading costs from the database.

8. **Never log full request/response bodies by default.** They may contain PII.
   Always check `config.logging.log_request_bodies` before logging.

9. **Provider API keys must be encrypted at rest.** Never store them as plaintext
   in the database. Use the AES-256-GCM encryption helpers in `src/crypto.rs`.

10. **Semantic index is in-memory only (linear scan over Vec).** It is rebuilt from
    PostgreSQL embeddings at startup via `warm_from_db()`. No snapshot file needed —
    the DB is the source of truth. HNSW indexing is planned for V3-1.

11. **Test function names MUST match the file name convention — use `vX_Y_` not `vXpY_`.**
    File `tests/v4_0_foundation.rs` → functions named `v4_0_something`, NOT `v4p0_something`.
    Rule: the dot in the version number is always an underscore `_`, never the letter `p`.
    This matters because `cargo test v4_0` uses substring matching — if functions say `v4p0_`
    the filter finds nothing. Always verify: `cargo test v4_0 -- --list` shows your tests.

---

## Test Commands (run these to verify health)

```bash
# Full test suite — must be green before any commit
cargo test

# Specific phase tests
cargo test phase0
cargo test phase1
cargo test phase2

# Check code style (must have zero warnings before committing)
cargo clippy -- -D warnings

# Check formatting
cargo fmt -- --check

# Build check only (fast, no linking)
cargo check

# Run with full logging
RUST_LOG=debug cargo run

# Run tests with output (useful for debugging)
cargo test -- --nocapture
```

---

## Phase Completion Ritual

**Before marking any phase complete, execute all of these:**

```bash
# 1. All tests pass
cargo test
# Expected: test result: ok. X passed; 0 failed

# 2. No compiler warnings
cargo clippy -- -D warnings
# Expected: no warnings

# 3. Code is formatted
cargo fmt -- --check
# Expected: no output (means no changes needed)

# 4. Project builds in release mode
cargo build --release
# Expected: Compiling velox ... Finished

# 5. Manual smoke test (curl commands specific to the phase)
# See each phase's "Manual Verification" section in VELOX_ROADMAP.md

# 6. Commit and tag
git add -A
git commit -m "Phase X complete: [description]"
git tag phase-X-complete
```

---

## Phase History

| Phase | Status | Completed | Commit |
|---|---|---|---|
| 0 | Complete | 2026-05-22 | d5545ab |
| 1 | Complete | 2026-05-22 | 251fbe8 |
| 2 | Complete | 2026-05-22 | 6a2abbe |
| 3 | Complete | 2026-05-22 | 0e538ef |
| 4 | Complete | 2026-05-22 | 4d15374 |
| 5 | Complete | 2026-05-22 | e0e2d9c |
| 6 | Complete | 2026-05-22 | 395c7b4 |
| 7 | Complete (finalized) | 2026-05-22 | TBD |
| 8 | Skipped | — | — |
| 9 | Skipped | — | — |

---

## AWS Credentials

Already configured in `.env`:
- `AWS_ACCESS_KEY_ID` ✓
- `AWS_SECRET_ACCESS_KEY` ✓
- `AWS_REGION=eu-north-1` ✓

Used for: AWS Bedrock provider adapter in Phase 1.

---

*Last updated: 2026-05-22 — v0.1.0 feature-complete (Phases 8 and 9 skipped)*
*Update this file at the end of every session.*
