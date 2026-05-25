# JANUS — Future Backlog
> Items here are real and important but not scheduled for V4 or earlier.
> Before starting any item, create a proper roadmap section for it.
> Nothing here is a commitment — it is a parking lot.

---

## 1. Testing & Quality Assurance

### Frontend E2E Tests (Playwright)
**Why deferred**: Writing E2E tests requires a running Janus instance with seeded
data, and adds significant maintenance overhead. Deferred until dashboard stabilizes
after V4-7.

**What it covers**: Every dashboard page tested with real browser automation —
create/edit/delete flows for keys, alerts, prompts; playground sending a real
request; cost simulator returning results; RBAC restricting access.

**When to do it**: After V4-7 (Dashboard Overhaul) is complete and stable.
One E2E test session per page is enough to start.

### Frontend Manual Verification Checklists
**Why deferred**: Should be added to V4-1 and V4-7 before those phases execute,
not in the roadmap planning phase. Reminder: add smoke test checklists to each
frontend phase before starting implementation.

**What it covers**: Per-feature checklist:
- Create / edit / delete each entity
- Verify API calls succeed (Network tab)
- Verify errors surface to user (not silent failures)
- Verify one-time-display flows (key rotation, key creation)

### Load / Stress Testing
**Why deferred**: No confirmed production scale targets yet.

**What it covers**:
- Gateway throughput under 1000 req/sec
- Dedup layer under high concurrency
- Semantic cache performance at 100k+ entries (HNSW)
- Database connection pool behavior under spike traffic

### Chaos Testing
**Why deferred**: Requires infrastructure (chaos toolkit, test environment).

**What it covers**:
- Provider failure mid-stream
- Database connection loss mid-request
- OTel exporter down (V3-2) — should be zero impact
- Cache layer failure — should fall through to provider

---

## 2. Security & Compliance

### SSO / SAML / OIDC
**Why deferred (V3 and V4)**: Requires identity provider integration layer —
Auth0, Okta, Keycloak, or similar. Significant scope.

**What it covers**:
- Login via external IdP instead of username/password
- Group-to-role mapping from IdP claims
- SCIM for user provisioning/deprovisioning

**Dependencies**: V4-8 (RBAC) must be complete first.

### Admin-Configurable Policy Rules
**Why deferred**: V3-4 Plugin Middleware handles code-level policies. An
admin-configurable rules engine (stored in DB, UI-manageable) is a separate layer.

**What it covers**:
- Rule builder in dashboard: if X then Y
- Examples: "if model = gpt-4o and workspace = free-tier → reject"
- "if prompt contains PII pattern → route to internal provider only"
- "if hour < 9 or hour > 18 → use cheaper model"

**Dependencies**: V3-4 Plugin Middleware + V4-8 RBAC.

### Audit Log Tamper-Evident Chain
**Why deferred**: V3-5 adds per-response hash (`X-Janus-Audit-Hash`).
A proper tamper-evident chain requires each log entry to include the hash
of the previous entry — this is a more significant change.

**What it covers**:
- Each request record includes `previous_hash` field
- Export includes chain verification tool
- Detects if any row was modified or deleted from audit log

### Prompt Injection Detection
**Why deferred**: Requires either a dedicated ML model or LLM-as-judge — both
add latency and cost to every request.

**What it covers**:
- Detect prompt injection attempts in user messages
- Configurable: block / flag / log
- Can be built as a V3-4 plugin once that infrastructure exists

---

## 3. Scalability & Infrastructure

### Qdrant / Pinecone Integration (V4-9 if demand exists)
**Why conditional**: HNSW (V3-1) scales to hundreds of thousands of entries.
External vector stores only needed at millions of entries or specific customer request.

**Start condition**: > 500k semantic cache entries in a single deployment.

### Horizontal Semantic Cache (Multi-node)
**Why deferred**: V2-6 clustering shares the exact cache (PostgreSQL-backed) but
the semantic index (HNSW in-memory) is per-node. Semantic hits on node B after
node A populated the index are misses.

**What it covers**:
- Share semantic embeddings across nodes via external vector store (Qdrant)
- Or: periodic embedding sync between nodes via PostgreSQL BYTEA table
  (already exists — just not loaded on all nodes at runtime)

**Dependencies**: V4-9 (External Vector Stores).

### Async Job Queue (for OpenAI Batch API)
**Why deferred**: Supporting OpenAI's Batch API requires an async job queue,
job status endpoint, and result polling — significant architectural addition.

**What it covers**:
- `POST /v1/batches` — submit batch job
- `GET /v1/batches/:id` — poll status
- Jobs stored in DB, processed by background worker
- Cost savings: OpenAI batch is 50% cheaper

### Budget Forecasting (ML-based)
**Why deferred**: Requires time-series model trained on usage patterns. V4-5
gives the cost data; forecasting is the next step.

**What it covers**:
- "At current rate, this workspace will hit budget in N days"
- Dashboard widget with spend trajectory
- Alert integration: fire when forecast exceeds threshold

---

## 4. New API Capabilities

### `/v1/images/generations` Pass-through
**Why deferred**: Not in V1-V4 scope. Straightforward to add as a new provider
trait method + handler, but no demand confirmed yet.

### `/v1/audio/transcriptions` and `/v1/audio/speech` Pass-through
**Why deferred**: Same as images — straightforward, no confirmed demand.

### Fine-tuning Job Proxy (`/v1/fine_tuning/*`)
**Why deferred**: Fine-tuning is async, requires job polling, and involves
large file uploads. More complex than completion proxying.

### Prompt Compression / Token Optimization
**Why deferred (V3 and V4)**: Janus is a transparent proxy. Modifying prompts
by default violates user trust. Only acceptable as explicit opt-in.

**If implemented**: Must be a V3-4 plugin, opt-in per API key, with
`X-Janus-Compressed: true` response header so callers know the prompt was modified.
Never default behavior.

---

## 5. SDKs & Integrations

### Python SDK
**Why deferred**: Separate repository. OpenAI's Python SDK already works against
Janus by changing `base_url` — a thin Janus-specific wrapper adds analytics,
key management helpers, and typed responses.

### Node.js / TypeScript SDK
**Why deferred**: Same as Python SDK.

### Terraform Provider
**Why deferred**: For infrastructure-as-code users — manage API keys, providers,
alerts, workspaces as Terraform resources.

### GitHub Actions Integration
**Why deferred**: Pre-built action to run Janus in CI for LLM-dependent test
suites with cost caps and caching.

---

## 6. Observability & Analytics

### OTel Trace Visualization in Dashboard
**Why deferred**: V3-2 adds OTel export to external backends (Jaeger, Grafana
Tempo). Embedding a trace viewer in the dashboard is significant UI work and
duplicates what Jaeger/Grafana already do well.

**What it covers**: Clickable trace waterfall per request in the request detail view.

### Rate Limit Analytics
**Why deferred**: Rate limit hits are logged but there is no analysis.

**What it covers**:
- Which keys hit rate limits most often
- Peak usage windows
- Recommendation: "key X would benefit from a higher RPM limit"

### Function Calling Analytics
**Why deferred**: Tool use passes through in V2-3, but which tools are called,
how often, and at what cost is not tracked.

**What it covers**:
- Per-tool call frequency and cost breakdown
- Error rate per tool
- Dashboard visualization

### SLA Reporting Per Workspace
**Why deferred**: Requires defining SLA metrics per workspace — uptime, p95
latency, error rate — and generating monthly reports.

---

## 7. Developer Experience

### `janus dev` — Local Development Mode
**Why deferred**: Currently, seeing dashboard changes requires `cargo build`.

**What it covers**:
- `janus dev` starts Rust on :8080 and Next.js dev server on :3000
- Next.js proxies API calls to Rust
- Hot reload for dashboard changes without Rust rebuild
- This was noted in V4 planning as "decide before V4-d"

### OpenAPI / Swagger Spec
**Why deferred**: No machine-readable spec for the admin API exists.

**What it covers**:
- Auto-generated from axum handler signatures
- Served at `GET /admin/openapi.json`
- Swagger UI at `GET /admin/docs`
- Enables client SDK generation

### Notification Channels Beyond Webhooks
**Why deferred**: V2-2 adds webhook alerts. Additional channels are straightforward
extensions but require external service integrations.

**What it covers**: Email (SMTP), Slack App (OAuth, not just webhook), PagerDuty,
OpsGenie. Each as a new `WebhookFormat` variant or separate `NotificationChannel` enum.

---

## 8. Managed / Hosted Offering

### Cloud Control Plane
**Why deferred (V3 and V4)**: Product strategy, not engineering. Requires
billing, account management, multi-tenant isolation, and ops infrastructure.

**What it covers**:
- SaaS version of Janus
- Per-workspace isolation
- Usage-based billing
- Managed upgrades

---

*Last updated: 2026-05-24*
*Add items here when a feature is explicitly deferred from a roadmap phase.*
*Remove items when they are scheduled in a concrete roadmap.*
