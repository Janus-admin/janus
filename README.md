# Janus — Self-Hosted AI Gateway

**One base_url. Every model. Every provider. Your VPC.**

```bash
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=$(openssl rand -base64 32) \
  -e OPENAI_API_KEY=$YOUR_KEY \
  ghcr.io/Janus-admin/janus:latest
```

Or run locally: `cargo run` (requires Postgres and `models/`)

---

## What Is Janus?

Janus is a **self-hosted AI gateway** that sits between your applications and every LLM provider (OpenAI, Anthropic, Google Gemini, Groq, DeepSeek, AWS Bedrock). Change one `base_url` and get:

- 🔀 **Smart model routing** — automatically picks the cheapest model that can handle each request
- 💾 **Two-layer caching** — exact match (< 2ms) + semantic similarity (< 10ms, ONNX embeddings)
- 💰 **Cost control** — per-token pricing, per-key budgets, per-team cost tagging
- 🔄 **Provider failover** — circuit breakers, automatic retry, priority-based fallback
- 🔐 **Auth & RBAC** — API keys, JWT, OIDC/SSO, workspace roles
- 🚨 **Alerts** — Slack and email notifications on spend, latency, and error thresholds
- 📊 **Observability** — Prometheus `/metrics`, structured audit log, web dashboard
- 🛡️ **PII redaction** — scrubs cards, SSNs, emails, and tokens before any log or cache row

### What It's NOT

Janus is **not** a database, a BaaS, a Firebase clone, or a generic ML platform. It's specifically designed for LLM gateway use cases.

---

## 5-Minute Quickstart

### 1. Start Postgres

```bash
docker run -d \
  --name janus-postgres \
  -e POSTGRES_PASSWORD=janus_dev \
  -e POSTGRES_DB=janus \
  -p 5432:5432 \
  postgres:16
```

### 2. Set environment variables

```bash
export DATABASE_URL=postgres://postgres:janus_dev@localhost:5432/janus
export JWT_SECRET=$(openssl rand -base64 32)
export ENCRYPTION_KEY=$(openssl rand -base64 32)

# Set at least one provider key
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
export GEMINI_API_KEY=...
export GROQ_API_KEY=...
export DEEPSEEK_API_KEY=...
```

### 3. Run Janus

```bash
cargo run --release
# Server listening on 0.0.0.0:8080
```

### 4. Create an API key and test

```bash
# Create a key (shown once, never again)
curl -X POST http://localhost:8080/admin/keys \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"test","budget":100}'

# Proxy an LLM call (OpenAI-compatible)
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer jn-sk-..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

Full interactive API docs: `http://localhost:8080/admin/docs` (Swagger UI, no auth required)

---

## Features

| Capability | Details |
|---|---|
| **Providers** | OpenAI, Anthropic, AWS Bedrock, Gemini, Groq, DeepSeek, any OpenAI-compatible endpoint |
| **Gateway endpoints** | `/v1/chat/completions`, `/v1/embeddings`, `/v1/completions`, `/v1/models`, `/v1/images/generations`, `/v1/audio/speech`, `/v1/audio/transcriptions` |
| **Smart routing** | Complexity-scored automatic model selection; 4-layer pipeline; admin routing rules per workspace |
| **Caching** | Exact (SHA-256, < 2ms) + semantic (ONNX cosine similarity, < 10ms, configurable threshold) |
| **Cost tracking** | Per-token, per-image, per-audio-second pricing; per-key and per-team budgets; cost tags |
| **Alerts** | Slack block-kit and SMTP email on spend, error rate, and latency thresholds |
| **Failover** | Per-provider circuit breakers, automatic retry, priority-based provider switching |
| **Auth** | Gateway API keys (`jn-sk-…`), admin JWT, OIDC/SSO (PKCE, group→role mapping, JIT provisioning) |
| **RBAC** | ReadOnly / BillingViewer / ApiManager / Admin roles, scoped per workspace |
| **Workspaces** | Multi-tenant — separate keys, budgets, routing rules, and members per workspace |
| **Prompts** | Versioned prompt library with create/update/delete and per-version activation |
| **MCP** | Model Context Protocol server — RPC + SSE endpoints for tool-calling agents |
| **Observability** | Prometheus `/metrics`, structured request audit log, web dashboard, live WebSocket stream |
| **Deployment** | Docker, Helm chart (`charts/janus/`), Railway, Fly.io, Render one-click configs |

---

## Smart Routing

When you omit `model` from a request, Janus selects one automatically using a 4-layer pipeline:

1. **Capability filter** — removes models that can't handle the request (vision, JSON mode, context window)
2. **Explicit contract** — applies tag-based rules (`X-Janus-Tags: quality=premium`) and admin-defined workspace routing rules
3. **Complexity scoring** — scores the request on token estimate, conversation depth, tool use, and complex-verb patterns; maps to a micro / standard / premium tier
4. **Config default** — falls back to the workspace's configured default model

```bash
# Let Janus pick the model (omit "model")
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer jn-sk-..." \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [{"role": "user", "content": "What is 2+2?"}]
  }'

# Response headers tell you what was chosen and why
# X-Janus-Model-Selected: groq/llama-3.1-8b-instant
# X-Janus-Routing-Reason: complexity:micro(score=1)
```

Route premium requests explicitly with a tag:

```bash
curl ... \
  -H "X-Janus-Tags: quality=premium" \
  -d '{"messages": [...]}'
```

Admin routing rules (workspace-scoped, ordered, first-match wins) are managed at:

```
GET/PUT   /admin/workspaces/:id/smart-routing/config
GET/POST  /admin/workspaces/:id/smart-routing/rules
PATCH/DELETE /admin/workspaces/:id/smart-routing/rules/:rule_id
```

---

## Configuration

Copy `janus.toml.example` to `janus.toml` and customize:

```toml
host = "0.0.0.0"
port = 8080
database_url = "postgres://..."
jwt_secret = "your-secret"
encryption_key = "your-encryption-key"
openai_api_key = "sk-..."
anthropic_api_key = "sk-ant-..."

cache_enabled = true
semantic_cache_threshold = 0.90

rate_limit_window_secs = 60
max_retries = 1

prometheus_enabled = true

[smart_routing]
enabled = false         # set true to allow omitting "model"
default_model = ""      # fallback when no routing rule matches
```

All settings can be overridden with `UPPERCASE_ENV_VARS`.
Full reference: [`docs/configuration.md`](docs/configuration.md)

---

## API Examples

### Gateway (OpenAI-compatible)

**Standard call with a pinned model**

```json
POST /v1/chat/completions
{
  "model": "gpt-4o",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": false
}
```

**Smart-routed call (model omitted)**

```json
POST /v1/chat/completions
{
  "messages": [{"role": "user", "content": "Hello"}]
}
```

Response headers:

| Header | Example value |
|---|---|
| `X-Janus-Model-Selected` | `groq/llama-3.1-8b-instant` |
| `X-Janus-Routing-Reason` | `complexity:micro(score=1)` |
| `X-Janus-Cache-Hit` | `exact` or `semantic` |
| `X-Janus-Cache-Similarity` | `0.9542` (semantic hits only) |

Skip cache for a single request:

```bash
curl ... -H "X-Janus-Cache: false"
```

Tag a request for cost attribution:

```bash
curl ... -H "X-Janus-Tags: team=growth,feature=summariser"
```

### Admin API (selected endpoints)

```
POST   /admin/keys                   — Create API key
GET    /admin/keys                   — List keys (safe view, no secrets)
GET    /admin/analytics/overview     — Daily costs, request counts, top models
GET    /admin/analytics/cost-by-tag  — Cost breakdown by tag key
GET    /admin/cache/stats            — Hit ratio, tokens saved, cost saved
DELETE /admin/cache                  — Flush cache
GET    /admin/alerts                 — List alert rules
POST   /admin/alerts                 — Create alert rule
GET    /admin/providers              — List configured providers + health
GET    /admin/workspaces             — List workspaces and members
GET    /admin/prompts                — Versioned prompt library
GET    /metrics                      — Prometheus metrics
GET    /admin/docs                   — Swagger UI (OpenAPI 3.1)
GET    /admin/openapi.json           — Raw OpenAPI spec
```

### `janus` CLI

```bash
# Key management
janus keys list
janus keys create --name production --budget 500

# Migrations
janus migrate up
janus migrate status

# Import from competitors
janus import litellm  --file litellm_config.yaml
janus import portkey  --file portkey.json

# Backup / restore
janus backup create  --out backup.tar.gz
janus backup restore --file backup.tar.gz
```

---

## Caching Strategy

### Exact Cache

- **Key:** SHA-256 of normalized request body (stream field excluded)
- **Lookup:** < 2ms (in-memory DashMap hot layer, Postgres persistent)

### Semantic Cache

- **Key:** Cosine similarity over prompt embeddings (ONNX, all-MiniLM-L6-v2, 384-dim)
- **Lookup:** < 10ms
- **Threshold:** 0.90 (configurable via `semantic_cache_threshold`)
- **Optional backend:** Qdrant vector store (`semantic_cache_backend = "qdrant"`)

Download the embedding model before first run:

```bash
mkdir -p models
# Download all-MiniLM-L6-v2.onnx + tokenizer.json from HuggingFace
# Janus degrades gracefully to exact-only caching if the model is absent
```

---

## OIDC / SSO

Configure identity providers from the admin dashboard or API:

```bash
POST /admin/idp
{
  "kind": "oidc",
  "name": "Okta",
  "issuer": "https://your-org.okta.com",
  "client_id": "...",
  "client_secret": "...",
  "group_role_map": {"ai-platform": "Admin", "viewers": "ReadOnly"}
}
```

Login flow: `GET /auth/oidc/:idp_id/start` → IdP → `GET /auth/oidc/:idp_id/callback` → JWT.
Users are provisioned JIT on first login. Group claims map to Janus RBAC roles.

---

## Metrics (Prometheus)

Available at `GET /metrics`:

```
janus_requests_total{provider="openai",model="gpt-4o",status="success",cache_type="exact"} 142
janus_request_duration_seconds_bucket{provider="openai",model="gpt-4o",le="5ms"} 45
janus_tokens_total{provider="openai",model="gpt-4o",direction="prompt"} 2840
janus_cost_usd_total{provider="openai",model="gpt-4o"} 0.142857
janus_cache_exact_size 1204
janus_cache_semantic_size 398
janus_cache_hit_ratio 0.71
```

---

## Deployment

### Docker

```bash
docker build -t janus:latest .
docker run -p 8080:8080 \
  -e DATABASE_URL=postgres://... \
  -e JWT_SECRET=... \
  -e ENCRYPTION_KEY=... \
  -e OPENAI_API_KEY=... \
  janus:latest
```

Full Docker Compose setup: [`docs/deployment/docker.md`](docs/deployment/docker.md)

### Kubernetes (Helm)

```bash
helm install janus charts/janus \
  --set secrets.jwtSecret=$(openssl rand -base64 32) \
  --set secrets.encryptionKey=$(openssl rand -base64 32) \
  --set secrets.openaiApiKey=$OPENAI_API_KEY \
  --set database.url=postgres://...
```

Full Helm reference: [`docs/deployment/helm.md`](docs/deployment/helm.md)

### One-click cloud deploys

| Platform | Config |
|---|---|
| Railway | [`deploy/railway/`](deploy/railway/) |
| Fly.io | [`deploy/fly/`](deploy/fly/) |
| Render | [`deploy/render/`](deploy/render/) |

HA runbook (multiple nodes, Postgres replica, encryption key rotation): [`docs/deployment/ha.md`](docs/deployment/ha.md)

---

## Contributing

Janus is source-available under the Elastic License 2.0. PRs welcome.

**Before submitting:**

1. `cargo test` — all tests must pass
2. `cargo clippy -- -D warnings` — zero warnings
3. `cargo fmt` — code must be formatted

**Development setup:**

```bash
git clone https://github.com/Janus-admin/janus.git
cd janus
cp janus.toml.example janus.toml
# Edit janus.toml with your Postgres + provider API keys
cargo test
cargo run
```

---

## License

[Elastic License 2.0 (ELv2)](LICENSE) — free to self-host, modify, and contribute. You may not offer Janus as a hosted managed service to third parties.

---

## Support

- **Issues:** [GitHub Issues](https://github.com/Janus-admin/janus/issues)
- **Discussions:** [GitHub Discussions](https://github.com/Janus-admin/janus/discussions)
- **Security:** Open a GitHub issue with the `security` label

---

**Self-host your AI gateway.**
