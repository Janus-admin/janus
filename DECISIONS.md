# DECISIONS.md — Locked Architectural Decisions
> Every decision here was made deliberately.
> Do NOT change anything in this file without an explicit conversation with the user.
> If you disagree with a decision, raise it — do not silently work around it.

---

## Decision Log Format

Each decision contains:
- **What**: The decision
- **Why**: The reasoning
- **Rejected alternatives**: What we considered and why we said no
- **Consequences**: What this means for implementation
- **Locked since**: Phase when this was locked

---

## D-001: Web Framework — axum

**What**: Use `axum 0.7` as the HTTP framework.

**Why**: Already in the project. Excellent async performance. Type-safe extractors.
First-class Tower middleware support. Actively maintained by the Tokio team.

**Rejected alternatives**:
- `actix-web`: Different async model, macros are more magical, harder to compose middleware
- `warp`: Composable but complex type signatures become unmanageable in large apps
- `rocket`: Requires nightly Rust in older versions, less control over async

**Consequences**: All handlers use axum extractors. Middleware uses Tower layers.
Do not introduce another HTTP framework at any point.

**Locked since**: Project start

---

## D-002: Database — PostgreSQL (primary), SQLite (v2)

**What**: PostgreSQL is the database for v1. SQLite support is a v2 feature.

**Why**: PostgreSQL is already running via docker-compose and working. Migrating
to SQLite as default mid-project would require rewriting all sqlx queries (different
type system, different UUID handling, different timestamp handling). The "single binary
with SQLite" story is compelling but not worth the risk of mid-project schema changes.

**Rejected alternatives**:
- SQLite now: Adds complexity to achieve compatibility with both drivers simultaneously
- MySQL: No advantage over PostgreSQL for our use case

**Consequences**:
- All sqlx queries use PostgreSQL syntax
- `gen_random_uuid()` NOT used in SQL — UUIDs generated in Rust via `Uuid::new_v4()`
- `NOW()` NOT used in SQL — timestamps set in Rust via `Utc::now()`
- This makes SQLite migration in v2 much easier

**Locked since**: Phase 0

---

## D-003: Migration Strategy

**What**: Migrations are append-only numbered SQL files. Existing migrations are immutable.

**Why**: Changing an existing migration after it has been applied to any database
(dev, staging, production) causes `sqlx migrate run` to fail with a checksum error.
This would break every deployment.

**Rejected alternatives**:
- Schema-only approach (recreate on startup): Destroys data in staging/production
- ORM-managed migrations: Adds abstraction that hides what SQL is actually running

**Consequences**:
- `migrations/0001_create_users.sql` — NEVER touch this file
- New changes always go in a new file: `migrations/0002_*`, `migrations/0003_*`, etc.
- If a column name was wrong: add a new migration that renames it
- Migration filenames: `XXXX_descriptive_name.sql` where XXXX is zero-padded number

**Locked since**: Phase 0

---

## D-004: API Key Design

**What**: Gateway API keys have the format `vx-sk-[48 alphanumeric chars]`.
Two hashes are maintained: bcrypt (storage) and SHA-256 (fast lookup).

**Why**:
- Prefix `vx-sk-` makes keys identifiable (users know it's a Velox key)
- bcrypt hash in DB: if DB is compromised, keys cannot be reversed
- SHA-256 hash in dashmap: bcrypt is intentionally slow (cost 12 = ~300ms).
  We cannot run bcrypt on every API request. SHA-256 of the key is stored in
  memory for O(1) lookup. This is safe because the key itself has 48 chars of entropy.
- Keys are shown ONCE at creation. No recovery. User must rotate if lost.

**Rejected alternatives**:
- JWT as API keys: Too complex, harder to revoke instantly
- Random UUID: Too short, no prefix, no visual identification
- bcrypt for every request: Too slow (300ms per request is unacceptable)

**Consequences**:
```rust
// At key creation:
let key = format!("vx-sk-{}", generate_random_alphanumeric(48));
let bcrypt_hash = bcrypt::hash(&key, 12)?;        // stored in DB
let sha256_hash = sha256(&key);                    // stored in dashmap

// At request validation:
let sha256_hash = sha256(incoming_key);
let key_record = dashmap.get(&sha256_hash)?;       // sub-microsecond
check_expiry(key_record)?;
check_budget(key_record)?;
```

**Locked since**: Phase 0

---

## D-005: OpenAI-Compatible Gateway API

**What**: The gateway API at `/v1/` is a drop-in replacement for OpenAI's API.
Request and response shapes match OpenAI exactly.

**Why**: This is the single most important adoption decision. If a developer can
switch from OpenAI to Velox by changing one line (base_url), adoption is frictionless.
Any deviation forces users to change their application code.

**Rejected alternatives**:
- Custom API format: Would require users to rewrite all their OpenAI calls. Fatal to adoption.
- Partial compatibility: Still forces code changes. Not worth the complexity savings.

**Consequences**:
- `POST /v1/chat/completions` — request body: exactly OpenAI's ChatCompletionRequest
- Response body: exactly OpenAI's ChatCompletionResponse
- Streaming: exactly OpenAI's SSE format (`data: {...}\n\n`, ending with `data: [DONE]\n\n`)
- Error responses: must match OpenAI's error format too
- When Anthropic or Bedrock are used, their responses are normalized to OpenAI format before returning

**Reference**: https://platform.openai.com/docs/api-reference/chat/create

**Locked since**: Phase 0

---

## D-006: Provider Trait Design

**What**: All LLM providers implement a common `Provider` trait.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> u8;
    fn is_enabled(&self) -> bool;

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError>;

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, ProviderError>> + Send>>, ProviderError>;

    async fn health_check(&self) -> HealthStatus;
}
```

**Why**:
- Trait objects (`Box<dyn Provider>`) allow adding new providers without touching core
- `Send + Sync` required because providers live in AppState across async tasks
- Streaming returns a pinned boxed Stream — works with async Rust's ownership model
- Each provider handles its own auth, URL building, and error mapping

**Rejected alternatives**:
- Enum with match statements: Adding a new provider requires modifying core routing code
- Configuration-only (no code): Not flexible enough for different auth schemes (AWS SigV4 vs Bearer)

**Consequences**:
- `ProviderRegistry` holds `Vec<Arc<dyn Provider>>`, sorted by priority
- When routing, iterate providers by priority until one succeeds
- Each provider file is self-contained: OpenAI knows nothing about Anthropic

**Locked since**: Phase 1

---

## D-007: Streaming Architecture

**What**: Use `tokio::sync::mpsc` channels internally. axum `Sse<>` for SSE responses.

**Why**:
- `mpsc` channel decouples the provider's stream from the HTTP response stream
- Allows inserting middleware steps between receiving chunks and sending them to client
- axum's `Sse<>` type handles all SSE framing (the `data: ...\n\n` format)
- If client disconnects, the channel is dropped, causing the provider stream to be cancelled

```
Provider SSE stream
    → tokio::sync::mpsc::channel
    → Cost counting per chunk
    → axum Sse<ReceiverStream<...>>
    → Client
```

**Rejected alternatives**:
- Direct pipe from provider to client: Cannot insert cost counting or logging middleware
- WebSocket for all responses: SSE is simpler for unidirectional streaming, better browser support

**Consequences**:
- All streaming handlers use `Sse<ReceiverStream<Result<Event, Infallible>>>`
- Chunks are counted as they flow through the channel
- Final cost is calculated from accumulated token count, logged after stream ends

**Locked since**: Phase 2

---

## D-008: Error Type Hierarchy

**What**: Use `thiserror` for typed errors, `anyhow` for application propagation.

```rust
// Specific domain errors — use thiserror
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Authentication failed")]
    Unauthorized,
    #[error("Provider unavailable: {0}")]
    Unavailable(String),
    #[error("Request timeout after {0}ms")]
    Timeout(u64),
}

// Application-level — use anyhow
// These are unexpected errors that should be 500s
async fn handler(...) -> Result<Json<...>, AppError> {
    let result = do_something().context("failed to do something")?;
    ...
}
```

**Why**: `thiserror` for errors you handle (retry on RateLimit, failover on Unavailable).
`anyhow` for errors you don't handle (log and return 500).

**Consequences**:
- `ProviderError` → maps to specific HTTP status codes
- `CacheError` → maps to specific HTTP status codes  
- `AppError` → wraps anyhow for 500 responses
- The `AppError` must implement `IntoResponse` (already exists in errors.rs)

**Locked since**: Phase 0

---

## D-009: Configuration System

**What**: Use the `config` crate with TOML file + environment variable override.

```toml
# velox.toml
[server]
port = 8080
```

```bash
# Environment variable overrides: VELOX_{SECTION}_{KEY}
VELOX_SERVER_PORT=9090
```

**Why**: 
- TOML is readable and widely understood
- Environment variable override is essential for Docker/Kubernetes deployments
- The `config` crate handles both sources with priority ordering automatically

**Rejected alternatives**:
- `.env` only: Not suitable for structured configuration with sections
- YAML: More complex syntax, no advantage for our use case
- Command-line flags only: Too verbose for large configuration

**Consequences**:
- Config file is `velox.toml` in the working directory
- All sensitive values (API keys) can be set via environment variables
- The `Config` struct in `src/config.rs` must have `serde::Deserialize` derived

**Locked since**: Phase 0

---

## D-010: Cost Storage Precision

**What**: Store monetary values as `DECIMAL(12,8)` in PostgreSQL, mapped to `rust_decimal::Decimal` in Rust.

**Why**: LLM costs can be extremely small (0.000002 USD per token for cheap models).
Using `f64` (IEEE 754 float) would cause rounding errors that accumulate over millions
of requests. `DECIMAL` is exact.

Example: GPT-4o-mini costs $0.00000015 per token. In f64, this would have precision loss.

**Rejected alternatives**:
- `f64`: Floating point precision errors accumulate. Unacceptable for financial data.
- Store as integer (millionths of a cent): Possible but requires manual conversion everywhere.

**Consequences**:
- Add `rust_decimal` to Cargo.toml
- All cost fields in sqlx structs use `Decimal` type
- `sqlx` feature `rust_decimal` must be enabled

**Locked since**: Phase 1

---

## D-011: In-Memory State for Rate Limiting

**What**: Rate limiting state is kept in a `dashmap` in `AppState`, NOT in the database.

**Why**: Rate limiting requires checking on every single request (sub-millisecond budget).
A database query per request would add 5-20ms latency to every API call. Unacceptable.

Tradeoff: If the process restarts, rate limit counters reset. This is acceptable — it
means a restart gives users a brief window above their rate limit. The alternative (DB-based)
would make the gateway unacceptably slow.

**Rejected alternatives**:
- Redis for rate limiting: Adds a required external dependency, breaks single-binary story
- PostgreSQL for rate limiting: Too slow for per-request checks

**Consequences**:
- Rate limit state is: `DashMap<Uuid, VecDeque<Instant>>` (key_id → timestamps of recent requests)
- Background task sweeps old entries every 60 seconds
- On multi-node deployments, rate limits are per-node (acceptable for v1)

**Locked since**: Phase 3

---

## D-012: Semantic Cache Embedding Model

**What**: Use `all-MiniLM-L6-v2` ONNX model via the `ort` crate for local embeddings.

**Why**:
- Local: no API cost, no external dependency, no latency to embedding provider
- 80MB model size: acceptable for a server deployment
- 384 dimensions: good balance of accuracy vs memory
- ~5ms per embedding on CPU: fast enough for cache lookups

**Rejected alternatives**:
- OpenAI `text-embedding-3-small`: Adds API cost + latency for every cache miss
- Fastembed-rs: Alternative but `ort` + `all-MiniLM` is more battle-tested
- Skip semantic cache: Eliminates the main cost-saving feature

**Consequences**:
- `ort` crate + ONNX runtime must be added in Phase 5
- Model file downloaded on first run (or bundled as feature flag)
- Embedding generation: ~5ms on CPU, ~1ms on GPU (not required)
- HNSW index uses `instant-distance` crate (pure Rust, no system dependencies)

**Locked since**: Phase 5

---

## Dependencies Per Phase

> Add dependencies ONLY in the phase listed. This prevents dependency conflicts
> from being introduced too early.

### Phase 0
```toml
config = "0.14"
clap = { version = "4", features = ["derive"] }
rust_decimal = { version = "1", features = ["db-postgres"] }
```

### Phase 1
```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
dashmap = "6"
sha2 = "0.10"
aes-gcm = "0.10"
tiktoken-rs = "0.6"
aws-config = { version = "1", features = ["behavior-version-latest"] }
aws-sdk-bedrockruntime = "1"
async-trait = "0.1"
futures = "0.3"
futures-util = "0.3"
bytes = "1"
rand = "0.8"      # secure random key generation (48-char vx-sk- suffix)
```

### Phase 2
```toml
eventsource-stream = "0.2"
tokio-stream = { version = "0.1", features = ["sync"] }
```

### Phase 3
```toml
# No new dependencies — dashmap already added in Phase 1
```

### Phase 4
```toml
# No new dependencies — sha2 already added in Phase 1
```

### Phase 5
```toml
ort = { version = "2.0.0-rc.12", features = ["download-binaries"] }
tokenizers = "0.19"   # HuggingFace tokenizers for BPE/WordPiece via tokenizer.json
# Note: ndarray not added as a direct dep — we use the (shape, Vec) tuple form
# for Tensor::from_array() to avoid a version conflict with ort's internal ndarray 0.17.
# Note: instant-distance dropped — it only supports batch-build, not online insertion.
# Linear scan (Vec + dot product on unit vectors) is used instead.
```

### Phase 6
```toml
include_dir = "0.7"
```

### Dev dependencies (add in Phase 0)
```toml
[dev-dependencies]
wiremock = "0.6"
tokio-test = "0.4"
tempfile = "3"
```

---

## Decisions Under Consideration (not yet locked)

> These are open questions. Do not implement them until they are moved to a locked decision above.

- Should the admin password be stored in the existing `users` table or a separate config file?
  - Current thinking: config file (simpler, no DB dependency for admin auth)
  - Status: decide in Phase 0

- Should the Prometheus metrics endpoint require authentication?
  - Current thinking: yes, same admin auth
  - Status: decide in Phase 7

---

*This file is append-only. New decisions are added at the bottom of the relevant section.*
*Never delete a decision — if it changes, add a new decision that supersedes it and note why.*
