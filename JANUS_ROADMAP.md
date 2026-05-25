# JANUS — Complete Engineering Roadmap
### Self-Hosted AI Gateway. Built in Rust.
> Version 1.0 — Reference Document

---

## Table of Contents

1. [Vision & Core Principles](#1-vision--core-principles)
2. [What Janus Is and Is Not](#2-what-janus-is-and-is-not)
3. [Full System Architecture](#3-full-system-architecture)
4. [Technology Stack with Rationale](#4-technology-stack-with-rationale)
5. [Database Schema](#5-database-schema)
6. [API Design](#6-api-design)
7. [Request Pipeline](#7-request-pipeline)
8. [Semantic Caching Architecture](#8-semantic-caching-architecture)
9. [Configuration System](#9-configuration-system)
10. [Security Architecture](#10-security-architecture)
11. [Performance Targets](#11-performance-targets)
12. [Testing Strategy](#12-testing-strategy)
13. [Phase-by-Phase Roadmap](#13-phase-by-phase-roadmap)
14. [Open Source Launch Plan](#14-open-source-launch-plan)
15. [v2 and Beyond](#15-v2-and-beyond)
16. [Project File Structure](#16-project-file-structure)

---

## 1. Vision & Core Principles

### The One-Line Vision
> Drop one binary in front of your LLM calls. Get streaming, caching, routing, and full observability instantly.

### Why Janus Exists
Every team building AI products faces the same operational problems:
- They have no visibility into which models cost the most and why
- Identical or semantically similar prompts are sent to LLMs repeatedly, wasting money
- When a provider goes down, apps break — there is no automatic failover
- Token streaming to thousands of concurrent users requires infrastructure most teams don't have
- Rate limiting and budget enforcement per user/feature require custom plumbing every time

LiteLLM (Python) is the current open-source standard. It works. But it requires a Python environment, Redis, PostgreSQL, and a Docker Compose file with 4+ containers. Running it on a small server is painful. Scaling it is painful.

Janus is one binary. Start it. Done.

### Core Principles

**1. Single Binary First**
The entire system — gateway, dashboard, database — ships as one compiled binary.
No Docker required. No Redis required. No Python environment. SQLite is embedded by default.
A developer should be able to run Janus on a $5 Hetzner server in under 2 minutes.

**2. OpenAI-Compatible API**
The gateway API is a drop-in replacement for OpenAI's API format.
Developers change one line of code (the base URL) and Janus works immediately.
No SDK changes. No prompt changes. Zero migration friction.

**3. Semantic Caching as the Killer Feature**
Exact-match caching is table stakes. Semantic caching — understanding that
"What is the capital of France?" and "Tell me France's capital city" are the same question —
is the moat. This is what reduces costs 40-70% in practice.

**4. Rust Performance is a Means, Not the Message**
Janus is faster than Python-based alternatives because of Rust + Tokio.
But the message to users is: "production-scale streaming and reliability."
Performance is the foundation that enables the promise, not the promise itself.

**5. Observability by Default**
Every request is logged. Every cost is tracked. Every cache hit is recorded.
No configuration needed. The dashboard works out of the box.

**6. Privacy Respecting**
Request bodies are optionally masked in logs (PII protection).
Provider API keys are encrypted at rest.
No telemetry is sent anywhere. Self-hosted means truly self-hosted.

---

## 2. What Janus Is and Is Not

### Janus IS:
- A reverse proxy specifically designed for LLM API calls
- A semantic caching layer that reduces costs
- A streaming infrastructure layer (SSE, WebSocket)
- An observability platform for AI applications
- A rate limiter and budget enforcer per API key
- A provider failover and retry engine
- A single-binary deployment tool

### Janus IS NOT:
- A replacement for your existing backend (it sits in front of LLM calls only)
- A vector database (it has an embedded vector index for caching only)
- A model training platform
- A prompt management system (v1)
- A Firebase/Supabase alternative
- A full BaaS platform

### The Mental Model
```
Your Application
      ↓
   [Janus]          ← This is what you are building
      ↓
  ┌───┴───┐
  │  LLM  │ OpenAI / Anthropic / AWS Bedrock / Others
  └───────┘
```

Your app talks to Janus exactly like it talks to OpenAI.
Janus handles everything in between.

---

## 3. Full System Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           JANUS SYSTEM                                   │
│                                                                          │
│  ┌──────────────┐    ┌──────────────────────────────────────────────┐   │
│  │   Dashboard  │    │              GATEWAY CORE (Rust)             │   │
│  │  (Next.js)   │◄──►│                                              │   │
│  │              │    │  ┌────────────┐  ┌──────────────────────┐   │   │
│  │  - Live feed │    │  │   Router   │  │   Request Pipeline   │   │   │
│  │  - Cost $$$  │    │  │            │  │                      │   │   │
│  │  - Latency   │    │  │  - Auth    │  │  1. Auth check       │   │   │
│  │  - Providers │    │  │  - Rate    │  │  2. Rate limit       │   │   │
│  │  - Cache     │    │  │    Limit   │  │  3. Budget check     │   │   │
│  │  - API Keys  │    │  │  - Budget  │  │  4. Exact cache      │   │   │
│  └──────────────┘    │  └────────────┘  │  5. Semantic cache   │   │   │
│                      │                  │  6. Provider route   │   │   │
│  ┌──────────────┐    │  ┌────────────┐  │  7. Stream response  │   │   │
│  │  Mobile App  │    │  │   Cache    │  │  8. Log + track cost │   │   │
│  │(React Native)│    │  │   Engine   │  └──────────────────────┘   │   │
│  │              │    │  │            │                              │   │
│  │  - Spend     │    │  │  Exact:    │  ┌──────────────────────┐   │   │
│  │  - Alerts    │    │  │  HashMap   │  │  Provider Adapters   │   │   │
│  │  - Live feed │    │  │            │  │                      │   │   │
│  └──────────────┘    │  │  Semantic: │  │  - OpenAI            │   │   │
│                      │  │  HNSW Vec  │  │  - Anthropic         │   │   │
│                      │  └────────────┘  │  - AWS Bedrock       │   │   │
│                      │                  │  - [extensible]      │   │   │
│                      │  ┌────────────┐  └──────────────────────┘   │   │
│                      │  │  Storage   │                              │   │
│                      │  │            │  ┌──────────────────────┐   │   │
│                      │  │  SQLite    │  │   Metrics Engine     │   │   │
│                      │  │  (default) │  │                      │   │   │
│                      │  │            │  │  Prometheus-compat   │   │   │
│                      │  │  Postgres  │  │  Real-time WebSocket │   │   │
│                      │  │ (optional) │  │  Cost aggregation    │   │   │
│                      │  └────────────┘  └──────────────────────┘   │   │
│                      └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility |
|---|---|
| Gateway Core | Request routing, streaming, middleware pipeline |
| Router | API key validation, rate limiting, budget enforcement |
| Cache Engine | Exact match + semantic similarity lookup |
| Provider Adapters | Normalize requests/responses per provider |
| Storage Layer | SQLite (default) or PostgreSQL, request logs, cost data |
| Metrics Engine | Aggregated stats, Prometheus endpoint, WebSocket stream |
| Dashboard | Admin UI, analytics, configuration |
| Mobile App | Monitoring, alerts, spend tracking (Phase 8) |

---

## 4. Technology Stack with Rationale

### Backend — Rust

| Crate | Purpose | Why This One |
|---|---|---|
| `axum 0.7` | HTTP framework | Already in project. Type-safe extractors. Excellent middleware. |
| `tokio 1` | Async runtime | Industry standard. Best performance for IO-bound work. |
| `sqlx 0.7` | Database | Already in project. Compile-time checked queries. |
| `reqwest` | HTTP client for providers | Best async HTTP client in Rust. Streaming support. |
| `tokio-tungstenite` | WebSocket | Standard Tokio-native WebSocket library. |
| `eventsource-stream` | Parse SSE from providers | Handles chunked SSE streams from OpenAI/Anthropic. |
| `dashmap` | Concurrent in-memory cache | Lock-free concurrent HashMap. Ideal for hot cache. |
| `sha2` + `hmac` | API key hashing | Secure hashing. Already have ring in project. |
| `aes-gcm` | Encrypt provider API keys at rest | Authenticated encryption. Standard choice. |
| `tiktoken-rs` | Token counting | OpenAI's own tokenizer ported to Rust. Accurate cost calc. |
| `aws-sdk-rust` | AWS Bedrock | Official AWS SDK. Your credentials already configured. |
| `clap` | CLI flags | Best CLI argument parser in Rust ecosystem. |
| `config` | Config file parsing | TOML/YAML/ENV config. Flexible for different deployments. |
| `serde` + `serde_json` | Serialization | Already in project. |
| `uuid` | IDs | Already in project. |
| `chrono` | Timestamps | Already in project. |
| `tracing` + `tracing-subscriber` | Structured logging | Already in project. |
| `include_dir` | Embed dashboard in binary | Bakes Next.js build into the Rust binary at compile time. |
| `usearch` or `instant-distance` | Vector similarity (HNSW) | Pure Rust HNSW implementation for semantic cache. |
| `ort` (optional) | Local embedding model | Run ONNX embedding models locally (no API cost). |
| `criterion` | Benchmarking | Industry standard for Rust benchmarks. |
| `wiremock` | Mock HTTP in tests | Mock provider APIs for integration tests. |

### Frontend — Next.js Dashboard

| Library | Purpose |
|---|---|
| `Next.js 14+` | Framework (App Router) |
| `TypeScript` | Type safety |
| `shadcn/ui` | Component library — clean, customizable, not opinionated |
| `Recharts` | Charts — cost graphs, latency histograms |
| `TanStack Query` | Server state, auto-refetch |
| `Zustand` | Client state management |
| `date-fns` | Date formatting |
| `WebSocket API` | Live request feed from gateway |

### Mobile App — React Native (Phase 8)

| Library | Purpose |
|---|---|
| `Expo` | Development framework |
| `React Navigation` | Navigation |
| `TanStack Query` | Data fetching |
| `Recharts (web) / Victory (native)` | Charts |
| `Expo Notifications` | Push alerts for spend thresholds |

### Infrastructure

| Tool | Purpose |
|---|---|
| `SQLite` | Default embedded database. Zero config. |
| `PostgreSQL` | Optional for larger deployments (> 10M requests/month) |
| `Docker` | Optional. Single binary runs without it. |
| `GitHub Actions` | CI/CD — build, test, release binaries |

---

## 5. Database Schema

### Overview
Default: SQLite (embedded). Optional: PostgreSQL.
The same schema works for both — sqlx handles the abstraction.

```sql
-- ─────────────────────────────────────────
-- API KEYS
-- ─────────────────────────────────────────
CREATE TABLE api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            VARCHAR(255) NOT NULL,
    key_hash        VARCHAR(255) NOT NULL UNIQUE,  -- bcrypt hash
    key_prefix      VARCHAR(12) NOT NULL,           -- first 8 chars for display: "jn-sk-abc12..."
    workspace_id    UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    budget_limit    DECIMAL(10,6),                  -- optional USD spend limit
    budget_used     DECIMAL(10,6) NOT NULL DEFAULT 0,
    rate_limit_rpm  INTEGER,                        -- requests per minute (NULL = unlimited)
    rate_limit_tpm  INTEGER,                        -- tokens per minute (NULL = unlimited)
    allowed_models  TEXT[],                         -- NULL = all models allowed
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ,
    metadata        JSONB                           -- custom key-value tags
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX idx_api_keys_workspace ON api_keys(workspace_id);

-- ─────────────────────────────────────────
-- WORKSPACES (multi-tenancy, optional)
-- ─────────────────────────────────────────
CREATE TABLE workspaces (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            VARCHAR(255) NOT NULL,
    slug            VARCHAR(100) NOT NULL UNIQUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ─────────────────────────────────────────
-- REQUESTS (core log table)
-- ─────────────────────────────────────────
CREATE TABLE requests (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id          UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    workspace_id        UUID REFERENCES workspaces(id) ON DELETE SET NULL,

    -- Routing
    provider            VARCHAR(50) NOT NULL,    -- openai | anthropic | bedrock
    model               VARCHAR(100) NOT NULL,   -- gpt-4o | claude-3-5-sonnet | etc.
    base_url            TEXT,                    -- which endpoint was used

    -- Tokens & Cost
    prompt_tokens       INTEGER,
    completion_tokens   INTEGER,
    total_tokens        INTEGER,
    cost_usd            DECIMAL(12,8),           -- precise to 1/100 of a cent

    -- Performance
    latency_ms          INTEGER,                 -- total request time
    ttfb_ms             INTEGER,                 -- time to first byte (streaming)

    -- Status
    status              VARCHAR(20) NOT NULL,    -- success | error | cached
    cache_type          VARCHAR(20),             -- exact | semantic | NULL
    cache_similarity    DECIMAL(5,4),            -- cosine similarity if semantic hit
    http_status         INTEGER,
    error_code          VARCHAR(100),
    error_message       TEXT,

    -- Optional payload logging (masked by default)
    request_body        TEXT,                    -- NULL if logging disabled
    response_body       TEXT,                    -- NULL if logging disabled

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    stream              BOOLEAN NOT NULL DEFAULT FALSE
);

-- Indexes for common dashboard queries
CREATE INDEX idx_requests_created_at ON requests(created_at DESC);
CREATE INDEX idx_requests_api_key ON requests(api_key_id, created_at DESC);
CREATE INDEX idx_requests_workspace ON requests(workspace_id, created_at DESC);
CREATE INDEX idx_requests_provider_model ON requests(provider, model);
CREATE INDEX idx_requests_status ON requests(status);
CREATE INDEX idx_requests_cached ON requests(cache_type) WHERE cache_type IS NOT NULL;

-- ─────────────────────────────────────────
-- CACHE ENTRIES
-- ─────────────────────────────────────────
CREATE TABLE cache_entries (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    prompt_hash         VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 of normalized prompt
    embedding           BYTEA,                        -- serialized f32[] vector
    provider            VARCHAR(50) NOT NULL,
    model               VARCHAR(100) NOT NULL,
    request_body        TEXT NOT NULL,
    response_body       TEXT NOT NULL,
    prompt_tokens       INTEGER,
    completion_tokens   INTEGER,
    cost_usd            DECIMAL(12,8),

    -- Cache performance
    hit_count           INTEGER NOT NULL DEFAULT 0,
    tokens_saved        BIGINT NOT NULL DEFAULT 0,
    cost_saved          DECIMAL(12,8) NOT NULL DEFAULT 0,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_hit_at         TIMESTAMPTZ,
    expires_at          TIMESTAMPTZ                   -- NULL = no expiry
);

CREATE INDEX idx_cache_hash ON cache_entries(prompt_hash);
CREATE INDEX idx_cache_expires ON cache_entries(expires_at) WHERE expires_at IS NOT NULL;

-- ─────────────────────────────────────────
-- PROVIDER CONFIGURATIONS
-- ─────────────────────────────────────────
CREATE TABLE providers (
    id                  VARCHAR(50) PRIMARY KEY,  -- openai | anthropic | bedrock
    display_name        VARCHAR(100) NOT NULL,
    is_enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    priority            INTEGER NOT NULL DEFAULT 1, -- lower = higher priority
    api_key_encrypted   TEXT,                        -- AES-256-GCM encrypted
    base_url            TEXT NOT NULL,
    timeout_ms          INTEGER NOT NULL DEFAULT 30000,
    max_retries         INTEGER NOT NULL DEFAULT 3,
    retry_delay_ms      INTEGER NOT NULL DEFAULT 1000,
    health_status       VARCHAR(20) NOT NULL DEFAULT 'unknown', -- healthy | degraded | down | unknown
    last_health_check   TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ─────────────────────────────────────────
-- MODEL PRICING TABLE
-- (kept in DB so it can be updated without redeployment)
-- ─────────────────────────────────────────
CREATE TABLE model_pricing (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider            VARCHAR(50) NOT NULL,
    model_id            VARCHAR(100) NOT NULL,
    model_display_name  VARCHAR(100),
    input_per_1m_tokens DECIMAL(10,6) NOT NULL,   -- USD per 1M input tokens
    output_per_1m_tokens DECIMAL(10,6) NOT NULL,  -- USD per 1M output tokens
    context_window      INTEGER,
    supports_streaming  BOOLEAN NOT NULL DEFAULT TRUE,
    supports_functions  BOOLEAN NOT NULL DEFAULT FALSE,
    is_active           BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(provider, model_id)
);

-- Seed data for model pricing
INSERT INTO model_pricing (provider, model_id, model_display_name, input_per_1m_tokens, output_per_1m_tokens, context_window, supports_functions) VALUES
('openai',    'gpt-4o',                  'GPT-4o',               5.00,   15.00,  128000, TRUE),
('openai',    'gpt-4o-mini',             'GPT-4o Mini',          0.15,   0.60,   128000, TRUE),
('openai',    'gpt-4-turbo',             'GPT-4 Turbo',          10.00,  30.00,  128000, TRUE),
('openai',    'gpt-3.5-turbo',           'GPT-3.5 Turbo',        0.50,   1.50,   16385,  TRUE),
('openai',    'o1',                      'o1',                   15.00,  60.00,  200000, FALSE),
('anthropic', 'claude-3-5-sonnet-20241022','Claude 3.5 Sonnet',  3.00,   15.00,  200000, TRUE),
('anthropic', 'claude-3-5-haiku-20241022', 'Claude 3.5 Haiku',   0.80,   4.00,   200000, TRUE),
('anthropic', 'claude-3-opus-20240229',  'Claude 3 Opus',        15.00,  75.00,  200000, TRUE),
('bedrock',   'anthropic.claude-3-5-sonnet-20241022-v2:0', 'Claude 3.5 Sonnet (Bedrock)', 3.00, 15.00, 200000, TRUE),
('bedrock',   'amazon.titan-text-express-v1', 'Titan Text Express', 0.20, 0.60, 8192, FALSE);

-- ─────────────────────────────────────────
-- DAILY COST AGGREGATES
-- (pre-computed for fast dashboard queries)
-- ─────────────────────────────────────────
CREATE TABLE daily_costs (
    date            DATE NOT NULL,
    workspace_id    UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    api_key_id      UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    provider        VARCHAR(50) NOT NULL,
    model           VARCHAR(100) NOT NULL,
    request_count   INTEGER NOT NULL DEFAULT 0,
    error_count     INTEGER NOT NULL DEFAULT 0,
    cache_hits      INTEGER NOT NULL DEFAULT 0,
    prompt_tokens   BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    total_cost_usd  DECIMAL(12,8) NOT NULL DEFAULT 0,
    avg_latency_ms  INTEGER,
    p95_latency_ms  INTEGER,
    PRIMARY KEY (date, provider, model, COALESCE(api_key_id, '00000000-0000-0000-0000-000000000000'::UUID))
);

CREATE INDEX idx_daily_costs_date ON daily_costs(date DESC);
CREATE INDEX idx_daily_costs_workspace ON daily_costs(workspace_id, date DESC);

-- ─────────────────────────────────────────
-- ALERTS
-- ─────────────────────────────────────────
CREATE TABLE alerts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    name            VARCHAR(255) NOT NULL,
    type            VARCHAR(50) NOT NULL,   -- spend_threshold | error_rate | latency_spike
    threshold       DECIMAL(10,4) NOT NULL,
    window_minutes  INTEGER NOT NULL DEFAULT 60,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    last_triggered  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

---

## 6. API Design

### Gateway API (OpenAI-Compatible)
This is what your users' applications call. It must be a drop-in replacement.

```
POST   /v1/chat/completions          → Chat completion (streaming + non-streaming)
POST   /v1/completions               → Text completion (legacy)
POST   /v1/embeddings                → Generate embeddings
GET    /v1/models                    → List available models (aggregated from all providers)
```

Every request requires:
```
Authorization: Bearer jn-sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
Content-Type: application/json
X-Janus-Provider: openai              (optional — override automatic routing)
X-Janus-Cache: false                  (optional — bypass cache for this request)
```

### Admin API
This is what the dashboard and your admin scripts use.

```
-- Authentication
POST   /admin/auth/login             → Get admin session token

-- API Keys
POST   /admin/keys                   → Create API key
GET    /admin/keys                   → List all API keys (paginated)
GET    /admin/keys/:id               → Get key details + usage stats
PATCH  /admin/keys/:id               → Update key (name, budget, rate limit)
DELETE /admin/keys/:id               → Revoke key

-- Requests
GET    /admin/requests               → List requests (filter by key, provider, model, status, date range)
GET    /admin/requests/:id           → Get single request details
GET    /admin/requests/export        → Export as CSV/JSON

-- Analytics
GET    /admin/analytics/overview     → Summary stats (today, 7d, 30d)
GET    /admin/analytics/costs        → Cost breakdown by provider/model/key/day
GET    /admin/analytics/latency      → Latency percentiles (p50, p95, p99) by model
GET    /admin/analytics/cache        → Cache hit rate, tokens saved, cost saved

-- Providers
GET    /admin/providers              → List providers + health status
PATCH  /admin/providers/:id          → Update provider config (enable, priority, keys)
POST   /admin/providers/:id/test     → Test provider connectivity

-- Cache
GET    /admin/cache/stats            → Cache statistics
POST   /admin/cache/flush            → Clear cache (or by provider/model)
DELETE /admin/cache/entries/:id      → Delete specific cache entry

-- Config
GET    /admin/config                 → Get current configuration
PATCH  /admin/config                 → Update configuration

-- Metrics (Prometheus-compatible)
GET    /metrics                      → Prometheus metrics endpoint

-- Health
GET    /health                       → System health (already built in your backend)

-- WebSocket (real-time dashboard feed)
WS     /admin/stream                 → Live request stream for dashboard
```

### API Response Format

Success:
```json
{
  "data": { ... },
  "meta": {
    "page": 1,
    "per_page": 50,
    "total": 1024
  }
}
```

Error:
```json
{
  "error": {
    "code": "RATE_LIMIT_EXCEEDED",
    "message": "Rate limit of 100 requests per minute exceeded",
    "details": {
      "limit": 100,
      "reset_at": "2026-05-21T21:00:00Z"
    }
  }
}
```

### API Key Format
```
jn-sk-[48 random alphanumeric characters]

Example: jn-sk-a8Kd92nPqRx4mTvL7wYjBc3hEiZsNfGu5oQpAb1Cy6Xk
Prefix stored in DB: jn-sk-a8Kd92n...
Full key shown only once at creation time.
```

---

## 7. Request Pipeline

Every request through the gateway follows this exact pipeline in order.
Each step can short-circuit the pipeline (return early with a response).

```
Incoming HTTP Request
│
├─ 1. PARSE & VALIDATE
│   ├─ Parse Authorization header → extract API key
│   ├─ Validate Content-Type
│   ├─ Parse request body (limit: 1MB)
│   └─ Validate required fields (model, messages)
│
├─ 2. AUTHENTICATION
│   ├─ Hash incoming key with SHA-256
│   ├─ Look up key_hash in database (cached in memory)
│   ├─ Check is_active = true
│   ├─ Check expires_at (if set)
│   └─ FAIL → 401 Unauthorized
│
├─ 3. RATE LIMITING
│   ├─ Check requests per minute (sliding window in dashmap)
│   ├─ Check tokens per minute (estimated from request size)
│   └─ FAIL → 429 Too Many Requests (with Retry-After header)
│
├─ 4. BUDGET CHECK
│   ├─ Load budget_used from database
│   ├─ Estimate request cost (from model + approximate tokens)
│   ├─ Check: budget_used + estimated_cost > budget_limit?
│   └─ FAIL → 402 Payment Required
│
├─ 5. MODEL AUTHORIZATION
│   ├─ Check if model is in allowed_models (if set on key)
│   └─ FAIL → 403 Forbidden
│
├─ 6. EXACT CACHE LOOKUP
│   ├─ Normalize request body (sort keys, strip whitespace)
│   ├─ SHA-256 hash of normalized body
│   ├─ Look up in dashmap (hot cache)
│   ├─ If miss: look up in database
│   ├─ Check expiry
│   └─ HIT → return cached response immediately (log as cache_type='exact')
│
├─ 7. SEMANTIC CACHE LOOKUP
│   ├─ Extract prompt text from messages
│   ├─ Generate embedding (local ONNX model or provider API)
│   ├─ Search HNSW index for nearest neighbors
│   ├─ If similarity >= threshold (default 0.95): cache hit
│   └─ HIT → return cached response (log as cache_type='semantic', cache_similarity=0.97)
│
├─ 8. PROVIDER ROUTING
│   ├─ If X-Janus-Provider header set: use that provider
│   ├─ Else: select by priority order
│   ├─ Check provider health status
│   ├─ If primary provider degraded: select next by priority
│   └─ No healthy provider → 503 Service Unavailable
│
├─ 9. REQUEST NORMALIZATION
│   ├─ Convert OpenAI-format request to provider-specific format
│   │   (Anthropic has different message format, Bedrock has its own)
│   └─ Add provider-specific headers and auth
│
├─ 10. PROVIDER REQUEST + RETRY
│   ├─ Send request to provider
│   ├─ On 429/500/502/503: exponential backoff retry (max_retries times)
│   ├─ On 429: try next provider if available (failover)
│   └─ On timeout: abort + error
│
├─ 11. RESPONSE STREAMING (if stream=true)
│   ├─ Parse provider SSE stream
│   ├─ Convert to OpenAI SSE format (normalize across providers)
│   ├─ Stream chunks to client as they arrive
│   ├─ Count tokens as they stream
│   └─ Record TTFB (time to first byte)
│
├─ 12. RESPONSE NORMALIZATION (if stream=false)
│   ├─ Parse provider JSON response
│   └─ Convert to OpenAI format
│
├─ 13. COST CALCULATION
│   ├─ Get actual token counts from response
│   ├─ Look up price from model_pricing table (cached in memory)
│   └─ Calculate: (prompt_tokens * input_price) + (completion_tokens * output_price)
│
├─ 14. CACHE STORAGE (async, non-blocking)
│   ├─ Store in dashmap (hot cache)
│   ├─ Store embedding in HNSW index
│   └─ Persist to database (async task)
│
└─ 15. LOGGING & METRICS (async, non-blocking)
    ├─ Insert request record to database
    ├─ Update budget_used on api_key
    ├─ Update daily_costs aggregate
    ├─ Update in-memory metrics counters
    ├─ Broadcast to WebSocket stream (for dashboard live feed)
    └─ Return response to client
```

---

## 8. Semantic Caching Architecture

This is the most technically sophisticated component and Janus's primary value differentiator.

### How It Works

**The Core Insight**: "What is the capital of France?" and "Can you tell me the capital city of France?" 
are semantically identical. Without semantic caching, both get sent to the LLM. With semantic caching, 
only the first does — the second returns the cached answer instantly.

### The Architecture

```
Incoming Prompt
      │
      ▼
┌─────────────────┐
│ Text Extraction │  Extract the actual prompt text from messages array
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Normalization   │  Lowercase, remove extra whitespace, strip system prompt
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Embedding Gen   │  Convert text → vector (384 or 1536 dimensions)
│                 │  Option A: Local ONNX model (all-MiniLM-L6-v2, 80MB)
│                 │  Option B: OpenAI text-embedding-3-small ($0.00002/1K)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  HNSW Search    │  Approximate nearest neighbor search
│                 │  Find top-5 candidates from index
│                 │  O(log n) time complexity
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Similarity Gate │  cosine_similarity >= threshold (default: 0.95)
│                 │  Below threshold → cache miss → forward to LLM
│                 │  Above threshold → cache hit → return cached response
└─────────────────┘
```

### Implementation Plan

**Step 1: Embedding Model**

Start with local ONNX (Option A) to avoid API costs and latency:
```
Model: all-MiniLM-L6-v2 (sentence-transformers)
Size:  80MB (bundled with binary or downloaded on first run)
Speed: ~5ms per embedding on CPU
Dims:  384-dimensional vectors
```

Use `ort` crate (ONNX Runtime bindings for Rust).

**Step 2: Vector Index**

Use `instant-distance` or `usearch` (pure Rust HNSW implementations):
```rust
// Index lives in memory. Persisted to disk on shutdown.
// Loaded from disk on startup.
struct SemanticCache {
    index: HnswIndex,              // in-memory HNSW graph
    id_to_cache_key: DashMap<u64, String>, // index ID → cache entry ID
}
```

**Step 3: Persistence**

HNSW index is in-memory for speed. Persistence strategy:
- On graceful shutdown: serialize index to `janus_cache.idx` file
- On startup: deserialize from file (fast, seconds not minutes)
- Database stores embeddings as BYTEA for recovery if index file is lost
- Periodic snapshots every 5 minutes (async background task)

**Step 4: Configuration**

```toml
[cache.semantic]
enabled = true
threshold = 0.95          # cosine similarity (0.0 to 1.0)
max_entries = 50000       # max vectors in HNSW index
embedding_model = "local" # "local" | "openai"
ttl_seconds = 86400       # 24h default
```

### Cost Savings Calculation

Every semantic cache hit saves:
- The full cost of the LLM call
- Latency goes from 500-3000ms → ~5ms (HNSW lookup)

The dashboard will show:
```
Cache Performance
  Total hits:       12,847
  Tokens saved:     4,231,000
  Cost saved:       $127.43
  Avg response:     4.2ms (vs 847ms without cache)
```

---

## 9. Configuration System

### janus.toml (complete example)

```toml
# ─────────────────────────────────────────
# JANUS CONFIGURATION
# ─────────────────────────────────────────

[server]
host = "0.0.0.0"
port = 8080
admin_port = 8081          # Admin API on separate port (optional)
request_timeout_ms = 60000

[database]
# SQLite (default — zero config)
url = "janus.db"

# PostgreSQL (optional — uncomment to use)
# url = "postgres://user:pass@localhost:5432/janus"
# max_connections = 20

[auth]
# Master password for admin dashboard
admin_password_hash = "$2b$12$..."   # bcrypt hash
jwt_secret = "change-this-in-production"
jwt_expiration_hours = 24
# Encryption key for storing provider API keys
# Generate with: janus generate-key
encryption_key = "base64-encoded-32-byte-key"

[cache]
enabled = true
ttl_seconds = 3600          # 1 hour default
max_entries = 100000         # maximum cache entries

[cache.semantic]
enabled = true
threshold = 0.95
embedding_model = "local"   # or "openai"
max_index_size = 50000

[providers.openai]
enabled = true
api_key = "${OPENAI_API_KEY}"   # supports env var interpolation
base_url = "https://api.openai.com/v1"
priority = 1
timeout_ms = 30000
max_retries = 3

[providers.anthropic]
enabled = true
api_key = "${ANTHROPIC_API_KEY}"
base_url = "https://api.anthropic.com"
priority = 2
timeout_ms = 30000
max_retries = 3

[providers.bedrock]
enabled = true
region = "eu-north-1"           # your existing AWS region
priority = 3
timeout_ms = 30000
max_retries = 2
# Uses AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY from environment

[logging]
level = "info"                  # error | warn | info | debug | trace
log_request_bodies = false      # careful — may log PII
log_response_bodies = false
format = "json"                 # json | pretty

[metrics]
prometheus_enabled = true
prometheus_path = "/metrics"

[dashboard]
enabled = true
path = "/"                      # serve dashboard at root
```

### Environment Variable Support
Every config value can be overridden with env vars:
```bash
JANUS_SERVER_PORT=9090
JANUS_DATABASE_URL=postgres://...
JANUS_PROVIDERS_OPENAI_API_KEY=sk-...
```

This follows the 12-factor app methodology and makes Docker/Kubernetes deployment clean.

---

## 10. Security Architecture

### API Key Security
```
Creation:
  1. Generate 48 random bytes (cryptographically secure: ring::rand)
  2. Encode as base62: "jn-sk-[48chars]"
  3. Hash with bcrypt (cost factor 12) for storage
  4. Store ONLY the hash in database
  5. Return FULL key to user ONCE — never shown again

Validation (hot path — must be fast):
  1. Hash incoming key with SHA-256 (not bcrypt — too slow for every request)
  2. Look up SHA-256 hash in dashmap (in-memory, sub-microsecond)
  3. Every 5 minutes: sync API key state from database to dashmap
```

Why two hash algorithms? bcrypt is for secure storage (slow by design — prevents brute force).
SHA-256 is for fast lookup on every request. The bcrypt hash is the source of truth in DB.
The SHA-256 hash is a fast in-memory index derived from it.

### Provider Key Encryption
```
Provider API keys (OpenAI sk-..., etc.) are stored encrypted:
  Algorithm: AES-256-GCM (authenticated encryption)
  Key: derived from admin encryption_key in config
  Each value gets a unique random nonce
  Stored as: base64(nonce + ciphertext + tag)
```

### Transport Security
- TLS termination should happen at the load balancer/reverse proxy (nginx, Caddy)
- Janus itself speaks HTTP internally
- Admin API should be on a separate port, not exposed publicly

### Request Body Privacy
```toml
[logging]
log_request_bodies = false   # DEFAULT: false
log_response_bodies = false  # DEFAULT: false
```

When enabled, a PII scrubber strips common patterns:
- Email addresses
- Phone numbers
- Credit card patterns
- Names appearing in known PII fields

### Rate Limiting Implementation
```
Sliding window counter per API key per minute:
- Stored in dashmap: key_id → VecDeque<Instant>
- On each request: push current time, drain entries older than 1 minute
- Count remaining items: if > rate_limit_rpm → reject
- Memory: O(rpm) per key — negligible
```

---

## 11. Performance Targets

These are the targets to benchmark against. Use `criterion` for micro-benchmarks
and `k6` or `oha` for load tests.

| Metric | Target | How to Measure |
|---|---|---|
| Exact cache hit latency | < 2ms p99 | criterion benchmark |
| Semantic cache hit latency | < 10ms p99 | criterion benchmark |
| Overhead per proxied request | < 10ms p99 | oha load test |
| Streaming TTFB overhead | < 5ms | custom benchmark |
| Concurrent WebSocket connections | > 50,000 | load test |
| Requests per second (non-streaming) | > 5,000 | oha load test |
| Memory per 10K cache entries | < 50MB | manual measurement |
| Startup time (cold) | < 500ms | manual measurement |
| Binary size | < 50MB | `ls -lh` |

### Benchmark Commands (document these in repo)
```bash
# HTTP load test
oha -n 10000 -c 100 http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer jn-sk-test" \
  -m POST -d @test_request.json

# Criterion benchmarks
cargo bench

# Memory profiling
heaptrack ./target/release/janus
```

---

## 12. Testing Strategy

### Layer 1: Unit Tests (in each module)
```
src/cache/mod.rs           → Test exact hash generation, TTL logic
src/cache/semantic.rs      → Test embedding generation, HNSW search, similarity threshold
src/pricing/mod.rs         → Test cost calculation for each provider/model
src/middleware/auth.rs     → Test key validation, expiry, budget checks
src/middleware/ratelimit.rs → Test sliding window algorithm
src/providers/openai.rs    → Test request normalization, response parsing
src/providers/anthropic.rs → Test message format conversion
src/routing/mod.rs         → Test priority routing, failover logic
```

### Layer 2: Integration Tests (tests/ directory)
Use `wiremock` to mock provider APIs:
```rust
// tests/integration/streaming_test.rs
#[tokio::test]
async fn test_openai_streaming_proxied_correctly() {
    let mock_server = MockServer::start().await;
    
    // Mock OpenAI SSE response
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n"))
        .mount(&mock_server)
        .await;
    
    let app = create_test_app(mock_server.uri()).await;
    // ... test streaming behavior
}
```

Key integration test scenarios:
```
✓ Successful proxied request (all providers)
✓ Streaming response correctly forwarded
✓ Exact cache hit returns without hitting provider
✓ Semantic cache hit returns without hitting provider
✓ Rate limit enforced after threshold
✓ Budget limit blocks request
✓ Provider failover on 429
✓ Retry with backoff on 500
✓ Invalid API key → 401
✓ Expired API key → 401
✓ Request logging persisted to database
✓ Cost calculation correct for each model
✓ Daily aggregates updated after request
```

### Layer 3: End-to-End Tests
Manual test checklist before each release:
```
□ Register → receive API key → use key to make request → see in dashboard
□ Stream a response → verify chunks arrive in real-time
□ Hit same prompt twice → second request shows as cached
□ Hit semantically similar prompt → shows as semantic cache hit
□ Exceed rate limit → get 429 with Retry-After
□ Exceed budget → get 402
□ Disable a provider → automatic failover to next
□ Dashboard live feed shows request in real-time
□ Cost graphs update after requests
□ Export requests as CSV
```

### Layer 4: Benchmark Suite
```
benches/
├── cache_lookup.rs          → exact + semantic cache lookup latency
├── request_pipeline.rs      → full pipeline overhead (mocked provider)
├── streaming.rs             → streaming throughput
└── concurrent.rs            → concurrent connection handling
```

---

## 13. Phase-by-Phase Roadmap

Each phase has: goal, tasks, deliverable, and definition of done.

---

### Phase 0: Foundation (Week 1)
**Goal**: Restructure the existing Rust backend into the Janus project structure.

#### Tasks
- [ ] Rename project from `rust-backend` to `janus` in Cargo.toml
- [ ] Redesign directory structure (see Section 16)
- [ ] Add new dependencies: `reqwest`, `dashmap`, `clap`, `config`, `tiktoken-rs`
- [ ] Implement `Config` system using `config` crate (TOML + ENV)
- [ ] Create SQLite migration 0001: create all tables from Section 5
- [ ] Seed `model_pricing` table with initial values
- [ ] Seed `providers` table with OpenAI, Anthropic, Bedrock configs
- [ ] Set up `AppState` with: db pool, config, in-memory cache (dashmap), metrics
- [ ] Add `GET /health` with extended info (providers, db, cache status)
- [ ] Set up GitHub repository with MIT license, basic README skeleton

**Deliverable**: Project compiles. Health endpoint returns provider/db status. All tables exist.

---

### Phase 1: Core Proxy (Weeks 2–4)
**Goal**: Route requests through Janus to OpenAI. Non-streaming only. No caching.

#### Tasks
- [ ] Implement API key model: create, hash, store, validate
- [ ] Implement auth middleware (fast SHA-256 lookup path)
- [ ] Implement `POST /v1/chat/completions` endpoint (non-streaming)
- [ ] Build OpenAI provider adapter (normalize request → OpenAI format, parse response)
- [ ] Build Anthropic provider adapter (different message format, different auth header)
- [ ] Build AWS Bedrock adapter (AWS SigV4 signing, different model ID format)
- [ ] Implement cost calculation from `model_pricing` table
- [ ] Log every request to `requests` table (async, non-blocking)
- [ ] Implement budget check middleware
- [ ] Implement `POST /admin/keys` and `GET /admin/keys` endpoints
- [ ] Write unit tests for each provider adapter
- [ ] Write integration tests with wiremock mocks

**Deliverable**: Can send a request to Janus using an OpenAI SDK with base_url changed. 
Request is proxied to OpenAI (or Anthropic or Bedrock). Cost is tracked. Request is logged.

**Proof of Phase 1 completion**:
```python
# User's application code — UNCHANGED except base_url
from openai import OpenAI
client = OpenAI(api_key="jn-sk-...", base_url="http://localhost:8080/v1")
response = client.chat.completions.create(model="gpt-4o", messages=[...])
```

---

### Phase 2: Streaming (Weeks 5–6)
**Goal**: Forward SSE token streams from all three providers to clients.

#### Tasks
- [ ] Parse OpenAI SSE format (chunked text/event-stream)
- [ ] Parse Anthropic SSE format (different event types than OpenAI)
- [ ] Parse Bedrock streaming response (uses different encoding)
- [ ] Normalize all provider SSE → OpenAI SSE format (so clients see one format)
- [ ] Implement backpressure handling (don't buffer if client is slow)
- [ ] Implement streaming cost tracking (count tokens as chunks arrive)
- [ ] Record TTFB (time to first byte) in request logs
- [ ] Handle aborted streams (client disconnects mid-stream)
- [ ] Integration test: verify stream chunks arrive in correct order

**Deliverable**: `stream: true` requests work. Tokens appear in client as they're generated.
All three providers stream correctly. Costs are tracked accurately for streaming.

---

### Phase 3: Rate Limiting & Reliability (Weeks 7–8)
**Goal**: Production-grade reliability. No single point of failure.

#### Tasks
- [ ] Implement sliding window rate limiter (requests per minute per key)
- [ ] Implement token-per-minute rate limiting (estimated)
- [ ] Return correct `Retry-After` header on 429
- [ ] Implement retry with exponential backoff (configurable max_retries, base_delay)
- [ ] Implement circuit breaker per provider (open after N failures, retry after timeout)
- [ ] Implement provider failover (on 429 or circuit open → try next priority)
- [ ] Implement provider health checks (background task, ping every 60s)
- [ ] Update `providers` table health_status from health check results
- [ ] Write tests for retry logic, failover, circuit breaker

**Deliverable**: Janus survives provider outages. Rate limits enforced accurately. 
Failover is transparent to the client.

---

### Phase 4: Exact Cache (Weeks 9–10)
**Goal**: Identical requests served from cache. Zero provider calls.

#### Tasks
- [ ] Implement request normalization (deterministic JSON serialization)
- [ ] Implement SHA-256 cache key generation
- [ ] Implement two-layer cache: dashmap (hot) + SQLite (persistent)
- [ ] Cache warmup on startup (load recent entries from DB into dashmap)
- [ ] Implement cache TTL with background cleanup task
- [ ] Store cache stats (hit_count, tokens_saved, cost_saved)
- [ ] Implement cache bypass via `X-Janus-Cache: false` header
- [ ] Add `GET /admin/cache/stats` endpoint
- [ ] Add `POST /admin/cache/flush` endpoint

**Deliverable**: Identical prompt sent twice → second response is instant (< 2ms).
Dashboard shows cache hit rate and cost savings.

---

### Phase 5: Semantic Cache (Weeks 11–14)
**Goal**: Semantically similar requests served from cache. The killer feature.

#### Tasks
- [ ] Add `ort` crate for ONNX runtime
- [ ] Download and bundle `all-MiniLM-L6-v2` ONNX model
- [ ] Implement embedding generation pipeline
- [ ] Add `instant-distance` or `usearch` for HNSW index
- [ ] Implement `SemanticCache` struct (HNSW index + ID mapping)
- [ ] Implement similarity search with configurable threshold
- [ ] Implement index persistence (serialize on shutdown, deserialize on startup)
- [ ] Implement periodic index snapshotting (async background task)
- [ ] Store embeddings as BYTEA in SQLite for recovery
- [ ] Add semantic cache stats to `/admin/cache/stats`
- [ ] Configurable threshold in `janus.toml`
- [ ] Benchmark embedding generation latency (target: < 10ms)
- [ ] Benchmark HNSW search latency (target: < 5ms for 50K entries)

**Deliverable**: "What is the capital of France?" and "Tell me France's capital" → same cached response.
Dashboard shows semantic cache hits separately from exact hits, with similarity scores.

---

### Phase 6: Web Dashboard (Weeks 15–20)
**Goal**: Beautiful, functional admin UI. Embedded in the binary.

#### Setup
```bash
# Dashboard lives in dashboard/ directory at project root
npx create-next-app@latest dashboard --typescript --tailwind --app
cd dashboard && npx shadcn@latest init
```

#### Pages & Components

**Overview Page** (`/`)
- [ ] Today's total cost (big number, prominent)
- [ ] Requests today / this week / this month
- [ ] Cache hit rate percentage
- [ ] Provider health indicators (green/yellow/red dots)
- [ ] Cost trend chart (last 30 days, by provider)
- [ ] Top 5 most expensive models today

**Live Request Feed** (`/requests`)
- [ ] Real-time WebSocket stream of incoming requests
- [ ] Each row: timestamp, model, tokens, cost, latency, cache status, status code
- [ ] Click row → full request/response detail (if logging enabled)
- [ ] Filters: provider, model, status, API key, date range
- [ ] Export to CSV button
- [ ] Pause/resume live feed toggle

**Cost Analytics** (`/analytics/costs`)
- [ ] Cost by day (bar chart, 30 days)
- [ ] Cost by provider (pie chart)
- [ ] Cost by model (horizontal bar chart)
- [ ] Cost by API key (table)
- [ ] Cache savings vs actual spend (stacked bar)

**Latency Analytics** (`/analytics/latency`)
- [ ] p50/p95/p99 by model (table)
- [ ] Latency distribution (histogram)
- [ ] TTFB distribution for streaming

**Cache Analytics** (`/analytics/cache`)
- [ ] Hit rate over time (line chart)
- [ ] Tokens saved over time
- [ ] Cost saved (running total)
- [ ] Recent cache entries (with similarity scores for semantic hits)
- [ ] Flush cache button

**API Keys** (`/keys`)
- [ ] List all keys with usage summary
- [ ] Create key modal (name, budget, rate limit)
- [ ] Key detail page (usage chart, recent requests)
- [ ] Revoke key button (with confirmation)

**Providers** (`/providers`)
- [ ] Provider cards with health status
- [ ] Enable/disable toggle
- [ ] Priority ordering (drag to reorder — optional)
- [ ] Edit API key (shown masked, click to update)
- [ ] Test connection button

**Settings** (`/settings`)
- [ ] Cache configuration (threshold, TTL, enable/disable)
- [ ] Logging configuration
- [ ] Rate limit defaults
- [ ] Danger zone (flush cache, export all data)

#### Embedding Dashboard in Binary
```rust
// build.rs — runs at compile time
use std::process::Command;

fn main() {
    // Build Next.js dashboard
    Command::new("npm").args(["run", "build"]).current_dir("dashboard").status().unwrap();
    println!("cargo:rerun-if-changed=dashboard/src");
}

// src/dashboard.rs — serve at runtime
use include_dir::{include_dir, Dir};
static DASHBOARD: Dir = include_dir!("$CARGO_MANIFEST_DIR/dashboard/out");
```

**Deliverable**: `./janus` starts server. Open browser at `http://localhost:8080` → 
fully functional dashboard. No separate process. No npm serve.

---

### Phase 7: Production Hardening (Weeks 21–23)
**Goal**: Ready for real-world production deployments.

#### Tasks
- [ ] Request size limits (reject bodies > 1MB)
- [ ] Graceful shutdown (finish in-flight requests, flush cache snapshot)
- [ ] Prometheus metrics endpoint (`/metrics`)
  - `janus_requests_total` (labels: provider, model, status, cache_type)
  - `janus_request_duration_seconds` (histogram)
  - `janus_tokens_total` (labels: provider, model, type)
  - `janus_cost_usd_total` (labels: provider, model)
  - `janus_cache_size` (gauge)
  - `janus_cache_hit_ratio` (gauge)
- [ ] CORS configuration for dashboard
- [ ] PII scrubber for request body logging
- [ ] Database connection pool tuning
- [ ] Add `CHANGELOG.md`
- [ ] Write full `README.md` with quickstart
- [ ] Create `docs/` directory with deployment guides
- [ ] Docker multi-stage build (single binary → minimal image)
- [ ] GitHub Actions: test → build → release binary for linux/amd64, darwin/arm64, darwin/amd64, windows/amd64
- [ ] Write `janus.toml.example` with full documentation
- [ ] Benchmark suite: run and document results

**Deliverable**: Binary available for download. Works on Linux and macOS.
README with 5-minute quickstart. Benchmarks published.

---

### Phase 8: Open Source Launch (Week 24–26)
**Goal**: Public launch. GitHub stars. Community.

#### Pre-Launch Checklist
- [ ] GitHub repository is public (MIT license)
- [ ] README hero section (what it is, install command, screenshot)
- [ ] 5-minute quickstart (copy-paste, works first try)
- [ ] Demo video (2 minutes: install, make requests, show dashboard)
- [ ] Documentation site (Nextra or Docusaurus, deploy to GitHub Pages)
- [ ] `CONTRIBUTING.md` — how to add a provider, run tests, submit PRs
- [ ] Issue templates (bug report, feature request, provider request)
- [ ] GitHub Actions badges in README (build passing, test passing)
- [ ] Published benchmarks page (compare with LiteLLM, raw provider latency)

#### Launch Sequence
1. **Hacker News: Show HN** — "Show HN: Janus — Self-hosted AI gateway in Rust, single binary"
2. **Reddit: r/rust** — Technical post about the HNSW semantic cache implementation
3. **Reddit: r/selfhosted** — Focus on single binary, no dependencies angle
4. **Dev.to** — Technical blog post: "Building a semantic cache for LLMs in Rust"
5. **Twitter/X** — Demo video, benchmark comparison
6. **Product Hunt** — Launch with demo video and description

**Deliverable**: Public GitHub repo. First external contributors welcomed.

---

### Phase 9: Mobile App (Weeks 27–33)
**Goal**: React Native monitoring app for spend alerts and live feed.

#### Screens
- [ ] **Dashboard** — Today's spend, request count, cache hit rate, provider health
- [ ] **Live Feed** — Real-time request stream (WebSocket), filterable
- [ ] **Cost Trends** — 30-day cost chart, breakdown by model
- [ ] **Alerts** — Configure spend thresholds, push notifications
- [ ] **Settings** — Connect to Janus instance (URL, admin token)

#### Technical
- [ ] Expo setup with TypeScript
- [ ] WebSocket connection for live feed
- [ ] Expo Notifications for budget alerts
- [ ] Secure credential storage (Expo SecureStore)
- [ ] iOS + Android support

**Deliverable**: App works on iOS and Android. Connects to your Janus instance.
Real-time spend monitoring on your phone.

---

## 14. Open Source Launch Plan

### Target Audience
1. **Primary**: Developers building AI features into their products (solo devs, small startups)
2. **Secondary**: Self-hosting enthusiasts who want to audit their LLM costs
3. **Tertiary**: Companies wanting to avoid LiteLLM's Python overhead

### Positioning Statement
> Janus is the simplest way to get observability, caching, and reliability 
> for your LLM calls — without vendor lock-in. One binary. Zero dependencies.

### What Will Get GitHub Stars
Based on the research (PocketBase, Tauri, uv pattern):
1. The README must show an install command in the first 3 lines
2. The dashboard screenshot must be in the README (before any technical explanation)
3. The quickstart must work in under 5 commands
4. The benchmark numbers must be published and reproducible

### The README Formula
```markdown
# Janus
**Self-hosted AI gateway. Single binary. Built in Rust.**

[Screenshot of dashboard]

## Install
curl -L https://github.com/you/janus/releases/latest/download/janus-linux-amd64 -o janus
chmod +x janus
./janus

## Use
Change one line in your app:
  base_url = "https://api.openai.com/v1"
  →
  base_url = "http://localhost:8080/v1"

That's it. Your costs are now tracked. Your prompts are now cached.
```

---

## 15. V2 and Beyond

These are intentionally NOT in v1. They are the natural evolution after real users use the product and tell you what they actually need.

### v2 Candidates
- **Prompt management** — Save, version, and A/B test prompts
- **Model playground** — Compare model outputs side by side in the dashboard
- **Cost budgets per team/feature** — Budget envelopes for different parts of your product
- **Webhook alerts** — POST to Slack/Discord/email when thresholds hit
- **Multi-node clustering** — Run multiple Janus instances with shared state
- **Plugin system** — Let community add providers via WASM plugins
- **MCP server** — Let Claude/GPT use Janus as a tool directly

### Potential Business Model (if you choose to commercialize)
```
Free tier:     Self-hosted, MIT license, forever free
Cloud tier:    Managed hosting, $29/month, no ops burden
Enterprise:    SSO, audit logs, SLA, dedicated support
```

---

## 16. Project File Structure

This is the target structure for the complete Janus project.

```
janus/
│
├── Cargo.toml                     ← Add new dependencies here
├── Cargo.lock
├── janus.toml.example             ← Full documented config example
├── README.md
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE                        ← MIT
│
├── build.rs                       ← Build Next.js dashboard, embed in binary
│
├── src/
│   ├── main.rs                    ← Entry point. Bootstrap. Start server.
│   ├── config.rs                  ← Config struct. Load from file + env.
│   ├── state.rs                   ← AppState (db, config, cache, metrics)
│   ├── errors.rs                  ← Error types → HTTP responses
│   │
│   ├── gateway/                   ← Core gateway logic
│   │   ├── mod.rs
│   │   ├── router.rs              ← Route gateway requests
│   │   └── pipeline.rs            ← Full request pipeline (steps 1-15)
│   │
│   ├── providers/                 ← LLM provider adapters
│   │   ├── mod.rs                 ← Provider trait definition
│   │   ├── openai.rs              ← OpenAI adapter
│   │   ├── anthropic.rs           ← Anthropic adapter
│   │   └── bedrock.rs             ← AWS Bedrock adapter
│   │
│   ├── cache/                     ← Caching subsystem
│   │   ├── mod.rs                 ← CacheEngine struct
│   │   ├── exact.rs               ← SHA-256 exact match cache
│   │   └── semantic.rs            ← HNSW semantic cache + embeddings
│   │
│   ├── middleware/                ← Axum middleware
│   │   ├── mod.rs
│   │   ├── auth.rs                ← API key validation
│   │   ├── rate_limit.rs          ← Sliding window rate limiter
│   │   ├── budget.rs              ← Budget enforcement
│   │   └── logging.rs             ← Request/response logging
│   │
│   ├── models/                    ← Data models
│   │   ├── mod.rs
│   │   ├── api_key.rs
│   │   ├── request.rs
│   │   ├── cache_entry.rs
│   │   └── provider.rs
│   │
│   ├── db/                        ← Database queries
│   │   ├── mod.rs
│   │   ├── api_keys.rs
│   │   ├── requests.rs
│   │   ├── cache.rs
│   │   ├── providers.rs
│   │   └── analytics.rs
│   │
│   ├── handlers/                  ← HTTP handlers
│   │   ├── mod.rs
│   │   ├── gateway.rs             ← POST /v1/chat/completions etc.
│   │   ├── admin/
│   │   │   ├── mod.rs
│   │   │   ├── keys.rs
│   │   │   ├── requests.rs
│   │   │   ├── analytics.rs
│   │   │   ├── cache.rs
│   │   │   ├── providers.rs
│   │   │   └── stream.rs          ← WebSocket live feed
│   │   └── health.rs
│   │
│   ├── pricing/                   ← Cost calculation
│   │   └── mod.rs
│   │
│   ├── metrics/                   ← Prometheus + in-memory metrics
│   │   └── mod.rs
│   │
│   ├── routing/                   ← Provider selection logic
│   │   └── mod.rs
│   │
│   └── dashboard.rs               ← Serve embedded Next.js static files
│
├── migrations/
│   ├── 0001_initial_schema.sql
│   └── 0002_seed_pricing.sql
│
├── dashboard/                     ← Next.js admin dashboard
│   ├── package.json
│   ├── next.config.js
│   ├── tsconfig.json
│   └── src/
│       ├── app/
│       │   ├── layout.tsx
│       │   ├── page.tsx           ← Overview
│       │   ├── requests/
│       │   ├── analytics/
│       │   ├── keys/
│       │   ├── providers/
│       │   └── settings/
│       ├── components/
│       └── lib/
│           ├── api.ts             ← API client
│           └── websocket.ts       ← Live feed WebSocket
│
├── mobile/                        ← React Native app (Phase 9)
│   └── ...
│
├── benches/                       ← Criterion benchmarks
│   ├── cache_lookup.rs
│   ├── request_pipeline.rs
│   └── streaming.rs
│
├── tests/                         ← Integration tests
│   ├── integration/
│   │   ├── auth_test.rs
│   │   ├── caching_test.rs
│   │   ├── streaming_test.rs
│   │   ├── failover_test.rs
│   │   └── cost_tracking_test.rs
│   └── fixtures/
│       ├── openai_response.json
│       ├── anthropic_response.json
│       └── bedrock_response.json
│
├── docs/                          ← Documentation (markdown)
│   ├── quickstart.md
│   ├── configuration.md
│   ├── deployment/
│   │   ├── docker.md
│   │   ├── systemd.md
│   │   └── kubernetes.md
│   ├── providers/
│   │   ├── openai.md
│   │   ├── anthropic.md
│   │   └── bedrock.md
│   └── api-reference.md
│
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                 ← Run tests on every PR
│   │   └── release.yml            ← Build binaries on tag push
│   └── ISSUE_TEMPLATE/
│       ├── bug_report.md
│       └── feature_request.md
│
├── Dockerfile                     ← Update to use new structure
├── docker-compose.yml             ← Update for Janus
└── .gitignore
```

---

## Quick Reference: What to Build First

If you want to start tomorrow, do this in order:

```
Day 1:    Set up new project structure, config system, updated schema
Day 2-3:  OpenAI adapter, basic proxy (non-streaming), API key auth
Day 4-5:  Anthropic + Bedrock adapters, cost calculation
Day 6:    Request logging, admin endpoints for keys
Day 7:    Streaming support for all three providers
Day 8:    Rate limiting, retry + failover
Day 9-10: Exact cache
Day 11-15: Semantic cache (hardest part)
Day 16-25: Dashboard (most time)
Day 26+:  Hardening, docs, launch
```

---

*This document is the single source of truth for the Janus project.*
*Update it as decisions change. Never let it go stale.*
*Version all changes with dates in the CHANGELOG.*
